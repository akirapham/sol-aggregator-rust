use rust_decimal::Decimal;
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

use sol_agg_rust::{
    AggregatorConfig, DataFetcherConfig, DexAggregator, DexType, ExecutionPriority, PoolState,
    PoolStateManager, RealTimeDataFetcher, SwapParams, Token,
};

/// Complete integration example showing real-time DEX aggregation
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    env_logger::init();

    println!("🚀 Real-Time Solana DEX Aggregator Integration Example");
    println!("=================================================");

    // Step 1: Create shared pool state manager
    let pool_manager = Arc::new(PoolStateManager::new());
    println!("✅ Pool state manager initialized");

    // Step 2: Configure and start real-time data fetcher
    let data_fetcher_config = DataFetcherConfig {
        rpc_url: "https://api.mainnet-beta.solana.com".to_string(),
        yellowstone_grpc_url: None, // Add your Yellowstone gRPC endpoint if available
        fetch_interval_ms: 3000,    // Fetch every 3 seconds
        max_pools_per_fetch: 500,
        enable_websocket: false, // Enable when WebSocket support is ready
        commitment: "confirmed".to_string(),
    };

    let mut data_fetcher = RealTimeDataFetcher::new(data_fetcher_config, Arc::clone(&pool_manager));

    println!("📡 Starting real-time data fetcher...");
    data_fetcher.start().await?;

    // Step 3: Simulate adding some pool data manually for demonstration
    simulate_pool_data(&pool_manager).await;

    // Step 4: Configure the DEX aggregator
    let aggregator_config = AggregatorConfig {
        rpc_url: "https://api.mainnet-beta.solana.com".to_string(),
        commitment: solana_sdk::commitment_config::CommitmentLevel::Confirmed,
        max_slippage: Decimal::new(1, 2), // 1%
        max_routes: 3,
        enabled_dexs: vec![
            DexType::Orca,
            DexType::Raydium,
            DexType::RaydiumCpmm,
            DexType::PumpFun,
        ],
        smart_routing: sol_agg_rust::SmartRoutingConfig {
            enabled: true,
            max_hops: 2,
            min_liquidity_threshold: 10_000_000,        // $10k
            price_impact_threshold: Decimal::new(3, 2), // 3%
            use_ai_optimization: false,
            prefer_low_mev: true,
        },
        gas_config: sol_agg_rust::GasConfig {
            max_gas_price: 500_000, // 500k lamports
            priority_fee: 5_000,    // 5k lamports
            gas_limit: 150_000,
            optimize_for_speed: false,
        },
        mev_protection: sol_agg_rust::MevProtectionConfig {
            use_private_mempool: false,
            max_slippage_tolerance: Decimal::new(15, 3), // 1.5%
            min_liquidity_threshold: 25_000_000,         // $25k
            max_mev_risk_tolerance: sol_agg_rust::MevRisk::Medium,
            use_flashloan_protection: false,
        },
        split_config: sol_agg_rust::SplitConfig {
            max_splits: 2,
            min_split_amount: 5_000_000, // 5M base units
            max_price_impact_per_split: Decimal::new(15, 3), // 1.5%
            prefer_low_mev: true,
        },
    };

    // Create aggregator with shared pool manager
    let aggregator =
        DexAggregator::new_with_pool_manager(aggregator_config, Arc::clone(&pool_manager));

    println!("💹 DEX Aggregator initialized with real-time pool data");

    // Step 5: Wait for initial data to populate
    println!("⏳ Waiting for initial pool data...");
    sleep(Duration::from_secs(5)).await;

    // Step 6: Show pool statistics
    display_pool_stats(&pool_manager).await;

    // Step 7: Example token addresses (mainnet)
    let usdc_mint: Pubkey = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".parse()?;
    let sol_mint: Pubkey = "So11111111111111111111111111111111111111112".parse()?;
    let user_wallet: Pubkey = Pubkey::new_unique();

    // Step 8: Test basic pool queries
    test_pool_queries(&pool_manager, &usdc_mint, &sol_mint).await;

    // Step 9: Test swap routing
    test_swap_routing(&aggregator, &usdc_mint, &sol_mint, &user_wallet).await;

    // Step 10: Monitor real-time updates
    monitor_real_time_updates(&pool_manager, &aggregator).await;

    println!("🎉 Integration example completed successfully!");
    Ok(())
}

