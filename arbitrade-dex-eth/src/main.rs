use anyhow::Result;
use arbitrade_dex_eth::{ArbitrageDetector, DexWsClient, PriceCache};
use dashmap::DashMap;
use dotenv::dotenv;
use env_logger::Env;
use log::{debug, error, info, warn};
use std::env;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Configuration for arbitrade-dex-eth service
#[derive(Debug, Clone)]
struct Config {
    /// WebSocket URL of amm-eth service
    amm_eth_ws_url: String,
    /// HTTP API URL of amm-eth service for pair data
    dex_pair_api_url: String,
    /// Minimum profit percentage to trigger arbitrage
    min_profit_percent: f64,
    /// Minimum price difference in ETH
    min_price_diff_eth: f64,
    /// Interval (seconds) to check for opportunities
    check_interval_secs: u64,
}

impl Config {
    fn from_env() -> Self {
        let amm_eth_ws_url =
            env::var("AMM_ETH_WS_URL").unwrap_or_else(|_| "ws://localhost:8080".to_string());

        let dex_pair_api_url =
            env::var("DEX_PAIR_API_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());

        let min_profit_percent = env::var("MIN_PROFIT_PERCENT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1.0);

        let check_interval_secs = env::var("CHECK_INTERVAL_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(5);

        Config {
            amm_eth_ws_url,
            dex_pair_api_url,
            min_profit_percent,
            min_price_diff_eth: 0.0, // Deprecated: we use min_profit_percent only now
            check_interval_secs,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    debug!("DEBUG: arbitrade-dex-eth main() started");

    // Load environment variables
    dotenv().ok();
    debug!("DEBUG: dotenv loaded");

    // Initialize logging
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();
    debug!("DEBUG: logger initialized");

    let config = Config::from_env();
    info!("🚀 Starting arbitrade-dex-eth service");
    info!(
        "📊 Configuration: min_profit={}%, check_interval={}s",
        config.min_profit_percent, config.check_interval_secs
    );
    info!(
        "🔗 Connecting to amm-eth WebSocket: {}",
        config.amm_eth_ws_url
    );

    // Load DEX configuration for base tokens and contract addresses
    let dex_config =
        eth_dex_quote::DexConfiguration::load().expect("Failed to load eth_dex_config.toml");
    let chain_config = dex_config
        .get_chain("ethereum")
        .expect("Failed to get ethereum chain config");

    // Parse base tokens (now tuples of (address, is_stable))
    let base_tokens: Vec<(ethers::types::Address, bool)> = chain_config
        .base_tokens
        .iter()
        .filter_map(|(addr, is_stable)| {
            addr.parse::<ethers::types::Address>()
                .ok()
                .map(|a| (a, *is_stable))
        })
        .collect();
    info!("📋 Loaded {} base tokens for flashloan", base_tokens.len());

    // Get router and quoter addresses
    let uniswap_v2 = chain_config
        .dexes
        .get("uniswap_v2")
        .expect("V2 config missing");
    let uniswap_v3 = chain_config
        .dexes
        .get("uniswap_v3")
        .expect("V3 config missing");
    let uniswap_v4 = chain_config
        .dexes
        .get("uniswap_v4")
        .expect("V4 config missing");

    let router_v2: ethers::types::Address = uniswap_v2.router.parse()?;
    let quoter_v3: ethers::types::Address = uniswap_v3.quoter.as_ref().unwrap().parse()?;
    let quoter_v4: ethers::types::Address = uniswap_v4.quoter.as_ref().unwrap().parse()?;

    // Setup provider for on-chain queries
    let rpc_url = env::var("ETH_RPC_URL")
        .unwrap_or_else(|_| "https://ethereum-rpc.publicnode.com".to_string());
    let provider = ethers::providers::Provider::<ethers::providers::Http>::try_from(rpc_url)?;
    let provider = Arc::new(provider);
    info!("✅ Connected to Ethereum RPC");

    // Create price cache
    let price_cache = Arc::new(PriceCache::new());
    debug!("DEBUG: Price cache created");

    // Create arbitrage detector
    let detector = Arc::new(ArbitrageDetector::new(
        price_cache.clone(),
        config.min_profit_percent,
        config.min_price_diff_eth,
    ));
    debug!("DEBUG: Arbitrage detector created");

    // Create WebSocket client
    let ws_client = DexWsClient::new(config.amm_eth_ws_url.clone());
    debug!("DEBUG: WebSocket client created");

    // Track opportunities detected
    let opportunities_detected = Arc::new(DashMap::new());
    let executions_attempted = Arc::new(std::sync::atomic::AtomicU64::new(0));

    // Start WebSocket listener in background - spawn detector on each price update
    let price_cache_clone = price_cache.clone();
    let ws_client_clone = ws_client.clone();
    let detector_clone = detector.clone();
    let opportunities_detected_clone = opportunities_detected.clone();
    let provider_for_ws = provider.clone();
    let base_tokens_for_ws = base_tokens.clone();

    tokio::spawn(async move {
        match ws_client_clone.start_with_reconnect().await {
            Ok(mut rx) => {
                while let Some(price_update) = rx.recv().await {
                    debug!(
                        "Received price update: token={}, pool={}, price={}",
                        price_update.token_address,
                        price_update.pool_address,
                        price_update.price_in_eth
                    );
                    price_cache_clone.update_price(price_update.clone());

                    // IMPORTANT: Check opportunities reactively when a price update arrives
                    // This is much more efficient than polling all tokens every N seconds
                    let token_address =
                        match price_update.token_address.parse::<ethers::types::Address>() {
                            Ok(addr) => addr,
                            Err(e) => {
                                warn!(
                                    "Failed to parse token_address '{}' as Address: {}",
                                    price_update.token_address, e
                                );
                                continue;
                            }
                        };
                    // check again if price cache has been updated
                    debug!(
                        "After update, cache has {} tokens",
                        price_cache_clone.get_all_prices(&token_address).len()
                    );

                    // Spawn async task to check opportunities for this token
                    let detector = detector_clone.clone();
                    let opps_detected = opportunities_detected_clone.clone();
                    let executions = executions_attempted.clone();
                    let provider_clone = provider_for_ws.clone();
                    let base_tokens_clone = base_tokens_for_ws.clone();
                    let router_v2_clone = router_v2;
                    let quoter_v3_clone = quoter_v3;
                    let quoter_v4_clone = quoter_v4;
                    let dex_pair_api_clone = config.dex_pair_api_url.clone();
                    let http_client = reqwest::Client::new();

                    tokio::spawn(async move {
                        // Check for arbitrage opportunities for this specific token
                        let opportunities = detector.check_opportunities_for_token(&token_address);

                        if !opportunities.is_empty() {
                            info!(
                                "💰 Found {} arbitrage opportunity(ies) for token {}",
                                opportunities.len(),
                                format!("{:?}", token_address).to_lowercase()
                            );

                            for opp in opportunities.iter().take(5) {
                                // let token_addr = opp.token_address;
                                // let buy_pool_addr = &opp.buy_pool.pool_address;
                                // let sell_pool_addr = &opp.sell_pool.pool_address;

                                // let token_etherscan =
                                //     format!("https://etherscan.io/token/{:?}", token_addr);
                                // let buy_pool_etherscan =
                                //     format!("https://etherscan.io/address/{}", buy_pool_addr);
                                // let sell_pool_etherscan =
                                //     format!("https://etherscan.io/address/{}", sell_pool_addr);
                                let eth_price = opp.buy_pool.eth_price_usd;

                                // info!(
                                //     "   🎯 Token: {} | Buy@${:.6} ({}) / Sell@${:.6} ({}) = {:.2}% profit",
                                //     token_etherscan,
                                //     opp.buy_pool.price_in_usd.unwrap_or(0.0),
                                //     buy_pool_etherscan,
                                //     opp.sell_pool.price_in_usd.unwrap_or(0.0),
                                //     sell_pool_etherscan,
                                //     opp.price_diff_percent
                                // );

                                // Spawn task to compute exact arbitrage profit using on-chain quoters
                                let opp_clone = opp.clone();
                                let provider_for_compute = provider_clone.clone();
                                let base_tokens_for_compute = base_tokens_clone.clone();
                                let http_client_for_3hop = http_client.clone();
                                let dex_pair_api_for_3hop = dex_pair_api_clone.clone();

                                tokio::spawn(async move {
                                    // Check if we can use this token as flashloan input
                                    let token_a = opp_clone.token_address;

                                    // Find a base token that pairs with token_a
                                    let mut found_base_token = None;
                                    let mut found_base_token_is_stable = false;
                                    for (base_token, is_stable) in &base_tokens_for_compute {
                                        // Check if buy_pool or sell_pool involves this base token
                                        // pool_token0 and pool_token1 are already Address types
                                        let buy_token0 = opp_clone.buy_pool.pool_token0;
                                        let buy_token1 = opp_clone.buy_pool.pool_token1;

                                        if buy_token0 == *base_token || buy_token1 == *base_token {
                                            found_base_token = Some(*base_token);
                                            found_base_token_is_stable = *is_stable;
                                            break;
                                        }
                                    }

                                    let token_x = match found_base_token {
                                        Some(t) => t,
                                        None => {
                                            debug!("No base token found for this opportunity, skipping computation");
                                            return;
                                        }
                                    };

                                    // check wether we should do 2 or 3 hop arbitrage
                                    let other_token_in_sell_pool =
                                        if opp_clone.sell_pool.pool_token0 == token_a {
                                            opp_clone.sell_pool.pool_token1
                                        } else {
                                            opp_clone.sell_pool.pool_token0
                                        };

                                    // Flashloan amount: 500 USDT or 500 USD worth (assuming 6 decimals for stables, 18 for WETH)
                                    let base_token_decimals =
                                        if found_base_token_is_stable { 6 } else { 18 };
                                    let flashloan_amount_val = if found_base_token_is_stable {
                                        500f64
                                    } else {
                                        500f64 / eth_price
                                    };
                                    let flashloan_amount = ethers::types::U256::from(
                                        (flashloan_amount_val
                                            * 10f64.powi(base_token_decimals as i32))
                                            as u64,
                                    );
                                    let is_2_hop = opp_clone.buy_pool.pool_token0
                                        == opp_clone.sell_pool.pool_token0
                                        && opp_clone.buy_pool.pool_token1
                                            == opp_clone.sell_pool.pool_token1;

                                    if is_2_hop {
                                        info!(
                                            "📊 Computing 2-hop arbitrage: {:?} -> {:?} -> {:?}",
                                            token_x, token_a, token_x
                                        );

                                        // Compute arbitrage profit
                                        let start_time = std::time::Instant::now();
                                        match arbitrade_dex_eth::utils::compute_arbitrage_path(
                                            provider_for_compute,
                                            flashloan_amount,
                                            token_x,
                                            token_a,
                                            &opp_clone.buy_pool,
                                            &opp_clone.sell_pool,
                                            router_v2_clone,
                                            quoter_v3_clone,
                                            quoter_v4_clone,
                                        )
                                        .await
                                        {
                                            Ok((amount_a, amount_x_out, net_profit)) => {
                                                let elapsed = start_time.elapsed();
                                                if net_profit > 0 {
                                                    let net_profit_after_decimals = net_profit
                                                        as f64
                                                        / (10u128.pow(base_token_decimals as u32)
                                                            as f64);
                                                    let net_profit_value = net_profit_after_decimals
                                                        * (if found_base_token_is_stable {
                                                            1.0
                                                        } else {
                                                            eth_price
                                                        });
                                                    info!(
                                                        "💰 PROFITABLE 2-HOP! {} -> {} -> {} | Net profit: {:.2} {} | Time: {:.2}ms",
                                                        flashloan_amount,
                                                        amount_a,
                                                        amount_x_out,
                                                        net_profit_value,
                                                        if found_base_token_is_stable { "USDT" } else { "ETH" },
                                                        elapsed.as_secs_f64() * 1000.0
                                                    );
                                                    // TODO: Execute the arbitrage if profit > gas costs
                                                } else {
                                                    let net_profit_abs = net_profit.abs();
                                                    let net_profit_after_decimals = net_profit_abs as f64
                                                        / (10u128.pow(base_token_decimals as u32) as f64);
                                                    let net_profit_value = net_profit_after_decimals * (if found_base_token_is_stable { 1.0 } else { eth_price });
                                                    info!("2-hop unprofitable: net_profit = -{} | Time: {:.2}ms", net_profit_value, elapsed.as_secs_f64() * 1000.0);
                                                }
                                            }
                                            Err(e) => {
                                                let elapsed = start_time.elapsed();
                                                warn!(
                                                    "Failed to compute 2-hop arbitrage profit: {:?} | Time: {:.2}ms",
                                                    e,
                                                    elapsed.as_secs_f64() * 1000.0
                                                );
                                            }
                                        }
                                    } else {
                                        // 3-hop: X -> A -> B -> X
                                        // Use amm-eth API to find the best B -> X swap
                                        info!(
                                            "📊 Computing 3-hop arbitrage: {:?} -> {:?} -> {:?} -> {:?}",
                                            token_x, token_a, other_token_in_sell_pool, token_x
                                        );

                                        // First, get the amount of token B we'll have after X -> A -> B
                                        let token_b = other_token_in_sell_pool;
                                        let start_time = std::time::Instant::now();
                                        match arbitrade_dex_eth::utils::compute_arbitrage_path(
                                            provider_for_compute.clone(),
                                            flashloan_amount,
                                            token_x,
                                            token_a,
                                            &opp_clone.buy_pool,
                                            &opp_clone.sell_pool,
                                            router_v2_clone,
                                            quoter_v3_clone,
                                            quoter_v4_clone,
                                        )
                                        .await
                                        {
                                            Ok((amount_a, amount_b, _profit_2hop)) => {
                                                let elapsed_2hop = start_time.elapsed();
                                                // Now find the best B -> X pool and compute final profit
                                                let start_3hop_time = std::time::Instant::now();
                                                match arbitrade_dex_eth::utils::find_best_b_to_x_swap(
                                                    http_client_for_3hop.clone(),
                                                    &dex_pair_api_for_3hop,
                                                    token_b,
                                                    token_x,
                                                    amount_b,
                                                    provider_for_compute.clone(),
                                                    router_v2_clone,
                                                    quoter_v3_clone,
                                                    quoter_v4_clone,
                                                )
                                                .await
                                                {
                                                    Ok((_best_pool, amount_x_final)) => {
                                                        let elapsed_3hop = start_3hop_time.elapsed();
                                                        // Calculate profit using U256 to avoid overflow
                                                        let net_profit = if amount_x_final >= flashloan_amount {
                                                            (amount_x_final - flashloan_amount).as_u128() as i128
                                                        } else {
                                                            -((flashloan_amount - amount_x_final).as_u128() as i128)
                                                        };

                                                        if net_profit > 0 {
                                                            let net_profit_after_decimals = net_profit as f64
                                                                / (10u128.pow(base_token_decimals as u32) as f64);
                                                            let net_profit_value = net_profit_after_decimals * (if found_base_token_is_stable { 1.0 } else { eth_price });
                                                            info!(
                                                                "💰 PROFITABLE 3-HOP! {} -> {} -> {} -> {} | Net profit: {:.2} {} | X->A->B: {:.2}ms, B->X: {:.2}ms, Total: {:.2}ms",
                                                                flashloan_amount,
                                                                amount_a,
                                                                amount_b,
                                                                amount_x_final,
                                                                net_profit_value,
                                                                if found_base_token_is_stable { "USDT" } else { "ETH" },
                                                                elapsed_2hop.as_secs_f64() * 1000.0,
                                                                elapsed_3hop.as_secs_f64() * 1000.0,
                                                                (elapsed_2hop + elapsed_3hop).as_secs_f64() * 1000.0
                                                            );
                                                            // TODO: Execute the arbitrage if profit > gas costs
                                                        } else {
                                                            let net_profit_abs = net_profit.abs();
                                                            let net_profit_after_decimals = net_profit_abs as f64
                                                                / (10u128.pow(base_token_decimals as u32) as f64);
                                                            let net_profit_value = net_profit_after_decimals * (if found_base_token_is_stable { 1.0 } else { eth_price });
                                                            info!("3-hop unprofitable: net_profit = -{} | X->A->B: {:.2}ms, B->X: {:.2}ms, Total: {:.2}ms",
                                                                net_profit_value,
                                                                elapsed_2hop.as_secs_f64() * 1000.0,
                                                                elapsed_3hop.as_secs_f64() * 1000.0,
                                                                (elapsed_2hop + elapsed_3hop).as_secs_f64() * 1000.0
                                                            );
                                                        }
                                                    }
                                                    Err(e) => {
                                                        let elapsed_3hop = start_3hop_time.elapsed();
                                                        warn!(
                                                            "Failed to find best B -> X swap: {:?} | X->A->B: {:.2}ms, B->X: {:.2}ms",
                                                            e,
                                                            elapsed_2hop.as_secs_f64() * 1000.0,
                                                            elapsed_3hop.as_secs_f64() * 1000.0
                                                        );
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                warn!(
                                                    "Failed to compute X -> A -> B amounts: {:?}",
                                                    e
                                                );
                                            }
                                        }
                                    }
                                });

                                // Store opportunity in memory (for API/dashboard later)
                                let opp_key = format!(
                                    "{}_{}",
                                    opp.token_address,
                                    SystemTime::now()
                                        .duration_since(UNIX_EPOCH)
                                        .unwrap()
                                        .as_secs()
                                );
                                opps_detected.insert(opp_key, opp.clone());
                            }

                            executions.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        }
                    });
                }
            }
            Err(e) => error!("WebSocket error: {}", e),
        }
    });
    info!("✅ WebSocket connected and listening for price updates");
    info!("🎯 Arbitrage detection is now REACTIVE - checks trigger on each price update");
    info!(
        "⚙️  Configuration: min_profit={}%",
        config.min_profit_percent
    );

    // Keep the service running
    // Opportunities are detected reactively as price updates arrive from WebSocket
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;

        let stats = price_cache.get_stats();
        info!(
            "📊 Cache stats - {} tokens, {} pools, {} with multiple pools",
            stats.unique_tokens, stats.total_pools, stats.tokens_with_multiple_pools
        );
    }
}
