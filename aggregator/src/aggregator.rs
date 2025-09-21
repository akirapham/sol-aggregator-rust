use solana_sdk::pubkey::Pubkey;
use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::sync::Arc;

use crate::constants::BASE_TOKENS;
use crate::pool_data_types::{DexType, PoolState};
use crate::pool_manager::PoolStateManager;
use crate::types::{AggregatorConfig, SwapParams, SwapStep};
use crate::utils::{calculate_min_output_amount, tokens_equal};
/// Main DEX aggregator that finds the best routes across multiple DEXs with real-time data
pub struct DexAggregator {
    config: AggregatorConfig,
    pool_manager: Arc<PoolStateManager>,
}

#[derive(Debug, Clone)]
pub struct SwapPath {
    pub dex: DexType,
    pub steps: Vec<SwapStep>,
    pub input_amount: u64,
    pub output_amount: u64,
}

#[derive(Debug, Clone)]
pub struct SwapRoute {
    pub paths: Vec<SwapPath>,
    pub input_token: Pubkey,
    pub output_token: Pubkey,
    pub input_amount: u64,
    pub output_amount: u64,
    pub other_output_amount: u64,
    pub slippage_bps: u16,
}

impl DexAggregator {
    /// Create a new DEX aggregator with the given configuration
    pub fn new(config: AggregatorConfig, pool_manager: Arc<PoolStateManager>) -> Self {
        Self::new_with_pool_manager(config, pool_manager)
    }

    /// Create a new DEX aggregator with a specific pool manager
    pub fn new_with_pool_manager(
        config: AggregatorConfig,
        pool_manager: Arc<PoolStateManager>,
    ) -> Self {
        Self {
            config,
            pool_manager,
        }
    }

    /// Get access to the pool manager
    pub fn get_pool_manager(&self) -> &Arc<PoolStateManager> {
        &self.pool_manager
    }

