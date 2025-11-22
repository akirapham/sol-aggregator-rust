mod aggregator;
mod api;
mod arbitrage_config;
mod arbitrage_monitor;
mod arbitrage_transaction_handler;
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

use binance_price_stream::{BinanceConfig, BinancePriceStream, StreamType};
use dotenv::dotenv;
use env_logger::Env;
use solana_sdk::signature::Signer;
use solana_sdk::signer::keypair::read_keypair_file;
use std::env;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::signal;
use solana_sdk::signer::keypair::Keypair;

use crate::arbitrage_config::ArbitrageConfig;
use crate::arbitrage_monitor::ArbitrageMonitor;
use crate::config::ConfigLoader;
use crate::grpc::create_grpc_service;
use crate::pool_manager::PoolStateManager;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    // 0. Start the price feed service
    log::info!("Starting Binance price feed service...");
    let price_service = Arc::new(BinancePriceStream::new(
        BinanceConfig::with_stream_type(StreamType::BookTicker),
        vec!["SOLUSDT".to_string()],
    ));
    let _ = price_service.start().await;

    // 1. Start the pool manager and gRPC streaming
    log::info!("Starting pool manager and gRPC streaming...");
    let (grpc_service, batch_rx) = create_grpc_service(50, 100).await?;
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
    let arb_config_path =
        env::var("ARBITRAGE_CONFIG_PATH").unwrap_or_else(|_| "arbitrage_config.toml".to_string());

    // Helper to attempt loading and wiring up the arbitrage monitor from a provided config
    async fn try_setup_arb(
        arb_path: PathBuf,
        pool_manager: Arc<PoolStateManager>,
        aggregator: Arc<aggregator::DexAggregator>,
    ) -> Option<(
        Arc<std::sync::RwLock<ArbitrageConfig>>,
        Arc<ArbitrageMonitor>,
    )> {
        if !arb_path.exists() {
            return None;
        }

        match ArbitrageConfig::from_file(arb_path.to_str().unwrap()) {
            Ok(mut arb_config) => {
                log::info!("Arbitrage configuration loaded from {}", arb_path.display());

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
                    "Monitoring arbitrage {} tokens",
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

                // Load mainnet configuration
                let rpc_url = env::var("SOLANA_RPC_URL")
                    .unwrap_or_else(|_| "https://sol-rpc.degalabs.fi/jsdh7483-0543-skdjs-84738-d383438e4sdfd".to_string());
                log::info!("Using Solana RPC: {}", rpc_url);

                // Load keypair for transaction signing
                let keypair_path = env::var("SOLANA_KEYPAIR_PATH").unwrap();
                let keypair = read_keypair_file(&keypair_path).ok()?;
                log::info!("Loaded keypair: {}", keypair.pubkey());

                let monitor = ArbitrageMonitor::new(
                    aggregator_clone,
                    arb_config.clone(),
                    "rocksdb_data/arbitrage_opportunities",
                    &rpc_url,
                    Arc::new(keypair),
                )
                .expect("Failed to create arbitrage monitor");

                // Wrap monitor in Arc for sharing across tasks
                let monitor = Arc::new(monitor);

                // Subscribe to broadcast pool updates from pool manager
                let pool_update_rx = pool_manager.subscribe_arbitrage_updates();
                let monitor_for_events = monitor.clone();
                monitor_for_events.subscribe_to_pool_updates(pool_update_rx);

                // Spawn a cleanup task to remove old opportunities periodically
                let monitor_for_cleanup = monitor.clone();
                tokio::spawn(async move {
                    let mut interval =
                        tokio::time::interval(tokio::time::Duration::from_secs(3600)); // Every hour
                    loop {
                        interval.tick().await;
                        match monitor_for_cleanup.cleanup_old_opportunities(86400) {
                            // Keep last 24 hours
                            Ok(count) => {
                                if count > 0 {
                                    log::info!("Cleaned up {} old arbitrage opportunities", count);
                                }
                            }
                            Err(e) => {
                                log::error!("Failed to cleanup old opportunities: {}", e);
                            }
                        }
                    }
                });

                log::info!("Arbitrage monitoring started successfully (broadcast mode)");
                Some((Arc::new(std::sync::RwLock::new(arb_config)), monitor))
            }
            Err(e) => {
                log::error!(
                    "Failed to parse arbitrage config at {}: {}",
                    arb_path.display(),
                    e
                );
                None
            }
        }
    }

    // Try explicit path first
    let mut arb_config_arc = None;
    let mut arbitrage_monitor = None;

    let requested = PathBuf::from(&arb_config_path);
    // If the requested path exists, try to load it
    if let Some((cfg, mon)) =
        try_setup_arb(requested.clone(), pool_manager.clone(), aggregator.clone()).await
    {
        arb_config_arc = Some(cfg);
        arbitrage_monitor = Some(mon);
    } else {
        // If the file wasn't found at the requested path, try common fallbacks
        let mut tried = Vec::new();
        tried.push(requested.clone());

        // Fallback inside aggregator-sol directory
        let fallback1 = PathBuf::from(env::current_dir()?).join("aggregator-sol").join(&arb_config_path);
        tried.push(fallback1.clone());

        // Fallback to repo root's config directory
        let fallback2 = PathBuf::from("config").join(&arb_config_path);
        tried.push(fallback2.clone());

        let mut found = false;
        for p in tried.into_iter() {
            if p.exists() {
                if let Some((cfg, mon)) =
                    try_setup_arb(p, pool_manager.clone(), aggregator.clone()).await
                {
                    arb_config_arc = Some(cfg);
                    arbitrage_monitor = Some(mon);
                    found = true;
                    break;
                }
            }
        }

        if !found {
            // Informative warn with working dir to help debugging
            match env::current_dir() {
                Ok(cwd) => log::warn!(
                    "Arbitrage configuration not found at {} (cwd: {}). Arbitrage monitoring disabled. Set ARBITRAGE_CONFIG_PATH to point to the file.",
                    arb_config_path,
                    cwd.display()
                ),
                Err(_) => log::warn!(
                    "Arbitrage configuration not found at {}. Arbitrage monitoring disabled. Set ARBITRAGE_CONFIG_PATH to point to the file.",
                    arb_config_path
                ),
            }
        }
    }

    // read port from env or default to 3000
    let port = std::env::var("API_PORT").unwrap_or_else(|_| "3000".into());
    log::info!("Starting REST API server on port {}...", port);

    // Create router with aggregator and arbitrage config
    let app = if let Some(arb_config) = arb_config_arc {
        api::create_router(aggregator, arb_config, arbitrage_monitor)
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
        api::create_router(
            aggregator,
            Arc::new(std::sync::RwLock::new(default_config)),
            None,
        )
    };

    let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).await?;

    log::info!("Server running on http://127.0.0.1:{}", port);
    log::info!("API endpoints:");
    log::info!("  POST /quote - Get swap quotes");
    log::info!("  GET  /pools/:token0/:token1 - Get pools for token pair");
    log::info!("  GET  /health - Health check");

    // 4. Start serving with graceful shutdown
    let (shutdown_tx, shutdown_rx) = tokio::sync::broadcast::channel(1);

    // Setup signal handlers for graceful shutdown
    let shutdown_tx_ctrl_c = shutdown_tx.clone();
    tokio::spawn(async move {
        match signal::ctrl_c().await {
            Ok(()) => {
                log::info!("Received SIGINT (Ctrl+C), initiating graceful shutdown...");
                let _ = shutdown_tx_ctrl_c.send(());
            }
            Err(err) => {
                log::error!("Failed to listen for SIGINT: {}", err);
            }
        }
    });

    // Setup signal handlers for SIGTERM
    let shutdown_tx_sigterm = shutdown_tx.clone();
    tokio::spawn(async move {
        match signal::unix::signal(signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
                log::info!("Received SIGTERM, initiating graceful shutdown...");
                let _ = shutdown_tx_sigterm.send(());
            }
            Err(err) => {
                log::error!("Failed to listen for SIGTERM: {}", err);
            }
        }
    });

    // Create a task that listens for shutdown signal
    let pool_manager_shutdown = pool_manager.clone();
    let shutdown_handle = tokio::spawn(async move {
        let mut rx = shutdown_rx;
        rx.recv().await.ok();

        log::info!("Saving pools to database before shutdown...");
        if let Err(e) = pool_manager_shutdown.save_pools().await {
            log::error!("Failed to save pools: {}", e);
        } else {
            log::info!("Pools saved successfully");
        }
    });

    // Run the server
    let server_result = axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            let mut rx = shutdown_tx.subscribe();
            rx.recv().await.ok();
        })
        .await;

    // Wait for shutdown handler to complete
    let _ = tokio::time::timeout(tokio::time::Duration::from_secs(30), shutdown_handle).await;

    server_result?;

    Ok(())
}
