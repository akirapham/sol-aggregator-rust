use anyhow::Result;
use cex_price_provider::kucoin::KucoinService;
use cex_price_provider::{FilterAddressType, PriceProvider};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .init();

    println!("Starting KuCoin Service Example with Contract Address Filtering...");
    println!("Note: KuCoin provides contract addresses via PUBLIC API (no auth needed!)");
    println!("Prices will be cached by contract address\n");

    // Create KuCoin service - no credentials needed!
    let kucoin_service = KucoinService::new(FilterAddressType::Ethereum);

    println!("\n=== Starting KuCoin WebSocket Service ===");
    println!("Fetching contract addresses and starting WebSocket connections...");

    // Start the service in a background task
    let service_clone = std::sync::Arc::new(kucoin_service);
    let service_for_start = service_clone.clone();

    tokio::spawn(async move {
        if let Err(e) = service_for_start.start().await {
            eprintln!("KuCoin service error: {}", e);
        }
    });

    // Wait a bit for prices to start coming in
    println!("Waiting for price data...");
    tokio::time::sleep(Duration::from_secs(15)).await;

    // Print some sample prices
    println!("\n=== Sample Prices from KuCoin (by Contract Address) ===");
    let all_prices = service_clone.get_all_prices().await;

    println!("Total tokens with prices: {}", all_prices.len());

    // Show first 10 prices
    for (i, price) in all_prices.iter().take(10).enumerate() {
        println!("{}. {}: ${:.6}", i + 1, price.symbol, price.price);
    }

    // Test getting specific token price by contract address
    println!("\n=== Specific Token Queries (by Contract Address) ===");
    println!("Note: Use contract addresses as keys for queries");

    // Test orderbook estimation
    println!("\n=== Orderbook Sell Estimation ===");
    println!("Note: This requires knowing the contract address");

    // Keep running to observe live updates
    println!("\n=== Monitoring Live Price Updates ===");
    println!("Press Ctrl+C to exit...\n");

    let mut interval = tokio::time::interval(Duration::from_secs(30));
    loop {
        interval.tick().await;

        let price_count = service_clone.get_all_prices().await.len();
        println!("Current active prices: {}", price_count);

        // Show a few sample prices
        let all = service_clone.get_all_prices().await;
        for price in all.iter().take(3) {
            println!("  {}: ${:.6}", price.symbol, price.price);
        }
    }
}
