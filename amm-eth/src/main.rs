use amm_eth::{EthConfig, EthSwapListener, PriceStore, TokenPairDb, WsServer};
use anyhow::Result;
use binance_price_stream::{BinanceConfig, BinancePriceStream, StreamType};
use dotenv::dotenv;
use env_logger::Env;
use log::info;
use std::sync::Arc;
use std::time::Duration;
use tokio::signal;

#[tokio::main]
async fn main() -> Result<()> {
    eprintln!("DEBUG: amm-eth main() started");

    // Load environment variables
    dotenv().ok();
    eprintln!("DEBUG: dotenv loaded");

    // Check critical environment variables
    match std::env::var("ETH_RPC_URL") {
        Ok(url) => eprintln!("DEBUG: ETH_RPC_URL = {}", url),
        Err(e) => {
            eprintln!("ERROR: ETH_RPC_URL not set: {}", e);
            return Err(anyhow::anyhow!("ETH_RPC_URL environment variable not set"));
        }
    }

    match std::env::var("ETH_WEBSOCKET_URL") {
        Ok(url) => eprintln!("DEBUG: ETH_WEBSOCKET_URL = {}", url),
        Err(e) => eprintln!("WARNING: ETH_WEBSOCKET_URL not set: {}", e),
    }

    // Initialize logger
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();
    eprintln!("DEBUG: logger initialized");

    info!("Starting Ethereum Uniswap swap listener");

    // Create WebSocket server
    let ws_port = std::env::var("ETH_PRICE_WS_PORT").unwrap_or_else(|_| "8080".to_string());
    let ws_addr = format!("0.0.0.0:{}", ws_port).parse()?;
    info!("Starting WebSocket server on: {}", ws_addr);
    eprintln!("DEBUG: WebSocket server address: {}", ws_addr);
    let ws_server = Arc::new(WsServer::new(ws_addr));
    let broadcaster = ws_server.get_broadcaster();

    // Start WebSocket server in background
    let ws_server_clone = ws_server.clone();
    tokio::spawn(async move {
        if let Err(e) = ws_server_clone.start().await {
            log::error!("WebSocket server error: {}", e);
        }
    });
    eprintln!("DEBUG: WebSocket server spawned");

    // Create configuration
    eprintln!("DEBUG: Creating EthConfig...");
    let config = EthConfig::default();
    eprintln!("DEBUG: EthConfig created successfully");

    // Start Binance WebSocket for ETH price updates
    info!("Starting Binance WebSocket for ETH/USDT price...");
    eprintln!("DEBUG: Starting Binance client...");
    let binance_config = BinanceConfig::with_stream_type(StreamType::BookTicker);
    let binance_client = BinancePriceStream::new(binance_config, vec!["ETHUSDT".to_string()]);
    let _eth_price_rx = binance_client.start().await?;
    eprintln!("DEBUG: Binance client started");

    // Update ETH price in background every second
    let eth_price_shared = config.eth_price_usd.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        loop {
            interval.tick().await;

            // Get latest ETH price from Binance cache
            if let Some(price_update) = binance_client.get_price("ETHUSDT") {
                if let Ok(mut price) = eth_price_shared.write() {
                    *price = Some(price_update.price);
                }
            }
        }
    });
    eprintln!("DEBUG: ETH price updater spawned");

    // Create price store with broadcaster
    eprintln!("DEBUG: Creating price store...");
    let price_store = PriceStore::with_broadcaster(broadcaster);
    eprintln!("DEBUG: Price store created");

    // Create the listener
    eprintln!("DEBUG: Creating EthSwapListener...");
    let listener = EthSwapListener::new(config, price_store.clone()).await?;
    eprintln!("DEBUG: EthSwapListener created successfully");

    // Open RocksDB for token pair persistence
    let db_path = "rocksdb_data/amm-eth";
    info!("Opening RocksDB at {}", db_path);
    eprintln!("DEBUG: Opening RocksDB at {}", db_path);
    let token_pair_db = Arc::new(TokenPairDb::open(db_path)?);
    eprintln!("DEBUG: RocksDB opened successfully");

    // Load existing token pairs and decimals from database
    let token_pair_cache = listener.get_token_pair_cache();
    let token_decimal_cache = listener.get_token_decimal_cache();
    let loaded_count =
        token_pair_db.load_all_into_cache(&token_pair_cache, &token_decimal_cache)?;
    info!("Loaded {} token pairs from RocksDB", loaded_count);
    eprintln!("DEBUG: Loaded {} token pairs from RocksDB", loaded_count);
    info!("Loaded {} token pairs from RocksDB", loaded_count);

    // Start a background task to save token pairs and decimals to RocksDB 60s
    let save_pair_cache = token_pair_cache.clone();
    let save_decimal_cache = token_decimal_cache.clone();
    let db_clone = token_pair_db.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        loop {
            interval.tick().await;

            match db_clone.save_all_from_cache(&save_pair_cache, &save_decimal_cache) {
                Ok(count) => {
                    info!("💾 Saved {} token pairs to RocksDB", count);
                }
                Err(e) => {
                    log::error!("Failed to save token pairs to RocksDB: {}", e);
                }
            }
        }
    });
    eprintln!("DEBUG: Token pair saver spawned");

    // Start a background task to log statistics periodically
    let stats_price_store = price_store.clone();
    let stats_ws_server = ws_server.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            interval.tick().await;
            stats_price_store.log_stats();
            info!(
                "📡 WebSocket clients connected: {}",
                stats_ws_server.client_count()
            );
        }
    });
    eprintln!("DEBUG: Statistics logger spawned");

    // Start listening to swap events
    info!("Starting swap event listener...");
    eprintln!("DEBUG: About to start listener.start()...");

    // Create a shutdown signal handler
    let shutdown_signal = async {
        signal::ctrl_c().await.ok();
    };

    // Run listener with shutdown signal
    tokio::select! {
        result = listener.start() => {
            eprintln!("DEBUG: listener.start() completed");
            result?;
        }
        _ = shutdown_signal => {
            info!("Shutdown signal received");
            eprintln!("DEBUG: Shutdown signal received");
        }
    }

    // Save all cached data to RocksDB before exit
    info!("Saving token pairs to RocksDB before shutdown...");
    match token_pair_db.save_all_from_cache(&token_pair_cache, &token_decimal_cache) {
        Ok(count) => {
            info!("💾 Saved {} token pairs to RocksDB during shutdown", count);
            eprintln!("DEBUG: Saved {} token pairs during shutdown", count);
        }
        Err(e) => {
            log::error!("Failed to save token pairs during shutdown: {}", e);
            eprintln!("ERROR: Failed to save token pairs during shutdown: {}", e);
        }
    }

    info!("Shutdown complete");
    eprintln!("DEBUG: Shutdown complete");
    Ok(())
}
