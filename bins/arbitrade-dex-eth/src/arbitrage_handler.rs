use std::sync::Arc;

use dashmap::DashMap;
use eth_dex_quote::{ChainConfig, QuoteRouterClient, TokenPriceUpdate};
use ethers::{
    providers::{Http, Provider},
    types::Address,
};
use log::{info, warn};

use crate::{
    ArbitrageDetectorTrait, ArbitrageExecutorTrait, DexArbitrageOpportunity, PriceCacheTrait,
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

        // IMPORTANT: Check opportunities reactively when a price update arrives
        // This is much more efficient than polling all tokens every N seconds
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

        // Spawn async task to check opportunities for this token
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
        _opportunities_detected: Arc<DashMap<String, DexArbitrageOpportunity>>,
        executions: Arc<std::sync::atomic::AtomicU64>,
        _provider_clone: Arc<Provider<Http>>,
        _base_tokens_clone: Vec<(ethers::types::Address, bool)>,
        _quote_router_client_clone: Arc<QuoteRouterClient<Provider<Http>>>,
        _chain_config_clone: ChainConfig,
        _executor_clone: Arc<dyn ArbitrageExecutorTrait>,
        _dex_pair_api_clone: String,
        _http_client: reqwest::Client,
    ) {
        // Check for arbitrage opportunities for this specific token
        let opportunities =
            detector.check_opportunities_for_token(&token_address, &token_price_update);
        info!("Checking arbitrage for token {}", token_address);
        if !opportunities.is_empty() {
            info!(
                "💰 Found {} arbitrage opportunity(ies) for token {}",
                opportunities.len(),
                format!("{:?}", token_address).to_lowercase()
            );
            // print out opportunities
            for opp in opportunities.iter() {
                info!("💰 Opportunity: Buy pool {:} at price {:}, Sell pool {:} at price {:}, diff % {:}", opp.buy_pool.pool_address, opp.buy_pool.price_in_usd.unwrap(), opp.sell_pool.pool_address, opp.sell_pool.price_in_usd.unwrap(), opp.price_diff_percent);
            }

            // for opp in opportunities.iter().take(5) {
            //     let eth_price = opp.buy_pool.eth_price_usd;

            //     // Spawn task to compute exact arbitrage profit using on-chain quoters
            //     let opp_clone = opp.clone();
            //     let provider_for_compute = provider_clone.clone();
            //     let base_tokens_for_compute = base_tokens_clone.clone();
            //     let http_client_for_3hop = http_client.clone();
            //     let dex_pair_api_for_3hop = dex_pair_api_clone.clone();
            //     let chain_config_clone = chain_config_clone.clone();
            //     let quote_router_client_clone = quote_router_client_clone.clone();
            //     let executor = executor_clone.clone();

            //     tokio::spawn(async move {
            //         // Check if we can use this token as flashloan input
            //         let token_a = opp_clone.token_address;

            //         // Find a base token that pairs with token_a
            //         let mut found_base_token = None;
            //         let mut found_base_token_is_stable = false;
            //         for (base_token, is_stable) in &base_tokens_for_compute {
            //             // Check if buy_pool or sell_pool involves this base token
            //             // pool_token0 and pool_token1 are already Address types
            //             let buy_token0 = opp_clone.buy_pool.pool_token0;
            //             let buy_token1 = opp_clone.buy_pool.pool_token1;

            //             if buy_token0 == *base_token || buy_token1 == *base_token {
            //                 found_base_token = Some(*base_token);
            //                 found_base_token_is_stable = *is_stable;
            //                 break;
            //             }
            //         }

            //         let token_x = match found_base_token {
            //             Some(t) => t,
            //             None => {
            //                 debug!(
            //                     "No base token found for this opportunity, skipping computation"
            //                 );
            //                 return;
            //             }
            //         };

            //         // check wether we should do 2 or 3 hop arbitrage
            //         let other_token_in_sell_pool = if opp_clone.sell_pool.pool_token0 == token_a {
            //             opp_clone.sell_pool.pool_token1
            //         } else {
            //             opp_clone.sell_pool.pool_token0
            //         };

            //         // Flashloan amount: 500 USDT or 500 USD worth (assuming 6 decimals for stables, 18 for WETH)
            //         let base_token_decimals = if found_base_token_is_stable { 6 } else { 18 };
            //         let flashloan_amount_val = if found_base_token_is_stable {
            //             500f64
            //         } else {
            //             500f64 / eth_price
            //         };
            //         let flashloan_amount = ethers::types::U256::from(
            //             (flashloan_amount_val * 10f64.powi(base_token_decimals)) as u64,
            //         );
            //         let is_2_hop = opp_clone.buy_pool.pool_token0
            //             == opp_clone.sell_pool.pool_token0
            //             && opp_clone.buy_pool.pool_token1 == opp_clone.sell_pool.pool_token1;

            //         if is_2_hop {
            //             info!(
            //                 "📊 Computing 2-hop arbitrage: {:?} -> {:?} -> {:?}",
            //                 token_x, token_a, token_x
            //             );

            //             // Compute arbitrage profit
            //             let start_time = std::time::Instant::now();
            //             match utils::compute_arbitrage_path(
            //                 provider_for_compute,
            //                 flashloan_amount,
            //                 token_x,
            //                 token_a,
            //                 &opp_clone.buy_pool,
            //                 &opp_clone.sell_pool,
            //                 &chain_config_clone,
            //                 quote_router_client_clone.as_ref(),
            //             )
            //             .await
            //             {
            //                 Ok((amount_a, amount_x_out, net_profit)) => {
            //                     let elapsed = start_time.elapsed();
            //                     if net_profit > 0 {
            //                         let net_profit_after_decimals = net_profit as f64
            //                             / (10u128.pow(base_token_decimals as u32) as f64);
            //                         let net_profit_value = net_profit_after_decimals
            //                             * (if found_base_token_is_stable {
            //                                 1.0
            //                             } else {
            //                                 eth_price
            //                             });
            //                         info!(
            //                                 "💰 PROFITABLE 2-HOP! {} -> {} -> {} | Net profit: {:.2} {} | Time: {:.2}ms",
            //                                 flashloan_amount,
            //                                 amount_a,
            //                                 amount_x_out,
            //                                 net_profit_value,
            //                                 if found_base_token_is_stable { "USDT" } else { "ETH" },
            //                                 elapsed.as_secs_f64() * 1000.0
            //                             );

            //                         let hop1 = utils::build_exec_hop(
            //                             &opp_clone.buy_pool,
            //                             token_x,
            //                             token_a,
            //                             &chain_config_clone,
            //                         );
            //                         let hop2 = utils::build_exec_hop(
            //                             &opp_clone.sell_pool,
            //                             token_a,
            //                             token_x,
            //                             &chain_config_clone,
            //                         );

            //                         match (hop1, hop2) {
            //                             (Ok(h1), Ok(h2)) => {
            //                                 match executor
            //                                     .execute_flashloan(
            //                                         opp_clone.clone(),
            //                                         vec![h1, h2],
            //                                         flashloan_amount,
            //                                         token_x,
            //                                         Some(net_profit_value),
            //                                     )
            //                                     .await
            //                                 {
            //                                     Ok(res) => info!(
            //                                         "⚡️ 2-Hop Flashloan executed! Hash: {}",
            //                                         res.tx_hash
            //                                     ),
            //                                     Err(e) => warn!("⚠️ Flashloan aborted: {}", e),
            //                                 }
            //                             }
            //                             (e1, e2) => warn!(
            //                                 "Failed building ExecHops: {:?} {:?}",
            //                                 e1.err(),
            //                                 e2.err()
            //                             ),
            //                         }
            //                     } else {
            //                         let net_profit_abs = net_profit.abs();
            //                         let net_profit_after_decimals = net_profit_abs as f64
            //                             / (10u128.pow(base_token_decimals as u32) as f64);
            //                         let net_profit_value = net_profit_after_decimals
            //                             * (if found_base_token_is_stable {
            //                                 1.0
            //                             } else {
            //                                 eth_price
            //                             });
            //                         info!(
            //                             "2-hop unprofitable: net_profit = -{:.4} | Time: {:.2}ms",
            //                             net_profit_value,
            //                             elapsed.as_secs_f64() * 1000.0
            //                         );
            //                     }
            //                 }
            //                 Err(e) => {
            //                     let elapsed = start_time.elapsed();
            //                     warn!(
            //                             "Failed to compute 2-hop arbitrage profit: {:?} | Time: {:.2}ms",
            //                             e,
            //                             elapsed.as_secs_f64() * 1000.0
            //                         );
            //                 }
            //             }
            //         } else {
            //             // 3-hop: X -> A -> B -> X
            //             // Use amm-eth API to find the best B -> X swap
            //             info!(
            //                 "📊 Computing 3-hop arbitrage: {:?} -> {:?} -> {:?} -> {:?}",
            //                 token_x, token_a, other_token_in_sell_pool, token_x
            //             );

            //             // First, get the amount of token B we'll have after X -> A -> B
            //             let token_b = other_token_in_sell_pool;
            //             let start_time = std::time::Instant::now();
            //             match utils::compute_arbitrage_path(
            //                 provider_for_compute.clone(),
            //                 flashloan_amount,
            //                 token_x,
            //                 token_a,
            //                 &opp_clone.buy_pool,
            //                 &opp_clone.sell_pool,
            //                 &chain_config_clone,
            //                 quote_router_client_clone.as_ref(),
            //             )
            //             .await
            //             {
            //                 Ok((amount_a, amount_b, _profit_2hop)) => {
            //                     let elapsed_2hop = start_time.elapsed();
            //                     // Now find the best B -> X pool and compute final profit
            //                     let start_3hop_time = std::time::Instant::now();
            //                     match utils::find_best_b_to_x_swap(
            //                         http_client_for_3hop.clone(),
            //                         &dex_pair_api_for_3hop,
            //                         token_b,
            //                         token_x,
            //                         amount_b,
            //                         provider_for_compute.clone(),
            //                         &chain_config_clone,
            //                         quote_router_client_clone.as_ref(),
            //                     )
            //                     .await
            //                     {
            //                         Ok((_best_pool, amount_x_final)) => {
            //                             let elapsed_3hop = start_3hop_time.elapsed();
            //                             // Calculate profit using U256 to avoid overflow
            //                             let net_profit = if amount_x_final >= flashloan_amount {
            //                                 (amount_x_final - flashloan_amount).as_u128() as i128
            //                             } else {
            //                                 -((flashloan_amount - amount_x_final).as_u128() as i128)
            //                             };

            //                             if net_profit > 0 {
            //                                 let net_profit_after_decimals = net_profit as f64
            //                                     / (10u128.pow(base_token_decimals as u32) as f64);
            //                                 let net_profit_value = net_profit_after_decimals
            //                                     * (if found_base_token_is_stable {
            //                                         1.0
            //                                     } else {
            //                                         eth_price
            //                                     });
            //                                 info!(
            //                                         "💰 PROFITABLE 3-HOP! {} -> {} -> {} -> {} | Net profit: {:.2} {} | X->A->B: {:.2}ms, B->X: {:.2}ms, Total: {:.2}ms",
            //                                         flashloan_amount,
            //                                         amount_a,
            //                                         amount_b,
            //                                         amount_x_final,
            //                                         net_profit_value,
            //                                         if found_base_token_is_stable { "USDT" } else { "ETH" },
            //                                         elapsed_2hop.as_secs_f64() * 1000.0,
            //                                         elapsed_3hop.as_secs_f64() * 1000.0,
            //                                         (elapsed_2hop + elapsed_3hop).as_secs_f64() * 1000.0
            //                                     );

            //                                 let hop1 = utils::build_exec_hop(
            //                                     &opp_clone.buy_pool,
            //                                     token_x,
            //                                     token_a,
            //                                     &chain_config_clone,
            //                                 );
            //                                 let hop2 = utils::build_exec_hop(
            //                                     &opp_clone.sell_pool,
            //                                     token_a,
            //                                     token_b,
            //                                     &chain_config_clone,
            //                                 );
            //                                 let hop3 = utils::build_exec_hop(
            //                                     &_best_pool,
            //                                     token_b,
            //                                     token_x,
            //                                     &chain_config_clone,
            //                                 );

            //                                 match (hop1, hop2, hop3) {
            //                                     (Ok(h1), Ok(h2), Ok(h3)) => {
            //                                         match executor
            //                                             .execute_flashloan(
            //                                                 opp_clone.clone(),
            //                                                 vec![h1, h2, h3],
            //                                                 flashloan_amount,
            //                                                 token_x,
            //                                                 Some(net_profit_value),
            //                                             )
            //                                             .await
            //                                         {
            //                                             Ok(res) => info!(
            //                                                 "⚡️ 3-Hop Flashloan executed! Hash: {}",
            //                                                 res.tx_hash
            //                                             ),
            //                                             Err(e) => {
            //                                                 warn!("⚠️ Flashloan aborted: {}", e)
            //                                             }
            //                                         }
            //                                     }
            //                                     _ => warn!("Failed building 3-Hop ExecHops"),
            //                                 }
            //                             } else {
            //                                 let net_profit_abs = net_profit.abs();
            //                                 let net_profit_after_decimals = net_profit_abs as f64
            //                                     / (10u128.pow(base_token_decimals as u32) as f64);
            //                                 let net_profit_value = net_profit_after_decimals
            //                                     * (if found_base_token_is_stable {
            //                                         1.0
            //                                     } else {
            //                                         eth_price
            //                                     });
            //                                 info!(
            //                                     "3-hop unprofitable: net_profit = -{:.4} | X->A->B: {:.2}ms, B->X: {:.2}ms, Total: {:.2}ms",
            //                                     net_profit_value,
            //                                     elapsed_2hop.as_secs_f64() * 1000.0,
            //                                     elapsed_3hop.as_secs_f64() * 1000.0,
            //                                     (elapsed_2hop + elapsed_3hop).as_secs_f64() * 1000.0
            //                                 );
            //                             }
            //                         }
            //                         Err(e) => {
            //                             let elapsed_3hop = start_3hop_time.elapsed();
            //                             warn!(
            //                                     "Failed to find best B -> X swap: {:?} | X->A->B: {:.2}ms, B->X: {:.2}ms",
            //                                     e,
            //                                     elapsed_2hop.as_secs_f64() * 1000.0,
            //                                     elapsed_3hop.as_secs_f64() * 1000.0
            //                                 );
            //                         }
            //                     }
            //                 }
            //                 Err(e) => {
            //                     warn!("Failed to compute X -> A -> B amounts: {:?}", e);
            //                 }
            //             }
            //         }
            //     });

            //     // Store opportunity in memory (for API/dashboard later)
            //     let opp_key = format!(
            //         "{}_{}",
            //         opp.token_address,
            //         SystemTime::now()
            //             .duration_since(UNIX_EPOCH)
            //             .unwrap()
            //             .as_secs()
            //     );
            //     opportunities_detected.insert(opp_key, opp.clone());
            // }

            executions.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        } else {
            info!(
                "No arbitrage opportunities found for token {}",
                token_address
            );
        }
    }
}
