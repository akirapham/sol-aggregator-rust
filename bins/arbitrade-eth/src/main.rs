use anyhow::Result;
use axum::Router;
use cex_price_provider::bitget::BitgetService;
use cex_price_provider::bybit::BybitService;
use cex_price_provider::gate::GateService;
use cex_price_provider::kucoin::KucoinService;
use cex_price_provider::mexc::MexcService;
use cex_price_provider::PriceProvider;
use dashmap::DashMap;
use dotenv::dotenv;
use log::{error, info};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tower_http::cors::CorsLayer;
use tracing_subscriber;
mod api;
mod arbitrage_api;
mod auth;
mod dashboard;
mod db;
mod dex_price;
mod kyber;
mod types;
use db::{ArbitrageDb, ArbitrageOpportunity};
use dex_price::{DexPriceClient, DexPriceConfig};
use kyber::{client::RouteSummary, KyberClient};
use std::env;

use crate::types::TokenPriceUpdate;

struct CexProvider {
    name: &'static str,
    service: Arc<dyn PriceProvider + Send + Sync>,
    // Store concrete service types for orderbook access
    mexc: Option<Arc<MexcService>>,
    bybit: Option<Arc<BybitService>>,
    kucoin: Option<Arc<KucoinService>>,
    bitget: Option<Arc<BitgetService>>,
    gate: Option<Arc<GateService>>,
    // Cached deposit address for this CEX (for Ethereum/ERC20 tokens)
    deposit_address: Option<String>,
}

/// Structure to hold CEX opportunity with liquidity info
struct CexOpportunity {
    cex_name: String,
    cex_price: f64,
    cex_symbol: String,
    price_diff_percent: f64,
    liquidity_usdt: f64, // Total USDT liquidity in orderbook
}

impl CexProvider {
    /// Get the minimum confirmation time estimate for deposits (in seconds)
    /// Based on exchange-specific confirmation requirements for ERC20 tokens
    fn get_min_deposit_time(&self) -> u64 {
        // Ethereum block time is ~12-15 seconds
        // Adding 50% buffer for network congestion
        const BLOCK_TIME_SECONDS: u64 = 13; // Conservative estimate
        const BUFFER_MULTIPLIER: f64 = 1.0;

        let confirmations = match self.name {
            "MEXC" => 16,    // MEXC requires 16 confirmations
            "Bybit" => 6,    // Bybit typically 12 confirmations
            "KuCoin" => 12,  // KuCoin typically 12 confirmations
            "Bitget" => 12,  // Bitget typically 12 confirmations
            "Gate.io" => 12, // Gate.io typically 12 confirmations
            _ => 12,         // Default to 12
        };

        let base_time = confirmations * BLOCK_TIME_SECONDS;
        (base_time as f64 * BUFFER_MULTIPLIER) as u64
    }

    /// Calculate total orderbook liquidity (bid side) in USDT
    async fn get_orderbook_liquidity(
        &self,
        token_contract: &str,
        token_amount: f64,
    ) -> Option<f64> {
        // Try to estimate sell output which uses orderbook depth
        if let Some(mexc) = &self.mexc {
            return mexc
                .estimate_sell_output(token_contract, token_amount)
                .await
                .ok();
        }
        if let Some(bybit) = &self.bybit {
            return bybit
                .estimate_sell_output(token_contract, token_amount)
                .await
                .ok();
        }
        if let Some(kucoin) = &self.kucoin {
            return kucoin
                .estimate_sell_output(token_contract, token_amount)
                .await
                .ok();
        }
        if let Some(bitget) = &self.bitget {
            return bitget
                .estimate_sell_output(token_contract, token_amount)
                .await
                .ok();
        }
        if let Some(gate) = &self.gate {
            return gate
                .estimate_sell_output(token_contract, token_amount)
                .await
                .ok();
        }
        None
    }

    /// Get the deposit address for this CEX
    /// Returns the cached deposit address if available
    fn get_deposit_address(&self) -> Option<&str> {
        self.deposit_address.as_deref()
    }
}

/// Structure to hold the best arbitrage opportunity across CEXes
struct BestArbitrageOpportunity {
    cex_name: String,
    cex_price: f64,
    cex_symbol: String,
    price_diff_percent: f64,
    liquidity_usdt: f64,
    usdt_from_cex: f64,
    profit: f64,
    profit_percent: f64,
}

/// Price trend direction for intelligent selling decisions
#[derive(Debug, Clone, Copy, PartialEq)]
enum PriceTrend {
    Rising,  // Price is going up
    Falling, // Price is going down
    Stable,  // Price is relatively stable
}

/// Detect price trend from recent price history
/// Uses simple linear regression slope to determine trend
fn detect_price_trend(prices: &[f64]) -> PriceTrend {
    if prices.len() < 3 {
        return PriceTrend::Stable;
    }

    // Calculate simple moving average slope
    let n = prices.len() as f64;
    let x_mean = (n - 1.0) / 2.0; // Mean of indices 0, 1, 2, ...
    let y_mean: f64 = prices.iter().sum::<f64>() / n;

    let mut numerator = 0.0;
    let mut denominator = 0.0;

    for (i, &price) in prices.iter().enumerate() {
        let x_diff = i as f64 - x_mean;
        let y_diff = price - y_mean;
        numerator += x_diff * y_diff;
        denominator += x_diff * x_diff;
    }

    let slope = if denominator != 0.0 {
        numerator / denominator
    } else {
        0.0
    };

    // Determine trend based on slope
    // Threshold: if slope change is more than 0.1% per data point, consider it a trend
    let price_avg = prices.iter().sum::<f64>() / n;
    let slope_threshold = price_avg * 0.001; // 0.1% threshold

    if slope > slope_threshold {
        PriceTrend::Rising
    } else if slope < -slope_threshold {
        PriceTrend::Falling
    } else {
        PriceTrend::Stable
    }
}

