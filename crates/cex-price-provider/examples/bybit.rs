use anyhow::Result;
use cex_price_provider::bybit::BybitService;
use cex_price_provider::{FilterAddressType, PriceProvider};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<()> {
    // Load environment variables from .env file
    dotenv::dotenv().ok();

    // Initialize logging
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .init();

    println!("Starting Bybit Service Example with Authentication...");
    println!("This example uses API credentials to filter by contract addresses");
    println!("Prices will be cached by contract address instead of symbol\n");

    // Get API credentials from environment variables
    let api_key =
        std::env::var("BYBIT_API_KEY").expect("BYBIT_API_KEY environment variable not set");
    let api_secret =
        std::env::var("BYBIT_API_SECRET").expect("BYBIT_API_SECRET environment variable not set");

    println!("Using API Key: {}...", &api_key[..8]);

    // First, let's test the client directly to debug the API response
    println!("\n=== Testing Bybit Client API ===");
    let client = cex_price_provider::bybit::client::BybitClient::with_credentials(
        FilterAddressType::Ethereum,
        api_key.clone(),
        api_secret.clone(),
    );

    // Test coin info fetching
    println!("Testing coin info API call...");
    match client.get_coin_info(Some("BTC")).await {
        Ok(coin_info) => {
            println!("✅ Successfully fetched BTC coin info");
            println!(
                "RetCode: {}, RetMsg: {}",
                coin_info.ret_code, coin_info.ret_msg
            );
            println!("Number of coins returned: {}", coin_info.result.len());

            for coin in &coin_info.result {
                println!("Coin: {} ({})", coin.name, coin.coin);
                for chain in &coin.chains {
                    if !chain.contract_address.is_empty() {
                        println!(
                            "  {} ({}): {}",
                            chain.chain_type, chain.chain, chain.contract_address
                        );
                    }
                }
            }
        }
        Err(e) => {
            println!("❌ Failed to fetch BTC coin info: {}", e);
            println!("This suggests API credentials or permissions issue");
            return Err(e);
        }
    }

    // Create Bybit service with authentication - enables contract address filtering
    let bybit_service =
        BybitService::with_credentials(FilterAddressType::Ethereum, api_key, api_secret);

    println!("\n=== Starting Bybit WebSocket Service ===");

    println!("Starting Bybit WebSocket connections...");

    // Start the service in a background task
    let service_clone = std::sync::Arc::new(bybit_service);
    let service_for_start = service_clone.clone();

    tokio::spawn(async move {
        if let Err(e) = service_for_start.start().await {
            eprintln!("Bybit service error: {}", e);
        }
    });

    // Wait a bit for prices to start coming in
    println!("Waiting for price data...");
    tokio::time::sleep(Duration::from_secs(10)).await;

    // Print some sample prices
    println!("\n=== Sample Prices from Bybit (by Contract Address) ===");
    let all_prices = service_clone.get_all_prices().await;

    println!("Total tokens with prices: {}", all_prices.len());

    // Show first 10 prices
    for (i, price) in all_prices.iter().take(10).enumerate() {
        println!("{}. {}: ${:.6}", i + 1, price.symbol, price.price);
    }

    // Test getting specific token price by contract address
    println!("\n=== Specific Token Queries (by Contract Address) ===");

    // Note: You'll need to know the contract addresses for this to work
    // You can get them from the bybit_auth example first
    println!("Note: Use contract addresses as keys (not symbols) when authenticated");
    println!("Run 'bybit_auth' example first to see available contract addresses");

    // Test orderbook estimation (still uses symbol for now)
    println!("\n=== Orderbook Sell Estimation ===");
    match service_clone.estimate_sell_output("BTC", 0.1).await {
        Ok(usdt_amount) => {
            println!(
                "Selling 0.1 BTC would get approximately: ${:.2} USDT",
                usdt_amount
            );
        }
        Err(e) => {
            println!("Failed to estimate sell output for BTC: {}", e);
        }
    }

    // Keep running to observe live updates
    println!("\n=== Monitoring Live Price Updates ===");
    println!("Press Ctrl+C to exit...\n");

    let mut interval = tokio::time::interval(Duration::from_secs(30));
    loop {
        interval.tick().await;

        let price_count = service_clone.get_all_prices().await.len();
        println!("Current active prices: {}", price_count);

        // Show a few random prices
        if let Some(btc_price) = service_clone.get_price("btc").await {
            println!("  BTC: ${:.2}", btc_price.price);
        }
        if let Some(eth_price) = service_clone.get_price("eth").await {
            println!("  ETH: ${:.2}", eth_price.price);
        }
    }
}
