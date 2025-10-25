// src/api/mod.rs
pub mod dto;
pub mod handlers;

use crate::aggregator::DexAggregator;
use crate::arbitrage_config::ArbitrageConfig;
use axum::{
    routing::{delete, get, post},
    Router,
};
use std::sync::{Arc, RwLock};

#[derive(Clone)]
pub struct AppState {
    pub aggregator: Arc<DexAggregator>,
    pub arbitrage_config: Arc<RwLock<ArbitrageConfig>>,
}

pub fn create_router(
    aggregator: Arc<DexAggregator>,
    arbitrage_config: Arc<RwLock<ArbitrageConfig>>,
) -> Router {
    let state = AppState {
        aggregator: aggregator.clone(),
        arbitrage_config,
    };

    Router::new()
        .route("/health", get(handlers::health_check))
        .route("/quote", post(handlers::get_quote))
        .route("/arbitrage", post(handlers::check_arbitrage))
        .route("/arbitrage/tokens", get(handlers::get_arbitrage_tokens))
        .route("/arbitrage/tokens", post(handlers::add_arbitrage_token))
        .route(
            "/arbitrage/tokens",
            delete(handlers::remove_arbitrage_token),
        )
        // .route("/routes", post(handlers::get_routes))
        .route("/pools/:token0/:token1", get(handlers::get_pools))
        .route("/stats", get(handlers::get_pool_stats))
        .with_state(state)
}
