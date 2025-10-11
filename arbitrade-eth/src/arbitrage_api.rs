use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json, Router,
    routing::{get, post, delete},
};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::db::{ArbitrageDb, ArbitrageOpportunity, DbStats};

pub struct AppState {
    pub db: Arc<ArbitrageDb>,
    pub blacklist: Arc<DashMap<String, ()>>,
}

#[derive(Debug, Deserialize)]
pub struct OpportunityQuery {
    pub token_address: Option<String>,
    pub limit: Option<usize>,
    pub start_time: Option<i64>,
    pub end_time: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct OpportunitiesResponse {
    pub opportunities: Vec<ArbitrageOpportunity>,
    pub count: usize,
}

#[derive(Debug, Serialize)]
pub struct StatsResponse {
    pub stats: DbStats,
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

/// GET /api/opportunities - Get arbitrage opportunities with optional filters
async fn get_opportunities(
    State(state): State<Arc<AppState>>,
    Query(params): Query<OpportunityQuery>,
) -> Result<Json<OpportunitiesResponse>, (StatusCode, Json<ErrorResponse>)> {
    log::debug!("Querying opportunities with params: {:?}", params);

    let opportunities = if params.start_time.is_some() || params.end_time.is_some() {
        // Time range query
        let start = params.start_time.unwrap_or(0);
        let end = params.end_time.unwrap_or(i64::MAX);

        state
            .db
            .get_opportunities_by_time_range(start, end, params.limit)
            .map_err(|e| {
                log::error!("Failed to query opportunities by time: {}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse {
                        error: format!("Database error: {}", e),
                    }),
                )
            })?
    } else {
        // Regular query with optional token filter
        state
            .db
            .get_opportunities(params.token_address.as_deref(), params.limit)
            .map_err(|e| {
                log::error!("Failed to query opportunities: {}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse {
                        error: format!("Database error: {}", e),
                    }),
                )
            })?
    };

    let count = opportunities.len();

    Ok(Json(OpportunitiesResponse {
        opportunities,
        count,
    }))
}

/// GET /api/opportunities/top - Get top profitable opportunities
async fn get_top_opportunities(
    State(state): State<Arc<AppState>>,
    Query(params): Query<OpportunityQuery>,
) -> Result<Json<OpportunitiesResponse>, (StatusCode, Json<ErrorResponse>)> {
    let limit = params.limit.unwrap_or(10);

    log::debug!("Querying top {} opportunities", limit);

    let opportunities = state.db.get_top_opportunities(limit).map_err(|e| {
        log::error!("Failed to query top opportunities: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Database error: {}", e),
            }),
        )
    })?;

    let count = opportunities.len();

    Ok(Json(OpportunitiesResponse {
        opportunities,
        count,
    }))
}

/// GET /api/stats - Get database statistics
async fn get_stats(
    State(state): State<Arc<AppState>>,
) -> Result<Json<StatsResponse>, (StatusCode, Json<ErrorResponse>)> {
    log::debug!("Querying database stats");

    let stats = state.db.get_stats().map_err(|e| {
        log::error!("Failed to query stats: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Database error: {}", e),
            }),
        )
    })?;

    Ok(Json(StatsResponse { stats }))
}

/// GET /health - Health check endpoint
async fn health_check() -> impl IntoResponse {
    (StatusCode::OK, "OK")
}

#[derive(Debug, Deserialize)]
pub struct BlacklistRequest {
    pub address: String,
}

#[derive(Debug, Serialize)]
pub struct BlacklistResponse {
    pub addresses: Vec<String>,
    pub count: usize,
}

#[derive(Debug, Serialize)]
pub struct SuccessResponse {
    pub message: String,
}

/// POST /api/blacklist - Add an address to the blacklist
async fn add_to_blacklist(
    State(state): State<Arc<AppState>>,
    Json(req): Json<BlacklistRequest>,
) -> Result<Json<SuccessResponse>, (StatusCode, Json<ErrorResponse>)> {
    let address = req.address.to_lowercase();

    log::info!("Adding address {} to blacklist", address);

    // Add to database
    state.db.add_to_blacklist(&address).map_err(|e| {
        log::error!("Failed to add address to blacklist: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Database error: {}", e),
            }),
        )
    })?;

    // Add to in-memory cache
    state.blacklist.insert(address.clone(), ());

    Ok(Json(SuccessResponse {
        message: format!("Address {} added to blacklist", address),
    }))
}

/// DELETE /api/blacklist - Remove an address from the blacklist
async fn remove_from_blacklist(
    State(state): State<Arc<AppState>>,
    Json(req): Json<BlacklistRequest>,
) -> Result<Json<SuccessResponse>, (StatusCode, Json<ErrorResponse>)> {
    let address = req.address.to_lowercase();

    log::info!("Removing address {} from blacklist", address);

    // Remove from database
    state.db.remove_from_blacklist(&address).map_err(|e| {
        log::error!("Failed to remove address from blacklist: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Database error: {}", e),
            }),
        )
    })?;

    // Remove from in-memory cache
    state.blacklist.remove(&address);

    Ok(Json(SuccessResponse {
        message: format!("Address {} removed from blacklist", address),
    }))
}

/// GET /api/blacklist - Get all blacklisted addresses
async fn get_blacklist(
    State(state): State<Arc<AppState>>,
) -> Result<Json<BlacklistResponse>, (StatusCode, Json<ErrorResponse>)> {
    log::debug!("Querying blacklist");

    let addresses: Vec<String> = state.blacklist.iter().map(|entry| entry.key().clone()).collect();
    let count = addresses.len();

    Ok(Json(BlacklistResponse { addresses, count }))
}

pub fn create_router(db: Arc<ArbitrageDb>, blacklist: Arc<DashMap<String, ()>>) -> Router {
    let state = Arc::new(AppState { db, blacklist });

    Router::new()
        .route("/api/opportunities", get(get_opportunities))
        .route("/api/opportunities/top", get(get_top_opportunities))
        .route("/api/stats", get(get_stats))
        .route("/api/blacklist", get(get_blacklist))
        .route("/api/blacklist", post(add_to_blacklist))
        .route("/api/blacklist", delete(remove_from_blacklist))
        .with_state(state)
}
