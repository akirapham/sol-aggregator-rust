use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use solana_sdk::pubkey::Pubkey;
use std::collections::{HashMap, VecDeque};

use crate::dex::traits::DexInterface;
use crate::error::{DexAggregatorError, Result};
use crate::types::{
    BestRoute, DexType, GasConfig, MevProtectionConfig, MevRisk, RouteType, SmartRoutingConfig,
    SplitConfig, SwapParams, SwapRoute, Token,
};
use crate::utils::*;

/// Smart routing engine that finds optimal routes using advanced algorithms
pub struct SmartRoutingEngine {
    config: SmartRoutingConfig,
    gas_config: GasConfig,
    mev_config: MevProtectionConfig,
    split_config: SplitConfig,
    dexs: HashMap<DexType, Box<dyn DexInterface + Send + Sync>>,
}

impl SmartRoutingEngine {
    pub fn new(
        config: SmartRoutingConfig,
        gas_config: GasConfig,
        mev_config: MevProtectionConfig,
        split_config: SplitConfig,
        dexs: HashMap<DexType, Box<dyn DexInterface + Send + Sync>>,
    ) -> Self {
        Self {
            config,
            gas_config,
            mev_config,
            split_config,
            dexs,
        }
    }

    /// Find the optimal route using smart routing algorithms
    pub async fn find_optimal_route(&self, params: &SwapParams) -> Result<BestRoute> {
        let mut all_routes = Vec::new();

        // 1. Single-hop routes (direct swaps)
        let single_hop_routes = self.find_single_hop_routes(params).await?;
        all_routes.extend(single_hop_routes);

        // 2. Multi-hop routes (if enabled)
        if self.config.enable_multi_hop {
            let multi_hop_routes = self.find_multi_hop_routes(params).await?;
            all_routes.extend(multi_hop_routes);
        }

        // 3. Split trading routes (if enabled)
        if self.config.enable_split_trading {
            let split_routes = self.find_split_routes(params).await?;
            all_routes.extend(split_routes);
        }

        // 4. Arbitrage opportunities (if enabled)
        if self.config.enable_arbitrage_detection {
            let arbitrage_routes = self.find_arbitrage_routes(params).await?;
            all_routes.extend(arbitrage_routes);
        }

        if all_routes.is_empty() {
            return Err(DexAggregatorError::RouteNotFound);
        }

        // 5. Route optimization and selection
        let optimized_routes = self.optimize_routes(all_routes, params).await?;

        // 6. Select best route based on multiple criteria
        let best_route = self.select_best_route(optimized_routes, params).await?;

        Ok(best_route)
    }

    /// Find single-hop routes (direct swaps)
    async fn find_single_hop_routes(&self, params: &SwapParams) -> Result<Vec<SwapRoute>> {
        let mut routes = Vec::new();
        let mut tasks = Vec::new();

        for (dex_type, dex) in &self.dexs {
            let dex_clone = dex.as_ref();
            let params_clone = params.clone();
            let dex_type_clone = *dex_type;

            let task = async move {
                match dex_clone.get_best_route(&params_clone).await {
                    Ok(Some(route)) => {
                        // Enhance route with smart routing data
                        let enhanced_route = Self::enhance_route(route, &dex_type_clone).await;
                        Some(enhanced_route)
                    }
                    _ => None,
                }
            };

            tasks.push(task);
        }

        let results = futures::future::join_all(tasks).await;
        for result in results {
            if let Some(route) = result {
                routes.push(route);
            }
        }

        Ok(routes)
    }

    /// Find multi-hop routes using graph traversal
    async fn find_multi_hop_routes(&self, params: &SwapParams) -> Result<Vec<SwapRoute>> {
        let mut routes = Vec::new();

        // Build a graph of all possible token connections
        let token_graph = self.build_token_graph().await?;

        // Find all possible paths from input to output token
        let paths = self
            .find_paths(
                &token_graph,
                &params.input_token,
                &params.output_token,
                self.config.max_hops,
            )
            .await?;

        // Convert paths to routes
        for path in paths {
            if let Ok(route) = self.path_to_route(path, params).await {
                routes.push(route);
            }
        }

        Ok(routes)
    }

