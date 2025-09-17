use futures::future;
use rust_decimal::Decimal;
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::time::{timeout, Duration};

use crate::dex::DexInterface;
use crate::error::{DexAggregatorError, Result};
use crate::pool_data_types::DexType;
use crate::pool_manager::PoolStateManager;
use crate::smart_routing::SmartRoutingEngine;
use crate::types::{
    AggregatorConfig, BestRoute, ExecutionPriority, MevRisk, PriceInfo, RouteType, SwapParams,
    SwapRoute,
};

/// Main DEX aggregator that finds the best routes across multiple DEXs with real-time data
pub struct DexAggregator {
    config: AggregatorConfig,
    dexs: HashMap<DexType, Box<dyn DexInterface + Send + Sync>>,
    smart_routing: SmartRoutingEngine,
    pool_manager: Arc<PoolStateManager>,
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
        let mut dexs: HashMap<DexType, Box<dyn DexInterface + Send + Sync>> = HashMap::new();

        // // Initialize DEXs based on configuration
        // for dex_type in &config.enabled_dexs {
        //     match dex_type {
        //         DexType::PumpFun => {
        //             dexs.insert(
        //                 DexType::PumpFun,
        //                 Box::new(PumpFunDex::new(config.rpc_url.clone())),
        //             );
        //         }
        //         DexType::PumpFunSwap => {
        //             dexs.insert(
        //                 DexType::PumpFunSwap,
        //                 Box::new(PumpFunSwapDex::new(config.rpc_url.clone())),
        //             );
        //         }
        //         DexType::Raydium => {
        //             dexs.insert(
        //                 DexType::Raydium,
        //                 Box::new(RaydiumDex::new(config.rpc_url.clone())),
        //             );
        //         }
        //         DexType::RaydiumCpmm => {
        //             dexs.insert(
        //                 DexType::RaydiumCpmm,
        //                 Box::new(RaydiumCpmmDex::new(config.rpc_url.clone())),
        //             );
        //         }
        //         DexType::Orca => {
        //             dexs.insert(
        //                 DexType::Orca,
        //                 Box::new(OrcaDex::new(
        //                     config.rpc_url.clone(),
        //                     Arc::clone(&pool_manager),
        //                 )),
        //             );
        //         }
        //     }
        // }

        // Create a copy of dexs for smart routing
        let mut smart_dexs: HashMap<DexType, Box<dyn DexInterface + Send + Sync>> = HashMap::new();
        // for (dex_type, _dex) in &dexs {
        //     // This is a simplified approach - in practice, you'd need to clone the DEXs properly
        //     match dex_type {
        //         DexType::PumpFun => {
        //             smart_dexs.insert(
        //                 DexType::PumpFun,
        //                 Box::new(PumpFunDex::new(config.rpc_url.clone())),
        //             );
        //         }
        //         DexType::PumpFunSwap => {
        //             smart_dexs.insert(
        //                 DexType::PumpFunSwap,
        //                 Box::new(PumpFunSwapDex::new(config.rpc_url.clone())),
        //             );
        //         }
        //         DexType::Raydium => {
        //             smart_dexs.insert(
        //                 DexType::Raydium,
        //                 Box::new(RaydiumDex::new(config.rpc_url.clone())),
        //             );
        //         }
        //         DexType::RaydiumCpmm => {
        //             smart_dexs.insert(
        //                 DexType::RaydiumCpmm,
        //                 Box::new(RaydiumCpmmDex::new(config.rpc_url.clone())),
        //             );
        //         }
        //         DexType::Orca => {
        //             smart_dexs.insert(
        //                 DexType::Orca,
        //                 Box::new(OrcaDex::new(
        //                     config.rpc_url.clone(),
        //                     Arc::clone(&pool_manager),
        //                 )),
        //             );
        //         }
        //     }
        // }

        let smart_routing = SmartRoutingEngine::new(
            config.smart_routing.clone(),
            config.gas_config.clone(),
            config.mev_protection.clone(),
            config.split_config.clone(),
            smart_dexs,
        );

        Self {
            config,
            dexs,
            smart_routing,
            pool_manager,
        }
    }

    /// Get access to the pool manager
    pub fn get_pool_manager(&self) -> &Arc<PoolStateManager> {
        &self.pool_manager
    }

    /// Find the best route for a swap using smart routing
    pub async fn find_best_route(&self, params: &SwapParams) -> Result<BestRoute> {
        // Use smart routing engine for advanced route finding
        self.smart_routing.find_optimal_route(params).await
    }

    /// Alias for find_best_route to maintain compatibility
    pub async fn get_best_route(&self, params: &SwapParams) -> Result<Option<BestRoute>> {
        match self.find_best_route(params).await {
            Ok(route) => Ok(Some(route)),
            Err(_) => Ok(None),
        }
    }

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

    /// Determine execution priority based on route characteristics
    fn determine_execution_priority(
        &self,
        routes: &[SwapRoute],
        user_priority: ExecutionPriority,
    ) -> ExecutionPriority {
        // If user specified priority, use it
        match user_priority {
            ExecutionPriority::High => return ExecutionPriority::High,
            ExecutionPriority::Low => return ExecutionPriority::Low,
            ExecutionPriority::Medium => {
                // Determine based on route characteristics
                if routes.is_empty() {
                    return ExecutionPriority::Medium;
                }

                // Check if any route has high price impact (suggests low liquidity)
                let has_high_impact = routes.iter().any(|r| r.price_impact > Decimal::new(5, 2)); // 5%

                if has_high_impact {
                    ExecutionPriority::High // Execute quickly to avoid slippage
                } else {
                    ExecutionPriority::Medium
                }
            }
        }
    }
}