/// Execute real arbitrage opportunity with actual swap transaction
/// Takes pre-fetched route data to avoid redundant API calls
async fn process_arbitrage(
    kyber_client: &KyberClient,
    best_cex: &CexProvider,
    update: &TokenPriceUpdate,
    arb_amount_usdt: f64,
    gas_fee_usd: f64,
    tokens_from_dex: f64,
    route_summary: kyber::client::RouteSummary,
    db: &Arc<ArbitrageDb>,
) {
    log::info!(
        "🎯 Processing arbitrage for token: {} with best CEX: {}",
        update.token_address,
        best_cex.name
    );

    // CRITICAL SAFETY CHECK: Verify token is safe for arbitrage on this CEX
    // This ensures:
    // 1. Token is tradeable on the CEX
    // 2. Deposits are enabled for the correct network (ERC20 for Ethereum)
    // 3. Network is verified to match our requirements
    let is_safe = best_cex
        .service
        .is_token_safe_for_arbitrage(
            &update.token_address.to_lowercase(),
            Some(&update.token_address),
        )
        .await;

    if !is_safe {
        log::warn!(
            "⚠️  {} - Token {} is NOT SAFE for arbitrage (trading disabled, deposits disabled, or wrong network). ABORTING.",
            best_cex.name,
            update.token_address
        );
        return;
    }

    let token_symbol = best_cex
        .service
        .get_token_symbol_for_contract_address(&update.token_address)
        .await
        .unwrap_or_else(|| update.token_address.clone());
    log::info!(
        "✅ {} - Token {}({}) verified safe for arbitrage",
        best_cex.name,
        token_symbol,
        update.token_address
    );

    // Check if we have a deposit address for this CEX
    let deposit_address = match best_cex.get_deposit_address() {
        Some(addr) => addr,
        None => {
            log::warn!(
                "⚠️  {} - No deposit address available. Cannot execute arbitrage. Please configure API credentials.",
                best_cex.name
            );
            return;
        }
    };

    log::info!(
        "✅ Using {} deposit address as recipient: {}",
        best_cex.name,
        deposit_address
    );

    // EXECUTE REAL SWAP: Use pre-fetched route data from screening phase
    log::info!(
        "📊 Using swap route: {} USDT → {:.6} tokens (gas: ${:.2})",
        arb_amount_usdt,
        tokens_from_dex,
        gas_fee_usd
    );

    // Execute the swap with CEX deposit address as recipient
    log::info!(
        "🚀 Executing swap transaction to {} deposit address...",
        best_cex.name
    );
    let tx_hash = match kyber_client
        .execute_swap(
            &route_summary,
            deposit_address,
            50, // 0.5% slippage tolerance
        )
        .await
    {
        Ok(hash) => {
            log::info!("✅ Swap transaction successful! TX: {}", hash);
            hash
        }
        Err(e) => {
            log::error!("❌ Swap transaction failed: {}", e);
            return;
        }
    };

    log::info!(
        "⏳ Swap executed! Tokens should arrive at {} soon.",
        best_cex.name
    );
    log::info!("   Transaction: https://etherscan.io/tx/{}", tx_hash);

    // Step 2: Wait for deposit confirmation on CEX by polling portfolio
    // Calculate expected minimum wait time based on exchange confirmation requirements
    let min_deposit_time = best_cex.get_min_deposit_time();
    let max_wait_time = (min_deposit_time * 3).max(600); // At least 3x min time or 10 minutes

    log::info!(
        "⏳ Waiting for token deposit to be confirmed on {}...",
        best_cex.name
    );
    log::info!(
        "   {} requires confirmations (est. {} seconds minimum)",
        best_cex.name,
        min_deposit_time
    );
    log::info!(
        "   Max wait time: {} seconds (~{} minutes)",
        max_wait_time,
        max_wait_time / 60
    );
    log::info!(
        "   Checking portfolio every 5 seconds for {} ({})...",
        token_symbol,
        update.token_address
    );

    let mut deposited_token_amount: f64 = 0.0;
    let mut elapsed_time = 0;
    let mut first_check_done = false;

    loop {
        // Don't spam the API immediately - wait at least until minimum confirmation time
        if !first_check_done && elapsed_time < min_deposit_time {
            log::debug!(
                "Waiting for minimum confirmation time... ({}/{} seconds)",
                elapsed_time,
                min_deposit_time
            );
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            elapsed_time += 5;
            continue;
        }
        first_check_done = true;

        // Check portfolio for the deposited token
        match best_cex.service.get_portfolio().await {
            Ok(portfolio) => {
                // Check both trading and funding accounts for the token
                // NOTE: MEXC and Bitget don't have separate accounts, so they return the same balances
                // To avoid double-counting, we need to handle this carefully
                let mut found_amount = 0.0;
                let mut trading_amount = 0.0;
                let mut funding_amount = 0.0;

                // Check trading account
                for balance in &portfolio.trading.balances {
                    if balance.asset.eq_ignore_ascii_case(&token_symbol) {
                        trading_amount = balance.free;
                        log::debug!(
                            "Found {:.6} {} in trading account",
                            balance.free,
                            token_symbol
                        );
                    }
                }

                // Check funding account
                for balance in &portfolio.funding.balances {
                    if balance.asset.eq_ignore_ascii_case(&token_symbol) {
                        funding_amount = balance.free;
                        log::debug!(
                            "Found {:.6} {} in funding account",
                            balance.free,
                            token_symbol
                        );
                    }
                }

                // For MEXC and Bitget: They don't have separate accounts, so trading = funding
                // If both amounts are exactly the same, it means they're the same account (don't double count)
                if trading_amount == funding_amount && trading_amount > 0.0 {
                    found_amount = trading_amount; // Use one, not both
                    log::debug!(
                        "{} has unified accounts (no separation). Using single balance: {:.6} {}",
                        best_cex.name,
                        found_amount,
                        token_symbol
                    );
                } else {
                    // For exchanges with separate accounts (Bybit, KuCoin, Gate.io): Add both
                    found_amount = trading_amount + funding_amount;
                    if trading_amount > 0.0 && funding_amount > 0.0 {
                        log::debug!(
                            "{} has separate accounts. Total: {:.6} {} (trading: {:.6}, funding: {:.6})",
                            best_cex.name,
                            found_amount,
                            token_symbol,
                            trading_amount,
                            funding_amount
                        );
                    }
                }

                // Check if we received at least 99% of the expected token amount
                let expected_minimum = tokens_from_dex * 0.99;

                if found_amount >= expected_minimum {
                    deposited_token_amount = found_amount;
                    let received_percent = (found_amount / tokens_from_dex) * 100.0;
                    log::info!(
                        "✅ Token deposit confirmed! Found {:.6} {} on {} ({:.2}% of expected {:.6})",
                        deposited_token_amount,
                        token_symbol,
                        best_cex.name,
                        received_percent,
                        tokens_from_dex
                    );
                    break;
                } else if found_amount > 0.0 {
                    let received_percent = (found_amount / tokens_from_dex) * 100.0;
                    log::warn!(
                        "⚠️  Found {:.6} {} but expecting at least {:.6} ({:.2}% received, need 99%+). Waiting...",
                        found_amount,
                        token_symbol,
                        expected_minimum,
                        received_percent
                    );
                } else {
                    log::debug!(
                        "Token not yet visible in portfolio, waiting 5 seconds... ({}/{}s elapsed)",
                        elapsed_time,
                        max_wait_time
                    );
                }
            }
            Err(e) => {
                log::warn!("Failed to get portfolio from {}: {}", best_cex.name, e);
            }
        }

        if elapsed_time >= max_wait_time {
            log::error!(
                "❌ Timeout waiting for token deposit on {}. Aborting arbitrage.",
                best_cex.name
            );
            return;
        }

        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
        elapsed_time += 5;
    }

    // Step 3: Execute sell workflow - Transfer to trading → Sell → Transfer USDT to funding
    log::info!("💱 Starting sell workflow on {}...", best_cex.name);

    // 3.1: Transfer all tokens to trading account
    log::info!(
        "📤 Transferring {:.6} {} to trading account...",
        deposited_token_amount,
        token_symbol
    );
    match best_cex
        .service
        .transfer_all_to_trading(Some(&token_symbol))
        .await
    {
        Ok(transferred_count) => {
            if transferred_count > 0 {
                log::info!(
                    "✅ Transferred {} tokens to trading account",
                    transferred_count
                );
            } else {
                log::debug!("Token already in trading account or no transfer needed");
            }
        }
        Err(e) => {
            log::warn!(
                "Transfer to trading failed (may already be in trading): {}",
                e
            );
        }
    }

    // Wait a bit for transfer to settle
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    // 3.2: Get quantity precision and round the amount before selling
    let quantity_precision = match best_cex.service.get_quantity_precision(&token_symbol).await {
        Ok(precision) => {
            log::debug!(
                "{}: Quantity precision for {} is {} decimals",
                best_cex.name,
                token_symbol,
                precision
            );
            precision
        }
        Err(e) => {
            log::warn!(
                "Failed to get quantity precision for {}, using default 8: {}",
                token_symbol,
                e
            );
            8 // Default to 8 decimal places
        }
    };

    // Round the quantity to the correct precision
    let divisor = 10_f64.powi(quantity_precision as i32);
    let rounded_amount = (deposited_token_amount * divisor).floor() / divisor;

    // 3.3: Smart profit maximization with price trend tracking
    log::info!("📊 Starting intelligent price monitoring for optimal sell timing...");
    let total_cost = arb_amount_usdt + gas_fee_usd;
    let mut estimated_usdt_output: f64 = 0.0;

    // Price history for trend analysis (timestamp, estimated_output)
    let mut price_history: Vec<(u64, f64)> = Vec::new();

    // Strategy parameters - OPTIMIZED FOR ARBITRAGE
    let stop_loss_threshold = total_cost * 0.95; // Max 5% loss (sell immediately if price drops this low)

    // Tiered timeout based on initial opportunity quality
    // Shorter base timeout = faster capital rotation = more trades per day
    let base_timeout = 300; // 5 minutes - arbitrage should be quick!
    let extended_timeout = 600; // 10 minutes - only if recovering well
    let aggressive_timeout = 180; // 3 minutes - if already profitable

    let mut max_wait_time = base_timeout;
    let mut price_wait_elapsed = 0;
    let check_interval = 15; // Check every 15 seconds
    let mut first_check = true; // Track if this is our first price check

    log::info!(
        "Strategy: Break-even @ ${:.2}, Stop-loss @ ${:.2} (5% max loss), Base timeout: {}s",
        total_cost,
        stop_loss_threshold,
        base_timeout
    );

    loop {
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Estimate how much USDT we'll get from selling
        match best_cex
            .get_orderbook_liquidity(&update.token_address.to_lowercase(), rounded_amount)
            .await
        {
            Some(estimated_output) => {
                estimated_usdt_output = estimated_output;
                let estimated_profit = estimated_output - total_cost;
                let estimated_profit_percent = (estimated_profit / total_cost) * 100.0;

                // Record price point for trend analysis
                price_history.push((current_time, estimated_output));

                // Keep only last 5 data points (last ~1 minute of data)
                if price_history.len() > 5 {
                    price_history.remove(0);
                }

                // OPTIMIZED DECISION LOGIC FOR ARBITRAGE:

                // 1. STOP-LOSS: If price dropped too much, sell immediately to prevent further loss
                if estimated_output < stop_loss_threshold {
                    log::error!(
                        "🛑 STOP-LOSS TRIGGERED! Price dropped to ${:.2} (below ${:.2} threshold). Selling immediately to prevent further loss!",
                        estimated_output,
                        stop_loss_threshold
                    );
                    break;
                }

                // 2. STRONG PROFIT (>2%): If already very profitable on first check, use aggressive timeout
                if first_check && estimated_profit_percent > 2.0 {
                    max_wait_time = aggressive_timeout; // 3 minutes - don't wait, this is a great arb!
                    log::info!(
                        "🚀 STRONG ARBITRAGE DETECTED! {:.2}% profit. Using aggressive {}s timeout.",
                        estimated_profit_percent,
                        aggressive_timeout
                    );
                }
                first_check = false;

                // 3. PROFITABLE: Price is profitable
                if estimated_profit > 0.0 {
                    // Analyze price trend if we have enough history
                    if price_history.len() >= 3 {
                        let recent_prices: Vec<f64> =
                            price_history.iter().map(|(_, p)| *p).collect();
                        let trend = detect_price_trend(&recent_prices);

                        match trend {
                            PriceTrend::Rising => {
                                // Only extend timeout if we're early in the cycle and profit is decent
                                if price_wait_elapsed < 120 && estimated_profit_percent > 0.5 {
                                    max_wait_time = extended_timeout; // Extend to 10 min
                                    log::info!(
                                        "📈 Price RISING and profitable (${:.2}, {:.2}%). Extended timeout to {}s to capture upside.",
                                        estimated_profit,
                                        estimated_profit_percent,
                                        extended_timeout
                                    );
                                } else {
                                    log::info!(
                                        "📈 Price RISING and profitable (${:.2}, {:.2}%). Monitoring...",
                                        estimated_profit,
                                        estimated_profit_percent
                                    );
                                }
                            }
                            PriceTrend::Falling => {
                                log::info!(
                                    "📉 Price FALLING but still profitable (${:.2}, {:.2}%). Selling now before it drops further!",
                                    estimated_profit,
                                    estimated_profit_percent
                                );
                                break; // Sell immediately when trending down
                            }
                            PriceTrend::Stable => {
                                log::info!(
                                    "📊 Price STABLE and profitable (${:.2}, {:.2}%). Selling now!",
                                    estimated_profit,
                                    estimated_profit_percent
                                );
                                break; // Sell when stable and profitable
                            }
                        }
                    } else {
                        // Not enough history yet, but it's profitable
                        log::info!(
                            "✅ Profitable (${:.2}, {:.2}%). Collecting price data...",
                            estimated_profit,
                            estimated_profit_percent
                        );
                    }
                } else {
                    // 4. UNPROFITABLE: Still at a loss
                    let loss_percent = (estimated_profit / total_cost) * 100.0;

                    // Analyze trend to decide if we should wait or cut losses
                    if price_history.len() >= 3 {
                        let recent_prices: Vec<f64> =
                            price_history.iter().map(|(_, p)| *p).collect();
                        let trend = detect_price_trend(&recent_prices);

                        match trend {
                            PriceTrend::Rising => {
                                // Price recovering - extend timeout to extended_timeout if loss is small
                                if loss_percent > -2.0 && price_wait_elapsed < 180 {
                                    max_wait_time = extended_timeout; // Give it full 10 min if recovering from small loss
                                    log::info!(
                                        "📈 Price RISING from small loss (${:.2}, {:.2}%). Extended to {}s for recovery... ({}/{}s)",
                                        estimated_profit,
                                        loss_percent,
                                        extended_timeout,
                                        price_wait_elapsed,
                                        max_wait_time
                                    );
                                } else {
                                    log::info!(
                                        "📈 Price RISING from loss (${:.2}, {:.2}%). Waiting for recovery... ({}/{}s)",
                                        estimated_profit,
                                        loss_percent,
                                        price_wait_elapsed,
                                        max_wait_time
                                    );
                                }
                            }
                            PriceTrend::Falling => {
                                // Price getting worse - aggressively cut timeout
                                let reduced_timeout =
                                    (base_timeout / 2).max(price_wait_elapsed + 45);
                                if max_wait_time > reduced_timeout {
                                    max_wait_time = reduced_timeout;
                                    log::warn!(
                                        "📉 Price FALLING and unprofitable (${:.2}, {:.2}%). Timeout cut to {}s! ({}/{}s)",
                                        estimated_profit,
                                        loss_percent,
                                        max_wait_time,
                                        price_wait_elapsed,
                                        max_wait_time
                                    );
                                } else {
                                    log::warn!(
                                        "📉 Price FALLING and unprofitable (${:.2}, {:.2}%). Preparing to cut losses... ({}/{}s)",
                                        estimated_profit,
                                        loss_percent,
                                        price_wait_elapsed,
                                        max_wait_time
                                    );
                                }
                            }
                            PriceTrend::Stable => {
                                log::warn!(
                                    "📊 Price STABLE but unprofitable (${:.2} loss, {:.2}%). Waiting... ({}/{}s)",
                                    estimated_profit,
                                    loss_percent,
                                    price_wait_elapsed,
                                    max_wait_time
                                );
                            }
                        }
                    } else {
                        log::warn!(
                            "⚠️  Currently unprofitable: ${:.2} loss ({:.2}%). Collecting price data... ({}/{}s)",
                            estimated_profit,
                            loss_percent,
                            price_wait_elapsed,
                            max_wait_time
                        );
                    }
                }
            }
            None => {
                log::warn!(
                    "Failed to estimate sell output from {}, retrying in {}s...",
                    best_cex.name,
                    check_interval
                );
            }
        }

        // Check timeout
        if price_wait_elapsed >= max_wait_time {
            let final_profit = estimated_usdt_output - total_cost;
            if final_profit > 0.0 {
                log::info!(
                    "⏰ Timeout reached but price is profitable (${:.2}). Selling now!",
                    final_profit
                );
            } else {
                log::error!(
                    "⏰ Timeout reached. Selling at loss (${:.2}) to recover funds. Final: ${:.2} USDT",
                    final_profit,
                    estimated_usdt_output
                );
            }
            break;
        }

        tokio::time::sleep(tokio::time::Duration::from_secs(check_interval)).await;
        price_wait_elapsed += check_interval;
    }

    // 3.4: Execute the sell in 4 smaller chunks
    let num_chunks = 4;
    let chunk_amount = rounded_amount / num_chunks as f64;
    let mut total_sold = 0.0;
    let mut total_usdt = 0.0;
    let mut order_ids = Vec::new();

    log::info!(
        "💰 Selling {:.6} {} for USDT in {} chunks (each {:.6})...",
        rounded_amount,
        token_symbol,
        num_chunks,
        chunk_amount
    );

    for i in 0..num_chunks {
        let mut sell_amount = if i == num_chunks - 1 {
            // Last chunk: sell remaining to avoid rounding errors
            rounded_amount - total_sold
        } else {
            chunk_amount
        };

        // Round each chunk to the correct precision (same as we did for the total amount)
        // This ensures each chunk meets exchange requirements
        let divisor = 10_f64.powi(quantity_precision as i32);
        sell_amount = (sell_amount * divisor).floor() / divisor;

        log::info!(
            "Chunk {}: Selling {:.6} {} for USDT...",
            i + 1,
            sell_amount,
            token_symbol
        );
        match best_cex
            .service
            .sell_token_for_usdt(&token_symbol, sell_amount)
            .await
        {
            Ok((order_id, sold_amount, usdt_amount)) => {
                log::info!(
                    "✅ Sold {:.6} {} for {:.2} USDT (Order: {})",
                    sold_amount,
                    token_symbol,
                    usdt_amount,
                    order_id
                );
                total_sold += sold_amount;
                total_usdt += usdt_amount;
                order_ids.push(order_id);
            }
            Err(e) => {
                log::error!(
                    "❌ Failed to sell chunk {} of {} for USDT: {}",
                    i + 1,
                    token_symbol,
                    e
                );
                // Optionally: break or continue
                break;
            }
        }
        // Wait a bit for each chunk to settle
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    }

    let (order_id, actual_sold_amount, usdt_received) =
        (order_ids.join(","), total_sold, total_usdt);

    // 3.5: Transfer USDT back to funding account
    log::info!(
        "📥 Transferring {:.2} USDT back to funding account...",
        usdt_received
    );
    match best_cex.service.transfer_all_to_funding(Some("USDT")).await {
        Ok(transferred_count) => {
            if transferred_count > 0 {
                log::info!(
                    "✅ Transferred USDT to funding account ({} transfers)",
                    transferred_count
                );
            } else {
                log::debug!("USDT transfer to funding completed");
            }
        }
        Err(e) => {
            log::warn!("Transfer USDT to funding failed: {}", e);
        }
    }

    // Step 4: Calculate actual profit and effective sell price
    let effective_sell_price = usdt_received / actual_sold_amount;
    let actual_profit = usdt_received - arb_amount_usdt - gas_fee_usd;
    let profit_percent = (actual_profit / (arb_amount_usdt + gas_fee_usd)) * 100.0;

    log::info!("📊 Arbitrage Results:");
    log::info!(
        "   Deposited: {:.6} {} (expected: {:.6})",
        deposited_token_amount,
        token_symbol,
        tokens_from_dex
    );
    log::info!("   USDT Received: {:.2} USDT", usdt_received);
    log::info!(
        "   Effective Sell Price: ${:.6} per {}",
        effective_sell_price,
        token_symbol
    );
    log::info!(
        "   Actual Profit: ${:.2} USDT ({:.2}%)",
        actual_profit,
        profit_percent
    );

    // Get current CEX price for comparison (optional, just for logging)
    let _current_cex_price = best_cex
        .service
        .get_price(&update.token_address.to_lowercase())
        .await
        .map(|p| p.price);

    let price_diff_percent =
        ((update.price_in_usd - effective_sell_price) / effective_sell_price) * 100.0;

    let best = BestArbitrageOpportunity {
        cex_name: best_cex.name.to_string(),
        cex_price: effective_sell_price,
        cex_symbol: token_symbol.clone(),
        price_diff_percent,
        liquidity_usdt: usdt_received,
        usdt_from_cex: usdt_received,
        profit: actual_profit,
        profit_percent,
    };

    // Log and save the opportunity
    if best.profit > 0.0 {
        log::info!(
            "🎯 ✅ ARBITRAGE EXECUTED SUCCESSFULLY - Token: {} ({}), CEX: {}",
            best.cex_symbol,
            update.token_address,
            best.cex_name
        );
        log::info!(
            "  💵 Investment: ${:.2} USDT + ${:.2} gas = ${:.2} total",
            arb_amount_usdt,
            gas_fee_usd,
            arb_amount_usdt + gas_fee_usd
        );
        log::info!(
            "  📈 DEX Buy: ${:.2} USDT → {:.6} {} @ ${:.6} per token",
            arb_amount_usdt,
            deposited_token_amount,
            best.cex_symbol,
            arb_amount_usdt / deposited_token_amount
        );
        log::info!(
            "  📉 CEX Sell: {:.6} {} → ${:.2} USDT @ ${:.6} per token",
            actual_sold_amount,
            best.cex_symbol,
            best.usdt_from_cex,
            best.cex_price
        );
        log::info!(
            "  💰 PROFIT: ${:.2} USDT ({:.2}%)",
            best.profit,
            best.profit_percent
        );
        log::info!(
            "  � Price Difference: DEX ${:.6} vs CEX ${:.6} ({:.2}%)",
            update.price_in_usd,
            best.cex_price,
            best.price_diff_percent
        );
        log::info!("  🔗 TX: https://etherscan.io/tx/{}", tx_hash);

        // Save to database
        let opportunity = ArbitrageOpportunity {
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64,
            token_address: update.token_address.clone(),
            token_symbol: best.cex_symbol.clone(),
            dex_price: update.price_in_usd,
            cex_name: best.cex_name.clone(),
            cex_price: best.cex_price,
            cex_symbol: best.cex_symbol.clone(),
            price_diff_percent: best.price_diff_percent,
            liquidity_usdt: best.liquidity_usdt,
            profit_usdt: best.profit,
            profit_percent: best.profit_percent,
            arb_amount_usdt,
            tokens_from_dex: deposited_token_amount,
            gas_fee_usd,
        };

        if let Err(e) = db.save_opportunity(&opportunity) {
            log::error!("Failed to save opportunity to database: {}", e);
        } else {
            log::info!("✅ Arbitrage opportunity saved to database");
        }
    } else {
        log::warn!(
            "⚠️  Arbitrage execution resulted in LOSS: ${:.2} USDT ({:.2}%) for token: {} on {}",
            best.profit,
            best.profit_percent,
            update.token_address,
            best.cex_name
        );
        log::warn!(
            "   Spent: ${:.2} USDT + ${:.2} gas, Received: ${:.2} USDT",
            arb_amount_usdt,
            gas_fee_usd,
            best.usdt_from_cex
        );

        // Still save to database for tracking
        let opportunity = ArbitrageOpportunity {
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64,
            token_address: update.token_address.clone(),
            token_symbol: best.cex_symbol.clone(),
            dex_price: update.price_in_usd,
            cex_name: best.cex_name.clone(),
            cex_price: best.cex_price,
            cex_symbol: best.cex_symbol.clone(),
            price_diff_percent: best.price_diff_percent,
            liquidity_usdt: best.liquidity_usdt,
            profit_usdt: best.profit,
            profit_percent: best.profit_percent,
            arb_amount_usdt,
            tokens_from_dex: deposited_token_amount,
            gas_fee_usd,
        };

        if let Err(e) = db.save_opportunity(&opportunity) {
            log::error!("Failed to save opportunity to database: {}", e);
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    eprintln!(
        "DEBUG: main() started at {:?}",
        std::time::SystemTime::now()
    );
    std::io::Write::flush(&mut std::io::stderr()).ok();

    // Load environment variables from .env file
    dotenv().ok();
    eprintln!("DEBUG: dotenv loaded");
    std::io::Write::flush(&mut std::io::stderr()).ok();

    // Initialize logging
    tracing_subscriber::fmt::init();
    eprintln!("DEBUG: tracing initialized");
    std::io::Write::flush(&mut std::io::stderr()).ok();

    info!("Starting CEX Pricing Service with Multi-CEX Support");
    eprintln!("INFO: Starting CEX Pricing Service with Multi-CEX Support");
    std::io::Write::flush(&mut std::io::stderr()).ok();

    // Initialize all CEX services
    info!("Initializing CEX services...");

    // Helper function to check if a CEX is enabled
    fn is_cex_enabled(cex_name: &str) -> bool {
        // Check if explicitly disabled via environment variable
        let disabled_cexes = env::var("DISABLED_CEXES")
            .unwrap_or_default()
            .to_uppercase();

        if disabled_cexes.contains(&cex_name.to_uppercase()) {
            return false;
        }

        // Check if only specific CEXes are enabled (whitelist)
        if let Ok(enabled_cexes) = env::var("ENABLED_CEXES") {
            let enabled = enabled_cexes.to_uppercase();
            return enabled.contains(&cex_name.to_uppercase());
        }

        true
    }

    // Initialize MEXC with or without credentials
    let mexc_service = if is_cex_enabled("MEXC") {
        info!("MEXC is enabled");
        match (env::var("MEXC_API_KEY"), env::var("MEXC_API_SECRET")) {
            (Ok(api_key), Ok(api_secret)) => {
                info!("MEXC API credentials found, initializing with authentication");
                Arc::new(MexcService::with_credentials(
                    cex_price_provider::FilterAddressType::Ethereum,
                    api_key,
                    api_secret,
                ))
            }
            _ => {
                info!("MEXC API credentials not found, initializing without authentication (deposit filtering disabled)");
                Arc::new(MexcService::new(
                    cex_price_provider::FilterAddressType::Ethereum,
                ))
            }
        }
    } else {
        info!("MEXC is disabled via configuration");
        Arc::new(MexcService::new(
            cex_price_provider::FilterAddressType::Ethereum,
        ))
    };

    // Initialize Bybit with or without credentials
    let bybit_service = if is_cex_enabled("BYBIT") {
        info!("Bybit is enabled");
        match (env::var("BYBIT_API_KEY"), env::var("BYBIT_API_SECRET")) {
            (Ok(api_key), Ok(api_secret)) => {
                info!("Bybit API credentials found, initializing with authentication");
                Arc::new(BybitService::with_credentials(
                    cex_price_provider::FilterAddressType::Ethereum,
                    api_key,
                    api_secret,
                ))
            }
            _ => {
                info!("Bybit API credentials not found, initializing without authentication (limited functionality)");
                Arc::new(BybitService::new(
                    cex_price_provider::FilterAddressType::Ethereum,
                ))
            }
        }
    } else {
        info!("Bybit is disabled via configuration");
        Arc::new(BybitService::new(
            cex_price_provider::FilterAddressType::Ethereum,
        ))
    };

    // Initial kucoin with credentials
    let kucoin_service = if is_cex_enabled("KUCOIN") {
        info!("KuCoin is enabled");
        match (
            env::var("KUCOIN_API_KEY"),
            env::var("KUCOIN_API_SECRET"),
            env::var("KUCOIN_API_PASSPHRASE"),
        ) {
            (Ok(api_key), Ok(api_secret), Ok(api_passphrase)) => {
                info!("KuCoin API credentials found, initializing with authentication");
                Arc::new(KucoinService::with_credentials(
                    cex_price_provider::FilterAddressType::Ethereum,
                    api_key,
                    api_secret,
                    api_passphrase,
                ))
            }
            _ => {
                info!("KuCoin API credentials not found, initializing without authentication (limited functionality)");
                Arc::new(KucoinService::new(
                    cex_price_provider::FilterAddressType::Ethereum,
                ))
            }
        }
    } else {
        info!("KuCoin is disabled via configuration");
        Arc::new(KucoinService::new(
            cex_price_provider::FilterAddressType::Ethereum,
        ))
    };

    // Initialize Bitget with credentials
    let bitget_service = if is_cex_enabled("BITGET") {
        info!("Bitget is enabled");
        match (
            env::var("BITGET_API_KEY"),
            env::var("BITGET_API_SECRET"),
            env::var("BITGET_API_PASSPHRASE"),
        ) {
            (Ok(api_key), Ok(api_secret), Ok(api_passphrase)) => {
                info!("Bitget API credentials found, initializing with authentication");
                Arc::new(BitgetService::with_credentials(
                    cex_price_provider::FilterAddressType::Ethereum,
                    api_key,
                    api_secret,
                    api_passphrase,
                ))
            }
            _ => {
                info!("Bitget API credentials not found, initializing without authentication (limited functionality)");
                Arc::new(BitgetService::new(
                    cex_price_provider::FilterAddressType::Ethereum,
                ))
            }
        }
    } else {
        info!("Bitget is disabled via configuration");
        Arc::new(BitgetService::new(
            cex_price_provider::FilterAddressType::Ethereum,
        ))
    };

    let gate_service = if is_cex_enabled("GATE") {
        info!("Gate.io is enabled");
        Arc::new(GateService::new(
            cex_price_provider::FilterAddressType::Ethereum,
        ))
    } else {
        info!("Gate.io is disabled via configuration");
        Arc::new(GateService::new(
            cex_price_provider::FilterAddressType::Ethereum,
        ))
    };
    info!("All CEX services initialized successfully");

    // Initialize RocksDB for storing arbitrage opportunities
    info!("Initializing arbitrage database...");
    let db_path = "rocksdb_data/arbitrade-eth";
    let arb_db = Arc::new(ArbitrageDb::open(db_path)?);
    info!("Arbitrage database initialized at {}", db_path);

    info!("Initializing KyberSwap client...");
    let kyber_client = Arc::new(KyberClient::new());
    info!("KyberSwap client initialized successfully");

    // Fetch deposit addresses for each CEX (for a common token like USDT to get the ERC20 deposit address)
    // We use "USDT" as the symbol since all exchanges support it and it gives us the Ethereum deposit address
    info!("Fetching deposit addresses for CEX exchanges...");
    let mexc_deposit_addr = env::var("MEXC_ERC20_DEPOSIT_ADDRESS").ok();

    let bybit_deposit_addr = env::var("BYBIT_ERC20_DEPOSIT_ADDRESS").ok();

    let kucoin_deposit_addr = env::var("KUCOIN_ERC20_DEPOSIT_ADDRESS").ok();

    let bitget_deposit_addr = env::var("BITGET_ERC20_DEPOSIT_ADDRESS").ok();

    let gate_deposit_addr: Option<String> = None;

    // Create a vector of CEX providers for arbitrage checking
    let mut cex_providers: Vec<CexProvider> = Vec::new();

    // Add MEXC if enabled
    if is_cex_enabled("MEXC") {
        cex_providers.push(CexProvider {
            name: "MEXC",
            service: mexc_service.clone() as Arc<dyn PriceProvider + Send + Sync>,
            mexc: Some(mexc_service.clone()),
            bybit: None,
            kucoin: None,
            bitget: None,
            gate: None,
            deposit_address: env::var("MEXC_ERC20_DEPOSIT_ADDRESS").ok(),
        });
    }

    // Add Bybit if enabled
    if is_cex_enabled("BYBIT") {
        cex_providers.push(CexProvider {
            name: "Bybit",
            service: bybit_service.clone() as Arc<dyn PriceProvider + Send + Sync>,
            mexc: None,
            bybit: Some(bybit_service.clone()),
            kucoin: None,
            bitget: None,
            gate: None,
            deposit_address: env::var("BYBIT_ERC20_DEPOSIT_ADDRESS").ok(),
        });
    }

    // Add KuCoin if enabled
    if is_cex_enabled("KUCOIN") {
        cex_providers.push(CexProvider {
            name: "KuCoin",
            service: kucoin_service.clone() as Arc<dyn PriceProvider + Send + Sync>,
            mexc: None,
            bybit: None,
            kucoin: Some(kucoin_service.clone()),
            bitget: None,
            gate: None,
            deposit_address: env::var("KUCOIN_ERC20_DEPOSIT_ADDRESS").ok(),
        });
    }

    // Add Bitget if enabled
    if is_cex_enabled("BITGET") {
        cex_providers.push(CexProvider {
            name: "Bitget",
            service: bitget_service.clone() as Arc<dyn PriceProvider + Send + Sync>,
            mexc: None,
            bybit: None,
            kucoin: None,
            bitget: Some(bitget_service.clone()),
            gate: None,
            deposit_address: env::var("BITGET_ERC20_DEPOSIT_ADDRESS").ok(),
        });
    }

    // Add Gate.io if enabled
    if is_cex_enabled("GATE") {
        cex_providers.push(CexProvider {
            name: "Gate.io",
            service: gate_service.clone() as Arc<dyn PriceProvider + Send + Sync>,
            mexc: None,
            bybit: None,
            kucoin: None,
            bitget: None,
            gate: Some(gate_service.clone()),
            deposit_address: None,
        });
    }

    info!(
        "Enabled CEX providers: {}",
        cex_providers
            .iter()
            .map(|p| p.name)
            .collect::<Vec<_>>()
            .join(", ")
    );

    if cex_providers.is_empty() {
        error!("No CEX providers enabled! Please configure ENABLED_CEXES or ensure DISABLED_CEXES doesn't disable all exchanges.");
        return Err(anyhow::anyhow!("No CEX providers enabled"));
    }

    let cex_providers = Arc::new(cex_providers);

    // Start all WebSocket services in background
    // Note: Each CEX service will only subscribe to tokens that:
    // 1. Have valid contract addresses for the target chain (Ethereum)
    // 2. Have deposits ENABLED on the exchange
    //    - Bybit: Full deposit status filtering (requires API credentials)
    //    - Bitget: Full deposit status filtering (public API)
    //    - Gate.io: Full deposit status filtering (public API)
    //    - KuCoin: Full deposit status filtering (public API)
    //    - MEXC: Full deposit status filtering (requires API credentials)
    // Set MEXC_API_KEY and MEXC_API_SECRET environment variables to enable MEXC deposit filtering
    // This filtering happens during service initialization to avoid
    // wasting resources on tokens that cannot be deposited for arbitrage
    info!("Starting CEX WebSocket services in background...");

    if is_cex_enabled("MEXC") {
        let mexc_clone = mexc_service.clone();
        tokio::spawn(async move {
            info!("MEXC WebSocket task started");
            if let Err(e) = mexc_clone.start().await {
                error!("MEXC service error: {}", e);
            }
            error!("MEXC WebSocket task exited!");
        });
    }

    if is_cex_enabled("BYBIT") {
        let bybit_clone = bybit_service.clone();
        tokio::spawn(async move {
            info!("Bybit WebSocket task started");
            if let Err(e) = bybit_clone.start().await {
                error!("Bybit service error: {}", e);
            }
            error!("Bybit WebSocket task exited!");
        });
    }

    if is_cex_enabled("KUCOIN") {
        let kucoin_clone = kucoin_service.clone();
        tokio::spawn(async move {
            info!("KuCoin WebSocket task started");
            if let Err(e) = kucoin_clone.start().await {
                error!("KuCoin service error: {}", e);
            }
            error!("KuCoin WebSocket task exited!");
        });
    }

    if is_cex_enabled("BITGET") {
        let bitget_clone = bitget_service.clone();
        tokio::spawn(async move {
            info!("Bitget WebSocket task started");
            if let Err(e) = bitget_clone.start().await {
                error!("Bitget service error: {}", e);
            }
            error!("Bitget WebSocket task exited!");
        });
    }

    if is_cex_enabled("GATE") {
        let gate_clone = gate_service.clone();
        tokio::spawn(async move {
            info!("Gate.io WebSocket task started");
            if let Err(e) = gate_clone.start().await {
                error!("Gate.io service error: {}", e);
            }
            error!("Gate.io WebSocket task exited!");
        });
    }

    info!("All CEX WebSocket services started in background");

    // Give CEX services time to initialize their connections
    info!("Waiting 10 seconds for CEX services to initialize...");
    tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
    info!("CEX initialization period complete");

    // Load blacklist from database into memory (outside spawn so it's accessible by API)
    let blacklist: Arc<DashMap<String, ()>> = Arc::new(DashMap::new());
    match arb_db.get_blacklist() {
        Ok(addresses) => {
            for addr in addresses {
                blacklist.insert(addr.to_lowercase(), ());
            }
            info!(
                "Loaded {} blacklisted addresses from database",
                blacklist.len()
            );
        }
        Err(e) => {
            error!("Failed to load blacklist from database: {}", e);
        }
    }

    // Example: Start DEX price client (if DEX_PRICE_STREAM environment variable is set)
    if std::env::var("DEX_PRICE_STREAM").is_ok() {
        let dex_config = DexPriceConfig::from_env();
        info!(
            "DEX_PRICE_STREAM is set, starting DEX price client with URL: {}",
            dex_config.websocket_url
        );

        let (dex_client, mut dex_receiver) = DexPriceClient::new(dex_config);

        // Start DEX client in background
        info!("Starting DEX client in background...");
        tokio::spawn(async move {
            info!("DEX price client task started");
            if let Err(e) = dex_client.start().await {
                error!("DEX price client error: {}", e);
            }
            error!("DEX price client task exited!");
        });
        info!("DEX client started in background");

        // Handle DEX price updates in background
        let cex_providers_clone = cex_providers.clone();
        let kyber_client_clone = kyber_client.clone();
        let arb_db = arb_db.clone();
        let blacklist_clone = blacklist.clone();
        tokio::spawn(async move {
            // Read minimum percentage difference threshold from environment
            let min_percent_diff: f64 = std::env::var("MIN_PERCENT_DIFF")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(2.0);
            log::info!(
                "Using minimum percentage difference threshold: {:.2}%",
                min_percent_diff
            );

            // Read arbitrage simulation amount from environment (default: 400 USDT)
            let arb_amount_usdt: f64 = std::env::var("ARB_AMOUNT_USDT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(400.0);

            // Read minimum cooldown period between arbitrage attempts for same token (default: 3600 seconds)
            let arb_cooldown_secs: u64 = std::env::var("ARB_COOLDOWN_SECS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(3600);

            // Cache to track last arbitrage processing time per token
            let arb_processing_cache: Arc<DashMap<String, u64>> = Arc::new(DashMap::new());

            while let Some(price_updates) = dex_receiver.recv().await {
                info!("Received {} DEX price updates", price_updates.len());

                for update in &price_updates {
                    // Skip blacklisted tokens
                    let token_address_lower = update.token_address.to_lowercase();
                    if blacklist_clone.contains_key(&token_address_lower) {
                        log::debug!("Skipping blacklisted token: {}", update.token_address);
                        continue;
                    }

                    // Check if any CEX has this token at a better price
                    let mut has_opportunity = false;
                    for cex in cex_providers_clone.iter() {
                        if let Some(price) = cex
                            .service
                            .get_price(&update.token_address.to_lowercase())
                            .await
                        {
                            let price_diff_percent =
                                ((update.price_in_usd - price.price) / price.price) * 100.0;
                            if price_diff_percent < -min_percent_diff {
                                has_opportunity = true;
                                break;
                            }
                        }
                    }

                    if has_opportunity {
                        let token_address = update.token_address.to_lowercase();
                        let current_time = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap()
                            .as_secs();

                        // Check if we can process this token (not in cooldown)
                        let can_process =
                            if let Some(last_time) = arb_processing_cache.get(&token_address) {
                                current_time - *last_time >= arb_cooldown_secs
                            } else {
                                true
                            };

                        if can_process {
                            // Mark token as being processed BEFORE spawning task to prevent duplicate processing
                            arb_processing_cache.insert(token_address.clone(), current_time);

                            // Step 1: Quick profit estimation before full simulation
                            // Get estimated DEX output and check if any CEX offers positive profit
                            let kyber_client = kyber_client_clone.clone();
                            let cex_providers = cex_providers_clone.clone();
                            let update = update.clone();
                            let db = arb_db.clone();

                            tokio::spawn(async move {
                                const USDT_ADDRESS: &str =
                                    "0xdAC17F958D2ee523a2206206994597C13D831ec7";
                                let usdt_amount_wei = (arb_amount_usdt * 1_000_000.0) as u64;

                                // Step 1: Get price differences across all CEXes
                                log::debug!(
                                    "Step 1: Checking price differences for {}",
                                    update.token_address
                                );
                                let mut has_price_opportunity = false;
                                for cex in cex_providers.iter() {
                                    if let Some(price) = cex
                                        .service
                                        .get_price(&update.token_address.to_lowercase())
                                        .await
                                    {
                                        let price_diff_percent =
                                            ((update.price_in_usd - price.price) / price.price)
                                                * 100.0;
                                        if price_diff_percent < 0.0 {
                                            has_price_opportunity = true;
                                            log::debug!(
                                                "  {} has favorable price: ${:.6} (diff: {:.2}%)",
                                                cex.name,
                                                price.price,
                                                price_diff_percent
                                            );
                                        }
                                    }
                                }

                                if !has_price_opportunity {
                                    log::debug!(
                                        "No favorable price differences found, skipping arbitrage"
                                    );
                                    return;
                                }

                                // Step 2: Get estimated output from Kyber aggregator for DEX swap
                                log::debug!(
                                    "Step 2: Getting Kyber swap estimate for {} USDT",
                                    arb_amount_usdt
                                );
                                let (tokens_from_dex, gas_fee_usd, route_response) =
                                    match kyber_client
                                        .get_swap_route(
                                            USDT_ADDRESS,
                                            &update.token_address,
                                            &usdt_amount_wei.to_string(),
                                        )
                                        .await
                                    {
                                        Ok(route_resp) => {
                                            let gas_usd = route_resp
                                                .data
                                                .route_summary
                                                .gas_usd
                                                .parse::<f64>()
                                                .unwrap_or(0.0);
                                            match route_resp
                                                .data
                                                .route_summary
                                                .amount_out
                                                .parse::<u128>()
                                            {
                                                Ok(wei) => {
                                                    let divisor =
                                                        10_u128.pow(update.decimals as u32);
                                                    let tokens = wei as f64 / divisor as f64;
                                                    log::debug!(
                                                    "  Kyber estimate: {} USDT → {:.6} tokens (gas: ${:.2})",
                                                    arb_amount_usdt,
                                                    tokens,
                                                    gas_usd
                                                );
                                                    (tokens, gas_usd, route_resp)
                                                }
                                                Err(e) => {
                                                    log::warn!(
                                                        "Failed to parse Kyber output amount: {}",
                                                        e
                                                    );
                                                    return;
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            log::warn!("Failed to get Kyber quote: {}", e);
                                            return;
                                        }
                                    };

                                // Step 3: Estimate output USDT from each CEX using their orderbook
                                log::debug!(
                                    "Step 3: Estimating CEX outputs for {:.6} tokens",
                                    tokens_from_dex
                                );
                                let mut best_profit: Option<f64> = None;
                                let mut best_cex_index: Option<usize> = None;

                                for (idx, cex) in cex_providers.iter().enumerate() {
                                    if let Some(usdt_output) = cex
                                        .get_orderbook_liquidity(
                                            &update.token_address.to_lowercase(),
                                            tokens_from_dex,
                                        )
                                        .await
                                    {
                                        let profit = usdt_output - arb_amount_usdt - gas_fee_usd;
                                        log::debug!(
                                            "  {} estimated output: ${:.2} USDT → Profit: ${:.2}",
                                            cex.name,
                                            usdt_output,
                                            profit
                                        );

                                        if profit > best_profit.unwrap_or(0.0) {
                                            best_profit = Some(profit);
                                            best_cex_index = Some(idx);
                                        }
                                    }
                                }

                                // Step 4: Only proceed with full arbitrage simulation if we have positive profit
                                if let (Some(profit), Some(cex_idx)) = (best_profit, best_cex_index)
                                {
                                    if profit > 0.0 {
                                        let best_cex = &cex_providers[cex_idx];
                                        let token_symbol = best_cex
                                            .service
                                            .get_token_symbol_for_contract_address(
                                                &update.token_address,
                                            )
                                            .await
                                            .unwrap_or_else(|| update.token_address.clone());
                                        log::info!(
                                            "✅ Positive profit potential detected for {} ({}): ${:.2} on {}",
                                            token_symbol,
                                            update.token_address,
                                            profit,
                                            best_cex.name
                                        );
                                        log::info!("Proceeding with full arbitrage execution...");

                                        // Run full arbitrage execution with pre-fetched route data
                                        // (Token already marked as processed before spawning this task)
                                        // Pass ONLY the best CEX provider, not all of them
                                        process_arbitrage(
                                            &kyber_client,
                                            best_cex,
                                            &update,
                                            arb_amount_usdt,
                                            gas_fee_usd,
                                            tokens_from_dex,
                                            route_response.data.route_summary,
                                            &db,
                                        )
                                        .await;
                                    } else {
                                        log::debug!(
                                            "❌ No positive profit for {}: best profit ${:.2}, skipping",
                                            update.token_address,
                                            profit
                                        );
                                    }
                                } else {
                                    log::info!(
                                        "❌ No profitable CEX outputs found for {}, skipping arbitrage",
                                        update.token_address
                                    );
                                }
                            });
                        } else {
                            let last_time = arb_processing_cache.get(&token_address).unwrap();
                            let time_remaining = arb_cooldown_secs - (current_time - *last_time);
                            log::debug!(
                                "Skipping arbitrage for {} - cooldown active ({} seconds remaining)",
                                token_address,
                                time_remaining
                            );
                        }
                    }
                }
            }
        });

        info!("DEX price client started");
    } else {
        info!("DEX_PRICE_STREAM not set, skipping DEX price client");
    }

    // Start HTTP API server
    info!("Starting HTTP API server...");
    eprintln!("DEBUG: Starting HTTP API server...");
    std::io::Write::flush(&mut std::io::stderr()).ok();

    let app = Router::new()
        .merge(api::create_router(mexc_service))
        .merge(arbitrage_api::create_router(
            arb_db.clone(),
            blacklist.clone(),
        ))
        .layer(CorsLayer::permissive());

    // Read port from ARBITRADE_PORT environment variable, default to 3001
    let port = env::var("ARBITRADE_PORT")
        .ok()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(3001);
    let bind_addr = format!("0.0.0.0:{}", port);

    info!("Binding to {}...", bind_addr);
    eprintln!("DEBUG: Binding to {}...", bind_addr);
    std::io::Write::flush(&mut std::io::stderr()).ok();

    let listener = match tokio::net::TcpListener::bind(&bind_addr).await {
        Ok(l) => {
            info!("Successfully bound to {}", bind_addr);
            eprintln!("DEBUG: Successfully bound to {}", bind_addr);
            std::io::Write::flush(&mut std::io::stderr()).ok();
            l
        }
        Err(e) => {
            error!("Failed to bind to {}: {}", bind_addr, e);
            eprintln!("ERROR: Failed to bind to {}: {}", bind_addr, e);
            std::io::Write::flush(&mut std::io::stderr()).ok();
            return Err(e.into());
        }
    };

    info!("Starting axum server...");
    eprintln!("DEBUG: Starting axum server, entering main loop...");
    std::io::Write::flush(&mut std::io::stderr()).ok();

    // Keep the main function alive by running the server
    match axum::serve(listener, app).await {
        Ok(_) => {
            info!("Server shutdown gracefully");
            eprintln!("DEBUG: Server shutdown gracefully");
            std::io::Write::flush(&mut std::io::stderr()).ok();
            Ok(())
        }
        Err(e) => {
            error!("Axum server error: {}", e);
            eprintln!("ERROR: Axum server error: {}", e);
            std::io::Write::flush(&mut std::io::stderr()).ok();
            Err(e.into())
        }
    }
}
