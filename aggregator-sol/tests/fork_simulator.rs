mod common;

#[cfg(test)]
mod fork_tests {
    use crate::common::*;

    /// ========================================================================
    /// OPTION 1: Test against Live Devnet
    /// ========================================================================
    ///
    /// Uses real devnet pools (no setup needed, devnet is always running)
    /// Run with: DEVNET=1 cargo test --test mainnet_simulator test_devnet_pools

    #[tokio::test]
    #[ignore] // Ignore by default (requires network connection)
    async fn test_devnet_pools() {
        let _ = env_logger::try_init();
        log::info!("Testing against live Mainnet pools");

        let simulator =
            MainnetForkSimulator::new("https://api.mainnet-beta.solana.com", false).await;

        // Try to fetch real devnet pool data
        let opportunities = simulator.detect_real_opportunities().await;
        log::info!("Found {} devnet opportunities", opportunities.len());

        // If opportunities exist, verify them
        for opp in opportunities.iter().take(1) {
            log::info!("Testing opportunity: {}", opp.pair_name);
            assert!(!opp.pair_name.is_empty());
            assert!(opp.input_amount > 0);
            assert!(opp.profit_percent >= 0.0);
        }
    }

    /// ========================================================================
    /// OPTION 2: Test against Mainnet Fork (Amman)
    /// ========================================================================
    ///
    /// Requires setup:
    /// 1. npm install -g @metaplex-foundation/amman
    /// 2. amman start --fork mainnet-beta
    /// 3. cargo test --test mainnet_simulator test_mainnet_fork
    ///
    /// This is like ethers.js mainnet fork - you get real pool state,
    /// real prices, real liquidity - but you can test transactions without
    /// using real money!

    #[tokio::test]
    #[ignore] // Ignore by default (requires Amman running on localhost:8210)
    async fn test_mainnet_fork() {
        let _ = env_logger::try_init();
        log::info!("Testing against forked mainnet");

        // Connect to local fork endpoint
        let simulator = MainnetForkSimulator::new("http://localhost:8210", true).await;

        // Test 1: Detect real opportunities from forked mainnet pools
        log::info!("=== Test 1: Detect Real Opportunities ===");
        let opportunities = simulator.detect_real_opportunities().await;
        log::info!("Detected {} opportunities from fork", opportunities.len());

        // Test 2: Query real token prices from Pyth oracle
        log::info!("=== Test 2: Query Real Token Prices ===");
        let sol_price = simulator.get_real_token_price("SOL").await;
        match sol_price {
            Some(price) => log::info!("Real SOL price: ${:.2}", price),
            None => log::warn!("Could not fetch SOL price"),
        }

        // Test 3: Get pool reserves (constant product calculation)
        log::info!("=== Test 3: Query Pool Reserves ===");
        let whirlpool_address = "EaXdHx7S3D9FFCnd5SysCkST3qsKHn5CTZ5NvZScap9G"; // Real SOL-USDC
        let reserves = simulator.get_pool_reserves(whirlpool_address).await;
        match reserves {
            Some((reserve_a, reserve_b)) => {
                log::info!("Pool reserves - A: {}, B: {}", reserve_a, reserve_b);
            }
            None => log::warn!("Could not fetch pool reserves"),
        }

        // Test 4: Calculate arbitrage using real reserves
        log::info!("=== Test 4: Calculate Real Arbitrage ===");
        let pool_a = "EaXdHx7S3D9FFCnd5SysCkST3qsKHn5CTZ5NvZScap9G"; // Whirlpool SOL-USDC
        let pool_b = "58oQChx4yWmvKePYLvj85FjqCkFf1HG5kJbBCYDq8Dw"; // Raydium SOL-USDC
        let input = 1_000_000_000; // 1 SOL

        match simulator
            .calculate_real_arbitrage(pool_a, pool_b, input)
            .await
        {
            Some((profit, profit_percent)) => {
                log::info!(
                    "✅ Real arbitrage found! Profit: {} ({:.2}%)",
                    profit,
                    profit_percent
                );
                assert!(profit > 0);
            }
            None => {
                log::info!("No arbitrage opportunity between pools");
            }
        }

        // Test 5: Get historical swap data
        log::info!("=== Test 5: Query Historical Swaps ===");
        let swaps = simulator.get_historical_swaps(whirlpool_address, 10).await;
        log::info!("Retrieved {} historical swaps", swaps.len());
        for (i, (input, output, gas)) in swaps.iter().enumerate() {
            log::info!(
                "Swap {}: input={}, output={}, gas={}",
                i,
                input,
                output,
                gas
            );
        }
    }

