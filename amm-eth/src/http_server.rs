use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use dashmap::DashMap;
use ethers::types::Address;
use log::info;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;
use tower_http::cors::CorsLayer;

use crate::types::PairInfo;

/// HTTP API response for pair information
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PairInfoResponse {
    pub pool_address: String,
    pub pool_token0: String,
    pub pool_token1: String,
    pub dex_version: String,
    pub decimals0: u8,
    pub decimals1: u8,
    pub factory: String,
    pub fee_tier: Option<u32>,
    pub tick_spacing: Option<i32>,
}

impl From<PairInfo> for PairInfoResponse {
    fn from(pair: PairInfo) -> Self {
        Self {
            pool_address: pair.pool_address.to_lowercase(),
            pool_token0: format!("{:?}", pair.pool_token0).to_lowercase(),
            pool_token1: format!("{:?}", pair.pool_token1).to_lowercase(),
            dex_version: format!("{:?}", pair.dex_version),
            decimals0: pair.decimals0,
            decimals1: pair.decimals1,
            factory: format!("{:?}", pair.factory).to_lowercase(),
            fee_tier: pair.fee_tier,
            tick_spacing: pair.tick_spacing,
        }
    }
}

/// Response for getting pairs by token
#[derive(Debug, Serialize)]
pub struct TokenPairsResponse {
    pub token_address: String,
    pub pairs: Vec<PairInfoResponse>,
    pub count: usize,
}

/// Application state for HTTP server
#[derive(Clone)]
pub struct HttpServerState {
    pub pair_cache: Arc<DashMap<String, PairInfo>>,
    /// Token address (lowercase) -> Set of pool addresses for fast lookups
    pub token_to_pools: Arc<DashMap<String, HashSet<String>>>,
}

/// Get all pairs involving a specific token address
async fn get_pairs_by_token(
    State(state): State<HttpServerState>,
    Path(token_address): Path<String>,
) -> Result<Json<TokenPairsResponse>, (StatusCode, String)> {
    // Try to parse the token address
    let token_addr = token_address.parse::<Address>().map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            "Invalid token address format".to_string(),
        )
    })?;

    let token_str = format!("{:?}", token_addr).to_lowercase();

    // Get pool addresses for this token from the mapping
    let pools = state
        .token_to_pools
        .get(&token_str)
        .map(|entry| entry.clone())
        .unwrap_or_default();

    // Fetch pair info from pair_cache for each pool
    let mut pairs = Vec::new();
    for pool_address in pools {
        if let Some(pair_info) = state.pair_cache.get(&pool_address) {
            pairs.push(PairInfoResponse::from(pair_info.clone()));
        }
    }

    Ok(Json(TokenPairsResponse {
        token_address: token_str,
        count: pairs.len(),
        pairs,
    }))
}

/// Health check endpoint
async fn health_check() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "healthy",
        "service": "amm-eth"
    }))
}

/// Start HTTP server on the given address
pub async fn start_http_server(
    addr: String,
    pair_cache: Arc<DashMap<String, PairInfo>>,
    token_to_pools: Arc<DashMap<String, HashSet<String>>>,
) -> anyhow::Result<()> {
    let state = HttpServerState {
        pair_cache,
        token_to_pools,
    };

    let app = Router::new()
        .route("/health", get(health_check))
        .route("/pairs/:token_address", get(get_pairs_by_token))
        .with_state(state)
        .layer(CorsLayer::permissive());

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!("HTTP server listening on: {}", addr);

    axum::serve(listener, app).await?;
    Ok(())
}
