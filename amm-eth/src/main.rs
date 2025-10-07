use amm_eth::{EthConfig, EthSwapListener, PriceStore, TokenPairDb, WsServer};
use anyhow::Result;
use binance_price_stream::{BinanceConfig, BinancePriceStream, StreamType};
use dotenv::dotenv;
use env_logger::Env;
use log::info;
use std::sync::Arc;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<()> {
    // Load environment variables
    dotenv().ok();

    // Initialize logger
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    info!("Starting Ethereum Uniswap swap listener");

    // Create WebSocket server
    let ws_port = std::env::var("ETH_PRICE_WS_PORT").unwrap_or_else(|_| "8080".to_string());
    let ws_addr = format!("127.0.0.1:{}", ws_port).parse()?;
    info!("Starting WebSocket server on: {}", ws_addr);
    let ws_server = Arc::new(WsServer::new(ws_addr));
    let broadcaster = ws_server.get_broadcaster();

    // Start WebSocket server in background
    let ws_server_clone = ws_server.clone();
    tokio::spawn(async move {
        if let Err(e) = ws_server_clone.start().await {
            log::error!("WebSocket server error: {}", e);
        }
    });

    // Create configuration
    let config = EthConfig::default();

    // Start Binance WebSocket for ETH price updates
    info!("Starting Binance WebSocket for ETH/USDT price...");
    let binance_config = BinanceConfig::with_stream_type(StreamType::BookTicker);
    let binance_client = BinancePriceStream::new(binance_config, vec!["ETHUSDT".to_string()]);
    let _eth_price_rx = binance_client.start().await?;

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

    // Create price store with broadcaster
    let price_store = PriceStore::with_broadcaster(broadcaster);

    // Create the listener
    let listener = EthSwapListener::new(config, price_store.clone()).await?;

    // Open RocksDB for token pair persistence
    let db_path = "rocksdb_data/amm-eth";
    info!("Opening RocksDB at {}", db_path);
    let token_pair_db = TokenPairDb::open(db_path)?;

    // Load existing token pairs and decimals from database
    let token_pair_cache = listener.get_token_pair_cache();
    let token_decimal_cache = listener.get_token_decimal_cache();
    let loaded_count =
        token_pair_db.load_all_into_cache(&token_pair_cache, &token_decimal_cache)?;
    info!("Loaded {} token pairs from RocksDB", loaded_count);

    // Start a background task to save token pairs and decimals to RocksDB 60s
    let save_pair_cache = token_pair_cache.clone();
    let save_decimal_cache = token_decimal_cache.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        loop {
            interval.tick().await;

            match token_pair_db.save_all_from_cache(&save_pair_cache, &save_decimal_cache) {
                Ok(count) => {
                    info!("💾 Saved {} token pairs to RocksDB", count);
                }
                Err(e) => {
                    log::error!("Failed to save token pairs to RocksDB: {}", e);
                }
            }
        }
    });

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

    // Start listening to swap events
    info!("Starting swap event listener...");
    listener.start().await?;

    Ok(())
}
