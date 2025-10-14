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

    println!("\n🔍 CEX Token Safety Verification Test\n");
    println!(
        "This test verifies that tokens are tradeable AND depositable on the correct network."
    );
    println!("For Ethereum: Must support ERC20 deposits on Ethereum mainnet");
    println!("For Solana: Must support deposits on Solana network\n");

    // Test tokens - use actual tokens that would be traded, not stablecoins
    let link_eth_contract = "0x514910771af9ca656af840dff83e8264ecf986ca"; // LINK on Ethereum
    let uni_eth_contract = "0x1f9840a85d5af5bf1d1762f925bdaddc4201f984"; // UNI on Ethereum

    println!("═══════════════════════════════════════════════════════════════");
    println!("Testing MEXC");
    println!("═══════════════════════════════════════════════════════════════\n");

    test_mexc(link_eth_contract, uni_eth_contract).await?;

    println!("\n═══════════════════════════════════════════════════════════════");
    println!("Testing Bybit");
    println!("═══════════════════════════════════════════════════════════════\n");

    test_bybit(link_eth_contract, uni_eth_contract).await?;

    // println!("\n═══════════════════════════════════════════════════════════════");
    // println!("Testing KuCoin");
    // println!("═══════════════════════════════════════════════════════════════\n");

    // test_kucoin(link_eth_contract, uni_eth_contract).await?;

    println!("\n═══════════════════════════════════════════════════════════════");
    println!("Testing Bitget");
    println!("═══════════════════════════════════════════════════════════════\n");

    test_bitget(link_eth_contract, uni_eth_contract).await?;

    println!("\n═══════════════════════════════════════════════════════════════");
    println!("Testing Gate.io");
    println!("═══════════════════════════════════════════════════════════════\n");

    test_gate(link_eth_contract, uni_eth_contract).await?;

    Ok(())
}

async fn test_mexc(link_contract: &str, uni_contract: &str) -> Result<()> {
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
        println!("  Set MEXC_API_KEY and MEXC_API_SECRET for full verification");
        Arc::new(MexcService::new(FilterAddressType::Ethereum))
    };

    // Refresh token status
    println!("\n🔄 Refreshing token status...");
    match service.refresh_token_status().await {
        Ok(safe_symbols) => {
            println!(
                "✅ Successfully verified {} safe tokens",
                safe_symbols.len()
            );
        }
        Err(e) => {
            println!("❌ Failed to refresh token status: {}", e);
            return Ok(());
        }
    }

    // Test LINK
    println!("\n📊 Testing LINK ({})", link_contract);
    test_token(&*service, "LINK", Some(link_contract)).await;

    // Test UNI
    println!("\n📊 Testing UNI ({})", uni_contract);
    test_token(&*service, "UNI", Some(uni_contract)).await;

    // Test a problematic token (like SXP that was mentioned)
    println!("\n📊 Testing SXP (mentioned as having issues)");
    test_token(&*service, "SXP", None).await;

    Ok(())
}

async fn test_bybit(link_contract: &str, uni_contract: &str) -> Result<()> {
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
        println!("  Set BYBIT_API_KEY and BYBIT_API_SECRET for full verification");
        Arc::new(BybitService::new(FilterAddressType::Ethereum))
    };

    println!("\n🔄 Refreshing token status...");
    match service.refresh_token_status().await {
        Ok(safe_symbols) => {
            println!(
                "✅ Successfully verified {} safe tokens",
                safe_symbols.len()
            );
        }
        Err(e) => {
            println!("❌ Failed to refresh token status: {}", e);
            return Ok(());
        }
    }

    println!("\n📊 Testing LINK ({})", link_contract);
    test_token(&*service, "LINKUSDT", Some(link_contract)).await;

    println!("\n📊 Testing UNI ({})", uni_contract);
    test_token(&*service, "UNIUSDT", Some(uni_contract)).await;

    Ok(())
}

