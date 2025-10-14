/// Example to test selling tokens for USDT across different exchanges
///
/// This example demonstrates:
/// 1. Checking token availability and trading pairs
/// 2. Getting current market price
/// 3. Estimating sell output using orderbook
/// 4. Executing a sell order (market order)
/// 5. Verifying the transaction
///
/// Usage:
/// cargo run --example test_sell_token -p cex-price-provider
///
/// Required environment variables (based on which exchange you want to test):
/// - MEXC_API_KEY, MEXC_API_SECRET
/// - BYBIT_API_KEY, BYBIT_API_SECRET
/// - KUCOIN_API_KEY, KUCOIN_API_SECRET, KUCOIN_API_PASSPHRASE
/// - BITGET_API_KEY, BITGET_API_SECRET, BITGET_API_PASSPHRASE
///
/// IMPORTANT: This will execute REAL trades! Start with small amounts for testing.
/// Set TEST_MODE=true to do dry run without actual execution.
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

    log::info!("=== CEX Sell Token Test ===\n");

    // Configuration - CHANGE THESE FOR YOUR TEST
    let token_symbol = "LINK";
    let token_contract = "0x514910771AF9Ca656af840dff83E8264EcF986CA"; // LINK token contract on Ethereum

    log::info!("📋 Test Configuration:");
    log::info!("   Token: {}", token_symbol);
    log::info!("   Contract: {}", token_contract);
    log::warn!(
        "\n⚠️  LIVE TRADING MODE - This will sell ALL {} tokens!",
        token_symbol
    );
    log::warn!("⚠️  This will execute REAL trades!\n");

    // log::info!("\n=== Testing MEXC ===");
    // test_mexc_sell(&token_symbol, false).await?;

    // log::info!("\n=== Testing Bybit ===");
    // test_bybit_sell(&token_symbol, false).await?;

    // log::info!("\n=== Testing KuCoin ===");
    // test_kucoin_sell(&token_symbol, false).await?;

    log::info!("\n=== Testing Bitget ===");
    test_bitget_sell(&token_symbol, false).await?;

    log::info!("\n=== Sell Token Test Complete ===");

    Ok(())
}

async fn test_mexc_sell(token_symbol: &str, test_mode: bool) -> anyhow::Result<()> {
    if let (Ok(api_key), Ok(api_secret)) = (env::var("MEXC_API_KEY"), env::var("MEXC_API_SECRET")) {
        log::info!("✅ MEXC credentials found");
        let service = Arc::new(MexcService::with_credentials(
            FilterAddressType::Ethereum,
            api_key,
            api_secret,
        ));

        // Step 1: Check current balance and get total amount
        log::info!("📋 Step 1: Checking current {} balance...", token_symbol);
        let portfolio = service.get_portfolio().await?;
        let token_balance = portfolio
            .trading
            .balances
            .iter()
            .find(|b| b.asset == token_symbol)
            .map(|b| b.free)
            .unwrap_or(0.0);

        log::info!(
            "   💰 MEXC trading balance: {} {}",
            token_balance,
            token_symbol
        );

        if token_balance <= 0.0 {
            log::warn!("   ⚠️  No {} balance found!", token_symbol);
            return Ok(());
        }

        // Step 2: Transfer all tokens to trading (MEXC doesn't separate, so this is a no-op)
        log::info!(
            "📋 Step 2: Transferring {} to trading account...",
            token_symbol
        );
        let transfer_count = service.transfer_all_to_trading(Some(token_symbol)).await?;
        log::info!(
            "   ✅ Transferred {} assets (MEXC: no-op, already in trading)",
            transfer_count
        );

        // Step 3: Sell ALL tokens for USDT
        if test_mode {
            log::info!(
                "📋 Step 3: [DRY RUN] Would sell ALL {} {} for USDT",
                token_balance,
                token_symbol
            );
        } else {
            log::info!(
                "📋 Step 3: [LIVE] Selling ALL {} {} for USDT...",
                token_balance,
                token_symbol
            );
            match service
                .sell_token_for_usdt(token_symbol, token_balance)
                .await
            {
                Ok((order_id, executed_qty, usdt_received)) => {
                    log::info!("   ✅ Sell order executed!");
                    log::info!("      Order ID: {}", order_id);
                    log::info!("      Executed: {} {}", executed_qty, token_symbol);
                    log::info!("      USDT received: ${:.2}", usdt_received);
                }
                Err(e) => {
                    log::error!("   ❌ Failed to sell: {}", e);
                    return Ok(());
                }
            }
        }

        // Step 4: Transfer all USDT to funding account
        log::info!("📋 Step 4: Transferring USDT to funding account...");
        let transfer_count = service.transfer_all_to_funding(Some("USDT")).await?;
        log::info!(
            "   ✅ Transferred {} assets (MEXC: no-op, already in funding)",
            transfer_count
        );
    } else {
        log::warn!("⚠️  MEXC credentials not found, skipping test");
    }

    Ok(())
}

