use rust_decimal::Decimal;
use solana_sdk::pubkey::Pubkey;
use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::sync::Arc;

use crate::constants::BASE_TOKENS;
use crate::pool_manager::PoolStateManager;
use crate::types::{AggregatorConfig, ExecutionPriority, SwapParams};
use crate::utils::calculate_min_output_amount;
/// Main DEX aggregator that finds the best routes across multiple DEXs with real-time data
pub struct DexAggregator {
    config: AggregatorConfig,
    pool_manager: Arc<PoolStateManager>,
}

pub struct SwapPath {
    pub pool_addresses: Vec<Pubkey>,
    pub input_token: Pubkey,
    pub output_token: Pubkey,
    pub input_amount: u64,
    pub output_amount: u64,
}

pub struct SwapRoute {
    pub paths: Vec<SwapPath>,
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
        // first, direct path
        let direct_pool_addresses = self
            .pool_manager
            .get_pool_addresses_for_pair(&swap_param.input_token.address, &swap_param.output_token.address).await;

        // then, 2-hop route through an intermediary base token
        // input -> base -> output
        let mut input_to_base_pools = HashSet::new();
        // loop over BASE_TOKENS
        for base_token in BASE_TOKENS.iter() {
            let base_token_key = Pubkey::from_str(base_token).unwrap();
            let pools = self
                .pool_manager
                .get_pool_addresses_for_pair(&swap_param.input_token.address, &base_token_key)
                .await;
            input_to_base_pools.extend(pools);
        }

        let mut all_pool_state = HashMap::new();
        for pool_address in direct_pool_addresses.iter().chain(input_to_base_pools.iter()) {
            if let Some(pool_state) = self.pool_manager.get_pool_state_by_address(pool_address).await {
                all_pool_state.insert(*pool_address, pool_state);
            }
        }

        // find top direct oaths with highest liquidity, then sort by liquidity
        let mut top_direct_paths = direct_pool_addresses.iter().filter_map(|pool_address| {
            all_pool_state.get(pool_address).map(|pool_state| {
                (pool_address, pool_state.get_liquidity_usd())
            })
        }).collect::<Vec<_>>();
        top_direct_paths.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        // only keep top 3
        top_direct_paths.truncate(3);

        // compute output amount for each direct path
        let mut current_best_output = 0 as u64;
        let mut best_direct_pool = None;
        for (pool_address, _liquidity) in top_direct_paths.iter() {
            if let Some(pool_state) = all_pool_state.get(pool_address) {
                let output_amount = pool_state.calculate_output_amount(&swap_param.input_token.address, swap_param.input_amount);
                if output_amount > current_best_output {
                    current_best_output = output_amount;
                    best_direct_pool = Some(*pool_address);
                }
            }
        }

        if let Some(best_pool) = best_direct_pool {
            return Some(SwapRoute {
                paths: vec![SwapPath {
                    pool_addresses: vec![best_pool.clone()],
                    input_token: swap_param.input_token.address,
                    output_token: swap_param.output_token.address,
                    input_amount: swap_param.input_amount,
                    output_amount: current_best_output,
                }],
                input_amount: swap_param.input_amount,
                output_amount: current_best_output,
                other_output_amount: calculate_min_output_amount(current_best_output, swap_param.slippage_bps as u64),
                slippage_bps: swap_param.slippage_bps,
            });
        }

        None

        // find best path for direct route
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