    /// Find split trading routes
    async fn find_split_routes(&self, params: &SwapParams) -> Result<Vec<SwapRoute>> {
        let mut routes = Vec::new();

        // Get all possible single-hop routes
        let single_routes = self.find_single_hop_routes(params).await?;

        if single_routes.len() < 2 {
            return Ok(routes); // Need at least 2 routes for splitting
        }

        // Generate split combinations
        let split_combinations = self.generate_split_combinations(
            &single_routes,
            params.input_amount,
            self.split_config.max_splits,
        );

        for combination in split_combinations {
            if let Ok(split_route) = self.create_split_route(combination, params).await {
                routes.push(split_route);
            }
        }

        Ok(routes)
    }

    /// Find arbitrage opportunities
    async fn find_arbitrage_routes(&self, params: &SwapParams) -> Result<Vec<SwapRoute>> {
        let mut routes = Vec::new();

        // Get price information from all DEXs
        let mut price_tasks = Vec::new();

        for (dex_type, dex) in &self.dexs {
            let dex_clone = dex.as_ref();
            let input_token = params.input_token;
            let output_token = params.output_token;
            let amount = params.input_amount;
            let dex_type_clone = *dex_type;

            let task = async move {
                match dex_clone
                    .get_price(&input_token, &output_token, amount)
                    .await
                {
                    Ok(price_info) => Some((dex_type_clone, price_info)),
                    _ => None,
                }
            };

            price_tasks.push(task);
        }

        let price_results = futures::future::join_all(price_tasks).await;
        let mut prices: Vec<_> = price_results.into_iter().flatten().collect();

        // Sort by price (highest first)
        prices.sort_by(|a, b| b.1.price.cmp(&a.1.price));

        // Find arbitrage opportunities
        if prices.len() >= 2 {
            let best_price = &prices[0].1;
            let worst_price = &prices[prices.len() - 1].1;

            let price_diff = best_price.price - worst_price.price;
            let min_profitable_diff = Decimal::new(1, 3); // 0.1% minimum profit

            if price_diff > min_profitable_diff {
                // Create arbitrage route
                if let Ok(arbitrage_route) = self
                    .create_arbitrage_route(&prices[0], &prices[prices.len() - 1], params)
                    .await
                {
                    routes.push(arbitrage_route);
                }
            }
        }

        Ok(routes)
    }

    /// Build a graph of all token connections across DEXs
    async fn build_token_graph(&self) -> Result<HashMap<Pubkey, Vec<(Pubkey, DexType, Pubkey)>>> {
        let mut graph = HashMap::new();

        for (dex_type, dex) in &self.dexs {
            // Get all supported tokens for this DEX
            if let Ok(tokens) = dex.get_supported_tokens().await {
                for token in &tokens {
                    let connections = graph.entry(token.address).or_insert_with(Vec::new);

                    // Find all other tokens this token can be swapped with
                    for other_token in &tokens {
                        if other_token.address != token.address {
                            if dex
                                .supports_token_pair(&token.address, &other_token.address)
                                .await
                                .unwrap_or(false)
                            {
                                connections.push((other_token.address, *dex_type, token.address));
                            }
                        }
                    }
                }
            }
        }

        Ok(graph)
    }

    /// Find all paths between two tokens using BFS
    async fn find_paths(
        &self,
        graph: &HashMap<Pubkey, Vec<(Pubkey, DexType, Pubkey)>>,
        start: &Pubkey,
        end: &Pubkey,
        max_hops: usize,
    ) -> Result<Vec<Vec<(Pubkey, DexType, Pubkey)>>> {
        let mut paths = Vec::new();
        let mut queue = VecDeque::new();
        queue.push_back((vec![*start], Vec::new()));

        while let Some((current_path, current_edges)) = queue.pop_front() {
            if current_path.len() > max_hops + 1 {
                continue;
            }

            let current_token = current_path[current_path.len() - 1];

            if current_token == *end && current_path.len() > 1 {
                paths.push(current_edges);
                continue;
            }

            if let Some(connections) = graph.get(&current_token) {
                for (next_token, dex_type, pool_address) in connections {
                    if !current_path.contains(next_token) {
                        let mut new_path = current_path.clone();
                        new_path.push(*next_token);

                        let mut new_edges = current_edges.clone();
                        new_edges.push((*next_token, *dex_type, *pool_address));

                        queue.push_back((new_path, new_edges));
                    }
                }
            }
        }

        Ok(paths)
    }

