pub mod aggregator;
pub mod api;
pub mod arbitrage_config;
pub mod arbitrage_monitor;
// pub mod common;
pub mod config;
pub mod constants;
pub mod db;
pub mod dex;
pub mod error;
pub mod fetchers;
pub mod grpc;
pub mod pool_data_types;
pub mod pool_discovery;
pub mod pool_manager;
#[cfg(test)]
pub mod tests;
pub mod types;
pub mod utils;

use crate::pool_manager::ArbitragePoolUpdate;
use binance_price_stream::{BinanceConfig, BinancePriceStream, StreamType};
use solana_client::nonblocking::rpc_client::RpcClient;
use tokio::sync::broadcast;

use crate::config::ConfigLoader; // Ensure ConfigLoader is imported
use dotenv::dotenv;
use env_logger::Env;
use solana_sdk::pubkey::Pubkey;
use std::env;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::signal;

use crate::arbitrage_config::ArbitrageConfig;
use crate::arbitrage_monitor::ArbitrageMonitor;
use crate::grpc::create_grpc_service;
// use crate::pool_manager::traits::DatabaseTrait;
use crate::pool_manager::{PoolDataProvider, PoolStateManager};

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

    // 0. Initialize Database
    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let database = Arc::new(
        db::Database::new(&database_url)
            .await
            .expect("Failed to connect to database"),
    );

    // Run migrations (optional, better to use sqlx-cli in prod)
    // database.run_migrations().await.expect("Failed to run migrations"); // Implement if needed

    // 1. Start the pool manager and gRPC streaming
    log::info!("Starting pool manager and gRPC streaming...");
    let grpc_service = create_grpc_service(50, 100).await?;

    log::info!("Creating DEX aggregator...");
    let config = ConfigLoader::load().expect("Failed to load config");

    let rpc_client = Arc::new(RpcClient::new(config.rpc_url.clone()));
    let (arbitrage_pool_tx, _arbitrage_pool_rx) = broadcast::channel::<ArbitragePoolUpdate>(1000);

    let pool_manager = Arc::new(
        pool_manager::PoolStateManager::new(
            grpc_service.clone(),
            config.clone(),
            rpc_client.clone(),
            price_service.clone(),
            arbitrage_pool_tx.clone(),
            database.clone(),
        )
        .await,
    );
    // Start background event processing
    // Batch processing loop is now handled internally by GrpcService via subscribe_pool_updates

    // Start pool manager
    let pool_manager_clone = pool_manager.clone();
    tokio::spawn(async move {
        log::info!("🚀 Spawning pool_manager.start() task...");
        pool_manager_clone.start().await;
        log::info!(
            "✅ pool_manager.start() returned (should be long running or completely spawned)"
        );
    });

    // 2. Create and configure the aggregator
    // Config already loaded above
    let aggregator = Arc::new(aggregator::DexAggregator::new(
        config,
        pool_manager.clone() as Arc<dyn PoolDataProvider>,
    ));

    // 2.5. Load arbitrage configuration and start monitoring (optional)
    // Check if arbitrage detection is enabled via environment variable
    let arbitrage_enabled = env::var("ENABLE_ARBITRAGE_DETECTION")
        .unwrap_or_else(|_| "false".to_string())
        .to_lowercase()
        == "true";

    if !arbitrage_enabled {
        log::info!("Arbitrage detection is disabled (ENABLE_ARBITRAGE_DETECTION=false)");
    }

    let arb_config_path =
        env::var("ARBITRAGE_CONFIG_PATH").unwrap_or_else(|_| "arbitrage_config.toml".to_string());

    // Helper to attempt loading and wiring up the arbitrage monitor from a provided config
    async fn try_setup_arb(
        arb_path: PathBuf,
        pool_manager: Arc<PoolStateManager>,
        aggregator: Arc<aggregator::DexAggregator>,
        db_pool: sqlx::Pool<sqlx::Postgres>,
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
                if let Ok(db_token_pubkeys) = db.load_arbitrage_tokens().await {
                    if !db_token_pubkeys.is_empty() {
                        log::info!(
                            "Loaded {} tokens from Postgres, merging with TOML config",
                            db_token_pubkeys.len()
                        );
                        use crate::arbitrage_config::MonitoredToken;
                        let db_tokens: Vec<MonitoredToken> = db_token_pubkeys
                            .into_iter()
                            .map(|pk| MonitoredToken {
                                symbol: "UNKNOWN".to_string(),
                                address: pk.to_string(),
                                enabled: true,
                            })
                            .collect();
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
                let rpc_url = env::var("SOLANA_RPC_URL").unwrap_or_else(|_| "".to_string());
                log::info!("Using Solana RPC: {}", rpc_url);

                // Load keypair for transaction signing
                let payer_pubkey_str = env::var("PAYER_PUBKEY").unwrap();
                let payer_pubkey =
                    Pubkey::from_str(&payer_pubkey_str).expect("Invalid PAYER_PUBKEY");
                log::info!("Loaded keypair: {}", payer_pubkey);

                // Create the monitor
                let monitor = ArbitrageMonitor::new(
                    aggregator_clone,
                    arb_config.clone(),
                    db_pool,
                    &rpc_url,
                    payer_pubkey,
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
                        match monitor_for_cleanup.cleanup_old_opportunities(86400).await {
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

    if arbitrage_enabled {
        let requested = PathBuf::from(&arb_config_path);
        // If the requested path exists, try to load it
        // Note: functionality of try_setup_arb needs update too
        // But for now let's assume we update the manual construction if valid
        // Actually try_setup_arb is a helper function. We need to check it.
        // Assuming we replace it or update it.
        if let Some((cfg, mon)) = try_setup_arb(
            requested.clone(),
            pool_manager.clone(),
            aggregator.clone(),
            database.get_pool().clone(),
        )
        .await
        {
            arb_config_arc = Some(cfg);
            arbitrage_monitor = Some(mon);
        } else {
            // If the file wasn't found at the requested path, try common fallbacks
            let mut tried = Vec::new();
            tried.push(requested.clone());

            // Fallback inside aggregator-sol directory
            let fallback1 = env::current_dir()?
                .join("aggregator-sol")
                .join(&arb_config_path);
            tried.push(fallback1.clone());

            // Fallback to repo root's config directory
            let fallback2 = PathBuf::from("config").join(&arb_config_path);
            tried.push(fallback2.clone());

            let mut found = false;
            for p in tried.into_iter() {
                if p.exists() {
                    if let Some((cfg, mon)) = try_setup_arb(
                        p,
                        pool_manager.clone(),
                        aggregator.clone(),
                        database.get_pool().clone(),
                    )
                    .await
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
    } // End of arbitrage_enabled check

    // read port from env or default to 3000
    let port = std::env::var("API_PORT").unwrap_or_else(|_| "3000".into());
    log::info!("Starting REST API server on port {}...", port);

    // Create router with aggregator and arbitrage config
    let app = api::create_router(aggregator, rpc_client, arb_config_arc, arbitrage_monitor);

    let listener = TcpListener::bind(format!("0.0.0.0:{}", port)).await?;

    log::info!("Server running on http://0.0.0.0:{}", port);
    log::info!("API endpoints:");
    log::info!("  GET  /quote - Get swap quotes");
    log::info!("  GET  /quote-debug - Debug quote parameters");
    log::info!("  GET  /pools/:token0/:token1 - Get pools for token pair");
    log::info!("  GET  /health - Health check");

    // 4. Start serving with graceful shutdown
    let (shutdown_tx, _shutdown_rx) = tokio::sync::broadcast::channel(1);

    // Setup signal handlers for graceful shutdown
    let shutdown_tx_ctrl_c = shutdown_tx.clone();
    tokio::spawn(async move {
        log::info!("SIGINT handler registered and waiting...");
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
        log::info!("SIGTERM handler registered and waiting...");
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

    // Create a oneshot channel to signal when data is saved
    let (save_complete_tx, save_complete_rx) = tokio::sync::oneshot::channel();

    // Create a task that listens for shutdown signal and saves data
    let pool_manager_shutdown = pool_manager.clone();
    let mut shutdown_rx_handler = shutdown_tx.subscribe();
    tokio::spawn(async move {
        log::info!("Save task waiting for shutdown signal...");
        match shutdown_rx_handler.recv().await {
            Ok(_) => log::info!("Save task received shutdown signal"),
            Err(e) => log::error!("Save task failed to receive shutdown signal: {}", e),
        }

        log::info!("Saving pools to database before shutdown...");

        if let Err(e) = pool_manager_shutdown.save_pools().await {
            log::error!("Failed to save pools: {}", e);
        } else {
            log::info!("Pools saved successfully");
        }

        // Signal that save is complete
        let _ = save_complete_tx.send(());
        log::info!("Save complete signal sent");
    });

    // Run the server with graceful shutdown that waits for data save
    let server_result = axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            let mut rx = shutdown_tx.subscribe();
            rx.recv().await.ok();

            // Wait for data save to complete before shutting down server
            log::info!("Waiting for data save to complete...");
            let _ =
                tokio::time::timeout(tokio::time::Duration::from_secs(30), save_complete_rx).await;
            log::info!("Data save complete, shutting down server...");
        })
        .await;

    server_result?;

    Ok(())
}
