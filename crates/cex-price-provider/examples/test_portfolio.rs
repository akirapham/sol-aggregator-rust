/// Example to test portfolio functionality for exchanges with authentication support
///
/// This example demonstrates:
/// 1. Getting portfolio balances from exchanges
/// 2. Displaying asset holdings and total value
///
/// Usage:
/// cargo run --example test_portfolio -p cex-price-provider
///
/// Required environment variables:
/// - MEXC_API_KEY, MEXC_API_SECRET
/// - BYBIT_API_KEY, BYBIT_API_SECRET
/// - KUCOIN_API_KEY, KUCOIN_API_SECRET, KUCOIN_API_PASSPHRASE
/// - BITGET_API_KEY, BITGET_API_SECRET, BITGET_API_PASSPHRASE
///
/// Note: Other exchanges (Gate.io) need with_credentials()
/// constructors implemented before they can be tested here.
use cex_price_provider::{
    bitget::BitgetService, bybit::BybitService, kucoin::KucoinService, mexc::MexcService,
    FilterAddressType, PriceProvider,
};
use dotenv::dotenv;
use std::env;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load environment variables
    dotenv().ok();

    // Initialize logging
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    log::info!("=== CEX Portfolio Test ===\n");

    // Test MEXC Portfolio
    if let (Ok(api_key), Ok(api_secret)) = (env::var("MEXC_API_KEY"), env::var("MEXC_API_SECRET")) {
        log::info!("Testing MEXC Portfolio...");
        let mexc_service = Arc::new(MexcService::with_credentials(
            FilterAddressType::Ethereum,
            api_key,
            api_secret,
        ));

        match mexc_service.get_portfolio().await {
            Ok(portfolio) => {
                log::info!("portfolio {:?}", portfolio);
                log::info!("✅ MEXC Portfolio:");
                log::info!("   Total USDT Value: ${:.2}", portfolio.total_usdt_value);
                log::info!(
                    "   Trading Account (same as funding): ${:.2}",
                    portfolio.trading.total_usdt_value
                );
                log::info!("   Assets:");
                for balance in portfolio.trading.balances.iter().take(10) {
                    log::info!(
                        "     {} - Free: {:.6}, Locked: {:.6}, Total: {:.6}",
                        balance.asset,
                        balance.free,
                        balance.locked,
                        balance.total
                    );
                }
                if portfolio.trading.balances.len() > 10 {
                    log::info!(
                        "     ... and {} more assets",
                        portfolio.trading.balances.len() - 10
                    );
                }
            }
            Err(e) => {
                log::error!("❌ Failed to get MEXC portfolio: {}", e);
            }
        }
    } else {
        log::warn!("⚠️  MEXC credentials not found, skipping portfolio test");
    }

    log::info!("");

    // Test Bybit Portfolio
    if let (Ok(api_key), Ok(api_secret)) = (env::var("BYBIT_API_KEY"), env::var("BYBIT_API_SECRET"))
    {
        log::info!("Testing Bybit Portfolio...");
        let bybit_service = Arc::new(BybitService::with_credentials(
            FilterAddressType::Ethereum,
            api_key,
            api_secret,
        ));

        match bybit_service.get_portfolio().await {
            Ok(portfolio) => {
                log::info!("✅ Bybit Portfolio:");
                log::info!("   Total USDT Value: ${:.2}", portfolio.total_usdt_value);
                log::info!(
                    "   Trading Account (UNIFIED): ${:.2}",
                    portfolio.trading.total_usdt_value
                );
                for balance in &portfolio.trading.balances {
                    log::info!(
                        "     {} - Free: {:.6}, Locked: {:.6}, Total: {:.6}",
                        balance.asset,
                        balance.free,
                        balance.locked,
                        balance.total
                    );
                }
                log::info!(
                    "   Funding Account (FUND): ${:.2}",
                    portfolio.funding.total_usdt_value
                );
                for balance in &portfolio.funding.balances {
                    log::info!(
                        "     {} - Free: {:.6}, Locked: {:.6}, Total: {:.6}",
                        balance.asset,
                        balance.free,
                        balance.locked,
                        balance.total
                    );
                }
            }
            Err(e) => {
                log::error!("❌ Failed to get Bybit portfolio: {}", e);
            }
        }
    } else {
        log::warn!("⚠️  Bybit credentials not found, skipping portfolio test");
    }

    log::info!("");

    // Test KuCoin Portfolio
    if let (Ok(api_key), Ok(api_secret), Ok(passphrase)) = (
        env::var("KUCOIN_API_KEY"),
        env::var("KUCOIN_API_SECRET"),
        env::var("KUCOIN_API_PASSPHRASE"),
    ) {
        log::info!("Testing KuCoin Portfolio...");
        let kucoin_service = Arc::new(KucoinService::with_credentials(
            FilterAddressType::Ethereum,
            api_key,
            api_secret,
            passphrase,
        ));

        match kucoin_service.get_portfolio().await {
            Ok(portfolio) => {
                log::info!("✅ KuCoin Portfolio:");
                log::info!("   Total USDT Value: ${:.2}", portfolio.total_usdt_value);
                log::info!(
                    "   Trading Account (trade + margin): ${:.2}",
                    portfolio.trading.total_usdt_value
                );
                for balance in &portfolio.trading.balances {
                    log::info!(
                        "     {} - Free: {:.6}, Locked: {:.6}, Total: {:.6}",
                        balance.asset,
                        balance.free,
                        balance.locked,
                        balance.total
                    );
                }
                log::info!(
                    "   Funding Account (main): ${:.2}",
                    portfolio.funding.total_usdt_value
                );
                for balance in &portfolio.funding.balances {
                    log::info!(
                        "     {} - Free: {:.6}, Locked: {:.6}, Total: {:.6}",
                        balance.asset,
                        balance.free,
                        balance.locked,
                        balance.total
                    );
                }
            }
            Err(e) => {
                log::error!("❌ Failed to get KuCoin portfolio: {}", e);
            }
        }
    } else {
        log::warn!("⚠️  KuCoin credentials not found, skipping portfolio test");
    }

    log::info!("");

    // Test Bitget Portfolio
    if let (Ok(api_key), Ok(api_secret), Ok(passphrase)) = (
        env::var("BITGET_API_KEY"),
        env::var("BITGET_API_SECRET"),
        env::var("BITGET_API_PASSPHRASE"),
    ) {
        log::info!("Testing Bitget Portfolio...");
        let bitget_service = Arc::new(BitgetService::with_credentials(
            FilterAddressType::Ethereum,
            api_key,
            api_secret,
            passphrase,
        ));

        match bitget_service.get_portfolio().await {
            Ok(portfolio) => {
                log::info!("✅ Bitget Portfolio:");
                log::info!("   Total USDT Value: ${:.2}", portfolio.total_usdt_value);
                log::info!(
                    "   Trading Account (same as funding): ${:.2}",
                    portfolio.trading.total_usdt_value
                );
                log::info!("   Assets:");
                for balance in portfolio.trading.balances.iter().take(10) {
                    log::info!(
                        "     {} - Free: {:.6}, Locked: {:.6}, Total: {:.6}",
                        balance.asset,
                        balance.free,
                        balance.locked,
                        balance.total
                    );
                }
                if portfolio.trading.balances.len() > 10 {
                    log::info!(
                        "     ... and {} more assets",
                        portfolio.trading.balances.len() - 10
                    );
                }
            }
            Err(e) => {
                log::error!("❌ Failed to get Bitget portfolio: {}", e);
            }
        }
    } else {
        log::warn!("⚠️  Bitget credentials not found, skipping portfolio test");
    }

    log::info!("\n=== Portfolio Test Complete ===");
    log::info!("\n📝 Note: Gate.io still needs with_credentials() constructor.");

    Ok(())
}