    /// Convert a path to a route
    async fn path_to_route(
        &self,
        path: Vec<(Pubkey, DexType, Pubkey)>,
        params: &SwapParams,
    ) -> Result<SwapRoute> {
        if path.is_empty() {
            return Err(DexAggregatorError::RouteNotFound);
        }

        let mut total_input = params.input_amount;
        let mut total_output = 0u64;
        let mut total_fee = 0u64;
        let mut total_gas = 0u64;
        let mut max_mev_risk = MevRisk::Low;
        let mut route_path = Vec::new();

        for (token, dex_type, pool_address) in &path {
            if let Some(dex) = self.dexs.get(dex_type) {
                let hop_params = SwapParams {
                    input_token: params.input_token,
                    output_token: *token,
                    input_amount: total_input,
                    slippage_tolerance: params.slippage_tolerance,
                    user_wallet: params.user_wallet,
                    priority: params.priority,
                };

                if let Ok(Some(hop_route)) = dex.get_best_route(&hop_params).await {
                    total_output = hop_route.output_amount;
                    total_fee += hop_route.fee;
                    total_gas += hop_route.gas_cost;
                    max_mev_risk = std::cmp::max(max_mev_risk, hop_route.mev_risk);
                    route_path.push(*pool_address);
                    total_input = total_output; // For next hop
                }
            }
        }

        // Calculate price impact for the entire path
        let price_impact = calculate_price_impact(
            params.input_amount,
            total_output,
            Decimal::new(1, 0), // Placeholder market price
        )?;

        Ok(SwapRoute {
            dex: path[0].1, // First DEX in the path
            input_token: self.get_token_info(&params.input_token).await?,
            output_token: self.get_token_info(&params.output_token).await?,
            input_amount: params.input_amount,
            output_amount: total_output,
            price_impact,
            fee: total_fee,
            route_path,
            gas_cost: total_gas,
            execution_time_ms: self.estimate_execution_time(&path),
            mev_risk: max_mev_risk,
            liquidity_depth: self.calculate_liquidity_depth(&path).await?,
        })
    }

    /// Generate split trading combinations
    fn generate_split_combinations(
        &self,
        routes: &[SwapRoute],
        total_amount: u64,
        max_splits: usize,
    ) -> Vec<Vec<(usize, u64)>> {
        let mut combinations = Vec::new();

        // Generate all possible combinations of 2 to max_splits routes
        for num_splits in 2..=max_splits.min(routes.len()) {
            let mut amounts = vec![0u64; num_splits];
            let base_amount = total_amount / num_splits as u64;
            let remainder = total_amount % num_splits as u64;

            for i in 0..num_splits {
                amounts[i] = base_amount + if i < remainder as usize { 1 } else { 0 };
            }

            // Generate permutations of route indices
            let mut indices = (0..routes.len()).collect::<Vec<_>>();
            let mut perm_indices = Vec::new();
            self.generate_permutations(&mut indices, num_splits, &mut perm_indices);

            for perm in perm_indices {
                let mut combination = Vec::new();
                for (i, &route_idx) in perm.iter().enumerate() {
                    if amounts[i] >= self.split_config.min_split_amount {
                        combination.push((route_idx, amounts[i]));
                    }
                }
                if !combination.is_empty() {
                    combinations.push(combination);
                }
            }
        }

        combinations
    }

    /// Generate permutations for route combinations
    fn generate_permutations(&self, arr: &mut [usize], k: usize, result: &mut Vec<Vec<usize>>) {
        if k == 0 {
            result.push(arr[..k].to_vec());
            return;
        }

        for i in 0..arr.len() {
            arr.swap(0, i);
            self.generate_permutations(&mut arr[1..], k - 1, result);
            arr.swap(0, i);
        }
    }

