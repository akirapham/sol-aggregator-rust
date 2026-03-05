use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use dashmap::DashMap;
use eth_dex_quote::{ChainConfig, QuoteRouterClient, TokenPriceUpdate};
use ethers::{
    providers::{Http, Provider},
    types::Address,
};
use log::{debug, info, warn};

use crate::{
    utils, ArbitrageDetectorTrait, ArbitrageExecutorTrait, DexArbitrageOpportunity, PriceCacheTrait,
};

pub struct ArbitrageHandler {
    price_cache: Arc<dyn PriceCacheTrait>,
    arbitrage_detector: Arc<dyn ArbitrageDetectorTrait>,
    opportunities_detected: Arc<DashMap<String, DexArbitrageOpportunity>>,
    rpc_provider: Arc<Provider<Http>>,
    base_tokens: Vec<(ethers::types::Address, bool)>,
    quote_router_client: Arc<QuoteRouterClient<Provider<Http>>>,
    chain_config: ChainConfig,
    executor: Arc<dyn ArbitrageExecutorTrait>,
    executions_attempted: Arc<std::sync::atomic::AtomicU64>,
    dex_pair_api_url: String,
}

impl ArbitrageHandler {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        price_cache: Arc<dyn PriceCacheTrait>,
        arbitrage_detector: Arc<dyn ArbitrageDetectorTrait>,
        opportunities_detected: Arc<DashMap<String, DexArbitrageOpportunity>>,
        rpc_provider: Arc<Provider<Http>>,
        base_tokens: Vec<(ethers::types::Address, bool)>,
        quote_router_client: Arc<QuoteRouterClient<Provider<Http>>>,
        chain_config: ChainConfig,
        executor: Arc<dyn ArbitrageExecutorTrait>,
        dex_pair_api_url: String,
    ) -> Self {
        Self {
            price_cache,
            arbitrage_detector,
            opportunities_detected,
            rpc_provider,
            base_tokens,
            quote_router_client,
            chain_config,
            executor,
            executions_attempted: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            dex_pair_api_url,
        }
    }

    pub async fn handle_price_update_for_arbitrage(&self, price_update: &TokenPriceUpdate) {
        self.price_cache.update_price(price_update.clone());

        let token_address = match price_update.token_address.parse::<Address>() {
            Ok(addr) => addr,
            Err(e) => {
                warn!(
                    "Failed to parse token_address '{}' as Address: {}",
                    price_update.token_address, e
                );
                return;
            }
        };

        let detector = self.arbitrage_detector.clone();
        let opps_detected = self.opportunities_detected.clone();
        let executions = self.executions_attempted.clone();
        let provider_clone = self.rpc_provider.clone();
        let base_tokens_clone = self.base_tokens.clone();
        let chain_config_clone = self.chain_config.clone();
        let quote_router_client_clone = self.quote_router_client.clone();
        let dex_pair_api_clone = self.dex_pair_api_url.clone();
        let http_client = reqwest::Client::new();
        let executor_clone = self.executor.clone();
        let token_price_update = price_update.clone();

        tokio::spawn(async move {
            Self::handle_arbitrage_for_a_token(
                token_address,
                token_price_update,
                detector,
                opps_detected,
                executions,
                provider_clone,
                base_tokens_clone,
                quote_router_client_clone,
                chain_config_clone,
                executor_clone,
                dex_pair_api_clone,
                http_client,
            )
            .await;
        });
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn handle_arbitrage_for_a_token(
        token_address: Address,
        token_price_update: TokenPriceUpdate,
        detector: Arc<dyn ArbitrageDetectorTrait>,
        opportunities_detected: Arc<DashMap<String, DexArbitrageOpportunity>>,
        executions: Arc<std::sync::atomic::AtomicU64>,
        provider_clone: Arc<Provider<Http>>,
        base_tokens_clone: Vec<(ethers::types::Address, bool)>,
        quote_router_client_clone: Arc<QuoteRouterClient<Provider<Http>>>,
        chain_config_clone: ChainConfig,
        executor_clone: Arc<dyn ArbitrageExecutorTrait>,
        dex_pair_api_clone: String,
        http_client: reqwest::Client,
    ) {
        let opportunities =
            detector.check_opportunities_for_token(&token_address, &token_price_update);
        debug!("Checking arbitrage for token {}", token_address);
        if !opportunities.is_empty() {
            info!(
                "💰 Found {} arbitrage opportunity(ies) for token {}",
                opportunities.len(),
                format!("{:?}", token_address).to_lowercase()
            );

            for opp in opportunities.iter().take(5) {
                let eth_price = opp.buy_pool.eth_price_usd;

                let opp_clone = opp.clone();
                let provider_for_compute = provider_clone.clone();
                let base_tokens_for_compute = base_tokens_clone.clone();
                let http_client_for_3hop = http_client.clone();
                let dex_pair_api_for_3hop = dex_pair_api_clone.clone();
                let chain_config_clone = chain_config_clone.clone();
                let quote_router_client_clone = quote_router_client_clone.clone();
                let executor = executor_clone.clone();

                tokio::spawn(async move {
                    let token_a = opp_clone.token_address;
                    let flashloan_token = if opp_clone.buy_pool.pool_token0 == token_a {
                        opp_clone.buy_pool.pool_token1
                    } else {
                        opp_clone.buy_pool.pool_token0
                    };

                    let mut found_flashloan_token = false;
                    let mut flashloan_token_is_stable = false;
                    for (base_token, is_stable) in &base_tokens_for_compute {
                        if *base_token == flashloan_token {
                            found_flashloan_token = true;
                            flashloan_token_is_stable = *is_stable;
                            break;
                        }
                    }

                    if !found_flashloan_token {
                        debug!("Flashloan token not found in base tokens, skipping computation");
                        return;
                    }

                    let token_x = flashloan_token;
                    let other_token_in_sell_pool = if opp_clone.sell_pool.pool_token0 == token_a {
                        opp_clone.sell_pool.pool_token1
                    } else {
                        opp_clone.sell_pool.pool_token0
                    };

                    let base_token_decimals = if flashloan_token_is_stable { 6 } else { 18 };
                    let flashloan_amount_val = if flashloan_token_is_stable {
                        1000f64
                    } else {
                        1000f64 / eth_price
                    };
                    let flashloan_amount = ethers::types::U256::from(
                        (flashloan_amount_val * 10f64.powi(base_token_decimals)) as u64,
                    );

                    let is_2_hop = other_token_in_sell_pool == token_x;

                    if is_2_hop {
                        info!(
                            "📊 Computing 2-hop arbitrage: {:?} -> {:?} -> {:?}",
                            token_x, token_a, token_x
                        );

                        let start_time = std::time::Instant::now();
                        match utils::compute_arbitrage_path(
                            provider_for_compute,
                            flashloan_amount,
                            token_x,
                            token_a,
                            &opp_clone.buy_pool,
                            &opp_clone.sell_pool,
                            &chain_config_clone,
                            quote_router_client_clone.as_ref(),
                        )
                        .await
                        {
                            Ok((_amount_a, amount_x_out, net_profit)) => {
                                let elapsed = start_time.elapsed();
                                if net_profit > 0 {
                                    let net_profit_after_decimals = net_profit as f64
                                        / (10u128.pow(base_token_decimals as u32) as f64);
                                    let net_profit_value = net_profit_after_decimals
                                        * (if flashloan_token_is_stable {
                                            1.0
                                        } else {
                                            eth_price
                                        });
                                    info!(
                                        "💰 PROFITABLE 2-HOP! {} -> {} -> {} | Net profit: {:.2} {} | Time: {:.2}ms",
                                        flashloan_amount,
                                        _amount_a,
                                        amount_x_out,
                                        net_profit_value,
                                        if flashloan_token_is_stable { "USDT" } else { "ETH" },
                                        elapsed.as_secs_f64() * 1000.0
                                    );

                                    let hop1 = utils::build_exec_hop(
                                        &opp_clone.buy_pool,
                                        token_x,
                                        token_a,
                                        &chain_config_clone,
                                    );
                                    let hop2 = utils::build_exec_hop(
                                        &opp_clone.sell_pool,
                                        token_a,
                                        token_x,
                                        &chain_config_clone,
                                    );

                                    match (hop1, hop2) {
                                        (Ok(h1), Ok(h2)) => {
                                            match executor
                                                .execute_flashloan(
                                                    opp_clone.clone(),
                                                    vec![h1, h2],
                                                    flashloan_amount,
                                                    token_x,
                                                    Some(net_profit_value),
                                                )
                                                .await
                                            {
                                                Ok(res) => info!(
                                                    "⚡️ 2-Hop Flashloan executed! Hash: {}",
                                                    res.tx_hash
                                                ),
                                                Err(e) => warn!("⚠️ Flashloan aborted: {}", e),
                                            }
                                        }
                                        (e1, e2) => warn!(
                                            "Failed building ExecHops: {:?} {:?}",
                                            e1.err(),
                                            e2.err()
                                        ),
                                    }
                                } else {
                                    let net_profit_abs = net_profit.abs();
                                    let net_profit_after_decimals = net_profit_abs as f64
                                        / (10u128.pow(base_token_decimals as u32) as f64);
                                    let net_profit_value = net_profit_after_decimals
                                        * (if flashloan_token_is_stable {
                                            1.0
                                        } else {
                                            eth_price
                                        });
                                    info!(
                                        "2-hop unprofitable: net_profit = -{:.4} | Time: {:.2}ms",
                                        net_profit_value,
                                        elapsed.as_secs_f64() * 1000.0
                                    );
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
                        let token_b = other_token_in_sell_pool;
                        let start_time = std::time::Instant::now();
                        info!(
                            "📊 Computing 3-hop arbitrage: {:?} -> {:?} -> {:?} -> {:?}",
                            token_x, token_a, token_b, token_x
                        );

                        let hop1_result = utils::build_hop(
                            &opp_clone.buy_pool,
                            token_x,
                            token_a,
                            &chain_config_clone,
                        );
                        let hop2_result = utils::build_hop(
                            &opp_clone.sell_pool,
                            token_a,
                            token_b,
                            &chain_config_clone,
                        );

                        match (hop1_result, hop2_result) {
                            (Ok(hop1), Ok(hop2)) => {
                                match utils::fetch_b_to_x_pools(
                                    http_client_for_3hop.clone(),
                                    &dex_pair_api_for_3hop,
                                    token_b,
                                    token_x,
                                    &chain_config_clone,
                                )
                                .await
                                {
                                    Ok(b_to_x_pools) => {
                                        if b_to_x_pools.is_empty() {
                                            debug!("No B->X pools found for 3-hop");
                                        } else {
                                            let mut paths: Vec<eth_dex_quote::quote_router::PathQuote> = Vec::new();

                                            for pool_info in b_to_x_pools.iter() {
                                                if let Ok(hop3) = utils::build_hop(
                                                    &pool_info.0,
                                                    token_b,
                                                    token_x,
                                                    &chain_config_clone,
                                                ) {
                                                    let path = eth_dex_quote::quote_router::PathQuote {
                                                        hops: vec![hop1.clone(), hop2.clone(), hop3],
                                                        amount_in: flashloan_amount,
                                                    };
                                                    paths.push(path);
                                                }
                                            }

                                            if paths.is_empty() {
                                                debug!("No valid 3-hop paths could be built");
                                            } else {
                                                let paths_count = paths.len();
                                                match quote_router_client_clone
                                                    .quote_multi_paths(paths)
                                                    .await
                                                {
                                                    Ok(results) => {
                                                        let elapsed = start_time.elapsed();

                                                        let mut best_idx: Option<usize> = None;
                                                        let mut best_profit: i128 = 0;
                                                        let mut best_amount_out: ethers::types::U256 = ethers::types::U256::zero();

                                                        for (i, result) in results.iter().enumerate() {
                                                            if result.success && result.amount_out > best_amount_out {
                                                                let profit = if result.amount_out >= flashloan_amount {
                                                                    (result.amount_out - flashloan_amount).as_u128() as i128
                                                                } else {
                                                                    -((flashloan_amount - result.amount_out).as_u128() as i128)
                                                                };

                                                                if profit > best_profit {
                                                                    best_profit = profit;
                                                                    best_idx = Some(i);
                                                                    best_amount_out = result.amount_out;
                                                                }
                                                            }
                                                        }

                                                        if let Some(idx) = best_idx {
                                                            let best_pool = &b_to_x_pools[idx].0;

                                                            if best_profit > 0 {
                                                                let net_profit_after_decimals = best_profit as f64
                                                                    / (10u128.pow(base_token_decimals as u32) as f64);
                                                                let net_profit_value = net_profit_after_decimals
                                                                    * (if flashloan_token_is_stable {
                                                                        1.0
                                                                    } else {
                                                                        eth_price
                                                                    });

                                                                info!(
                                                                    "💰 PROFITABLE 3-HOP! {} -> {} -> {} -> {} | Net profit: {:.2} {} | Paths: {} | Time: {:.2}ms",
                                                                    flashloan_amount,
                                                                    token_a,
                                                                    token_b,
                                                                    best_amount_out,
                                                                    net_profit_value,
                                                                    if flashloan_token_is_stable { "USDT" } else { "ETH" },
                                                                    paths_count,
                                                                    elapsed.as_secs_f64() * 1000.0
                                                                );

                                                                let hop1_exec = utils::build_exec_hop(
                                                                    &opp_clone.buy_pool,
                                                                    token_x,
                                                                    token_a,
                                                                    &chain_config_clone,
                                                                );
                                                                let hop2_exec = utils::build_exec_hop(
                                                                    &opp_clone.sell_pool,
                                                                    token_a,
                                                                    token_b,
                                                                    &chain_config_clone,
                                                                );
                                                                let hop3_exec = utils::build_exec_hop(
                                                                    best_pool,
                                                                    token_b,
                                                                    token_x,
                                                                    &chain_config_clone,
                                                                );

                                                                match (hop1_exec, hop2_exec, hop3_exec) {
                                                                    (Ok(h1), Ok(h2), Ok(h3)) => {
                                                                        match executor
                                                                            .execute_flashloan(
                                                                                opp_clone.clone(),
                                                                                vec![h1, h2, h3],
                                                                                flashloan_amount,
                                                                                token_x,
                                                                                Some(net_profit_value),
                                                                            )
                                                                            .await
                                                                        {
                                                                            Ok(res) => info!(
                                                                                "⚡️ 3-Hop Flashloan executed! Hash: {}",
                                                                                res.tx_hash
                                                                            ),
                                                                            Err(e) => {
                                                                                warn!("⚠️ Flashloan aborted: {}", e)
                                                                            }
                                                                        }
                                                                    }
                                                                    _ => warn!("Failed building 3-Hop ExecHops"),
                                                                }
                                                            } else {
                                                                let net_profit_abs = best_profit.abs();
                                                                let net_profit_after_decimals = net_profit_abs as f64
                                                                    / (10u128.pow(base_token_decimals as u32) as f64);
                                                                let net_profit_value = net_profit_after_decimals
                                                                    * (if flashloan_token_is_stable {
                                                                        1.0
                                                                    } else {
                                                                        eth_price
                                                                    });
                                                                info!(
                                                                    "3-hop unprofitable: best_profit = -{:.4} USD | Paths: {} | Time: {:.2}ms",
                                                                    net_profit_value,
                                                                    paths_count,
                                                                    elapsed.as_secs_f64() * 1000.0
                                                                );
                                                            }
                                                        } else {
                                                            debug!("No profitable 3-hop paths found");
                                                        }
                                                    }
                                                    Err(e) => {
                                                        warn!("quoteMultiPaths failed: {:?}", e);
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        warn!("Failed to fetch B->X pools: {:?}", e);
                                    }
                                }
                            }
                            (Err(e1), _) => warn!("Failed building hop1 for 3-hop: {:?}", e1),
                            (_, Err(e2)) => warn!("Failed building hop2 for 3-hop: {:?}", e2),
                        }
                    }
                });

                let opp_key = format!(
                    "{}_{}",
                    opp.token_address,
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_secs()
                );
                opportunities_detected.insert(opp_key, opp.clone());
            }

            executions.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        } else {
            debug!(
                "No arbitrage opportunities found for token {}",
                token_address
            );
        }
    }
}
