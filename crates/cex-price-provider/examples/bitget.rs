use anyhow::Result;
use cex_price_provider::bitget::BitgetService;
use cex_price_provider::{FilterAddressType, PriceProvider};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .init();

    println!("Starting Bitget Service Example...");
    println!("This example subscribes to tokens with valid contract addresses\n");

    // Create Bitget service with Ethereum filter
    let bitget_service = BitgetService::new(FilterAddressType::Ethereum);

    println!("Starting Bitget WebSocket connections...");

    // Start the service in a background task
    let service_clone = std::sync::Arc::new(bitget_service);
    let service_for_start = service_clone.clone();

    tokio::spawn(async move {
        if let Err(e) = service_for_start.start().await {
            eprintln!("Bitget service error: {}", e);
        }
    });

    // Wait for prices to start coming in
    println!("Waiting for price data...");
    tokio::time::sleep(Duration::from_secs(10)).await;

    // Print some sample prices
    println!("\n=== Sample Prices from Bitget ===");
    let all_prices = service_clone.get_all_prices().await;

    println!("Total tokens with prices: {}", all_prices.len());

    // Show first 10 prices
    for (i, price) in all_prices.iter().take(10).enumerate() {
        println!("{}. {}: ${:.6}", i + 1, price.symbol, price.price);
    }

    // Test orderbook estimation
    println!("\n=== Orderbook Sell Estimation ===");
    
    // You would need to know the contract address to use this
    // For now, just demonstrate the API
    println!("Note: Use contract addresses as keys when available");

    // Keep running to observe live updates
    println!("\n=== Monitoring Live Price Updates ===");
    println!("Press Ctrl+C to exit...\n");

    let mut interval = tokio::time::interval(Duration::from_secs(30));
    loop {
        interval.tick().await;

        let price_count = service_clone.get_all_prices().await.len();
        println!("Current active prices: {}", price_count);

        // Show a few sample prices
        let all_prices = service_clone.get_all_prices().await;
        for price in all_prices.iter().take(3) {
            println!("  {}: ${:.6}", price.symbol, price.price);
        }
    }
}