/// Simulate adding pool data for demonstration
async fn simulate_pool_data(pool_manager: &Arc<PoolStateManager>) {
    println!("📊 Adding simulated pool data...");

    // USDC token
    let usdc_token = Token {
        address: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".parse().unwrap(),
        symbol: "USDC".to_string(),
        name: "USD Coin".to_string(),
        decimals: 6,
        logo_uri: Some("https://raw.githubusercontent.com/solana-labs/token-list/main/assets/mainnet/EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v/logo.png".to_string()),
    };

    // SOL token
    let sol_token = Token {
        address: "So11111111111111111111111111111111111111112".parse().unwrap(),
        symbol: "SOL".to_string(),
        name: "Solana".to_string(),
        decimals: 9,
        logo_uri: Some("https://raw.githubusercontent.com/solana-labs/token-list/main/assets/mainnet/So11111111111111111111111111111111111111112/logo.png".to_string()),
    };

    // Simulate Orca USDC/SOL pool
    let orca_pool = PoolState {
        address: Pubkey::new_unique(),
        dex: DexType::Orca,
        token_a: usdc_token.clone(),
        token_b: sol_token.clone(),
        reserve_a: 50_000_000_000,    // 50k USDC
        reserve_b: 500_000_000_000,   // 500 SOL
        fee_rate: Decimal::new(3, 4), // 0.03%
        lp_supply: 10_000_000_000,
        last_updated: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        volume_24h: 1_000_000_000_000,
        volume_7d: 5_000_000_000_000,
        transaction_count: 1250,
        liquidity_usd: Some(100_000.0),
        apr: Some(Decimal::new(15, 2)), // 15%
        tick_current: Some(-23028),
        tick_spacing: Some(64),
        sqrt_price: Some(79228162514264337593543950336u128),
        liquidity: Some(500_000_000_000u128),
        amp_factor: None,
        bonding_curve_reserve: None,
        virtual_sol_reserves: None,
        virtual_token_reserves: None,
        complete: None,
    };

    // Simulate Raydium USDC/SOL pool
    let raydium_pool = PoolState {
        address: Pubkey::new_unique(),
        dex: DexType::Raydium,
        token_a: usdc_token.clone(),
        token_b: sol_token.clone(),
        reserve_a: 75_000_000_000,     // 75k USDC
        reserve_b: 750_000_000_000,    // 750 SOL
        fee_rate: Decimal::new(25, 4), // 0.25%
        lp_supply: 15_000_000_000,
        last_updated: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        volume_24h: 2_000_000_000_000,
        volume_7d: 8_000_000_000_000,
        transaction_count: 2100,
        liquidity_usd: Some(150_000.0),
        apr: Some(Decimal::new(12, 2)), // 12%
        tick_current: None,
        tick_spacing: None,
        sqrt_price: None,
        liquidity: None,
        amp_factor: None,
        bonding_curve_reserve: None,
        virtual_sol_reserves: None,
        virtual_token_reserves: None,
        complete: None,
    };

    // Add pools to manager
    pool_manager.update_pool(orca_pool).await;
    pool_manager.update_pool(raydium_pool).await;

    println!("✅ Simulated pool data added");
}

/// Display current pool statistics
async fn display_pool_stats(pool_manager: &Arc<PoolStateManager>) {
    let stats = pool_manager.get_stats().await;

    println!("\n📈 Current Pool Statistics:");
    println!("  📍 Total pools: {}", stats.total_pools);
    println!("  🔗 Total pairs: {}", stats.total_pairs);
    println!("  🪙 Total tokens: {}", stats.total_tokens);
    println!("  🏛️ Pools by DEX:");

    for (dex, count) in &stats.pools_by_dex {
        println!("    • {}: {} pools", dex, count);
    }
    println!();
}

/// Test pool queries
async fn test_pool_queries(
    pool_manager: &Arc<PoolStateManager>,
    usdc_mint: &Pubkey,
    sol_mint: &Pubkey,
) {
    println!("🔍 Testing pool queries...");

    // Get all pools for USDC/SOL pair
    let pools = pool_manager.get_pools_for_pair(usdc_mint, sol_mint).await;
    println!("  📊 Found {} pools for USDC/SOL", pools.len());

    for (i, pool) in pools.iter().enumerate() {
        println!("    {}. {} Pool: {}", i + 1, pool.dex, pool.address);
        println!(
            "       💰 Reserves: {} USDC / {} SOL",
            pool.reserve_a as f64 / 1_000_000.0, // USDC has 6 decimals
            pool.reserve_b as f64 / 1_000_000_000.0  // SOL has 9 decimals
        );
        println!("       💸 Fee: {}%", pool.fee_rate * Decimal::new(100, 1));
        if let Some(liquidity_usd) = pool.liquidity_usd {
            println!("       💵 Liquidity: ${:.2}k", liquidity_usd / 1000.0);
        }
        println!("       📅 Last updated: {}", pool.last_updated);
    }

    // Get best pools sorted by liquidity
    let best_pools = pool_manager
        .get_best_pools_for_pair(usdc_mint, sol_mint, 3)
        .await;
    println!("\n  🏆 Top {} pools by liquidity:", best_pools.len());

    for (i, pool) in best_pools.iter().enumerate() {
        let total_reserves = pool.reserve_a + pool.reserve_b;
        println!(
            "    {}. {} - Total reserves: {}",
            i + 1,
            pool.dex,
            total_reserves
        );
    }
    println!();
}

