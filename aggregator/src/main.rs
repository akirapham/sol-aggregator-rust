mod aggregator;
mod api;
mod config;
mod constants;
mod dex;
mod error;
mod fetchers;
mod grpc;
mod pool_data_types;
mod pool_manager;
mod smart_routing;
mod types;
mod utils;

use axum::serve;
use dotenv::dotenv;
use env_logger::Env;
use std::sync::Arc;
use tokio::net::TcpListener;

use crate::aggregator::DexAggregator;
use crate::config::ConfigLoader;
use crate::grpc::create_grpc_service;
use crate::pool_manager::PoolStateManager;
use crate::types::AggregatorConfig;
use crate::utils::BinancePriceService;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    // 0. Start the price feed service
    log::info!("Starting Binance price feed service...");
    let price_service = Arc::new(BinancePriceService::new());
    price_service.start().await;

    // 1. Start the pool manager and gRPC streaming
    log::info!("Starting pool manager and gRPC streaming...");
    let (grpc_service, batch_rx) = create_grpc_service(50, 500).await?;
    let pool_manager = Arc::new(PoolStateManager::new(grpc_service, price_service.clone()).await);

    // Start background event processing
    let pool_update_sender = pool_manager.get_pool_update_sender().clone();
    PoolStateManager::start_batch_event_processing(batch_rx, pool_update_sender);

    // Start pool manager
    let pool_manager_clone = pool_manager.clone();
    tokio::spawn(async move {
        pool_manager_clone.start().await;
    });

    // 2. Create and configure the aggregator
    log::info!("Creating DEX aggregator...");
    let config = ConfigLoader::load().unwrap();
    let aggregator = Arc::new(aggregator::DexAggregator::new(config, pool_manager));

    // 3. Create and start the REST API server
    // read port from env or default to 3000
    let port = std::env::var("API_PORT").unwrap_or_else(|_| "3000".into());
    log::info!("Starting REST API server on port {}...", port);
    let app = api::create_router(aggregator);
    let listener = TcpListener::bind(format!("0.0.0.0:{}", port)).await?;

    log::info!("Server running on http://0.0.0.0:{}", port);
    log::info!("API endpoints:");
    log::info!("  POST /quote - Get swap quotes");
    log::info!("  GET  /pools/:token0/:token1 - Get pools for token pair");
    log::info!("  GET  /health - Health check");

    // 4. Start serving
    serve(listener, app).await?;

    Ok(())
}
