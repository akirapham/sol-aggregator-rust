mod aggregator;
mod api;
mod arbitrage_config;
mod arbitrage_monitor;
mod config;
mod constants;
mod dex;
mod error;
mod fetchers;
mod grpc;
mod pool_data_types;
mod pool_manager;
mod types;
mod utils;

use axum::serve;
use dotenv::dotenv;
use env_logger::Env;
use std::sync::Arc;
use tokio::net::TcpListener;

use crate::arbitrage_config::ArbitrageConfig;
use crate::arbitrage_monitor::ArbitrageMonitor;
use crate::config::ConfigLoader;
use crate::grpc::create_grpc_service;
use crate::pool_manager::PoolStateManager;
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
    let chain_state_update_sender = pool_manager.get_chain_state_update_sender().clone();

    PoolStateManager::start_batch_event_processing(
        batch_rx,
        pool_update_sender,
        chain_state_update_sender,
    );

    // Start pool manager
    let pool_manager_clone = pool_manager.clone();
    tokio::spawn(async move {
        pool_manager_clone.start().await;
    });

    // 2. Create and configure the aggregator
    log::info!("Creating DEX aggregator...");
    let config = ConfigLoader::load().unwrap();
    let aggregator = Arc::new(aggregator::DexAggregator::new(config, pool_manager.clone()));

    // 2.5. Load arbitrage configuration and start monitoring (optional)
    let arb_config_path = std::env::var("ARBITRAGE_CONFIG_PATH")
        .unwrap_or_else(|_| "arbitrage_config.toml".to_string());

    let arb_config_arc = if let Ok(mut arb_config) = ArbitrageConfig::from_file(&arb_config_path) {
        log::info!("Arbitrage configuration loaded from {}", arb_config_path);

        // Load tokens from DB and merge with TOML config
        let db = pool_manager.get_db();
        if let Ok(db_tokens) = ArbitrageConfig::load_tokens_from_db(&db) {
            if !db_tokens.is_empty() {
                log::info!(
                    "Loaded {} tokens from RocksDB, merging with TOML config",
                    db_tokens.len()
                );
                arb_config = arb_config.merge_with_db_tokens(db_tokens);
            }
        }

        log::info!(
            "Monitoring {} tokens",
            arb_config.get_enabled_tokens().len()
        );

        // Collect monitored token pubkeys for the pool manager to filter broadcasts
        let mut monitored_tokens = std::collections::HashSet::new();

        // Add base token
        if let Ok(base_token) = arb_config.get_base_token() {
            monitored_tokens.insert(base_token);
        }

        // Add all monitored tokens
        monitored_tokens.extend(arb_config.get_monitored_token_pubkeys());

        log::info!(
            "Total tokens to monitor in pool manager: {}",
            monitored_tokens.len()
        );

        // Also merge with any tokens already loaded from DB by PoolManager
        let db_loaded_tokens = pool_manager.get_arbitrage_monitored_tokens().await;
        if !db_loaded_tokens.is_empty() {
            log::info!(
                "Pool manager already loaded {} tokens from DB, merging...",
                db_loaded_tokens.len()
            );
            monitored_tokens.extend(db_loaded_tokens);
        }

        // Tell pool manager which tokens to monitor (will skip save if unchanged)
        pool_manager
            .set_arbitrage_monitored_tokens(monitored_tokens)
            .await;

        let aggregator_clone = aggregator.clone();
        let (monitor, mut opportunity_rx) =
            ArbitrageMonitor::new(aggregator_clone, arb_config.clone());

        // Wrap monitor in Arc for sharing across tasks
        let monitor = Arc::new(monitor);

        // Subscribe to broadcast pool updates from pool manager
        let pool_update_rx = pool_manager.subscribe_arbitrage_updates();
        let monitor_for_events = monitor.clone();
        monitor_for_events.subscribe_to_pool_updates(pool_update_rx);

        // Handle detected arbitrage opportunities
        tokio::spawn(async move {
            while let Some(opportunity) = opportunity_rx.recv().await {
                log::info!(
                    "🎯 Arbitrage opportunity detected: {} | Profit: {} ({:.2}%) | Input: {} -> Forward: {} -> Reverse: {}",
                    opportunity.pair_name,
                    opportunity.profit_amount,
                    opportunity.profit_percent,
                    opportunity.input_amount,
                    opportunity.forward_output,
                    opportunity.reverse_output
                );
                // TODO: Implement execution logic here
                // For now, just logging the opportunities
            }
        });

        log::info!("Arbitrage monitoring started successfully (broadcast mode)");
        Some(Arc::new(std::sync::RwLock::new(arb_config)))
    } else {
        log::warn!(
            "Arbitrage configuration not found at {}. Arbitrage monitoring disabled.",
            arb_config_path
        );
        // Create a default config for API to work
        None
    };

    // 3. Create and start the REST API server
    // read port from env or default to 3000
    let port = std::env::var("API_PORT").unwrap_or_else(|_| "3000".into());
    log::info!("Starting REST API server on port {}...", port);

    // Create router with aggregator and arbitrage config
    let app = if let Some(arb_config) = arb_config_arc {
        api::create_router(aggregator, arb_config)
    } else {
        // Create a default empty config if not available
        let default_config = ArbitrageConfig {
            settings: crate::arbitrage_config::ArbitrageSettings {
                min_profit_bps: 50,
                base_token: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".to_string(), // USDC
                base_amount: 100_000_000,                                               // 100 USDC
                slippage_bps: 50,
                max_concurrent_checks: 10,
            },
            monitored_tokens: vec![],
        };
        api::create_router(aggregator, Arc::new(std::sync::RwLock::new(default_config)))
    };

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
