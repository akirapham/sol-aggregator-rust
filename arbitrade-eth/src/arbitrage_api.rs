use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json, Router,
    routing::get,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::db::{ArbitrageDb, ArbitrageOpportunity, DbStats};

pub struct AppState {
    pub db: Arc<ArbitrageDb>,
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

pub fn create_router(db: Arc<ArbitrageDb>) -> Router {
    let state = Arc::new(AppState { db });

    Router::new()
        .route("/api/opportunities", get(get_opportunities))
        .route("/api/opportunities/top", get(get_top_opportunities))
        .route("/api/stats", get(get_stats))
        .with_state(state)
}