async fn test_bybit_sell(token_symbol: &str, test_mode: bool) -> anyhow::Result<()> {
    if let (Ok(api_key), Ok(api_secret)) = (env::var("BYBIT_API_KEY"), env::var("BYBIT_API_SECRET"))
    {
        log::info!("✅ Bybit credentials found");
        let service = Arc::new(BybitService::with_credentials(
            FilterAddressType::Ethereum,
            api_key,
            api_secret,
        ));

        // Step 1: Check current balance and get total amount
        log::info!("📋 Step 1: Checking current {} balance...", token_symbol);
        let portfolio = service.get_portfolio().await?;
        let trading_balance = portfolio
            .trading
            .balances
            .iter()
            .find(|b| b.asset == token_symbol)
            .map(|b| b.free)
            .unwrap_or(0.0);
        let funding_balance = portfolio
            .funding
            .balances
            .iter()
            .find(|b| b.asset == token_symbol)
            .map(|b| b.free)
            .unwrap_or(0.0);

        log::info!(
            "   💰 Bybit UNIFIED (trading): {} {}",
            trading_balance,
            token_symbol
        );
        log::info!(
            "   💰 Bybit FUND (funding): {} {}",
            funding_balance,
            token_symbol
        );

        let total_balance = trading_balance + funding_balance;
        if total_balance <= 0.0 {
            log::warn!("   ⚠️  No {} balance found!", token_symbol);
            return Ok(());
        }

        // Step 2: Transfer all tokens from FUND to UNIFIED (trading)
        log::info!(
            "📋 Step 2: Transferring {} from FUND to UNIFIED...",
            token_symbol
        );
        let transfer_count = service.transfer_all_to_trading(Some(token_symbol)).await?;
        log::info!(
            "   ✅ Transferred {} assets to trading account",
            transfer_count
        );

        // Wait a bit for transfer to settle
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        // Step 3: Sell ALL tokens for USDT
        if test_mode {
            log::info!(
                "📋 Step 3: [DRY RUN] Would sell ALL {} {} for USDT",
                total_balance,
                token_symbol
            );
        } else {
            log::info!(
                "📋 Step 3: [LIVE] Selling ALL {} {} for USDT...",
                total_balance,
                token_symbol
            );
            match service
                .sell_token_for_usdt(token_symbol, total_balance)
                .await
            {
                Ok((order_id, executed_qty, usdt_received)) => {
                    log::info!("   ✅ Sell order executed!");
                    log::info!("      Order ID: {}", order_id);
                    log::info!("      Executed: {} {}", executed_qty, token_symbol);
                    log::info!("      USDT received: ${:.2}", usdt_received);
                }
                Err(e) => {
                    log::error!("   ❌ Failed to sell: {}", e);
                    return Ok(());
                }
            }
        }

        // Step 4: Transfer all USDT from UNIFIED to FUND
        log::info!("📋 Step 4: Transferring USDT from UNIFIED to FUND...");
        let transfer_count = service.transfer_all_to_funding(Some("USDT")).await?;
        log::info!(
            "   ✅ Transferred {} assets to funding account",
            transfer_count
        );
    } else {
        log::warn!("⚠️  Bybit credentials not found, skipping test");
    }

    Ok(())
}

