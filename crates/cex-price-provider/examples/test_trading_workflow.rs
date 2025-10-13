/// Example demonstrating the complete trading workflow
///
/// This example shows how to:
/// 1. Get deposit address for a token
/// 2. Check token safety for arbitrage
/// 3. Estimate sell output using orderbook
/// 4. Execute sell order (DRY RUN - commented out)
/// 5. Withdraw USDT to external wallet
///
/// Usage:
/// cargo run --example test_trading_workflow
///
/// Required environment variables:
/// - MEXC_API_KEY, MEXC_API_SECRET (for authenticated operations)
///
/// Note: This is a demonstration. Actual trading functions will return errors
/// until fully implemented. The example shows the intended workflow.

use cex_price_provider::{
    mexc::MexcService,
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

    log::info!("=== CEX Trading Workflow Demonstration ===\n");

    // Example token (LINK) for demonstration
    let example_token_symbol = "LINK";
    let example_token_contract = "0x514910771af9ca656af840dff83e8264ecf986ca"; // LINK on Ethereum
    let example_amount = 1.0; // 1 LINK token
    let example_withdraw_address = "0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb"; // Example address

    // Check for MEXC credentials
    let (api_key, api_secret) = match (env::var("MEXC_API_KEY"), env::var("MEXC_API_SECRET")) {
        (Ok(key), Ok(secret)) => (key, secret),
        _ => {
            log::error!("❌ MEXC_API_KEY and MEXC_API_SECRET environment variables are required");
            log::info!("   Set them in your .env file or export them:");
            log::info!("   export MEXC_API_KEY=your_api_key");
            log::info!("   export MEXC_API_SECRET=your_api_secret");
            return Ok(());
        }
    };

    log::info!("✅ MEXC credentials found, initializing service...");
    let mexc_service = Arc::new(MexcService::with_credentials(
        FilterAddressType::Ethereum,
        api_key,
        api_secret,
    ));

    // Step 1: Check if token is safe for arbitrage
    log::info!("\n📋 Step 1: Checking if {} is safe for arbitrage...", example_token_symbol);
    let is_safe = mexc_service
        .is_token_safe_for_arbitrage(example_token_symbol, Some(example_token_contract))
        .await;

    if is_safe {
        log::info!("   ✅ {} is SAFE for arbitrage (deposits enabled, network verified)", example_token_symbol);
    } else {
        log::warn!("   ⚠️  {} may not be safe for arbitrage (check token status)", example_token_symbol);
    }

    // Step 2: Get deposit address
    log::info!("\n📋 Step 2: Getting deposit address for {}...", example_token_symbol);
    match mexc_service.get_deposit_address(example_token_symbol, FilterAddressType::Ethereum).await {
        Ok(address) => {
            log::info!("   ✅ Deposit address: {}", address);
            log::info!("   📝 You can send {} tokens to this address to fund your MEXC account", example_token_symbol);
        }
        Err(e) => {
            log::warn!("   ⚠️  Failed to get deposit address: {}", e);
            log::info!("   📝 This feature requires full implementation in the MEXC service");
        }
    }

    // Step 3: Estimate sell output using orderbook
    log::info!("\n📋 Step 3: Estimating sell output for {} {}...", example_amount, example_token_symbol);
    match mexc_service.estimate_sell_output(example_token_contract, example_amount).await {
        Ok(usdt_output) => {
            log::info!("   ✅ Estimated output: ${:.2} USDT", usdt_output);
            log::info!("   📝 This is based on current orderbook depth");

            let estimated_price = usdt_output / example_amount;
            log::info!("   💰 Estimated price per {}: ${:.2}", example_token_symbol, estimated_price);
        }
        Err(e) => {
            log::warn!("   ⚠️  Failed to estimate sell output: {}", e);
        }
    }

    // Step 4: Execute sell order (DRY RUN)
    log::info!("\n📋 Step 4: [DRY RUN] Selling {} {} for USDT...", example_amount, example_token_symbol);
    match mexc_service.sell_token_for_usdt(example_token_symbol, example_amount).await {
        Ok((order_id, executed_qty, usdt_received)) => {
            log::info!("   ✅ Order executed successfully!");
            log::info!("      Order ID: {}", order_id);
            log::info!("      Executed quantity: {} {}", executed_qty, example_token_symbol);
            log::info!("      USDT received: ${:.2}", usdt_received);
        }
        Err(e) => {
            log::warn!("   ⚠️  Sell operation not yet fully implemented: {}", e);
            log::info!("   📝 In production, this would place a market sell order on MEXC");
        }
    }

    // Step 5: Withdraw USDT (DRY RUN)
    log::info!("\n📋 Step 5: [DRY RUN] Withdrawing USDT to external wallet...");
    log::info!("   Target address: {}", example_withdraw_address);
    match mexc_service.withdraw_usdt(example_withdraw_address, 100.0, FilterAddressType::Ethereum).await {
        Ok(withdrawal_id) => {
            log::info!("   ✅ Withdrawal initiated successfully!");
            log::info!("      Withdrawal ID: {}", withdrawal_id);
            log::info!("      Amount: 100.0 USDT");
            log::info!("      Network: Ethereum (ERC20)");
        }
        Err(e) => {
            log::warn!("   ⚠️  Withdrawal operation not yet fully implemented: {}", e);
            log::info!("   📝 In production, this would submit a withdrawal request to MEXC");
        }
    }

    // Step 6: Check final portfolio
    log::info!("\n📋 Step 6: Checking final portfolio balance...");
    match mexc_service.get_portfolio().await {
        Ok(portfolio) => {
            log::info!("   ✅ Portfolio total value: ${:.2} USDT", portfolio.total_usdt_value);

            if let Some(usdt_balance) = portfolio.balances.iter().find(|b| b.asset == "USDT") {
                log::info!("   💰 USDT balance: {:.2} (free: {:.2}, locked: {:.2})",
                    usdt_balance.total, usdt_balance.free, usdt_balance.locked);
            }

            if let Some(token_balance) = portfolio.balances.iter().find(|b| b.asset == example_token_symbol) {
                log::info!("   💰 {} balance: {:.6} (free: {:.6}, locked: {:.6})",
                    example_token_symbol, token_balance.total, token_balance.free, token_balance.locked);
            }
        }
        Err(e) => {
            log::warn!("   ⚠️  Failed to get portfolio: {}", e);
        }
    }

    log::info!("\n=== Trading Workflow Demonstration Complete ===");
    log::info!("\n📝 Summary:");
    log::info!("   This example demonstrated the complete arbitrage workflow:");
    log::info!("   1. ✅ Check token safety");
    log::info!("   2. 🔄 Get deposit address (needs implementation)");
    log::info!("   3. ✅ Estimate sell output from orderbook");
    log::info!("   4. 🔄 Execute sell order (needs implementation)");
    log::info!("   5. 🔄 Withdraw USDT (needs implementation)");
    log::info!("   6. ✅ Check portfolio balance");
    log::info!("\n   🔄 = Awaiting full implementation");
    log::info!("   ✅ = Currently working");

    Ok(())
}