async fn test_gate(link_contract: &str, uni_contract: &str) -> Result<()> {
    let service = Arc::new(GateService::new(FilterAddressType::Ethereum));

    println!("\n🔄 Refreshing token status...");
    match service.refresh_token_status().await {
        Ok(safe_symbols) => {
            println!(
                "✅ Successfully verified {} safe tokens",
                safe_symbols.len()
            );
        }
        Err(e) => {
            println!("❌ Failed to refresh token status: {}", e);
            return Ok(());
        }
    }

    println!("\n📊 Testing LINK ({})", link_contract);
    test_token(&*service, "LINK_USDT", Some(link_contract)).await;

    println!("\n📊 Testing UNI ({})", uni_contract);
    test_token(&*service, "UNI_USDT", Some(uni_contract)).await;

    Ok(())
}

async fn test_kucoin(link_contract: &str, uni_contract: &str) -> Result<()> {
    let service = Arc::new(KucoinService::new(FilterAddressType::Ethereum));

    println!("\n🔄 Refreshing token status...");
    match service.refresh_token_status().await {
        Ok(safe_symbols) => {
            println!(
                "✅ Successfully verified {} safe tokens",
                safe_symbols.len()
            );
        }
        Err(e) => {
            println!("❌ Failed to refresh token status: {}", e);
            return Ok(());
        }
    }

    println!("\n📊 Testing LINK ({})", link_contract);
    test_token(&*service, "LINK-USDT", Some(link_contract)).await;

    println!("\n📊 Testing UNI ({})", uni_contract);
    test_token(&*service, "UNI-USDT", Some(uni_contract)).await;

    Ok(())
}

async fn test_bitget(link_contract: &str, uni_contract: &str) -> Result<()> {
    let service = Arc::new(BitgetService::new(FilterAddressType::Ethereum));

    println!("\n🔄 Refreshing token status...");
    match service.refresh_token_status().await {
        Ok(safe_symbols) => {
            println!(
                "✅ Successfully verified {} safe tokens",
                safe_symbols.len()
            );
        }
        Err(e) => {
            println!("❌ Failed to refresh token status: {}", e);
            return Ok(());
        }
    }

    println!("\n📊 Testing LINK ({})", link_contract);
    test_token(&*service, "LINKUSDT", Some(link_contract)).await;

    println!("\n📊 Testing UNI ({})", uni_contract);
    test_token(&*service, "UNIUSDT", Some(uni_contract)).await;

    Ok(())
}

async fn test_token<T: PriceProvider>(service: &T, symbol: &str, contract: Option<&str>) {
    // Get detailed status first
    let status = service.get_token_status(symbol, contract).await;

    // Check if token is safe for arbitrage
    let is_safe = service.is_token_safe_for_arbitrage(symbol, contract).await;

    if let Some(status) = status {
        println!("  Symbol: {}", status.symbol);
        if let Some(addr) = &status.contract_address {
            println!("  Contract: {}", addr);
        }
        println!(
            "  ✓ Trading enabled: {}",
            if status.is_trading { "✅" } else { "❌" }
        );
        println!(
            "  ✓ Deposits enabled: {}",
            if status.is_deposit_enabled {
                "✅"
            } else {
                "❌"
            }
        );
        println!(
            "  ✓ Network verified: {}",
            if status.network_verified {
                "✅"
            } else {
                "❌"
            }
        );

        // Final verdict
        println!(
            "\n  � Safe for arbitrage: {}",
            if is_safe { "✅" } else { "❌" }
        );

        if !is_safe {
            println!("     ⚠️  Reasons:");
            if !status.is_trading {
                println!("        • Trading is disabled");
            }
            if !status.is_deposit_enabled {
                println!("        • Deposits are disabled");
            }
            if !status.network_verified {
                println!("        • Wrong network or network not verified");
            }
        }
    } else {
        println!("  ❌ No status information available");
        println!(
            "  🎯 Safe for arbitrage: {}",
            if is_safe { "✅" } else { "❌" }
        );
    }
}
