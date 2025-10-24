use solana_sdk::pubkey::Pubkey;
use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::sync::Arc;

use crate::constants::BASE_TOKENS;
use crate::pool_data_types::{DexType, GetAmmConfig, PoolState};
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
    pub context_slot: u64,
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
        let self_arc: Arc<dyn GetAmmConfig> = self.pool_manager.clone();
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

        // with 100% input amount
        let mut all_routes_with_out_amounts: Vec<(Vec<SwapStepInternal>, u64)> = vec![];

        for (pool_address, _liquidity) in top_direct_paths.iter() {
            if let Some(pool_state) = all_pool_state.get(pool_address) {
                let output_amount = pool_state
                    .calculate_output_amount(
                        &swap_param.input_token.address,
                        swap_param.input_amount,
                        self_arc.clone(),
                    )
                    .await;
                if output_amount > 0 {
                    all_routes_with_out_amounts.push((
                        vec![SwapStepInternal {
                            dex: pool_state.dex(),
                            input_token: swap_param.input_token.address,
                            output_token: swap_param.output_token.address,
                            pool_address: **pool_address,
                            pool_state: pool_state.clone(),
                            input_amount: swap_param.input_amount,
                            output_amount,
                            percent: 100,
                        }],
                        output_amount,
                    ));
                }
            }
        }

        // 2. Find routes with 1 hop (2 pools)
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
                    let intermediate_amount = input_to_base_pool
                        .calculate_output_amount(
                            &swap_param.input_token.address,
                            swap_param.input_amount,
                            self_arc.clone(),
                        )
                        .await;

                    if intermediate_amount == 0 {
                        continue;
                    }

                    // Calculate final output amount (base -> output)
                    let final_output_amount = base_to_output_pool
                        .calculate_output_amount(
                            &base_token_key,
                            intermediate_amount,
                            self_arc.clone(),
                        )
                        .await;

                    if final_output_amount > 0 {
                        all_routes_with_out_amounts.push((
                            vec![
                                SwapStepInternal {
                                    dex: input_to_base_pool.dex(),
                                    input_token: swap_param.input_token.address,
                                    output_token: base_token_key,
                                    pool_address: **input_to_base_pool_addr,
                                    pool_state: (*input_to_base_pool).clone(),
                                    input_amount: swap_param.input_amount,
                                    output_amount: intermediate_amount,
                                    percent: 100,
                                },
                                SwapStepInternal {
                                    dex: base_to_output_pool.dex(),
                                    input_token: base_token_key,
                                    output_token: swap_param.output_token.address,
                                    pool_address: **base_to_output_pool_addr,
                                    pool_state: (*base_to_output_pool).clone(),
                                    input_amount: intermediate_amount,
                                    output_amount: final_output_amount,
                                    percent: 100,
                                },
                            ],
                            final_output_amount,
                        ));
                    }
                }
            }
        }

        // filter top 2 routes by output amount
        all_routes_with_out_amounts.sort_by(|a, b| b.1.cmp(&a.1));
        all_routes_with_out_amounts.truncate(2);
        if all_routes_with_out_amounts.is_empty() {
            return None;
        }

        if all_routes_with_out_amounts.len() == 1 {
            // return the only route
            let (steps, output_amount) = &all_routes_with_out_amounts[0];
            return Some(SwapRoute {
                paths: vec![SwapPath {
                    steps: steps.clone(),
                    input_amount: swap_param.input_amount,
                    output_amount: *output_amount,
                }],
                input_token: swap_param.input_token.address,
                output_token: swap_param.output_token.address,
                input_amount: swap_param.input_amount,
                output_amount: *output_amount,
                other_output_amount: calculate_min_output_amount(
                    *output_amount,
                    swap_param.slippage_bps as u64,
                ),
                slippage_bps: swap_param.slippage_bps,
                context_slot: self.pool_manager.get_chain_state().await.slot,
            });
        }

        // smart route with 2 splits
        let mut splits_with_distributions: Vec<Vec<Option<SwapPath>>> = vec![
            vec![None; percent_distribution.len()],
            vec![None; percent_distribution.len()],
        ];

        for (split_index, (split, _)) in all_routes_with_out_amounts.iter().enumerate() {
            if split.len() == 1 {
                // direct route
                let swap_step = &split[0];
                for (i, percent) in percent_distribution.iter().enumerate() {
                    let input_amount = swap_param.input_amount * percent / 100;
                    let output_amount = swap_step
                        .pool_state
                        .calculate_output_amount(
                            &swap_param.input_token.address,
                            input_amount,
                            self_arc.clone(),
                        )
                        .await;
                    splits_with_distributions[split_index][i] = Some(SwapPath {
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
            } else if split.len() == 2 {
                // hop route
                let swap_step_1 = &split[0];
                let swap_step_2 = &split[1];
                for (i, percent) in percent_distribution.iter().enumerate() {
                    let input_amount = swap_param.input_amount * percent / 100;
                    let intermediate_amount = swap_step_1
                        .pool_state
                        .calculate_output_amount(
                            &swap_param.input_token.address,
                            input_amount,
                            self_arc.clone(),
                        )
                        .await;
                    let output_amount = swap_step_2
                        .pool_state
                        .calculate_output_amount(
                            &swap_step_1.output_token,
                            intermediate_amount,
                            self_arc.clone(),
                        )
                        .await;
                    splits_with_distributions[split_index][i] = Some(SwapPath {
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

        // Combine: smart routing with 2 splits
        let mut swap_route: SwapRoute = SwapRoute {
            paths: vec![],
            input_token: swap_param.input_token.address,
            output_token: swap_param.output_token.address,
            input_amount: swap_param.input_amount,
            output_amount: 0,
            other_output_amount: 0,
            slippage_bps: swap_param.slippage_bps,
            context_slot: self.pool_manager.get_chain_state().await.slot,
        };

        let len = percent_distribution.len();
        for i in 0..len {
            let mut combined_output_amount = 0;
            let mut current_paths = vec![];

            // Check if we can combine these paths (they shouldn't share pools)
            let mut can_combine = true;
            let mut used_pools = HashSet::new();

            if let Some(direct_path) = &splits_with_distributions[0][i] {
                if direct_path.output_amount > 0 {
                    // Collect pools from this path
                    for step in &direct_path.steps {
                        used_pools.insert(step.pool_address);
                    }
                    current_paths.push(direct_path.clone());
                    combined_output_amount += direct_path.output_amount;
                }
            }

            if let Some(hop_path) = &splits_with_distributions[1][len - 1 - i] {
                if hop_path.output_amount > 0 {
                    // Check if this path shares any pools with already added paths
                    for step in &hop_path.steps {
                        if used_pools.contains(&step.pool_address) {
                            can_combine = false;
                            break;
                        }
                    }

                    if can_combine {
                        current_paths.push(hop_path.clone());
                        combined_output_amount += hop_path.output_amount;
                    } else {
                        // If we can't combine, just use the path with better output
                        if let Some(direct_path) = &splits_with_distributions[0][i] {
                            if hop_path.output_amount > direct_path.output_amount {
                                current_paths.clear();
                                current_paths.push(hop_path.clone());
                                combined_output_amount = hop_path.output_amount;
                            }
                            // Otherwise keep the direct path we already added
                        } else {
                            current_paths.push(hop_path.clone());
                            combined_output_amount = hop_path.output_amount;
                        }
                    }
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