    /// Create a split trading route
    async fn create_split_route(
        &self,
        combination: Vec<(usize, u64)>,
        params: &SwapParams,
    ) -> Result<SwapRoute> {
        let mut total_output = 0u64;
        let mut total_fee = 0u64;
        let mut total_gas = 0u64;
        let mut max_mev_risk = MevRisk::Low;
        let mut route_path = Vec::new();

        for (_route_idx, amount) in combination {
            // This would need access to the original routes
            // For now, return a placeholder
            total_output += amount; // Simplified
            total_fee += amount / 1000; // 0.1% fee
            total_gas += 5000; // 5k gas per split
            max_mev_risk = MevRisk::Medium;
        }

        let price_impact =
            calculate_price_impact(params.input_amount, total_output, Decimal::new(1, 0))?;

        Ok(SwapRoute {
            dex: DexType::Raydium, // Placeholder
            input_token: self.get_token_info(&params.input_token).await?,
            output_token: self.get_token_info(&params.output_token).await?,
            input_amount: params.input_amount,
            output_amount: total_output,
            price_impact,
            fee: total_fee,
            route_path,
            gas_cost: total_gas,
            execution_time_ms: 1000, // 1 second
            mev_risk: max_mev_risk,
            liquidity_depth: 1000000000, // 1B placeholder
        })
    }

    /// Create an arbitrage route
    async fn create_arbitrage_route(
        &self,
        buy_dex: &(DexType, crate::types::PriceInfo),
        sell_dex: &(DexType, crate::types::PriceInfo),
        params: &SwapParams,
    ) -> Result<SwapRoute> {
        // Simplified arbitrage route creation
        let profit = (buy_dex.1.price - sell_dex.1.price) * Decimal::from(params.input_amount);

        Ok(SwapRoute {
            dex: buy_dex.0,
            input_token: self.get_token_info(&params.input_token).await?,
            output_token: self.get_token_info(&params.output_token).await?,
            input_amount: params.input_amount,
            output_amount: params.input_amount + profit.to_u64().unwrap_or(0),
            price_impact: Decimal::ZERO,
            fee: params.input_amount / 1000, // 0.1% fee
            route_path: vec![],
            gas_cost: 10000,         // Higher gas for arbitrage
            execution_time_ms: 2000, // 2 seconds
            mev_risk: MevRisk::High,
            liquidity_depth: 1000000000,
        })
    }

    /// Enhance a route with smart routing data
    async fn enhance_route(mut route: SwapRoute, dex_type: &DexType) -> SwapRoute {
        route.gas_cost = Self::estimate_gas_cost(dex_type);
        route.execution_time_ms = Self::estimate_execution_time_single(dex_type);
        route.mev_risk = Self::assess_mev_risk(&route);
        route.liquidity_depth = route.output_amount * 100; // Simplified

        route
    }

    /// Estimate gas cost for a DEX
    fn estimate_gas_cost(dex_type: &DexType) -> u64 {
        match dex_type {
            DexType::PumpFun => 5000,
            DexType::PumpFunSwap => 8000,
            DexType::Raydium => 10000,
            DexType::RaydiumCpmm => 12000,
            DexType::Orca => 15000,
        }
    }

    /// Estimate execution time for a single DEX
    fn estimate_execution_time_single(dex_type: &DexType) -> u64 {
        match dex_type {
            DexType::PumpFun => 500,
            DexType::PumpFunSwap => 800,
            DexType::Raydium => 1000,
            DexType::RaydiumCpmm => 1200,
            DexType::Orca => 1500,
        }
    }

    /// Assess MEV risk for a route
    fn assess_mev_risk(route: &SwapRoute) -> MevRisk {
        if route.liquidity_depth > 10000000000 {
            // 10B
            MevRisk::Low
        } else if route.liquidity_depth > 1000000000 {
            // 1B
            MevRisk::Medium
        } else if route.liquidity_depth > 100000000 {
            // 100M
            MevRisk::High
        } else {
            MevRisk::Critical
        }
    }

