// use rust_decimal::Decimal;
// use solana_sdk::pubkey::Pubkey;
// use std::sync::Arc;
// use std::time::Duration;
// use tokio::time::sleep;

// use sol_agg_rust::{
//     AggregatorConfig, DataFetcherConfig, DexAggregator, DexType, ExecutionPriority,
//     PoolStateManager, RealTimeDataFetcher, SwapParams,
// };

// #[tokio::main]
// async fn main() -> Result<(), Box<dyn std::error::Error>> {
//     // Initialize logging
//     env_logger::init();

//     println!("🚀 Starting Real-Time Solana DEX Aggregator");

//     // Create pool state manager for real-time data
//     let pool_manager = Arc::new(PoolStateManager::new());

//     // Configure data fetcher for real-time updates
//     let data_fetcher_config = DataFetcherConfig {
//         rpc_url: "https://api.mainnet-beta.solana.com".to_string(),
//         yellowstone_grpc_url: None, // Add Yellowstone gRPC URL if available
//         fetch_interval_ms: 5000,    // Fetch every 5 seconds
//         max_pools_per_fetch: 1000,
//         enable_websocket: false, // Set to true when WebSocket support is ready
//         commitment: "confirmed".to_string(),
//     };

//     // Create and start real-time data fetcher
//     let mut data_fetcher = RealTimeDataFetcher::new(data_fetcher_config, Arc::clone(&pool_manager));

//     println!("📡 Starting real-time data fetcher...");
//     data_fetcher.start().await?;

//     // Wait a bit for initial data to be fetched
//     sleep(Duration::from_secs(10)).await;

//     // Configure the aggregator
//     let aggregator_config = AggregatorConfig {
//         rpc_url: "https://api.mainnet-beta.solana.com".to_string(),
//         commitment: solana_sdk::commitment_config::CommitmentLevel::Confirmed,
//         max_slippage: Decimal::new(1, 2), // 1%
//         max_routes: 5,
//         enabled_dexs: vec![
//             DexType::Orca,
//             DexType::Raydium,
//             DexType::RaydiumCpmm,
//             DexType::PumpFun,
//             DexType::PumpFunSwap,
//         ],
//         smart_routing: sol_agg_rust::SmartRoutingConfig {
//             enabled: true,
//             max_hops: 3,
//             min_liquidity_threshold: 100_000_000,       // $100k
//             price_impact_threshold: Decimal::new(5, 2), // 5%
//             use_ai_optimization: false,
//             prefer_low_mev: true,
//         },
//         gas_config: sol_agg_rust::GasConfig {
//             max_gas_price: 1_000_000, // 1M lamports
//             priority_fee: 10_000,     // 10k lamports
//             gas_limit: 200_000,
//             optimize_for_speed: false,
//         },
//         mev_protection: sol_agg_rust::MevProtectionConfig {
//             use_private_mempool: false,
//             max_slippage_tolerance: Decimal::new(2, 2), // 2%
//             min_liquidity_threshold: 50_000_000,        // $50k
//             max_mev_risk_tolerance: sol_agg_rust::MevRisk::Medium,
//             use_flashloan_protection: false,
//         },
//         split_config: sol_agg_rust::SplitConfig {
//             max_splits: 3,
//             min_split_amount: 1_000_000, // 1M base units
//             max_price_impact_per_split: Decimal::new(1, 2), // 1%
//             prefer_low_mev: true,
//         },
//     };

//     // Create aggregator with real-time pool data
//     let aggregator = DexAggregator::new(aggregator_config);

//     println!("💹 DEX Aggregator initialized with real-time data support");

//     // Example token addresses (replace with actual addresses)
//     let usdc_mint: Pubkey = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".parse()?;
//     let sol_mint: Pubkey = "So11111111111111111111111111111111111111112".parse()?;
//     let user_wallet: Pubkey = Pubkey::new_unique(); // Replace with actual wallet

//     // Show pool stats
//     let stats = pool_manager.get_stats().await;
//     println!("📊 Pool Manager Stats:");
//     println!("  Total pools: {}", stats.total_pools);
//     println!("  Total pairs: {}", stats.total_pairs);
//     println!("  Total tokens: {}", stats.total_tokens);
//     println!("  Pools by DEX: {:?}", stats.pools_by_dex);

//     // Example 1: Get best pools for a token pair
//     println!("\n🔍 Getting best pools for USDC/SOL...");
//     let best_pools = pool_manager
//         .get_best_pools_for_pair(&usdc_mint, &sol_mint, 5)
//         .await;

