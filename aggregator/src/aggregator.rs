use solana_sdk::pubkey::Pubkey;
use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::sync::Arc;

use crate::constants::BASE_TOKENS;
use crate::pool_data_types::{DexType, PoolState};
use crate::pool_manager::PoolStateManager;
use crate::types::{AggregatorConfig, SwapParams};
use crate::utils::{calculate_min_output_amount, tokens_equal};
/// Main DEX aggregator that finds the best routes across multiple DEXs with real-time data
pub struct DexAggregator {
    config: AggregatorConfig,
    pool_manager: Arc<PoolStateManager>,
}

#[derive(Debug, Clone)]
pub struct SwapStepInternal {
    pub dex: DexType,
    pub input_token: Pubkey,
    pub output_token: Pubkey,
    pub pool_address: Pubkey,
    pool_state: Arc<PoolState>,
    pub input_amount: u64,
    pub output_amount: u64,
    pub percent: u64,
}

#[derive(Debug, Clone)]
pub struct SwapPath {
    pub steps: Vec<SwapStepInternal>,
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
        let min_liquidity_usd = 1000.0_f64;
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
                if pool_state.get_liquidity_usd() < min_liquidity_usd {
                    // Skip pools with very low liquidity
                    continue;
                }
                all_pool_state.insert(*pool_address, Arc::new(pool_state));
            }
        }

        // 0. prepare percent distribution for smart routing
        let base_percent = 5; // 5% per base token
                               // generate percent distribution array [0, 5, 10, ..., 100]
        let percent_distribution: Vec<u64> =
            (0..=100 / base_percent).map(|i| i * base_percent).collect();

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
        top_direct_paths.retain(|(_, liquidity)| *liquidity > min_liquidity_usd);

        // Compute output amount for each direct path
        let mut best_direct_route: Option<Vec<SwapStepInternal>> = None;
        let mut best_direct_output = 0u64;

        for (pool_address, _liquidity) in top_direct_paths.iter() {
            if let Some(pool_state) = all_pool_state.get(pool_address) {
                let output_amount = pool_state.calculate_output_amount(
                    &swap_param.input_token.address,
                    swap_param.input_amount,
                );
                if output_amount > best_direct_output {
                    best_direct_output = output_amount;
                    best_direct_route = Some(vec![SwapStepInternal {
                        dex: pool_state.dex(),
                        input_token: swap_param.input_token.address,
                        output_token: swap_param.output_token.address,
                        pool_address: **pool_address,
                        pool_state: pool_state.clone(),
                        input_amount: swap_param.input_amount,
                        output_amount,
                        percent: 100,
                    }]);
                }
            }
        }

        // initilize with count percent_distribution.len() of None
        let mut best_direct_route_distributions: Vec<Option<SwapPath>> =
            vec![None; percent_distribution.len()];

        if let Some(ref swap_steps) = best_direct_route {
            if !swap_steps.is_empty() {
                let swap_step = &swap_steps[0];
                for (i, percent) in percent_distribution.iter().enumerate() {
                    let input_amount = swap_param.input_amount * percent / 100;
                    let output_amount = swap_step
                        .pool_state
                        .calculate_output_amount(&swap_param.input_token.address, input_amount);
                    best_direct_route_distributions[i] = Some(SwapPath {
                        steps: vec![SwapStepInternal {
                            dex: swap_step.dex,
                            input_token: swap_step.input_token,
                            output_token: swap_step.output_token,
                            pool_address: swap_step.pool_address,
                            input_amount,
                            output_amount,
                            percent: *percent,
                            pool_state: swap_step.pool_state.clone(),
                        }],
                        input_amount,
                        output_amount,
                    });
                }
            }
        }

        // 2. Find best one-hop routes through base tokens
        let mut best_hop_route: Option<Vec<SwapStepInternal>> = None;
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
                                pool_state.get_liquidity_usd() > min_liquidity_usd
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
                                pool_state.get_liquidity_usd() > min_liquidity_usd
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
                        best_hop_route = Some(vec![
                            SwapStepInternal {
                                dex: input_to_base_pool.dex(),
                                input_token: swap_param.input_token.address,
                                output_token: base_token_key,
                                pool_address: **input_to_base_pool_addr,
                                pool_state: input_to_base_pool.clone().clone(),
                                input_amount: swap_param.input_amount,
                                output_amount: intermediate_amount,
                                percent: 100,
                            },
                            SwapStepInternal {
                                dex: base_to_output_pool.dex(),
                                input_token: base_token_key,
                                output_token: swap_param.output_token.address,
                                pool_address: **base_to_output_pool_addr,
                                pool_state: base_to_output_pool.clone().clone(),
                                input_amount: intermediate_amount,
                                output_amount: final_output_amount,
                                percent: 100,
                            },
                        ]);
                    }
                }
            }
        }

        // initilize with count percent_distribution.len() of None
        let mut best_hop_route_distributions: Vec<Option<SwapPath>> =
            vec![None; percent_distribution.len()];

        if let Some(ref swap_steps) = best_hop_route {
            if swap_steps.len() == 2 {
                let swap_step_1 = &swap_steps[0];
                let swap_step_2 = &swap_steps[1];
                for (i, percent) in percent_distribution.iter().enumerate() {
                    let input_amount = swap_param.input_amount * percent / 100;
                    let intermediate_amount = swap_step_1
                        .pool_state
                        .calculate_output_amount(&swap_param.input_token.address, input_amount);
                    let output_amount = swap_step_2
                        .pool_state
                        .calculate_output_amount(&swap_step_1.output_token, intermediate_amount);
                    best_hop_route_distributions[i] = Some(SwapPath {
                        steps: vec![
                            SwapStepInternal {
                                dex: swap_step_1.dex,
                                input_token: swap_step_1.input_token,
                                output_token: swap_step_1.output_token,
                                pool_address: swap_step_1.pool_address,
                                input_amount,
                                output_amount: intermediate_amount,
                                percent: *percent,
                                pool_state: swap_step_1.pool_state.clone(),
                            },
                            SwapStepInternal {
                                dex: swap_step_2.dex,
                                input_token: swap_step_2.input_token,
                                output_token: swap_step_2.output_token,
                                pool_address: swap_step_2.pool_address,
                                input_amount: intermediate_amount,
                                output_amount,
                                percent: *percent, // TODO: what percent to use here?
                                pool_state: swap_step_2.pool_state.clone(),
                            },
                        ],
                        input_amount,
                        output_amount,
                    });
                }
            }
        }

        // Combine best direct and hop routes
        let mut swap_route: SwapRoute = SwapRoute {
            paths: vec![],
            input_token: swap_param.input_token.address,
            output_token: swap_param.output_token.address,
            input_amount: swap_param.input_amount,
            output_amount: 0,
            other_output_amount: 0,
            slippage_bps: swap_param.slippage_bps,
        };
        let len = percent_distribution.len();
        for i in 0..len {
            let mut combined_output_amount = 0;
            let mut current_paths = vec![];
            if let Some(direct_path) = &best_direct_route_distributions[i] {
                if direct_path.output_amount > 0 {
                    current_paths.push(direct_path.clone());
                    combined_output_amount += direct_path.output_amount;
                }
            }
            if let Some(hop_path) = &best_hop_route_distributions[len - 1 - i] {
                if hop_path.output_amount > 0 {
                    current_paths.push(hop_path.clone());
                    combined_output_amount += hop_path.output_amount;
                }
            }

            if combined_output_amount > swap_route.output_amount {
                swap_route.output_amount = combined_output_amount;
                swap_route.paths = current_paths;
            }
        }

        swap_route.other_output_amount =
            calculate_min_output_amount(swap_route.output_amount, swap_param.slippage_bps as u64);
        // Return smart route with split paths
        Some(swap_route)
    }

    /// Get configuration
    pub fn get_config(&self) -> &AggregatorConfig {
        &self.config
    }
}
