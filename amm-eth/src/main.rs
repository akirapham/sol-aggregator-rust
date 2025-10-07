use amm_eth::{EthConfig, EthSwapListener, PriceStore};
use anyhow::Result;
use binance_price_stream::{BinanceConfig, BinancePriceStream, StreamType};
use dotenv::dotenv;
use env_logger::Env;
use log::info;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<()> {
    // Load environment variables
    dotenv().ok();

    // Initialize logger
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    info!("Starting Ethereum Uniswap swap listener");

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
                    info!("💰 ETH Price Updated: ${:.2}", price_update.price);
                }
            }
        }
    });

    // Create price store
    let price_store = PriceStore::new();

    // Create the listener
    let listener = EthSwapListener::new(config, price_store.clone()).await?;

    // Start a background task to log statistics periodically
    let stats_price_store = price_store.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            interval.tick().await;
            stats_price_store.log_stats();
        }
    });

    // Start listening to swap events
    info!("Starting swap event listener...");
    listener.start().await?;

    Ok(())
}