//     for (i, pool) in best_pools.iter().enumerate() {
//         println!("  {}. {} Pool: {}", i + 1, pool.dex, pool.address);
//         println!("     Reserves: {} / {}", pool.reserve_a, pool.reserve_b);
//         println!("     Fee: {}%", pool.fee_rate * Decimal::new(100, 1));
//         println!("     Last updated: {}", pool.last_updated);
//     }

//     // Example 2: Simulate real-time swap routing
//     let swap_params = SwapParams {
//         input_token: usdc_mint,
//         output_token: sol_mint,
//         input_amount: 1_000_000_000,            // 1000 USDC (6 decimals)
//         slippage_tolerance: Decimal::new(1, 2), // 1%
//         user_wallet,
//         priority: ExecutionPriority::Medium,
//     };

//     println!("\n💱 Finding best route for swap: 1000 USDC → SOL");

//     // Get routes from aggregator (this would use real pool data)
//     match aggregator.get_best_route(&swap_params).await {
//         Ok(Some(best_route)) => {
//             println!("✅ Best route found!");
//             println!("  Total routes: {}", best_route.routes.len());
//             println!("  Input: {}", best_route.total_input_amount);
//             println!("  Output: {}", best_route.total_output_amount);
//             println!("  Total fee: {}", best_route.total_fee);
//             println!(
//                 "  Price impact: {}%",
//                 best_route.total_price_impact * Decimal::new(100, 1)
//             );
//             println!("  Gas cost: {} lamports", best_route.total_gas_cost);
//             println!(
//                 "  Execution time: {}ms",
//                 best_route.estimated_execution_time_ms
//             );
//             println!("  MEV risk: {:?}", best_route.max_mev_risk);

//             for (i, route) in best_route.routes.iter().enumerate() {
//                 println!(
//                     "  Route {}: {} (Output: {})",
//                     i + 1,
//                     route.dex,
//                     route.output_amount
//                 );
//             }
//         }
//         Ok(None) => {
//             println!("❌ No route found for this token pair");
//         }
//         Err(e) => {
//             println!("❌ Error finding route: {:?}", e);
//         }
//     }

//     // Example 3: Monitor pool changes in real-time
//     println!("\n🔄 Monitoring real-time pool updates...");
//     println!("The aggregator will continue to update pool states in the background.");
//     println!("Press Ctrl+C to stop.");

//     // In a real application, you would:
//     // 1. Set up WebSocket connections to DEXs
//     // 2. Subscribe to Yellowstone gRPC for account changes
//     // 3. Process incoming transactions and update pool states
//     // 4. Trigger re-routing when pool states change significantly

//     // Simulation: Check pool updates every 10 seconds
//     for i in 1..=6 {
//         sleep(Duration::from_secs(10)).await;

//         let updated_stats = pool_manager.get_stats().await;
//         println!(
//             "\n📈 Update #{}: {} pools tracked",
//             i, updated_stats.total_pools
//         );

//         // Check if any new pools were discovered
//         if updated_stats.total_pools > stats.total_pools {
//             println!(
//                 "🆕 {} new pools discovered!",
//                 updated_stats.total_pools - stats.total_pools
//             );
//         }

//         // In a real scenario, you might also:
//         // - Check for price movements
//         // - Update routing tables
//         // - Notify users of better routes
//         // - Execute pending swaps when conditions improve
//     }

//     println!("\n✨ Real-time DEX aggregator demonstration completed!");
//     println!("In production, this would run continuously with:");
//     println!("  • WebSocket subscriptions to DEX APIs");
//     println!("  • Yellowstone gRPC for Solana account updates");
//     println!("  • Automatic re-routing based on pool changes");
//     println!("  • MEV protection and optimal execution");

//     Ok(())
// }

// /// Helper function to demonstrate pool state monitoring
// async fn monitor_pool_changes(
//     pool_manager: Arc<PoolStateManager>,
//     token_a: &Pubkey,
//     token_b: &Pubkey,
// ) {
//     loop {
//         let pools = pool_manager.get_pools_for_pair(token_a, token_b).await;

//         for pool in pools {
//             // In a real implementation, you would:
//             // - Track price changes
//             // - Monitor liquidity changes
//             // - Alert on significant movements
//             // - Update routing algorithms

//             println!(
//                 "🔄 Pool {} - Reserves: {}/{}, Fee: {}%",
//                 pool.address,
//                 pool.reserve_a,
//                 pool.reserve_b,
//                 pool.fee_rate * Decimal::new(100, 1)
//             );
//         }

//         sleep(Duration::from_secs(30)).await;
//     }
// }
