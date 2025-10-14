use anyhow::Result;
use cex_price_provider::{
    bitget::{BitgetClient, BitgetService},
    bybit::{BybitClient, BybitService},
    gate::{GateClient, GateService},
    kucoin::{KucoinClient, KucoinService},
    mexc::MexcService,
    FilterAddressType, PriceProvider,
};
use std::env;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env file
    dotenv::dotenv().ok();

    env_logger::init();

    println!("\n🔢 CEX Quantity Precision Test\n");
    println!("This test retrieves the quantity precision (decimal places) for various tokens");
    println!(
        "on different exchanges. This is crucial for order placement to avoid 'quantity scale"
    );
    println!("is invalid' errors.\n");

    // Test tokens
    let test_symbols = vec![
        ("LINK", "Chainlink"),
        ("UNI", "Uniswap"),
        ("AAVE", "Aave"),
        ("WLFI", "World Liberty Financial"),
        ("PEPE", "Pepe"),
    ];

    println!("═══════════════════════════════════════════════════════════════");
    println!("Testing MEXC");
    println!("═══════════════════════════════════════════════════════════════\n");

    test_mexc(&test_symbols).await?;

    println!("\n═══════════════════════════════════════════════════════════════");
    println!("Testing Bybit");
    println!("═══════════════════════════════════════════════════════════════\n");

    test_bybit(&test_symbols).await?;

    println!("\n═══════════════════════════════════════════════════════════════");
    println!("Testing KuCoin");
    println!("═══════════════════════════════════════════════════════════════\n");

    test_kucoin(&test_symbols).await?;

    println!("\n═══════════════════════════════════════════════════════════════");
    println!("Testing Bitget");
    println!("═══════════════════════════════════════════════════════════════\n");

    test_bitget(&test_symbols).await?;

    println!("\n═══════════════════════════════════════════════════════════════");
    println!("Testing Gate.io");
    println!("═══════════════════════════════════════════════════════════════\n");

    test_gate(&test_symbols).await?;

    println!("\n═══════════════════════════════════════════════════════════════");
    println!("Summary");
    println!("═══════════════════════════════════════════════════════════════\n");
    println!("Quantity precision varies by exchange and token:");
    println!("• MEXC: Fetches from API (typically 2-8 decimals)");
    println!("• Bybit: Currently hardcoded to 2 decimals (TODO: fetch from API)");
    println!("• KuCoin: Currently hardcoded to 8 decimals (TODO: fetch from API)");
    println!("• Bitget: Currently hardcoded to 4 decimals (TODO: fetch from API)");
    println!("• Gate.io: Currently hardcoded to 8 decimals (TODO: fetch from API)");
    println!("\nWhen placing orders, quantities should be rounded to the specified precision:");
    println!("  rounded = (amount * 10^precision).floor() / 10^precision");

    Ok(())
}

async fn test_mexc(test_symbols: &[(&str, &str)]) -> Result<()> {
    let api_key = env::var("MEXC_API_KEY").ok();
    let api_secret = env::var("MEXC_API_SECRET").ok();

    let service = if let (Some(key), Some(secret)) = (api_key, api_secret) {
        println!("✓ Using authenticated MEXC API");
        Arc::new(MexcService::with_credentials(
            FilterAddressType::Ethereum,
            key,
            secret,
        ))
    } else {
        println!("⚠ No MEXC credentials found - using public API only");
        Arc::new(MexcService::new(FilterAddressType::Ethereum))
    };

    // Refresh token status first to populate precision cache
    println!("🔄 Refreshing token status to populate precision cache...\n");
    match service.refresh_token_status().await {
        Ok(_) => println!("✅ Token status refreshed\n"),
        Err(e) => {
            println!("❌ Failed to refresh: {}\n", e);
            return Ok(());
        }
    }

    test_precision_for_symbols(&*service, test_symbols).await;

    Ok(())
}

async fn test_bybit(test_symbols: &[(&str, &str)]) -> Result<()> {
    let api_key = env::var("BYBIT_API_KEY").ok();
    let api_secret = env::var("BYBIT_API_SECRET").ok();

    let service = if let (Some(key), Some(secret)) = (api_key, api_secret) {
        println!("✓ Using authenticated Bybit API");
        Arc::new(BybitService::with_credentials(
            FilterAddressType::Ethereum,
            key,
            secret,
        ))
    } else {
        println!("⚠ No Bybit credentials found - using public API only");
        Arc::new(BybitService::new(FilterAddressType::Ethereum))
    };

    test_precision_for_symbols(&*service, test_symbols).await;

    Ok(())
}

async fn test_kucoin(test_symbols: &[(&str, &str)]) -> Result<()> {
    let service = Arc::new(KucoinService::new(FilterAddressType::Ethereum));

    test_precision_for_symbols(&*service, test_symbols).await;

    Ok(())
}

async fn test_bitget(test_symbols: &[(&str, &str)]) -> Result<()> {
    let service = Arc::new(BitgetService::new(FilterAddressType::Ethereum));

    test_precision_for_symbols(&*service, test_symbols).await;

    Ok(())
}

async fn test_gate(test_symbols: &[(&str, &str)]) -> Result<()> {
    let service = Arc::new(GateService::new(FilterAddressType::Ethereum));

    test_precision_for_symbols(&*service, test_symbols).await;

    Ok(())
}

async fn test_precision_for_symbols<T: PriceProvider>(service: &T, symbols: &[(&str, &str)]) {
    for (symbol, name) in symbols {
        print!("📊 {} ({}): ", name, symbol);

        match service.get_quantity_precision(symbol).await {
            Ok(precision) => {
                println!("✅ {} decimal places", precision);

                // Show example rounding
                let example_amount = 123.456789;
                let divisor = 10_f64.powi(precision as i32);
                let rounded = (example_amount * divisor).floor() / divisor;
                println!(
                    "   Example: {:.8} → {:.8} (rounded to {} decimals)",
                    example_amount, rounded, precision
                );
            }
            Err(e) => {
                println!("❌ Error: {}", e);
            }
        }
    }
}
