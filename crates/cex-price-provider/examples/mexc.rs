use anyhow::Result;
use cex_price_provider::{mexc::MexcService, PriceProvider};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    // Load environment variables
    dotenv::dotenv().ok();

    // Initialize logging
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    println!("🚀 Starting MEXC Price Service Example");
    println!("📊 Filtering for Ethereum tokens with deposits enabled\n");

    // Create MEXC service with credentials if available
    let mexc = match (
        std::env::var("MEXC_API_KEY"),
        std::env::var("MEXC_API_SECRET"),
    ) {
        (Ok(api_key), Ok(api_secret)) => {
            println!("✅ MEXC API credentials found - deposit filtering ENABLED");
            Arc::new(MexcService::with_credentials(
                cex_price_provider::FilterAddressType::Ethereum,
                api_key,
                api_secret,
            ))
        }
        _ => {
            println!("⚠️  MEXC API credentials NOT found - deposit filtering DISABLED");
            println!("   Set MEXC_API_KEY and MEXC_API_SECRET environment variables to enable");
            println!();
            Arc::new(MexcService::new(
                cex_price_provider::FilterAddressType::Ethereum,
            ))
        }
    };

    // Start the service in background
    let mexc_clone = mexc.clone();
    tokio::spawn(async move {
        if let Err(e) = mexc_clone.start().await {
            eprintln!("❌ MEXC service error: {}", e);
        }
    });

    // Wait for service to initialize
    println!("⏳ Waiting 15 seconds for MEXC service to initialize...");
    tokio::time::sleep(tokio::time::Duration::from_secs(15)).await;

    // Test fetching prices for some popular tokens
    let test_contracts = vec![
        ("USDC", "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48"),
        ("WETH", "0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2"),
        ("LINK", "0x514910771af9ca656af840dff83e8264ecf986ca"),
        ("UNI", "0x1f9840a85d5af5bf1d1762f925bdaddc4201f984"),
    ];

    println!("\n📈 Testing price fetching for popular tokens:");
    println!("{:-<70}", "");

    for (name, contract) in test_contracts {
        match mexc.get_price(contract).await {
            Some(price_info) => {
                println!(
                    "✅ {} ({}) - Price: ${:.6} | Symbol: {}",
                    name, contract, price_info.price, price_info.symbol
                );
            }
            None => {
                println!("⚠️  {} ({}) - No price data available", name, contract);
            }
        }
    }

    println!("\n{:-<70}", "");
    println!("✨ MEXC service is running. Press Ctrl+C to stop.");
    println!("{:-<70}\n", "");

    // Keep the service running
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
        println!("💓 Service still running... (prices updating in real-time)");
    }
}