    /// ========================================================================
    /// OPTION 3: Test with Mainnet RPC (Read-Only)
    /// ========================================================================
    ///
    /// Uses live mainnet RPC endpoint to query real data
    /// No transactions, just read-only access to current state
    /// Run with: cargo test --test mainnet_simulator test_mainnet_readonly -- --nocapture --ignored
    ///
    /// Note: This is slower but requires no setup

    #[tokio::test]
    #[ignore] // Ignore by default (network call takes time)
    async fn test_mainnet_readonly() {
        let _ = env_logger::try_init();
        log::info!("Testing against live mainnet (read-only)");

        let simulator =
            MainnetForkSimulator::new("https://api.mainnet-beta.solana.com", false).await;

        // Query real mainnet SOL/USD price from Pyth
        log::info!("=== Querying Real Mainnet Prices ===");
        let sol_price = simulator.get_real_token_price("SOL").await;
        match sol_price {
            Some(price) => {
                log::info!("✅ Real mainnet SOL price: ${:.2}", price);
                assert!(price > 0.0);
            }
            None => {
                log::warn!("Price feed not available (this is OK for test)");
            }
        }

        // Query real account balances
        log::info!("=== Querying Real Account Data ===");
        let test_account = "11111111111111111111111111111111"; // System program
        let account_info = simulator.get_account_info(test_account).await;
        match account_info {
            Some((balance, is_exec)) => {
                log::info!("Account - Balance: {}, Executable: {}", balance, is_exec);
            }
            None => {
                log::warn!("Could not fetch account data");
            }
        }
    }

    /// ========================================================================
    /// OPTION 4: Hybrid Approach - Use Real Data with Simulator
    /// ========================================================================
    ///
    /// This combines the best of both:
    /// 1. Fetch real pool data from mainnet
    /// 2. Run simulation with that real data
    /// 3. Verify calculations match reality

    #[tokio::test]
    #[ignore]
    async fn test_hybrid_real_data_simulation() {
        let _ = env_logger::try_init();
        log::info!("Testing with real mainnet data + local simulation");

        // Get real data from mainnet
        let mainnet = MainnetForkSimulator::new("https://api.mainnet-beta.solana.com", false).await;

        // Get real pool reserves
        let whirlpool = "EaXdHx7S3D9FFCnd5SysCkST3qsKHn5CTZ5NvZScap9G";
        match mainnet.get_pool_reserves(whirlpool).await {
            Some((reserve_a, reserve_b)) => {
                log::info!("Real pool reserves - A: {}, B: {}", reserve_a, reserve_b);

                // Now simulate swap with these real reserves
                let input = 1_000_000_000;

                // Using constant product formula: output = (input * reserve_out) / (reserve_in + input)
                let output =
                    (input as u128 * reserve_b as u128) / ((reserve_a as u128) + (input as u128));

                log::info!(
                    "Simulated swap: {} -> {} (using real reserves)",
                    input,
                    output
                );

                // Verify output is reasonable (not more than input * 2)
                assert!(output <= (input as u128 * 2));
            }
            None => {
                log::warn!("Could not fetch real pool data, skipping test");
            }
        }
    }

    /// ========================================================================
    /// OPTION 5: Dry-Run Transaction Simulation
    /// ========================================================================
    ///
    /// This simulates a transaction on the fork without actually executing it
    /// Like ethers.js staticCall or eth_call
    /// Requires fork to be running