    pub async fn get_swap_route(&self, swap_param: &SwapParams) -> Option<SwapRoute> {
        // First, direct path
        let direct_pool_addresses = self
            .pool_manager
            .get_pool_addresses_for_pair(
                &swap_param.input_token.address,
                &swap_param.output_token.address,
            )
            .await;

        // Then, 2-hop route through an intermediary base token
        // input -> base -> output
        let mut input_to_base_pools = HashSet::new();
        let mut base_to_output_pools = HashSet::new();
        let mut input_to_base_pool_addresses_by_pair: HashMap<(Pubkey, Pubkey), HashSet<Pubkey>> =
            HashMap::new();
        let mut base_to_output_pool_addresses_by_pair: HashMap<(Pubkey, Pubkey), HashSet<Pubkey>> =
            HashMap::new();

        // Loop over BASE_TOKENS to find hop routes
        for base_token in BASE_TOKENS.iter() {
            let base_token_key = Pubkey::from_str(base_token).unwrap();

            // Skip if base token is same as input or output
            if tokens_equal(&base_token_key, &swap_param.input_token.address)
                || tokens_equal(&base_token_key, &swap_param.output_token.address)
            {
                continue;
            }

            // Find pools: input -> base
            let pools = self
                .pool_manager
                .get_pool_addresses_for_pair(&swap_param.input_token.address, &base_token_key)
                .await;
            input_to_base_pools.extend(pools.clone());
            input_to_base_pool_addresses_by_pair
                .insert((swap_param.input_token.address, base_token_key), pools);

            // Find pools: base -> output
            let pools = self
                .pool_manager
                .get_pool_addresses_for_pair(&base_token_key, &swap_param.output_token.address)
                .await;
            base_to_output_pools.extend(pools.clone());
            base_to_output_pool_addresses_by_pair
                .insert((base_token_key, swap_param.output_token.address), pools);
        }

        // Collect all pool states we need
        let mut all_pool_state: HashMap<Pubkey, Arc<PoolState>> = HashMap::new();
        for pool_address in direct_pool_addresses
            .iter()
            .chain(input_to_base_pools.iter())
            .chain(base_to_output_pools.iter())
        {
            if let Some(pool_state) = self
                .pool_manager
                .get_pool_state_by_address(pool_address)
                .await
            {
                all_pool_state.insert(*pool_address, Arc::new(pool_state));
            }
        }

        // 1. Find best direct paths
        let mut top_direct_paths = direct_pool_addresses
            .iter()
            .filter_map(|pool_address| {
                all_pool_state
                    .get(pool_address)
                    .map(|pool_state| (pool_address, pool_state.get_liquidity_usd()))
            })
            .collect::<Vec<_>>();
        top_direct_paths.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        // truncate pool liquidity less than 1000 USD
        top_direct_paths.retain(|(_, liquidity)| *liquidity > 1000.0);

        // Compute output amount for each direct path
        let mut best_direct_route: Option<SwapRoute> = None;
        let mut best_direct_output = 0u64;

        for (pool_address, _liquidity) in top_direct_paths.iter() {
            if let Some(pool_state) = all_pool_state.get(pool_address) {
                let output_amount = pool_state.calculate_output_amount(
                    &swap_param.input_token.address,
                    swap_param.input_amount,
                );
                if output_amount > best_direct_output {
                    best_direct_output = output_amount;
                    best_direct_route = Some(SwapRoute {
                        paths: vec![SwapPath {
                            steps: vec![SwapStep {
                                dex: pool_state.dex(),
                                input_token: swap_param.input_token.address.to_string(),
                                output_token: swap_param.output_token.address.to_string(),
                                input_amount: swap_param.input_amount,
                                output_amount: best_direct_output,
                                percent: 100,
                                pool_address: pool_address.to_string(),
                            }],
                            input_amount: swap_param.input_amount,
                            output_amount,
                            dex: pool_state.dex(),
                        }],
                        input_token: swap_param.input_token.address,
                        output_token: swap_param.output_token.address,
                        input_amount: swap_param.input_amount,
                        output_amount,
                        other_output_amount: calculate_min_output_amount(
                            output_amount,
                            swap_param.slippage_bps as u64,
                        ),
                        slippage_bps: swap_param.slippage_bps,
                    });
                }
            }
        }

        log::info!("best_direct_route: {:?}", best_direct_route.clone());

        // 2. Find best one-hop routes through base tokens
        let mut best_hop_route: Option<SwapRoute> = None;
        let mut best_hop_output = 0u64;

        for base_token in BASE_TOKENS.iter() {
            let base_token_key = Pubkey::from_str(base_token).unwrap();

            // Skip if base token is same as input or output
            if tokens_equal(&base_token_key, &swap_param.input_token.address)
                || tokens_equal(&base_token_key, &swap_param.output_token.address)
            {
                continue;
            }

            // Find best pool for input -> base
            let input_to_base_pools: Vec<(&Pubkey, &Arc<PoolState>)> =
                input_to_base_pool_addresses_by_pair
                    .get(&(swap_param.input_token.address, base_token_key))
                    .map(|addrs| {
                        addrs
                            .iter()
                            .filter_map(|pool_addr| {
                                all_pool_state
                                    .get(pool_addr)
                                    .map(|pool_state| (pool_addr, pool_state))
                            })
                            .filter(|(_, pool_state)| {
                                // Filter out pools with very low liquidity
                                pool_state.get_liquidity_usd() > 1000.0
                            })
                            .collect()
                    })
                    .unwrap_or_default();

            // Find best pool for base -> output
            let base_to_output_pools: Vec<(&Pubkey, &Arc<PoolState>)> =
                base_to_output_pool_addresses_by_pair
                    .get(&(base_token_key, swap_param.output_token.address))
                    .map(|addrs| {
                        addrs
                            .iter()
                            .filter_map(|pool_addr| {
                                all_pool_state
                                    .get(pool_addr)
                                    .map(|pool_state| (pool_addr, pool_state))
                            })
                            .filter(|(_, pool_state)| {
                                // Filter out pools with very low liquidity
                                pool_state.get_liquidity_usd() > 1000.0
                            })
                            .collect()
                    })
                    .unwrap_or_default();

            if input_to_base_pools.is_empty() || base_to_output_pools.is_empty() {
                continue;
            }

            // Try all combinations of input->base and base->output pools
            for (input_to_base_pool_addr, input_to_base_pool) in &input_to_base_pools {
                for (base_to_output_pool_addr, base_to_output_pool) in &base_to_output_pools {
                    // Calculate intermediate amount (input -> base)
                    let intermediate_amount = input_to_base_pool.calculate_output_amount(
                        &swap_param.input_token.address,
                        swap_param.input_amount,
                    );

                    if intermediate_amount == 0 {
                        continue;
                    }

                    // Calculate final output amount (base -> output)
                    let final_output_amount = base_to_output_pool
                        .calculate_output_amount(&base_token_key, intermediate_amount);

                    if final_output_amount > best_hop_output {
                        best_hop_output = final_output_amount;
                        best_hop_route = Some(SwapRoute {
                            paths: vec![SwapPath {
                                steps: vec![
                                    SwapStep {
                                        dex: input_to_base_pool.dex(),
                                        input_token: swap_param.input_token.address.to_string(),
                                        output_token: base_token_key.to_string(),
                                        input_amount: swap_param.input_amount,
                                        output_amount: intermediate_amount,
                                        percent: 100,
                                        pool_address: input_to_base_pool_addr.to_string(),
                                    },
                                    SwapStep {
                                        dex: base_to_output_pool.dex(),
                                        input_token: base_token_key.to_string(),
                                        output_token: swap_param.output_token.address.to_string(),
                                        input_amount: intermediate_amount,
                                        output_amount: final_output_amount,
                                        percent: 100,
                                        pool_address: base_to_output_pool_addr.to_string(),
                                    },
                                ],
                                input_amount: swap_param.input_amount,
                                output_amount: intermediate_amount,
                                dex: input_to_base_pool.dex(),
                            }],
                            input_token: swap_param.input_token.address,
                            output_token: base_token_key,
                            input_amount: swap_param.input_amount,
                            output_amount: final_output_amount,
                            other_output_amount: calculate_min_output_amount(
                                final_output_amount,
                                swap_param.slippage_bps as u64,
                            ),
                            slippage_bps: swap_param.slippage_bps,
                        });
                    }
                }
            }
        }

        // 3. Compare direct and hop routes, return the best one
        log::info!(
            "Best direct output: {}, Best hop output: {}",
            best_direct_output,
            best_hop_output
        );
        match (best_direct_route, best_hop_route) {
            (Some(direct), Some(hop)) => {
                if direct.output_amount >= hop.output_amount {
                    Some(direct)
                } else {
                    Some(hop)
                }
            }
            (Some(direct), None) => Some(direct),
            (None, Some(hop)) => Some(hop),
            (None, None) => None,
        }
    }