    /// Estimate execution time for a multi-hop path
    fn estimate_execution_time(&self, path: &[(Pubkey, DexType, Pubkey)]) -> u64 {
        path.iter()
            .map(|(_, dex_type, _)| Self::estimate_execution_time_single(dex_type))
            .sum()
    }

    /// Calculate liquidity depth for a path
    async fn calculate_liquidity_depth(&self, path: &[(Pubkey, DexType, Pubkey)]) -> Result<u64> {
        // let mut min_liquidity = u64::MAX;

        // for (_, dex_type, pool_address) in path {
        //     if let Some(dex) = self.dexs.get(dex_type) {
        //         if let Ok(pool_state) = dex.get_pool_state(pool_address) {
        //             let liquidity = pool_state.liquidity_usd;
        //             min_liquidity = min_liquidity.min(liquidity);
        //         }
        //     }
        // }

        // Ok(if min_liquidity == u64::MAX {
        //     0
        // } else {
        //     min_liquidity
        // })
        Ok(0)
    }

    /// Get token information
    async fn get_token_info(&self, token_address: &Pubkey) -> Result<Token> {
        // Try to get token info from any DEX
        for dex in self.dexs.values() {
            if let Ok(token) = dex.get_token_info(token_address).await {
                return Ok(token);
            }
        }

        // Fallback to placeholder
        Ok(Token {
            address: *token_address,
            decimals: 6,
        })
    }

    /// Optimize routes using various criteria
    async fn optimize_routes(
        &self,
        mut routes: Vec<SwapRoute>,
        params: &SwapParams,
    ) -> Result<Vec<SwapRoute>> {
        // Filter routes based on MEV protection settings
        routes.retain(|route| {
            route.mev_risk <= self.mev_config.max_mev_risk_tolerance
                && route.liquidity_depth >= self.mev_config.min_liquidity_threshold
        });

        // Sort by a composite score that considers multiple factors
        routes.sort_by(|a, b| {
            let score_a = self.calculate_route_score(a, params);
            let score_b = self.calculate_route_score(b, params);
            score_b
                .partial_cmp(&score_a)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        Ok(routes)
    }

    /// Calculate a composite score for route optimization
    fn calculate_route_score(&self, route: &SwapRoute, params: &SwapParams) -> f64 {
        let output_score = route.output_amount as f64 / params.input_amount as f64;
        let fee_penalty = route.fee as f64 / params.input_amount as f64;
        let gas_penalty = route.gas_cost as f64 / 1000000.0; // Normalize gas cost
        let mev_penalty = match route.mev_risk {
            MevRisk::Low => 0.0,
            MevRisk::Medium => 0.1,
            MevRisk::High => 0.3,
            MevRisk::Critical => 0.5,
        };
        let liquidity_bonus = (route.liquidity_depth as f64 / 1000000000.0).min(1.0);

        output_score - fee_penalty - gas_penalty - mev_penalty + liquidity_bonus
    }

    /// Select the best route from optimized routes
    async fn select_best_route(
        &self,
        routes: Vec<SwapRoute>,
        params: &SwapParams,
    ) -> Result<BestRoute> {
        if routes.is_empty() {
            return Err(DexAggregatorError::RouteNotFound);
        }

        let best_route = routes[0].clone();
        let total_gas_cost = routes.iter().map(|r| r.gas_cost).sum();
        let max_mev_risk = routes
            .iter()
            .map(|r| r.mev_risk)
            .max()
            .unwrap_or(MevRisk::Low);
        let estimated_time = routes
            .iter()
            .map(|r| r.execution_time_ms)
            .max()
            .unwrap_or(0);

        Ok(BestRoute {
            routes,
            total_input_amount: params.input_amount,
            total_output_amount: best_route.output_amount,
            total_fee: best_route.fee,
            total_price_impact: best_route.price_impact,
            execution_priority: params.priority,
            total_gas_cost,
            estimated_execution_time_ms: estimated_time,
            max_mev_risk,
            route_type: RouteType::Optimal,
            split_ratio: None,
        })
    }
}