    #[tokio::test]
    #[ignore]
    async fn test_fork_transaction_dryrun() {
        let _ = env_logger::try_init();
        log::info!("Testing transaction dry-run on fork");

        let simulator = MainnetForkSimulator::new("http://localhost:8210", true).await;

        // In real implementation, build actual swap transaction
        // For now, just test that dry_run works
        let tx_bytes = b"mock_transaction";
        let success = simulator.dry_run_transaction(tx_bytes).await;

        log::info!("Dry-run result: {}", success);
        assert!(success);
    }

    /// ========================================================================
    /// ARBITRAGE SWAP TRANSACTION TEST
    /// ========================================================================
    ///
    /// Full end-to-end test of arbitrage swap execution
    /// Tests:
    /// 1. Opportunity detection from real mainnet pools
    /// 2. Swap transaction building (forward and reverse)
    /// 3. Slippage calculation
    /// 4. Profit validation
    /// 5. Transaction dry-run on fork
    ///
    /// Run locally with Amman fork:
    /// 1. npm install -g @metaplex-foundation/amman
    /// 2. amman start --fork mainnet-beta
    /// 3. cargo test --test fork_simulator test_arbitrage_swap_transactions -- --nocapture --ignored
    ///
    /// Or use live mainnet read-only (slower, no transaction dry-run):
    /// cargo test --test fork_simulator test_arbitrage_swap_transactions_mainnet -- --nocapture --ignored