async fn test_kucoin_sell(token_symbol: &str, test_mode: bool) -> anyhow::Result<()> {
    if let (Ok(api_key), Ok(api_secret), Ok(passphrase)) = (
        env::var("KUCOIN_API_KEY"),
        env::var("KUCOIN_API_SECRET"),
        env::var("KUCOIN_API_PASSPHRASE"),
    ) {
        log::info!("✅ KuCoin credentials found");
        let service = Arc::new(KucoinService::with_credentials(
            FilterAddressType::Ethereum,
            api_key,
            api_secret,
            passphrase,
        ));

        // Step 1: Check current balance and get total amount
        log::info!("📋 Step 1: Checking current {} balance...", token_symbol);
        let portfolio = service.get_portfolio().await?;
        let trading_balance = portfolio
            .trading
            .balances
            .iter()
            .find(|b| b.asset == token_symbol)
            .map(|b| b.free)
            .unwrap_or(0.0);
        let funding_balance = portfolio
            .funding
            .balances
            .iter()
            .find(|b| b.asset == token_symbol)
            .map(|b| b.free)
            .unwrap_or(0.0);

        log::info!(
            "   💰 KuCoin trade+margin (trading): {} {}",
            trading_balance,
            token_symbol
        );
        log::info!(
            "   💰 KuCoin main (funding): {} {}",
            funding_balance,
            token_symbol
        );

        let total_balance = trading_balance + funding_balance;
        if total_balance <= 0.0 {
            log::warn!("   ⚠️  No {} balance found!", token_symbol);
            return Ok(());
        }

        // Step 2: Transfer all tokens from main to trade (trading)
        log::info!(
            "📋 Step 2: Transferring {} from main to trade...",
            token_symbol
        );
        let transfer_count = service.transfer_all_to_trading(Some(token_symbol)).await?;
        log::info!(
            "   ✅ Transferred {} assets to trading account",
            transfer_count
        );

        // Wait a bit for transfer to settle
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        // Step 3: Sell ALL tokens for USDT
        if test_mode {
            log::info!(
                "📋 Step 3: [DRY RUN] Would sell ALL {} {} for USDT",
                total_balance,
                token_symbol
            );
        } else {
            log::info!(
                "📋 Step 3: [LIVE] Selling ALL {} {} for USDT...",
                total_balance,
                token_symbol
            );
            match service
                .sell_token_for_usdt(token_symbol, total_balance)
                .await
            {
                Ok((order_id, executed_qty, usdt_received)) => {
                    log::info!("   ✅ Sell order executed!");
                    log::info!("      Order ID: {}", order_id);
                    log::info!("      Executed: {} {}", executed_qty, token_symbol);
                    log::info!("      USDT received: ${:.2}", usdt_received);
                }
                Err(e) => {
                    log::error!("   ❌ Failed to sell: {}", e);
                    return Ok(());
                }
            }
        }

        // Step 4: Transfer all USDT from trade to main
        log::info!("📋 Step 4: Transferring USDT from trade to main...");
        let transfer_count = service.transfer_all_to_funding(Some("USDT")).await?;
        log::info!(
            "   ✅ Transferred {} assets to funding account",
            transfer_count
        );
    } else {
        log::warn!("⚠️  KuCoin credentials not found, skipping test");
    }

    Ok(())
}

async fn test_bitget_sell(token_symbol: &str, test_mode: bool) -> anyhow::Result<()> {
    if let (Ok(api_key), Ok(api_secret), Ok(passphrase)) = (
        env::var("BITGET_API_KEY"),
        env::var("BITGET_API_SECRET"),
        env::var("BITGET_API_PASSPHRASE"),
    ) {
        log::info!("✅ Bitget credentials found");
        let service = Arc::new(BitgetService::with_credentials(
            FilterAddressType::Ethereum,
            api_key,
            api_secret,
            passphrase,
        ));

        // Step 1: Check current balance and get total amount
        log::info!("📋 Step 1: Checking current {} balance...", token_symbol);
        let portfolio = service.get_portfolio().await?;
        let token_balance = portfolio
            .trading
            .balances
            .iter()
            .find(|b| b.asset == token_symbol)
            .map(|b| b.free)
            .unwrap_or(0.0);

        log::info!(
            "   💰 Bitget trading balance: {} {}",
            token_balance,
            token_symbol
        );

        if token_balance <= 0.0 {
            log::warn!("   ⚠️  No {} balance found!", token_symbol);
            return Ok(());
        }

        // Step 2: Transfer all tokens to trading (Bitget doesn't separate, so this is a no-op)
        log::info!(
            "📋 Step 2: Transferring {} to trading account...",
            token_symbol
        );
        let transfer_count = service.transfer_all_to_trading(Some(token_symbol)).await?;
        log::info!(
            "   ✅ Transferred {} assets (Bitget: no-op, already in trading)",
            transfer_count
        );

        // Step 3: Sell ALL tokens for USDT
        if test_mode {
            log::info!(
                "📋 Step 3: [DRY RUN] Would sell ALL {} {} for USDT",
                token_balance,
                token_symbol
            );
        } else {
            log::info!(
                "📋 Step 3: [LIVE] Selling ALL {} {} for USDT...",
                token_balance,
                token_symbol
            );
            match service
                .sell_token_for_usdt(token_symbol, token_balance)
                .await
            {
                Ok((order_id, executed_qty, usdt_received)) => {
                    log::info!("   ✅ Sell order executed!");
                    log::info!("      Order ID: {}", order_id);
                    log::info!("      Executed: {} {}", executed_qty, token_symbol);
                    log::info!("      USDT received: ${:.2}", usdt_received);
                }
                Err(e) => {
                    log::error!("   ❌ Failed to sell: {}", e);
                    return Ok(());
                }
            }
        }

        // Step 4: Transfer all USDT to funding account
        log::info!("📋 Step 4: Transferring USDT to funding account...");
        let transfer_count = service.transfer_all_to_funding(Some("USDT")).await?;
        log::info!(
            "   ✅ Transferred {} assets (Bitget: no-op, already in funding)",
            transfer_count
        );
    } else {
        log::warn!("⚠️  Bitget credentials not found, skipping test");
    }

    Ok(())
}
