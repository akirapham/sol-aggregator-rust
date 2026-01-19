use crate::constants::BASE_TOKENS;
use crate::pool_data_types::{traits::BuildSwapInstruction, DexType, GetAmmConfig, PoolState};
use crate::pool_manager::PoolStateManager;
use crate::types::{AggregatorConfig, ExecutionPriority, SwapParams};
use crate::utils::{calculate_min_output_amount, tokens_equal};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    instruction::Instruction, message::Message, pubkey::Pubkey, transaction::Transaction,
};
use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::sync::Arc;

#[allow(unused)]
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
    pub pool_state: Arc<PoolState>, // Made public for trait access
    pub input_amount: u64,
    pub output_amount: u64,
    pub percent: u64,
}

#[allow(unused)]
#[derive(Debug, Clone)]
pub struct SwapPath {
    pub steps: Vec<SwapStepInternal>,
    pub input_amount: u64,
    pub output_amount: u64,
}

#[allow(unused)]
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
        // Preserve backward compatibility: call new implementation with empty exclude set
        let exclude_pools: HashSet<Pubkey> = HashSet::new();
        self.get_swap_route_with_exclude(swap_param, &exclude_pools, false)
            .await
    }

    /// Get swap route with ability to exclude a set of pools (by address)
    pub async fn get_swap_route_with_exclude(
        &self,
        swap_param: &SwapParams,
        exclude_pools: &HashSet<Pubkey>,
        direct_only: bool,
    ) -> Option<SwapRoute> {
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
            // Skip pools in the exclude list
            if exclude_pools.contains(pool_address) {
                log::debug!("Skipping excluded pool: {}", pool_address);
                continue;
            }

            // Get pool state to check type
            let pool_state = self
                .pool_manager
                .get_pool_state_by_address(pool_address)
                .await;

            if let Some(pool_state) = pool_state {
                let no_needs_tick_sync = matches!(pool_state.dex(), DexType::PumpFun)
                    || matches!(pool_state.dex(), DexType::RaydiumCpmm)
                    || matches!(pool_state.dex(), DexType::PumpFunSwap)
                    || matches!(pool_state.dex(), DexType::Raydium)
                    || matches!(pool_state.dex(), DexType::MeteoraDbc)
                    || matches!(pool_state.dex(), DexType::MeteoraDammV2);
                if !no_needs_tick_sync && !self.pool_manager.is_pool_tick_synced(pool_address).await
                {
                    continue;
                }
                if pool_state.get_liquidity_usd() < min_liquidity_usd {
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
        // Skip hop routes if direct_only is true
        if !direct_only {
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
        } // End of if !direct_only block

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
                    splits_with_distributions[0][i] = Some(SwapPath {
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

    /// Calculate arbitrage profit for a round-trip swap: tokenA -> tokenB -> tokenA
    /// Forward route finds best price (may split if one pool is mispriced)
    /// Reverse route uses best available route (direct or multi-hop)
    /// Returns (profit_amount, forward_route, reverse_route) if profitable, None otherwise
    pub async fn calculate_arbitrage_profit(
        &self,
        token_a: &SwapParams,
        token_b_address: &Pubkey,
        slippage_bps: u16,
    ) -> Option<(i64, SwapRoute, SwapRoute)> {
        let exclude_pools: HashSet<Pubkey> = HashSet::new();

        // Step 1: Get best forward route from tokenA -> tokenB
        // Allow splits to detect when one pool has mispricing vs others
        // But limit to direct paths only (no multi-hop) to keep it simple
        let forward_route = self
            .get_swap_route_with_exclude(token_a, &exclude_pools, true)
            .await?;

        // Check if we got a valid route (1 or 2 paths max for potential split)
        if forward_route.paths.is_empty() || forward_route.paths.len() > 2 {
            return None;
        }

        // Ensure all paths are direct (single pool each)
        for path in &forward_route.paths {
            if path.steps.len() != 1 {
                return None;
            }
        }

        let token_b_amount = forward_route.other_output_amount;

        // Step 2: Get the tokenB info from pool manager
        let token_b = self.pool_manager.get_token(token_b_address).await?;

        // Step 3: Create reverse swap params (tokenB -> tokenA)
        let reverse_params = SwapParams {
            input_token: token_b,
            output_token: token_a.input_token.clone(),
            input_amount: token_b_amount,
            slippage_bps,
            user_wallet: token_a.user_wallet,
            priority: token_a.priority,
        };

        // Step 4: Get BEST reverse route (allow multi-hop for best price back)
        let reverse_route = self
            .get_swap_route_with_exclude(&reverse_params, &exclude_pools, false)
            .await?;

        // Step 5: Check transaction size and fallback if needed
        let mut final_reverse_route = reverse_route;

        // Check if the combined transaction is too large
        let is_too_large = self
            .estimate_transaction_size(&forward_route, &final_reverse_route, token_a.user_wallet)
            .await
            .unwrap_or_else(|e| {
                log::error!("estimate_transaction_size failed: {}", e);
                true
            }); // Treat error as too large to be safe

        if is_too_large {
            log::debug!(
                "⚠️ Transaction too large with complex route, falling back to direct route..."
            );

            // Try to get a DIRECT reverse route instead
            let direct_reverse_route = self
                .get_swap_route_with_exclude(&reverse_params, &exclude_pools, true) // direct_only = true
                .await;

            if let Some(direct_route) = direct_reverse_route {
                // Verify the direct route is actually smaller/valid
                let direct_is_too_large = self
                    .estimate_transaction_size(&forward_route, &direct_route, token_a.user_wallet)
                    .await
                    .unwrap_or_else(|e| {
                        log::error!("estimate_transaction_size failed for direct route: {}", e);
                        true
                    });

                if !direct_is_too_large {
                    log::debug!("✅ Fallback to direct route successful");
                    final_reverse_route = direct_route;
                } else {
                    log::debug!("❌ Direct route also too large or check failed");
                    return None;
                }
            } else {
                log::debug!("❌ No direct fallback route available");
                return None;
            }
        }

        // Recalculate profit with the final route
        let final_token_a_amount = final_reverse_route.other_output_amount;
        let profit = final_token_a_amount as i64 - token_a.input_amount as i64;
        let profit_percent = (profit as f64 / token_a.input_amount as f64) * 100.0;

        log::info!(
            "final_token_a_amount: {} input_amount: {} forward_route min output amount: {} profit: {} input token: {} output token: {} forward_route paths: {:?} reverse_route paths: {:?}",
            final_token_a_amount,
            token_a.input_amount,
            forward_route.other_output_amount,
            profit,
            token_a.input_token.address,
            token_a.output_token.address,
            forward_route.paths.iter().map(|p| p.steps.iter().map(|s| s.dex).collect::<Vec<_>>()).collect::<Vec<_>>(),
            final_reverse_route.paths.iter().map(|p| p.steps.iter().map(|s| s.dex).collect::<Vec<_>>()).collect::<Vec<_>>()
        );

        // Step 6: Check conditions: Profit > 0 OR Abnormal profit/loss (> 5% or < -5%)
        if profit > 0 || profit_percent.abs() > 5.0 {
            Some((profit, forward_route, final_reverse_route))
        } else {
            None
        }
    }

    /// Estimate if the transaction size is within limits (1232 bytes)
    async fn estimate_transaction_size(
        &self,
        forward_route: &SwapRoute,
        reverse_route: &SwapRoute,
        payer: Pubkey,
    ) -> Result<bool, String> {
        // We use a dummy execution priority here as it doesn't affect instructions size significantly for this check
        // or we can just use the one from config if available, but Medium is safe default.
        let priority = ExecutionPriority::Medium;

        // We don't have the rpc_client here readily available in `calculate_arbitrage_profit`
        // BUT we need it to build the transaction (get blockhash).
        // However, `build_arbitrage_transaction` requires `rpc_client`.
        // Wait, `calculate_arbitrage_profit` does NOT have rpc_client.
        // We can't easily build the FULL transaction with blockhash without RPC.
        // BUT we can build the instructions and estimate size.

        let mut all_instructions = Vec::new();

        for path in &forward_route.paths {
            for step in &path.steps {
                let instructions = self
                    .build_step_instructions(step, forward_route.slippage_bps, priority, payer)
                    .await
                    .map_err(|e| {
                        log::error!("estimate_transaction_size forward_route error: {}", e);
                        e
                    })?;
                all_instructions.extend(instructions);
            }
        }

        for path in &reverse_route.paths {
            for step in &path.steps {
                let instructions = self
                    .build_step_instructions(step, reverse_route.slippage_bps, priority, payer)
                    .await
                    .map_err(|e| {
                        log::error!("estimate_transaction_size reverse_route error: {}", e);
                        e
                    })?;
                all_instructions.extend(instructions);
            }
        }

        let all_instructions = Self::deduplicate_instructions(all_instructions);

        // Estimate size based on instructions
        // A simple transaction has header (approx 100 bytes) + instructions
        // We can construct a Transaction with a default blockhash/payer to check size
        let message = Message::new(&all_instructions, Some(&payer));

        let serialized = message.serialize();
        let size = serialized.len();

        // Legacy transaction limit is 1232 bytes
        // We add some buffer (signatures take 64 bytes per signer)
        // Message size + 1 signature (64) < 1232
        const MAX_TX_SIZE: usize = 1232;
        const SIGNATURE_SIZE: usize = 64;

        let total_size = size + SIGNATURE_SIZE;

        log::debug!(
            "📏 Estimated transaction size: {} bytes (Limit: {})",
            total_size,
            MAX_TX_SIZE
        );

        Ok(total_size > MAX_TX_SIZE)
    }

    pub async fn build_route_transaction(
        &self,
        route: &SwapRoute,
        priority: ExecutionPriority,
        payer: Pubkey,
        rpc_client: &RpcClient,
    ) -> Result<Transaction, String> {
        let mut all_instructions = Vec::new();

        for path in &route.paths {
            for step in &path.steps {
                let instructions = self
                    .build_step_instructions(step, route.slippage_bps, priority, payer)
                    .await
                    .map_err(|e| {
                        log::error!("build_route_transaction error: {}", e);
                        e
                    })?;
                all_instructions.extend(instructions);
            }
        }

        // Deduplicate instructions to prevent duplicate ATA creation across multiple swap steps
        let all_instructions = Self::deduplicate_instructions(all_instructions);

        // Build unsigned transaction for client-side signing
        let blockhash = rpc_client
            .get_latest_blockhash()
            .await
            .map_err(|e| format!("Failed to get blockhash: {}", e))?;

        let message = Message::new_with_blockhash(&all_instructions, Some(&payer), &blockhash);
        let transaction = Transaction::new_unsigned(message);
        Ok(transaction)
    }

    pub async fn build_arbitrage_transaction(
        &self,
        forward_route: &SwapRoute,
        reverse_route: &SwapRoute,
        priority: ExecutionPriority,
        payer: Pubkey,
        rpc_client: &RpcClient,
    ) -> Result<Transaction, String> {
        let mut all_instructions = Vec::new();

        for path in &forward_route.paths {
            for step in &path.steps {
                let instructions = self
                    .build_step_instructions(step, forward_route.slippage_bps, priority, payer)
                    .await
                    .map_err(|e| {
                        log::error!("build_arbitrage_transaction forward_route error: {}", e);
                        e
                    })?;
                all_instructions.extend(instructions);
            }
        }

        for path in &reverse_route.paths {
            for step in &path.steps {
                let instructions = self
                    .build_step_instructions(step, reverse_route.slippage_bps, priority, payer)
                    .await
                    .map_err(|e| {
                        log::error!("build_arbitrage_transaction reverse_route error: {}", e);
                        e
                    })?;
                all_instructions.extend(instructions);
            }
        }

        // Deduplicate instructions to prevent duplicate ATA creation across multiple swap steps
        let all_instructions = Self::deduplicate_instructions(all_instructions);

        // Build unsigned transaction for client-side signing
        let blockhash = rpc_client
            .get_latest_blockhash()
            .await
            .map_err(|e| format!("Failed to get blockhash: {}", e))?;

        use solana_sdk::message::Message;
        let message = Message::new_with_blockhash(&all_instructions, Some(&payer), &blockhash);
        let transaction = Transaction::new_unsigned(message);

        Ok(transaction)
    }

    /// Deduplicate instructions by comparing program_id, accounts, and data
    /// This prevents duplicate ATA creation instructions in multi-step transactions
    fn deduplicate_instructions(instructions: Vec<Instruction>) -> Vec<Instruction> {
        let mut seen = HashSet::new();
        let mut deduped = Vec::new();

        for ix in instructions {
            // Create a unique key for this instruction
            let key = (
                ix.program_id,
                ix.accounts
                    .iter()
                    .map(|a| (a.pubkey, a.is_signer, a.is_writable))
                    .collect::<Vec<_>>(),
                ix.data.clone(),
            );

            if seen.insert(key) {
                deduped.push(ix);
            }
        }

        deduped
    }

    /// Build swap instructions for a single step using the pool state (works for all DEX types)
    async fn build_step_instructions(
        &self,
        step: &SwapStepInternal,
        slippage_bps: u16,
        priority: ExecutionPriority,
        payer: Pubkey,
    ) -> Result<Vec<Instruction>, String> {
        let self_arc: Arc<dyn GetAmmConfig> = self.pool_manager.clone();
        let token_a = self.pool_manager.get_token(&step.input_token).await;
        let token_b = self.pool_manager.get_token(&step.output_token).await;

        let params = SwapParams {
            input_token: token_a.unwrap(),
            output_token: token_b.unwrap(),
            input_amount: step.input_amount,
            slippage_bps,
            user_wallet: payer,
            priority,
        };

        step.pool_state
            .build_swap_instruction(&params, self_arc)
            .await
            .map_err(|e| {
                log::error!(
                    "Failed to build swap instruction for DEX {:?} Pool {}: {}",
                    step.pool_state.dex(),
                    step.pool_address,
                    e
                );
                e
            })
    }

    #[allow(unused)]
    /// Get configuration
    pub fn get_config(&self) -> &AggregatorConfig {
        &self.config
    }
}