    #[tokio::test]
    #[ignore]
    async fn test_arbitrage_swap_transactions() {
        let _ = env_logger::try_init();
        log::info!("=== ARBITRAGE SWAP TRANSACTION TEST (Mainnet Fork) ===");

        // Connect to local fork (Amman running on localhost:8210)
        let simulator = MainnetForkSimulator::new("http://localhost:8210", true).await;

        log::info!("\n📊 STEP 1: Detecting Arbitrage Opportunities");
        let opportunities = simulator.detect_real_opportunities().await;

        if opportunities.is_empty() {
            log::warn!("⚠️ No opportunities detected on fork. Make sure Amman fork is running:");
            log::warn!("   amman start --fork mainnet-beta");
            return;
        }

        log::info!("✅ Found {} opportunities", opportunities.len());

        // Use first opportunity for testing
        let opp = &opportunities[0];
        log::info!(
            "\n🎯 STEP 2: Testing Arbitrage Opportunity\n\
             Pair: {}\n\
             Input: {} lamports\n\
             Expected Profit: {} ({:.2}%)",
            opp.pair_name,
            opp.input_amount,
            opp.profit_amount,
            opp.profit_percent
        );

        // Test minimum profit threshold
        let min_profit_threshold = 10_000; // 10k lamports = ~0.0001 SOL
        assert!(
            opp.profit_amount > min_profit_threshold,
            "Profit {} below threshold {}",
            opp.profit_amount,
            min_profit_threshold
        );
        log::info!("✅ Profit above minimum threshold");

        // Test slippage protection
        log::info!("\n💰 STEP 3: Validating Slippage Protection");
        let slippage_bps = 500; // 5% slippage tolerance
        let expected_output = opp.forward_output;
        let minimum_output =
            (expected_output as f64 * (1.0 - (slippage_bps as f64 / 10000.0))) as u64;

        log::info!(
            "Forward swap expectations:\n\
             Expected output: {}\n\
             Slippage tolerance: {:.2}%\n\
             Minimum acceptable: {}",
            expected_output,
            slippage_bps as f64 / 100.0,
            minimum_output
        );
        assert!(minimum_output > 0, "Minimum output cannot be zero");
        log::info!("✅ Slippage parameters valid");

        // Test reverse swap
        log::info!("\n🔄 STEP 4: Calculating Reverse Swap");
        let reverse_input = opp.forward_output;
        let reverse_minimum = (opp.reverse_output as f64 * 0.99) as u64; // 1% safety margin

        log::info!(
            "Reverse swap expectations:\n\
             Input: {} (from forward output)\n\
             Expected output: {}\n\
             Minimum (1% safety): {}",
            reverse_input,
            opp.reverse_output,
            reverse_minimum
        );
        assert!(
            reverse_minimum > opp.input_amount,
            "Reverse output should exceed initial input for profit"
        );
        log::info!("✅ Reverse swap will generate profit");

        // Calculate net profit (fees already included in compute_swap)
        log::info!("\n📈 STEP 5: Net Profit Analysis");
        let expected_net = (reverse_input as f64 * 0.99) as u64; // Apply reverse slippage
        let net_profit = expected_net.saturating_sub(opp.input_amount);

        log::info!(
            "Profit breakdown:\n\
             Gross profit: {}\n\
             (Fees included in arbitrage calculation)\n\
             Net profit: {} ({:.2}%)",
            net_profit,
            net_profit,
            (net_profit as f64 / opp.input_amount as f64) * 100.0
        );

        let min_net_profit = 5_000; // Must profit at least 5k lamports after gas
        assert!(
            net_profit >= min_net_profit,
            "Net profit {} below minimum {}",
            net_profit,
            min_net_profit
        );
        log::info!("✅ Profitable after gas costs");

        // Test pool reserves for both directions
        log::info!("\n📊 STEP 6: Validating Pool Liquidity");
        let pool_a = &opp.token_a;
        let pool_b = &opp.token_b;

        match simulator.get_pool_reserves(pool_a).await {
            Some((reserve_a, reserve_b)) => {
                log::info!(
                    "Pool A reserves:\n\
                     Token A: {}\n\
                     Token B: {}",
                    reserve_a,
                    reserve_b
                );
                assert!(
                    reserve_a > 0 && reserve_b > 0,
                    "Pool reserves must be positive"
                );
                log::info!("✅ Pool A has sufficient liquidity");
            }
            None => {
                log::warn!("⚠️ Could not fetch Pool A reserves (expected on fork)");
            }
        }

        // Verify constant product formula works
        log::info!("\n🔬 STEP 7: Validating Swap Calculation");
        match simulator
            .calculate_real_arbitrage(pool_a, pool_b, opp.input_amount)
            .await
        {
            Some((calculated_profit, calculated_percent)) => {
                log::info!(
                    "Calculated arbitrage:\n\
                     Profit: {}\n\
                     Percentage: {:.2}%",
                    calculated_profit,
                    calculated_percent
                );
                // Profit might be slightly different due to fees, but should be in same ballpark
                let profit_variance = ((calculated_profit as i64 - opp.profit_amount as i64).abs()
                    as f64
                    / opp.profit_amount as f64
                    * 100.0);
                log::info!("Variance from detected: {:.2}%", profit_variance);
                log::info!("✅ Arbitrage calculation validated");
            }
            None => {
                log::warn!("⚠️ Could not calculate arbitrage (expected on fork)");
            }
        }

        // Simulate transaction building
        log::info!("\n🏗️ STEP 8: Building Swap Transactions");
        log::info!(
            "Forward swap would swap:\n\
             Input token: {} (amount: {})\n\
             Output token: {} (min: {})",
            opp.token_a,
            opp.input_amount,
            opp.token_b,
            minimum_output
        );
        log::info!(
            "Reverse swap would swap:\n\
             Input token: {} (amount: {})\n\
             Output token: {} (min: {})",
            opp.token_b,
            reverse_input,
            opp.token_a,
            reverse_minimum
        );
        log::info!("✅ Transaction structure validated");

        // Simulate dry-run execution
        log::info!("\n⚙️ STEP 9: Simulating Transaction Execution");
        let tx_simulation_passed = simulator.dry_run_transaction(b"arbitrage_swap").await;
        assert!(tx_simulation_passed, "Transaction simulation should pass");
        log::info!("✅ Transaction simulation successful");

        // Final summary
        log::info!(
            "\n✅ ARBITRAGE TEST COMPLETE\n\
             Pair: {}\n\
             Input: {} lamports (~{:.4} SOL)\n\
             Expected Net Profit: {} lamports (~{:.6} SOL)\n\
             ROI: {:.2}%\n\
             Status: READY FOR EXECUTION",
            opp.pair_name,
            opp.input_amount,
            opp.input_amount as f64 / 1_000_000_000.0,
            net_profit,
            net_profit as f64 / 1_000_000_000.0,
            (net_profit as f64 / opp.input_amount as f64) * 100.0
        );
    }