/// Test swap routing
async fn test_swap_routing(
    aggregator: &DexAggregator,
    usdc_mint: &Pubkey,
    sol_mint: &Pubkey,
    user_wallet: &Pubkey,
) {
    println!("💱 Testing swap routing...");

    let swap_params = SwapParams {
        input_token: *usdc_mint,
        output_token: *sol_mint,
        input_amount: 1_000_000_000,            // 1000 USDC
        slippage_tolerance: Decimal::new(1, 2), // 1%
        user_wallet: *user_wallet,
        priority: ExecutionPriority::Medium,
    };

    println!("  🔄 Swap: 1000 USDC → SOL");
    println!("  ⚙️ Slippage tolerance: 1%");
    println!("  📊 Priority: Medium");

    match aggregator.get_best_route(&swap_params).await {
        Ok(Some(best_route)) => {
            println!("\n  ✅ Best route found!");
            println!("    📈 Routes: {}", best_route.routes.len());
            println!(
                "    💰 Input: {} USDC",
                best_route.total_input_amount as f64 / 1_000_000.0
            );
            println!(
                "    💰 Output: {} SOL",
                best_route.total_output_amount as f64 / 1_000_000_000.0
            );
            println!(
                "    💸 Total fee: {} USDC",
                best_route.total_fee as f64 / 1_000_000.0
            );
            println!(
                "    📉 Price impact: {:.3}%",
                best_route.total_price_impact * Decimal::new(100, 1)
            );
            println!("    ⛽ Gas cost: {} lamports", best_route.total_gas_cost);
            println!(
                "    ⏱️ Est. time: {}ms",
                best_route.estimated_execution_time_ms
            );
            println!("    🛡️ MEV risk: {:?}", best_route.max_mev_risk);
            println!("    🎯 Route type: {:?}", best_route.route_type);

            println!("\n    📍 Route breakdown:");
            for (i, route) in best_route.routes.iter().enumerate() {
                println!("      {}. {} DEX:", i + 1, route.dex);
                println!(
                    "         💰 Output: {} SOL",
                    route.output_amount as f64 / 1_000_000_000.0
                );
                println!("         💸 Fee: {} USDC", route.fee as f64 / 1_000_000.0);
                println!(
                    "         📉 Price impact: {:.3}%",
                    route.price_impact * Decimal::new(100, 1)
                );
                println!("         ⛽ Gas: {} lamports", route.gas_cost);
                println!("         🛡️ MEV risk: {:?}", route.mev_risk);
            }
        }
        Ok(None) => {
            println!("  ❌ No route found for this token pair");
        }
        Err(e) => {
            println!("  ❌ Error finding route: {:?}", e);
        }
    }
    println!();
}

/// Monitor real-time updates
async fn monitor_real_time_updates(
    pool_manager: &Arc<PoolStateManager>,
    _aggregator: &DexAggregator,
) {
    println!("🔄 Monitoring real-time updates...");
    println!("  (In production, this would show live pool state changes)");

    for i in 1..=5 {
        sleep(Duration::from_secs(3)).await;

        let stats = pool_manager.get_stats().await;
        println!("  📊 Update #{}: {} pools tracked", i, stats.total_pools);

        // Simulate a pool update
        if i == 3 {
            println!("  🔄 Simulating pool update...");
            // In a real scenario, this would come from WebSocket/gRPC updates
        }

        // Display key metrics
        if i % 2 == 0 {
            println!("  📈 Pool activity metrics would be displayed here");
            println!("     • Volume changes");
            println!("     • Liquidity movements");
            println!("     • Price impact updates");
            println!("     • New arbitrage opportunities");
        }
    }

    println!("\n✨ Real-time monitoring demonstration complete!");
    println!("\n🔮 In a production environment, this system would:");
    println!("  • 📡 Subscribe to DEX-specific WebSocket feeds");
    println!("  • 🔗 Listen to Yellowstone gRPC for account changes");
    println!("  • ⚡ Process thousands of updates per second");
    println!("  • 🧠 Continuously optimize routing algorithms");
    println!("  • 🛡️ Provide MEV protection and sandwich attack detection");
    println!("  • 📊 Maintain historical data for analytics");
    println!("  • 🎯 Execute swaps at optimal timing");
    println!("  • 💹 Support advanced features like:");
    println!("    - Limit orders");
    println!("    - Dollar-cost averaging");
    println!("    - Portfolio rebalancing");
    println!("    - Cross-chain swaps");
}
