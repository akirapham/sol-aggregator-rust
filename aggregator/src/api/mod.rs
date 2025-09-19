// src/api/mod.rs
pub mod dto;
pub mod handlers;

use crate::aggregator::DexAggregator;
use axum::{
    routing::{get, post},
    Router,
};
use std::sync::Arc;

pub fn create_router(aggregator: Arc<DexAggregator>) -> Router {
    Router::new()
        .route("/health", get(handlers::health_check))
        .route("/quote", post(handlers::get_quote))
        // .route("/routes", post(handlers::get_routes))
        .route("/pools/:token0/:token1", get(handlers::get_pools))
        // .route("/stats", get(handlers::get_stats))
        .with_state(aggregator)
}
