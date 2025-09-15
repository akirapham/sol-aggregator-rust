use rust_decimal::Decimal;
use sol_agg_rust::{AggregatorConfig, DexAggregator, ExecutionPriority, SwapParams};
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    env_logger::init();

    // Create aggregator configuration with smart routing enabled
    let config = AggregatorConfig {
        rpc_url: "https://api.mainnet-beta.solana.com".to_string(),
        commitment: solana_sdk::commitment_config::CommitmentLevel::Confirmed,
        max_slippage: Decimal::new(5, 2), // 5%
        max_routes: 3,
        enabled_dexs: vec![
            sol_agg_rust::DexType::PumpFun,
            sol_agg_rust::DexType::PumpFunSwap,
            sol_agg_rust::DexType::Raydium,
            sol_agg_rust::DexType::RaydiumCpmm,
            sol_agg_rust::DexType::Orca,
        ],
        smart_routing: sol_agg_rust::SmartRoutingConfig {
            enable_multi_hop: true,
            enable_split_trading: true,
            enable_arbitrage_detection: true,
            max_hops: 3,
            min_liquidity_threshold: 1000000,
            price_impact_threshold: Decimal::new(5, 2),
            enable_route_simulation: true,
            enable_dynamic_slippage: true,
        },
        gas_config: sol_agg_rust::GasConfig {
            max_gas_price: 5000,
            priority_fee: 1000,
            gas_limit: 200000,
            optimize_for_speed: false,
        },
        mev_protection: sol_agg_rust::MevProtectionConfig {
            use_private_mempool: false,
            max_slippage_tolerance: Decimal::new(1, 2),
            min_liquidity_threshold: 10000000,
            max_mev_risk_tolerance: sol_agg_rust::MevRisk::Medium,
            use_flashloan_protection: false,
        },
        split_config: sol_agg_rust::SplitConfig {
            max_splits: 3,
            min_split_amount: 1000000,
            max_price_impact_per_split: Decimal::new(2, 2),
            prefer_low_mev: true,
        },
    };

    // Create the aggregator
    let aggregator = DexAggregator::new(config);

    // Example token addresses (these are placeholder addresses)
    let sol_mint = Pubkey::from_str("So11111111111111111111111111111111111111112")?;
    let usdc_mint = Pubkey::from_str("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v")?;

    // Create swap parameters
    let swap_params = SwapParams {
        input_token: sol_mint,
        output_token: usdc_mint,
        input_amount: 1000000000,               // 1 SOL (in lamports)
        slippage_tolerance: Decimal::new(1, 2), // 1%
        user_wallet: Pubkey::new_unique(),      // Placeholder wallet
        priority: ExecutionPriority::Medium,
    };

    println!("🔍 Finding best route for SOL -> USDC swap...");

    // Find the best route
    match aggregator.find_best_route(&swap_params).await {
        Ok(best_route) => {
            println!("✅ Best route found!");
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

            println!("\n🛣️  Routes:");
            for (i, route) in best_route.routes.iter().enumerate() {
                println!(
                    "  {}. {} -> {} via {}",
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
        }
        Err(e) => {
            println!("❌ Error finding route: {}", e);
        }
    }

    println!("\n🔍 Getting price comparison across all DEXs...");

    // Get price comparison
    match aggregator
        .get_price_comparison(&sol_mint, &usdc_mint, 1000000000)
        .await
    {
        Ok(prices) => {
            println!("📊 Price comparison:");
            for price_info in prices {
                println!(
                    "  {}: {} USDC per SOL (liquidity: {})",
                    price_info.dex, price_info.price, price_info.liquidity
                );
            }
        }
        Err(e) => {
            println!("❌ Error getting prices: {}", e);
        }
    }

    println!("\n🔍 Checking if token pair is supported...");

    // Check if token pair is supported
    match aggregator
        .is_token_pair_supported(&sol_mint, &usdc_mint)
        .await
    {
        Ok(supported) => {
            if supported {
                println!("✅ Token pair is supported by at least one DEX");
            } else {
                println!("❌ Token pair is not supported by any DEX");
            }
        }
        Err(e) => {
            println!("❌ Error checking support: {}", e);
        }
    }

    println!("\n🔍 Getting all supported tokens...");

    // Get all supported tokens
    match aggregator.get_all_supported_tokens().await {
        Ok(tokens) => {
            println!("📋 Supported tokens ({} total):", tokens.len());
            for token in tokens.iter().take(10) {
                // Show first 10
                println!("  {} ({}) - {}", token.symbol, token.name, token.address);
            }
            if tokens.len() > 10 {
                println!("  ... and {} more", tokens.len() - 10);
            }
        }
        Err(e) => {
            println!("❌ Error getting tokens: {}", e);
        }
    }

    Ok(())
}
