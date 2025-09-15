use rust_decimal::Decimal;
use sol_agg_rust::{AggregatorConfig, DexAggregator, ExecutionPriority, SwapParams};
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

/// Example showing integration between Rust aggregator and Anchor program
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    env_logger::init();

    println!("🚀 Solana DEX Aggregator with Anchor Program Integration");
    println!("========================================================");

    // 1. Load configuration from environment
    println!("🔧 Loading configuration from environment...");
    let config = AggregatorConfig::from_env()?;
    println!("✅ Configuration loaded successfully");

    // 2. Create aggregator with configuration
    let aggregator = DexAggregator::new(config);

    // 3. Example token addresses
    let sol_mint = Pubkey::from_str("So11111111111111111111111111111111111111112")?;
    let usdc_mint = Pubkey::from_str("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v")?;

    // 4. Create swap parameters
    let swap_params = SwapParams {
        input_token: sol_mint,
        output_token: usdc_mint,
        input_amount: 1000000000,               // 1 SOL
        slippage_tolerance: Decimal::new(1, 2), // 1%
        user_wallet: Pubkey::new_unique(),
        priority: ExecutionPriority::Medium,
    };

    println!("\n🔍 Finding optimal route...");
    println!("Input: {} SOL", swap_params.input_amount as f64 / 1e9);
    println!("Output: USDC");
    println!(
        "Slippage tolerance: {:.2}%",
        swap_params.slippage_tolerance * Decimal::from(100)
    );

    // 5. Find best route using smart routing
    match aggregator.find_best_route(&swap_params).await {
        Ok(best_route) => {
            println!("\n✅ Best route found!");
            println!(
                "📊 Total input: {} SOL",
                best_route.total_input_amount as f64 / 1e9
            );
            println!(
                "📊 Total output: {} USDC",
                best_route.total_output_amount as f64 / 1e6
            );
            println!("💰 Total fee: {} lamports", best_route.total_fee);
            println!(
                "📈 Price impact: {:.2}%",
                best_route.total_price_impact * Decimal::from(100)
            );
            println!("⚡ Execution priority: {:?}", best_route.execution_priority);
            println!("⛽ Total gas cost: {} lamports", best_route.total_gas_cost);
            println!(
                "⏱️  Estimated execution time: {} ms",
                best_route.estimated_execution_time_ms
            );
            println!("🛡️  Max MEV risk: {:?}", best_route.max_mev_risk);
            println!("🛣️  Route type: {:?}", best_route.route_type);

            // 6. Display route details
            println!("\n🛣️  Route Details:");
            for (i, route) in best_route.routes.iter().enumerate() {
                println!(
                    "  {}. {} -> {} via {:?}",
                    i + 1,
                    route.input_token.symbol,
                    route.output_token.symbol,
                    route.dex
                );
                println!("     Input: {} lamports", route.input_amount);
                println!("     Output: {} lamports", route.output_amount);
                println!("     Fee: {} lamports", route.fee);
                println!(
                    "     Price impact: {:.2}%",
                    route.price_impact * Decimal::from(100)
                );
                println!("     Gas cost: {} lamports", route.gas_cost);
                println!("     Execution time: {} ms", route.execution_time_ms);
                println!("     MEV risk: {:?}", route.mev_risk);
                println!("     Liquidity depth: {} lamports", route.liquidity_depth);
            }

            // 7. Simulate Anchor program integration
            println!("\n🔗 Anchor Program Integration Simulation:");
            println!("=====================================");

            // Calculate fee for Anchor program
            let fee_rate = 100; // 1% in basis points (from Anchor program)
            let fee_amount = (swap_params.input_amount * fee_rate) / 10000;
            println!("💳 Fee calculation:");
            println!("   Swap amount: {} lamports", swap_params.input_amount);
            println!("   Fee rate: {} basis points", fee_rate);
            println!("   Fee amount: {} lamports", fee_amount);

            // Simulate Anchor program calls
            println!("\n📋 Anchor Program Operations:");
            println!("1. ✅ Validate swap parameters");
            println!("2. ✅ Check DEX is enabled");
            println!("3. ✅ Validate price impact threshold");
            println!("4. ✅ Check MEV risk tolerance");
            println!("5. ✅ Collect fee: {} lamports", fee_amount);
            println!("6. ✅ Execute swap through {:?}", best_route.routes[0].dex);
            println!("7. ✅ Update user fee tracking");
            println!("8. ✅ Update total fees collected");

            // 8. Show configuration details
            println!("\n⚙️  Configuration Details:");
            println!("=========================");
            println!(
                "Max slippage: {:.2}%",
                config.max_slippage * Decimal::from(100)
            );
            println!("Max routes: {}", config.max_routes);
            println!("Enabled DEXs: {:?}", config.enabled_dexs);
            println!(
                "Smart routing enabled: {}",
                config.smart_routing.enable_multi_hop
            );
            println!(
                "Split trading enabled: {}",
                config.smart_routing.enable_split_trading
            );
            println!(
                "Arbitrage detection: {}",
                config.smart_routing.enable_arbitrage_detection
            );
            println!("Max hops: {}", config.smart_routing.max_hops);
            println!(
                "Min liquidity threshold: {} lamports",
                config.smart_routing.min_liquidity_threshold
            );
            println!(
                "Price impact threshold: {:.2}%",
                config.smart_routing.price_impact_threshold * Decimal::from(100)
            );

            // 9. Show MEV protection details
            println!("\n🛡️  MEV Protection:");
            println!("==================");
            println!(
                "Use private mempool: {}",
                config.mev_protection.use_private_mempool
            );
            println!(
                "Max slippage tolerance: {:.2}%",
                config.mev_protection.max_slippage_tolerance * Decimal::from(100)
            );
            println!(
                "Min liquidity threshold: {} lamports",
                config.mev_protection.min_liquidity_threshold
            );
            println!(
                "Max MEV risk tolerance: {:?}",
                config.mev_protection.max_mev_risk_tolerance
            );
            println!(
                "Flashloan protection: {}",
                config.mev_protection.use_flashloan_protection
            );

            // 10. Show gas configuration
            println!("\n⛽ Gas Configuration:");
            println!("===================");
            println!(
                "Max gas price: {} lamports",
                config.gas_config.max_gas_price
            );
            println!("Priority fee: {} lamports", config.gas_config.priority_fee);
            println!("Gas limit: {} compute units", config.gas_config.gas_limit);
            println!(
                "Optimize for speed: {}",
                config.gas_config.optimize_for_speed
            );

            // 11. Show split trading configuration
            println!("\n🔄 Split Trading:");
            println!("================");
            println!("Max splits: {}", config.split_config.max_splits);
            println!(
                "Min split amount: {} lamports",
                config.split_config.min_split_amount
            );
            println!(
                "Max price impact per split: {:.2}%",
                config.split_config.max_price_impact_per_split * Decimal::from(100)
            );
            println!("Prefer low MEV: {}", config.split_config.prefer_low_mev);

            println!("\n🎉 Integration example completed successfully!");
            println!("💡 The Rust aggregator provides optimal routing while the Anchor program handles fee collection and configuration management.");
        }
        Err(e) => {
            println!("❌ Error finding route: {}", e);
        }
    }

    Ok(())
}