    /// Find the best route for a swap using smart routing
    // pub async fn find_best_route(&self, params: &SwapParams) -> Result<BestRoute> {
    //     // Use smart routing engine for advanced route finding
    //     self.smart_routing.find_optimal_route(params).await
    // }

    /// Alias for find_best_route to maintain compatibility
    // pub async fn get_best_route(&self, params: &SwapParams) -> Result<Option<BestRoute>> {
    //     match self.find_best_route(params).await {
    //         Ok(route) => Ok(Some(route)),
    //         Err(_) => Ok(None),
    //     }
    // }

    // /// Find the best route using basic routing (fallback)
    // pub async fn find_best_route_basic(&self, params: &SwapParams) -> Result<BestRoute> {
    //     let mut all_routes = Vec::new();

    //     // Query all enabled DEXs in parallel
    //     let mut tasks = Vec::new();

    //     for (dex_type, dex) in &self.dexs {
    //         if self.config.enabled_dexs.contains(dex_type) {
    //             let dex_clone = dex.as_ref();
    //             let params_clone = params.clone();
    //             let dex_type_clone = *dex_type;

    //             let task = async move {
    //                 match timeout(
    //                     Duration::from_secs(5),
    //                     dex_clone.get_best_route(&params_clone),
    //                 )
    //                 .await
    //                 {
    //                     Ok(Ok(Some(route))) => Some((dex_type_clone, route)),
    //                     Ok(Ok(None)) => None,
    //                     Ok(Err(e)) => {
    //                         log::warn!("DEX {} error: {}", dex_type_clone, e);
    //                         None
    //                     }
    //                     Err(_) => {
    //                         log::warn!("DEX {} timeout", dex_type_clone);
    //                         None
    //                     }
    //                 }
    //             };

    //             tasks.push(task);
    //         }
    //     }

    //     // Wait for all DEX queries to complete
    //     let results = future::join_all(tasks).await;

    //     for result in results {
    //         if let Some((_dex_type, route)) = result {
    //             all_routes.push(route);
    //         }
    //     }

    //     if all_routes.is_empty() {
    //         return Err(DexAggregatorError::RouteNotFound);
    //     }

    //     // Sort routes by output amount (descending)
    //     all_routes.sort_by(|a, b| b.output_amount.cmp(&a.output_amount));

    //     // Take the best routes up to max_routes
    //     let selected_routes: Vec<SwapRoute> = all_routes
    //         .into_iter()
    //         .take(self.config.max_routes)
    //         .collect();

    //     // Calculate totals
    //     let total_input_amount = selected_routes.iter().map(|r| r.input_amount).sum();
    //     let total_output_amount = selected_routes.iter().map(|r| r.output_amount).sum();
    //     let total_fee = selected_routes.iter().map(|r| r.fee).sum();

    //     // Calculate weighted average price impact
    //     let total_price_impact = if !selected_routes.is_empty() {
    //         let weighted_impact: Decimal = selected_routes
    //             .iter()
    //             .map(|r| r.price_impact * Decimal::from(r.input_amount))
    //             .sum::<Decimal>()
    //             / Decimal::from(total_input_amount);
    //         weighted_impact
    //     } else {
    //         Decimal::ZERO
    //     };

    //     // Determine execution priority based on route characteristics
    //     let execution_priority =
    //         self.determine_execution_priority(&selected_routes, params.priority);

    //     let total_gas_cost = selected_routes.iter().map(|r| r.gas_cost).sum();
    //     let estimated_execution_time_ms = selected_routes
    //         .iter()
    //         .map(|r| r.execution_time_ms)
    //         .max()
    //         .unwrap_or(0);
    //     let max_mev_risk = selected_routes
    //         .iter()
    //         .map(|r| r.mev_risk)
    //         .max()
    //         .unwrap_or(MevRisk::Low);

    //     Ok(BestRoute {
    //         routes: selected_routes,
    //         total_input_amount,
    //         total_output_amount,
    //         total_price_impact,
    //         execution_priority,
    //         max_mev_risk,
    //         route_type: RouteType::SingleHop,
    //         split_ratio: None,
    //     })
    // }

    /// Get configuration
    pub fn get_config(&self) -> &AggregatorConfig {
        &self.config
    }
}
