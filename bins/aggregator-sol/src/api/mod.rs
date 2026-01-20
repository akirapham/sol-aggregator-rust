// src/api/mod.rs
pub mod auth;
pub mod dashboard;
pub mod dto;
pub mod handlers;

use crate::aggregator::DexAggregator;
use crate::arbitrage_config::ArbitrageConfig;
use crate::arbitrage_monitor::ArbitrageMonitor;
use axum::{
    middleware,
    routing::{delete, get, post},
    Router,
};
use solana_client::nonblocking::rpc_client::RpcClient;
use std::sync::{Arc, RwLock};

use auth::{auth_middleware, AuthConfig};

#[derive(Clone)]
pub struct AppState {
    pub aggregator: Arc<DexAggregator>,
    pub rpc_client: Arc<RpcClient>,
    pub arbitrage_config: Option<Arc<RwLock<ArbitrageConfig>>>,
    pub arbitrage_monitor: Option<Arc<ArbitrageMonitor>>,
}

pub fn create_router(
    aggregator: Arc<DexAggregator>,
    rpc_client: Arc<RpcClient>,
    arbitrage_config: Option<Arc<RwLock<ArbitrageConfig>>>,
    arbitrage_monitor: Option<Arc<ArbitrageMonitor>>,
) -> Router {
    let state = Arc::new(AppState {
        aggregator: aggregator.clone(),
        rpc_client,
        arbitrage_config,
        arbitrage_monitor,
    });

    let auth_config = Arc::new(AuthConfig::from_env());

    // Protected routes that require authentication
    let protected_routes = Router::new()
        .route("/", get(dashboard::dashboard_page))
        .route("/dashboard", get(dashboard::dashboard_page))
        .route("/arbitrage", post(handlers::check_arbitrage))
        .route("/arbitrage/tokens", get(handlers::get_arbitrage_tokens))
        .route("/arbitrage/tokens", post(handlers::add_arbitrage_token))
        .route(
            "/arbitrage/tokens",
            delete(handlers::remove_arbitrage_token),
        )
        .with_state(state.clone())
        .layer(middleware::from_fn_with_state(
            auth_config.clone(),
            auth_middleware,
        ));

    // Public routes (no auth required)
    Router::new()
        .route("/health", get(handlers::health_check))
        .route("/quote", post(handlers::get_quote))
        .route("/pools/{token0}/{token1}", get(handlers::get_pools))
        .route(
            "/token/{token_address}/pools",
            get(handlers::get_token_pools),
        )
        .route("/stats", get(handlers::get_pool_stats))
        .with_state(state)
        .merge(protected_routes)
}