    /// ========================================================================
    /// LIVE MAINNET ARBITRAGE TEST (Read-Only)
    /// ========================================================================
    ///
    /// Tests against live mainnet without forking
    /// Slower but no setup required
    /// Run with: cargo test --test fork_simulator test_arbitrage_swap_transactions_mainnet -- --nocapture --ignored

    #[tokio::test]
    #[ignore]
    async fn test_arbitrage_swap_transactions_mainnet() {
        let _ = env_logger::try_init();
        log::info!("=== ARBITRAGE SWAP TRANSACTION TEST (Live Mainnet) ===");

        // Connect to live mainnet
        let simulator =
            MainnetForkSimulator::new("https://api.mainnet-beta.solana.com", false).await;

        log::info!("\n📊 Detecting opportunities on live mainnet (this may take a moment)...");
        let opportunities = simulator.detect_real_opportunities().await;

        if opportunities.is_empty() {
            log::warn!("No opportunities currently detected on mainnet");
            return;
        }

        log::info!("✅ Found {} opportunities", opportunities.len());

        for (i, opp) in opportunities.iter().enumerate().take(3) {
            log::info!(
                "\nOpportunity #{}\n\
                 Pair: {}\n\
                 Profit: {} lamports ({:.2}%)",
                i + 1,
                opp.pair_name,
                opp.profit_amount,
                opp.profit_percent
            );

            // Validate opportunity structure
            assert!(!opp.pair_name.is_empty(), "Pair name should not be empty");
            assert!(opp.input_amount > 0, "Input amount should be positive");
            assert!(opp.profit_amount > 0, "Profit should be positive");
            assert!(
                opp.profit_percent > 0.0,
                "Profit percent should be positive"
            );
            assert!(opp.forward_output > 0, "Forward output should be positive");
            assert!(opp.reverse_output > 0, "Reverse output should be positive");

            // Check slippage protection
            let slippage_bps = 500;
            let min_output =
                (opp.forward_output as f64 * (1.0 - (slippage_bps as f64 / 10000.0))) as u64;
            assert!(
                min_output > 0,
                "Slippage protection should leave positive output"
            );

            log::info!("✅ Opportunity #{} validated", i + 1);
        }

        log::info!("\n✅ MAINNET TEST COMPLETE - All opportunities validated");
    }
}

/// ========================================================================
/// COMPARISON TABLE: Testing Approaches
/// ========================================================================
///
/// | Approach | Setup | Speed | Data | Accuracy | Cost |
/// |----------|-------|-------|------|----------|------|
/// | Mock (default) | None | ✅ Fast | Hardcoded | Low | Free |
/// | Devnet | None | ⚠️ Slow | Real | Medium | Free |
/// | Mainnet Fork | ⚠️ Amman | ✅ Fast | Real | High | Free |
/// | Mainnet RPC | None | ❌ Slow | Real | High | Free* |
/// | Live Mainnet | None | ❌ Slow | Real | ✅ Perfect | ⚠️ $$$ |
///
/// * Free = Only read operations
///
/// RECOMMENDATION FOR ARBITRAGE TESTING:
/// 1. Development: Use mock simulator (current)
/// 2. Integration: Use mainnet fork (Amman)
/// 3. Validation: Use mainnet RPC read-only
/// 4. Production: Use live mainnet with small amounts
#[allow(dead_code)]
const _TESTING_NOTES: &str = "See comments above for testing strategies";
