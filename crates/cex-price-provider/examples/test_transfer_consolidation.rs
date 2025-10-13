/// Example demonstrating unified asset transfer consolidation across exchanges
///
/// This example shows how to use the unified transfer interface to:
/// 1. Transfer all assets to trading accounts (for trading)
/// 2. Transfer all assets to funding accounts (for withdrawal)
///
/// For exchanges without separate accounts (like MEXC), these are no-ops.

use cex_price_provider::{
    bybit::BybitService,
    kucoin::KucoinService,
    mexc::MexcService,
    PriceProvider,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("\n=== Asset Transfer Consolidation Example ===\n");

    // Initialize services (replace with your actual API keys)
    let bybit = BybitService::new(
        std::env::var("BYBIT_API_KEY").expect("BYBIT_API_KEY not set"),
        std::env::var("BYBIT_API_SECRET").expect("BYBIT_API_SECRET not set"),
        false, // testnet
    );

    let kucoin = KucoinService::new(
        std::env::var("KUCOIN_API_KEY").expect("KUCOIN_API_KEY not set"),
        std::env::var("KUCOIN_API_SECRET").expect("KUCOIN_API_SECRET not set"),
        std::env::var("KUCOIN_PASSPHRASE").expect("KUCOIN_PASSPHRASE not set"),
        false, // testnet
    );

    let mexc = MexcService::new(
        std::env::var("MEXC_API_KEY").expect("MEXC_API_KEY not set"),
        std::env::var("MEXC_API_SECRET").expect("MEXC_API_SECRET not set"),
    );

    // Example 1: Transfer all assets to trading accounts (prepare for trading)
    println!("=== Example 1: Transfer All Assets to Trading ===\n");

    println!("Bybit:");
    match bybit.transfer_all_to_trading(None).await {
        Ok(count) => println!("  ✓ Transferred {} assets\n", count),
        Err(e) => println!("  ✗ Error: {}\n", e),
    }

    println!("KuCoin:");
    match kucoin.transfer_all_to_trading(None).await {
        Ok(count) => println!("  ✓ Transferred {} assets\n", count),
        Err(e) => println!("  ✗ Error: {}\n", e),
    }

    println!("MEXC:");
    match mexc.transfer_all_to_trading(None).await {
        Ok(count) => println!("  ✓ Transferred {} assets (no-op for MEXC)\n", count),
        Err(e) => println!("  ✗ Error: {}\n", e),
    }

    // Example 2: Transfer specific coin to trading
    println!("\n=== Example 2: Transfer Specific Coin (LINK) to Trading ===\n");

    println!("Bybit:");
    match bybit.transfer_all_to_trading(Some("LINK")).await {
        Ok(count) => println!("  ✓ Transferred {} LINK\n", count),
        Err(e) => println!("  ✗ Error: {}\n", e),
    }

    println!("KuCoin:");
    match kucoin.transfer_all_to_trading(Some("LINK")).await {
        Ok(count) => println!("  ✓ Transferred {} LINK\n", count),
        Err(e) => println!("  ✗ Error: {}\n", e),
    }

    // Example 3: Transfer all assets to funding accounts (prepare for withdrawal)
    println!("\n=== Example 3: Transfer All Assets to Funding ===\n");

    println!("Bybit:");
    match bybit.transfer_all_to_funding(None).await {
        Ok(count) => println!("  ✓ Transferred {} assets\n", count),
        Err(e) => println!("  ✗ Error: {}\n", e),
    }

    println!("KuCoin:");
    match kucoin.transfer_all_to_funding(None).await {
        Ok(count) => println!("  ✓ Transferred {} assets\n", count),
        Err(e) => println!("  ✗ Error: {}\n", e),
    }

    println!("MEXC:");
    match mexc.transfer_all_to_funding(None).await {
        Ok(count) => println!("  ✓ Transferred {} assets (no-op for MEXC)\n", count),
        Err(e) => println!("  ✗ Error: {}\n", e),
    }

    // Example 4: Check portfolio after transfers
    println!("\n=== Example 4: Check Portfolio After Transfers ===\n");

    println!("Bybit Portfolio:");
    match bybit.get_portfolio().await {
        Ok(portfolio) => {
            println!("  Total USDT Value: ${:.2}", portfolio.total_usdt_value);
            println!("  Balances:");
            for balance in portfolio.balances {
                if balance.total > 0.0 {
                    println!("    {} {}: free={}, locked={}",
                        balance.asset, balance.total, balance.free, balance.locked);
                }
            }
            println!();
        }
        Err(e) => println!("  ✗ Error: {}\n", e),
    }

    println!("KuCoin Portfolio:");
    match kucoin.get_portfolio().await {
        Ok(portfolio) => {
            println!("  Total USDT Value: ${:.2}", portfolio.total_usdt_value);
            println!("  Balances:");
            for balance in portfolio.balances {
                if balance.total > 0.0 {
                    println!("    {} {}: free={}, locked={}",
                        balance.asset, balance.total, balance.free, balance.locked);
                }
            }
            println!();
        }
        Err(e) => println!("  ✗ Error: {}\n", e),
    }

    println!("\n=== Transfer Consolidation Complete ===\n");

    Ok(())
}
