use crate::failed_pool_cache;
use anyhow::{anyhow, Result};
use eth_dex_quote::v2::V2Quoter;
use eth_dex_quote::v3::V3Quoter;
use eth_dex_quote::v4::V4Quoter;
use eth_dex_quote::{DexType, TokenPriceUpdate, UniswapV2Quoter, UniswapV3Quoter, UniswapV4Quoter};
use ethers::types::{Address, U256};
use log::info;
use std::sync::Arc;

/// Compute output amount using V2 quoter for a token swap
///
/// # Arguments
/// * `provider` - Ethereum provider
/// * `amount_in` - Input amount (with decimals)
/// * `token_in` - Input token address (e.g., USDT)
/// * `token_out` - Output token address (e.g., the arbitrage token)
/// * `router_address` - Address of Uniswap V2 Router
///
/// # Returns
/// Expected output amount in tokens from the actual pool contract
pub async fn compute_output_amount_v2<P: ethers::providers::Middleware + 'static>(
    provider: Arc<P>,
    amount_in: U256,
    token_in: Address,
    token_out: Address,
    router_address: Address,
) -> Result<U256> {
    // Create V2 quoter
    let quoter = UniswapV2Quoter::new(provider).with_router(router_address);

    // Path: token_in -> token_out
    let path = vec![token_in, token_out];

    // Get quote from router
    let amount_out = quoter
        .get_quote(amount_in, path)
        .await
        .map_err(|e| anyhow!("V2 quote failed: {:?}", e))?;

    Ok(amount_out)
}

/// Compute output amount using V3 quoter for a token swap
///
/// # Arguments
/// * `provider` - Ethereum provider
/// * `amount_in` - Input amount (with decimals)
/// * `token_in` - Input token address (e.g., USDT)
/// * `token_out` - Output token address (e.g., the arbitrage token)
/// * `fee_tier` - Fee tier for the V3 pool (e.g., 3000 for 0.3%)
/// * `quoter_v3_address` - Address of Uniswap V3 Quoter contract
///
/// # Returns
/// Expected output amount in tokens from the actual pool contract
pub async fn compute_output_amount_v3<P: ethers::providers::Middleware + 'static>(
    provider: Arc<P>,
    amount_in: U256,
    token_in: Address,
    token_out: Address,
    fee_tier: u32,
    quoter_v3_address: Address,
) -> Result<U256> {
    // Create V3 quoter
    let quoter = UniswapV3Quoter::new(provider, quoter_v3_address);

    // Get quote from V3 quoter contract
    let swap_quote = quoter
        .get_quote(token_in, token_out, amount_in, fee_tier)
        .await
        .map_err(|e| anyhow!("V3 quote failed: {:?}", e))?;

    Ok(swap_quote.amount_out)
}

/// Compute output amount using V4 quoter for a token swap
///
/// # Arguments
/// * `provider` - Ethereum provider
/// * `amount_in` - Input amount (with decimals)
/// * `token_in` - Input token address (e.g., USDT)
/// * `token_out` - Output token address (e.g., the arbitrage token)
/// * `fee_tier` - Fee tier for the V4 pool
/// * `tick_spacing` - Tick spacing for the V4 pool
/// * `quoter_v4_address` - Address of Uniswap V4 QuoteRouter contract
///
/// # Returns
/// Expected output amount in tokens from the actual pool contract
pub async fn compute_output_amount_v4<P: ethers::providers::Middleware + 'static>(
    provider: Arc<P>,
    amount_in: U256,
    token_in: Address,
    token_out: Address,
    fee_tier: u32,
    tick_spacing: i32,
    quoter_v4_address: Address,
    pool_id: Option<String>,
    hooks: Address,
) -> Result<U256> {
    // Create V4 quoter
    let quoter = UniswapV4Quoter::new(provider, quoter_v4_address);

    // Check if pool is known to fail
    if let Some(pid) = &pool_id {
        if failed_pool_cache::is_pool_failed(pid) {
            return Err(anyhow!("Skipping known failing pool: {}", pid));
        }
    }

    // Get quote from V4 quoter contract
    let swap_quote = quoter
        .get_quote(
            token_in,
            token_out,
            amount_in,
            fee_tier,
            tick_spacing,
            pool_id.clone(),
            hooks,
        )
        .await
        .map_err(|e| {
            // Cache failing pool if it reverts
            if let Some(pid) = &pool_id {
                let err_str = format!("{:?}", e);
                if err_str.contains("Revert") || err_str.contains("V4 Quoter call failed") {
                    failed_pool_cache::mark_pool_failed(pid);
                    info!("Marked pool {} as failed due to revert", pid);
                }
            }
            anyhow!(format!("V4 quote failed (token_in={:?}, token_out={:?}, amount_in={}, fee_tier={}, tick_spacing={}, quoter_v4_address={:?}) : {:?}", token_in, token_out, amount_in, fee_tier, tick_spacing, quoter_v4_address, e))
        })?;

    Ok(swap_quote.amount_out)
}

/// Execute a multi-hop arbitrage path with variable number of swaps
/// Example: X -> A -> B -> X (3-hop) or X -> A -> X (2-hop)
///
/// # Arguments
/// * `provider` - Ethereum provider
/// * `flashloan_amount` - Amount of token X to flashloan (e.g., USDT amount)
/// * `swap_path` - Array of (token_in, token_out, pool) tuples representing the swap path
/// * `router_v2_address` - V2 router address
/// * `quoter_v3_address` - V3 quoter address
/// * `quoter_v4_address` - V4 quoter address
///
/// # Returns
/// (intermediate_amounts, final_amount_x, net_profit_x) where net_profit_x = final_amount_x - flashloan_amount
pub async fn compute_multi_hop_arbitrage<P: ethers::providers::Middleware + 'static>(
    provider: Arc<P>,
    flashloan_amount: U256,
    swap_path: Vec<(Address, Address, &TokenPriceUpdate)>, // (token_in, token_out, pool)
    router_v2_address: Address,
    quoter_v3_address: Address,
    quoter_v4_address: Address,
) -> Result<(Vec<U256>, U256, i128)> {
    if swap_path.is_empty() {
        return Err(anyhow!("Swap path cannot be empty"));
    }

    let mut intermediate_amounts = Vec::new();
    let mut current_amount = flashloan_amount;

    // Execute each swap in the path
    for (token_in, token_out, pool) in swap_path {
        let amount_out = compute_swap_output(
            provider.clone(),
            current_amount,
            token_in,
            token_out,
            pool,
            router_v2_address,
            quoter_v3_address,
            quoter_v4_address,
        )
        .await?;

        intermediate_amounts.push(amount_out);
        current_amount = amount_out;
    }

    // Final amount should be in the same token as flashloan (token X)
    let final_amount = current_amount;

    // Calculate profit: final_amount - flashloan_amount
    // Use U256 subtraction to avoid overflow, then convert to i128 for the result
    let net_profit = if final_amount >= flashloan_amount {
        (final_amount - flashloan_amount).as_u128() as i128
    } else {
        -((flashloan_amount - final_amount).as_u128() as i128)
    };

    Ok((intermediate_amounts, final_amount, net_profit))
}

/// Execute a 2-hop arbitrage path: X -> A -> X
///
/// # Arguments
/// * `provider` - Ethereum provider
/// * `flashloan_amount` - Amount of token X to flashloan (e.g., USDT amount)
/// * `token_x` - Pairing token address (e.g., USDT/USDC)
/// * `token_a` - Arbitrage token address
/// * `buy_pool` - TokenPriceUpdate for the buy pool (X -> A)
/// * `sell_pool` - TokenPriceUpdate for the sell pool (A -> X)
/// * `router_v2_address` - V2 router address
/// * `quoter_v3_address` - V3 quoter address
/// * `quoter_v4_address` - V4 quoter address
///
/// # Returns
/// (amount_a_from_buy, amount_x_from_sell, net_profit_x) where net_profit_x = amount_x_from_sell - flashloan_amount
pub async fn compute_arbitrage_path<P: ethers::providers::Middleware + 'static>(
    provider: Arc<P>,
    flashloan_amount: U256,
    token_x: Address,
    token_a: Address,
    buy_pool: &TokenPriceUpdate,
    sell_pool: &TokenPriceUpdate,
    router_v2_address: Address,
    quoter_v3_address: Address,
    quoter_v4_address: Address,
) -> Result<(U256, U256, i128)> {
    let other_token_in_sell = if token_a == sell_pool.pool_token0 {
        sell_pool.pool_token1
    } else {
        sell_pool.pool_token0
    };
    let swap_path = vec![
        (token_x, token_a, buy_pool),              // X -> A
        (token_a, other_token_in_sell, sell_pool), // A -> X
    ];

    let (amounts, final_amount, net_profit) = compute_multi_hop_arbitrage(
        provider,
        flashloan_amount,
        swap_path,
        router_v2_address,
        quoter_v3_address,
        quoter_v4_address,
    )
    .await?;

    // amounts[0] = amount_a, amounts[1] = amount_x
    Ok((amounts[0], final_amount, net_profit))
}

/// Execute a 3-hop arbitrage path: X -> A -> B -> X
///
/// # Arguments
/// * `provider` - Ethereum provider
/// * `flashloan_amount` - Amount of token X to flashloan (e.g., USDT amount)
/// * `token_x` - Pairing token address (e.g., USDT/USDC)
/// * `token_a` - First arbitrage token address
/// * `token_b` - Second arbitrage token address
/// * `pool_x_to_a` - TokenPriceUpdate for the first pool (X -> A)
/// * `pool_a_to_b` - TokenPriceUpdate for the second pool (A -> B)
/// * `pool_b_to_x` - TokenPriceUpdate for the third pool (B -> X)
/// * `router_v2_address` - V2 router address
/// * `quoter_v3_address` - V3 quoter address
/// * `quoter_v4_address` - V4 quoter address
///
/// # Returns
/// (amount_a, amount_b, amount_x_final, net_profit_x) where net_profit_x = amount_x_final - flashloan_amount
pub async fn compute_3hop_arbitrage_path<P: ethers::providers::Middleware + 'static>(
    provider: Arc<P>,
    flashloan_amount: U256,
    token_x: Address,
    token_a: Address,
    token_b: Address,
    pool_x_to_a: &TokenPriceUpdate,
    pool_a_to_b: &TokenPriceUpdate,
    pool_b_to_x: &TokenPriceUpdate,
    router_v2_address: Address,
    quoter_v3_address: Address,
    quoter_v4_address: Address,
) -> Result<(U256, U256, U256, i128)> {
    let swap_path = vec![
        (token_x, token_a, pool_x_to_a), // X -> A
        (token_a, token_b, pool_a_to_b), // A -> B
        (token_b, token_x, pool_b_to_x), // B -> X
    ];

    let (amounts, final_amount, net_profit) = compute_multi_hop_arbitrage(
        provider,
        flashloan_amount,
        swap_path,
        router_v2_address,
        quoter_v3_address,
        quoter_v4_address,
    )
    .await?;

    // amounts[0] = amount_a, amounts[1] = amount_b, amounts[2] = amount_x
    Ok((amounts[0], amounts[1], final_amount, net_profit))
}
/// Helper function to compute swap output based on pool's dex_version
async fn compute_swap_output<P: ethers::providers::Middleware + 'static>(
    provider: Arc<P>,
    amount_in: U256,
    token_in: Address,
    token_out: Address,
    pool: &TokenPriceUpdate,
    router_v2_address: Address,
    quoter_v3_address: Address,
    quoter_v4_address: Address,
) -> Result<U256> {
    // Determine dex type from version string
    let dex_type = match pool.dex_version.to_lowercase().as_str() {
        s if s.contains("v2") => DexType::UniswapV2,
        s if s.contains("v3") => DexType::UniswapV3,
        s if s.contains("v4") => DexType::UniswapV4,
        _ => return Err(anyhow!("Unknown DEX version: {}", pool.dex_version)),
    };

    match dex_type {
        DexType::UniswapV2 => {
            compute_output_amount_v2(provider, amount_in, token_in, token_out, router_v2_address)
                .await
        }
        DexType::UniswapV3 => {
            let fee_tier = pool
                .fee_tier
                .ok_or_else(|| anyhow!("No fee tier for V3 pool"))?;
            compute_output_amount_v3(
                provider,
                amount_in,
                token_in,
                token_out,
                fee_tier,
                quoter_v3_address,
            )
            .await
        }
        DexType::UniswapV4 => {
            let fee_tier = pool
                .fee_tier
                .ok_or_else(|| anyhow!("No fee tier for V4 pool"))?;
            let tick_spacing = pool
                .tick_spacing
                .ok_or_else(|| anyhow!("No tick spacing for V4 pool"))?;
            info!("Computing V4 quote: pool_address={:?}, fee_tier={}, tick_spacing={}, token0={:?}, token1={:?}", pool.pool_address, fee_tier, tick_spacing, pool.pool_token0, pool.pool_token1);
            compute_output_amount_v4(
                provider,
                amount_in,
                token_in,
                token_out,
                fee_tier,
                tick_spacing,
                quoter_v4_address,
                Some(pool.pool_address.clone()),
                pool.hooks.unwrap_or_default(),
            )
            .await
        }
    }
}

/// Helper to convert f64 USDT amount to U256 with 6 decimals (USDT standard)
pub fn usdt_to_u256(amount: f64) -> U256 {
    let decimals = 6; // USDT has 6 decimals
    let multiplier = 10_f64.powi(decimals);
    let amount_with_decimals = amount * multiplier;
    U256::from(amount_with_decimals as u64)
}

/// Helper to convert f64 USDC amount to U256 with 6 decimals (USDC standard)
pub fn usdc_to_u256(amount: f64) -> U256 {
    let decimals = 6; // USDC has 6 decimals
    let multiplier = 10_f64.powi(decimals);
    let amount_with_decimals = amount * multiplier;
    U256::from(amount_with_decimals as u64)
}

/// Fetch available pairs for token B from amm-eth API and find best B -> X swap
///
/// # Arguments
/// * `http_client` - HTTP client for making API requests
/// * `dex_pair_api_url` - Base URL of amm-eth API (e.g., http://localhost:8080)
/// * `token_b` - Token B address (intermediate token)
/// * `token_x` - Token X address (base token to return to)
/// * `amount_b` - Amount of token B to swap
/// * `provider` - Ethereum provider for on-chain quotes
/// * `router_v2_address` - V2 router address
/// * `quoter_v3_address` - V3 quoter address
/// * `quoter_v4_address` - V4 quoter address
///
/// # Returns
/// (best_pool, best_amount_x, pool_info) tuple with best swap result
pub async fn find_best_b_to_x_swap<P: ethers::providers::Middleware + 'static>(
    http_client: reqwest::Client,
    dex_pair_api_url: &str,
    token_b: Address,
    token_x: Address,
    amount_b: U256,
    provider: Arc<P>,
    router_v2_address: Address,
    quoter_v3_address: Address,
    quoter_v4_address: Address,
) -> Result<(TokenPriceUpdate, U256)> {
    // Fetch all pairs containing token B from amm-eth API
    let token_b_str = format!("{:?}", token_b).to_lowercase();
    let url = format!("{}/pairs/{}", dex_pair_api_url, token_b_str);

    let response = http_client
        .get(&url)
        .send()
        .await
        .map_err(|e| anyhow!("Failed to fetch pairs from amm-eth: {}", e))?;

    let pairs_data: serde_json::Value = response
        .json()
        .await
        .map_err(|e| anyhow!("Failed to parse pairs response: {}", e))?;

    let pairs_array = pairs_data
        .get("pairs")
        .and_then(|p| p.as_array())
        .ok_or_else(|| anyhow!("No pairs found in response"))?;

    if pairs_array.is_empty() {
        return Err(anyhow!("No pairs available for token B"));
    }

    let token_x_str = format!("{:?}", token_x).to_lowercase();
    let mut best_amount_out = U256::zero();
    let mut best_pool: Option<TokenPriceUpdate> = None;

    // Iterate through all pairs and find the one with highest output for B -> X
    for pair_json in pairs_array {
        let pool_address = pair_json
            .get("pool_address")
            .and_then(|p| p.as_str())
            .unwrap_or("");
        let token0 = pair_json
            .get("pool_token0")
            .and_then(|p| p.as_str())
            .unwrap_or("");
        let token1 = pair_json
            .get("pool_token1")
            .and_then(|p| p.as_str())
            .unwrap_or("");
        let dex_version = pair_json
            .get("dex_version")
            .and_then(|p| p.as_str())
            .expect("Dex version missing");
        let fee_tier = pair_json
            .get("fee_tier")
            .and_then(|p| p.as_u64())
            .map(|f| f as u32);
        let tick_spacing = pair_json
            .get("tick_spacing")
            .and_then(|p| p.as_i64())
            .map(|t| t as i32);

        let hooks = pair_json
            .get("hooks")
            .and_then(|p| p.as_str())
            .and_then(|s| s.parse::<Address>().ok());

        let decimals0 = pair_json
            .get("decimals0")
            .and_then(|p| p.as_u64())
            .expect("Decimals0 missing") as u8;
        let decimals1 = pair_json
            .get("decimals1")
            .and_then(|p| p.as_u64())
            .expect("Decimals1 missing") as u8;

        // Check if this pair can swap B -> X (B in token0 or token1, X in the other)
        let (_token_in_str, token_out_str, decimals_out) =
            if token0 == token_b_str && token1 == token_x_str {
                (token0.to_string(), token1.to_string(), decimals1)
            } else if token1 == token_b_str && token0 == token_x_str {
                (token1.to_string(), token0.to_string(), decimals0)
            } else {
                // This pair does not support B -> X swap
                continue;
            };

        // info!("Pool = {}, token0 = {}, token1 = {}, dex_version = {}, token_in = {}, token_out = {}", pool_address, token0, token1, dex_version, token_in_str, token_out_str);

        // Create a TokenPriceUpdate for this pool
        let pool_update = TokenPriceUpdate {
            token_address: token_out_str.clone(),
            price_in_eth: 0.0, // We don't need this for quoting
            price_in_usd: None,
            pool_address: pool_address.to_string(),
            dex_version: dex_version.to_string(),
            decimals: decimals_out,
            last_updated: 0,
            pool_token0: token0.parse::<Address>().unwrap(),
            pool_token1: token1.parse::<Address>().unwrap(),
            eth_chain: eth_dex_quote::EthChain::Mainnet,
            fee_tier,
            tick_spacing,
            hooks,
            eth_price_usd: 2500.0, // just any price
        };

        // Get the actual quote for this swap
        match compute_swap_output(
            provider.clone(),
            amount_b,
            token_b,
            token_x,
            &pool_update,
            router_v2_address,
            quoter_v3_address,
            quoter_v4_address,
        )
        .await
        {
            Ok(amount_out) => {
                if amount_out > best_amount_out {
                    best_amount_out = amount_out;
                    best_pool = Some(pool_update);
                }
            }
            Err(_) => {
                // Skip this pool if quote fails
                continue;
            }
        }
    }

    let pool = best_pool.ok_or_else(|| anyhow!("No valid pool found for B -> X swap"))?;
    Ok((pool, best_amount_out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_usdt_to_u256() {
        // 100 USDT with 6 decimals = 100_000_000
        assert_eq!(usdt_to_u256(100.0), U256::from(100_000_000u64));

        // 0.5 USDT with 6 decimals = 500_000
        assert_eq!(usdt_to_u256(0.5), U256::from(500_000u64));
    }

    #[test]
    fn test_usdc_to_u256() {
        // 1000 USDC with 6 decimals = 1_000_000_000
        assert_eq!(usdc_to_u256(1000.0), U256::from(1_000_000_000u64));
    }

    #[test]
    fn test_usdt_to_u256_edge_cases() {
        // Test zero
        assert_eq!(usdt_to_u256(0.0), U256::from(0u64));

        // Test large amounts
        assert_eq!(usdt_to_u256(1_000_000.0), U256::from(1_000_000_000_000u64));

        // Test fractional amounts
        let one_cent = usdt_to_u256(0.01);
        assert_eq!(one_cent, U256::from(10_000u64)); // 0.01 * 10^6
    }

    #[test]
    fn test_usdc_to_u256_edge_cases() {
        // Test zero
        assert_eq!(usdc_to_u256(0.0), U256::from(0u64));

        // Test large amounts
        assert_eq!(usdc_to_u256(100_000.0), U256::from(100_000_000_000u64));
    }

    #[test]
    fn test_u256_to_f64_conversion() {
        // Test converting U256 back to human-readable format
        let amount_u256 = usdt_to_u256(100.0);
        let amount_in_wei = amount_u256.as_u128() as f64;
        let amount_in_tokens = amount_in_wei / 1_000_000.0; // 6 decimals
        assert_eq!(amount_in_tokens, 100.0);
    }

    #[test]
    fn test_profit_calculation_simulation() {
        // Simulate a 2-hop arbitrage profit calculation
        let flashloan_amount_u128: i128 = usdt_to_u256(100.0).as_u128() as i128;

        // Assume we get back 101 USDT after 2 hops
        let final_amount_u128: i128 = usdt_to_u256(101.0).as_u128() as i128;

        let net_profit = final_amount_u128 - flashloan_amount_u128;

        // net_profit should be 1 USDT (in wei units)
        assert_eq!(net_profit, usdt_to_u256(1.0).as_u128() as i128);

        // Calculate profit percentage
        let profit_percent = (net_profit as f64 / flashloan_amount_u128 as f64) * 100.0;
        assert!((profit_percent - 1.0).abs() < 0.01); // ~1% profit
    }

    #[test]
    fn test_negative_profit_calculation() {
        // Test scenario where we lose money (due to slippage/fees)
        let flashloan_amount_u128: i128 = usdt_to_u256(100.0).as_u128() as i128;

        // Assume we get back 98 USDT after 2 hops (2% loss)
        let final_amount_u128: i128 = usdt_to_u256(98.0).as_u128() as i128;

        let net_profit = final_amount_u128 - flashloan_amount_u128;

        // net_profit should be negative
        assert!(net_profit < 0);

        // Calculate loss percentage
        let loss_percent = (net_profit as f64 / flashloan_amount_u128 as f64) * 100.0;
        assert!((loss_percent - (-2.0)).abs() < 0.01); // ~2% loss
    }

    /// Test compute_swap_output with real on-chain V3 pool (USDT/USDC)
    #[tokio::test]
    async fn test_compute_swap_output_v3_real() {
        use ethers::providers::{Http, Provider};

        let rpc_url =
            std::env::var("ETH_RPC_URL").unwrap_or_else(|_| "https://eth.merkle.io".to_string());
        let provider = Arc::new(
            Provider::<Http>::try_from(rpc_url.as_str()).expect("Failed to create provider"),
        );

        // Real USDT/USDC V3 pool on Ethereum (0.01% fee tier)
        let usdt = "0xdac17f958d2ee523a2206206994597c13d831ec7"
            .parse::<Address>()
            .unwrap();
        let usdc = "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48"
            .parse::<Address>()
            .unwrap();

        let pool_update = TokenPriceUpdate {
            token_address: format!("{:?}", usdt).to_lowercase(),
            price_in_eth: 0.0004,
            price_in_usd: Some(1.0),
            pool_address: "0x3416cf6c708da44db2624d63ea0aaef7113527c6".to_string(),
            dex_version: "UniswapV3".to_string(),
            decimals: 6,
            last_updated: 0,
            pool_token0: usdt,
            pool_token1: usdc,
            eth_chain: eth_dex_quote::EthChain::Mainnet,
            fee_tier: Some(100), // 0.01%
            tick_spacing: Some(1),
            hooks: None,
            eth_price_usd: 2500.0,
        };

        // Test swap: 100 USDT -> USDC
        let amount_in = usdt_to_u256(100.0);
        let quoter_v3 = "0xb27308f9f90d607463bb33ea1bebb41c27ce5ab6"
            .parse::<Address>()
            .unwrap();
        let router_v2 = Address::zero(); // Not used for V3
        let quoter_v4 = Address::zero(); // Not used for V3

        let result = compute_swap_output(
            provider,
            amount_in,
            usdt,
            usdc,
            &pool_update,
            router_v2,
            quoter_v3,
            quoter_v4,
        )
        .await;

        match result {
            Ok(amount_out) => {
                println!(
                    "✅ V3 Swap Test: 100 USDT -> {} USDC",
                    amount_out.as_u128() as f64 / 1_000_000.0
                );
                assert!(amount_out > U256::zero(), "Output should be non-zero");
                // USDT and USDC should be roughly 1:1, accounting for fees
                let expected_min = usdc_to_u256(99.0); // Allow 1% slippage
                assert!(
                    amount_out > expected_min,
                    "Output too low: got {}, expected > {}",
                    amount_out,
                    expected_min
                );
            }
            Err(e) => {
                panic!("V3 swap test failed: {:?}", e);
            }
        }
    }

    /// Test compute_arbitrage_path with real USDT/USDC pools
    #[tokio::test]
    async fn test_compute_arbitrage_path_real() {
        use ethers::providers::{Http, Provider};

        let rpc_url =
            std::env::var("ETH_RPC_URL").unwrap_or_else(|_| "https://eth.merkle.io".to_string());
        let provider = Arc::new(
            Provider::<Http>::try_from(rpc_url.as_str()).expect("Failed to create provider"),
        );

        let usdt = "0xdac17f958d2ee523a2206206994597c13d831ec7"
            .parse::<Address>()
            .unwrap();
        let usdc = "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48"
            .parse::<Address>()
            .unwrap();

        // Buy USDC with USDT on V3 pool (0.01% fee)
        let buy_pool = TokenPriceUpdate {
            token_address: format!("{:?}", usdc).to_lowercase(),
            price_in_eth: 0.0004,
            price_in_usd: Some(1.0),
            pool_address: "0x3416cf6c708da44db2624d63ea0aaef7113527c6".to_string(),
            dex_version: "UniswapV3".to_string(),
            decimals: 6,
            last_updated: 0,
            pool_token0: usdt,
            pool_token1: usdc,
            eth_chain: eth_dex_quote::EthChain::Mainnet,
            fee_tier: Some(100),
            tick_spacing: Some(1),
            hooks: None,
            eth_price_usd: 2500.0,
        };

        // Sell USDC for USDT on V3 pool (0.05% fee)
        let sell_pool = TokenPriceUpdate {
            token_address: format!("{:?}", usdc).to_lowercase(),
            price_in_eth: 0.0004,
            price_in_usd: Some(1.0),
            pool_address: "0x7858e59e0c01ea06df3af3d20ac7b0003275d4bf".to_string(),
            dex_version: "UniswapV3".to_string(),
            decimals: 6,
            last_updated: 0,
            pool_token0: usdc,
            pool_token1: usdt,
            eth_chain: eth_dex_quote::EthChain::Mainnet,
            fee_tier: Some(500),
            tick_spacing: Some(10),
            hooks: None,
            eth_price_usd: 2500.0,
        };

        let flashloan_amount = usdt_to_u256(1000.0); // 1000 USDT
        let quoter_v3 = "0xb27308f9f90d607463bb33ea1bebb41c27ce5ab6"
            .parse::<Address>()
            .unwrap();
        let router_v2 = Address::zero();
        let quoter_v4 = Address::zero();

        let result = compute_arbitrage_path(
            provider,
            flashloan_amount,
            usdt,
            usdc,
            &buy_pool,
            &sell_pool,
            router_v2,
            quoter_v3,
            quoter_v4,
        )
        .await;

        match result {
            Ok((amount_usdc, amount_usdt_final, net_profit)) => {
                let flashloan_f64 = flashloan_amount.as_u128() as f64 / 1_000_000.0;
                let usdc_f64 = amount_usdc.as_u128() as f64 / 1_000_000.0;
                let final_f64 = amount_usdt_final.as_u128() as f64 / 1_000_000.0;
                let profit_f64 = net_profit as f64 / 1_000_000.0;

                println!("✅ 2-Hop Arbitrage Test:");
                println!("   Input:       {:.2} USDT", flashloan_f64);
                println!("   After Buy:   {:.2} USDC", usdc_f64);
                println!("   After Sell:  {:.2} USDT", final_f64);
                println!(
                    "   Net Profit:  {:.6} USDT ({:.4}%)",
                    profit_f64,
                    (profit_f64 / flashloan_f64) * 100.0
                );

                assert!(amount_usdc > U256::zero(), "USDC amount should be non-zero");
                assert!(
                    amount_usdt_final > U256::zero(),
                    "Final USDT should be non-zero"
                );

                // Due to fees, expect slight loss (0.01% + 0.05% = 0.06% in fees)
                // But output should still be reasonable
                let expected_min = usdt_to_u256(990.0); // Allow 1% total loss
                assert!(
                    amount_usdt_final > expected_min,
                    "Final amount too low: got {}, expected > {}",
                    amount_usdt_final,
                    expected_min
                );
            }
            Err(e) => {
                panic!("2-hop arbitrage test failed: {:?}", e);
            }
        }
    }

    /// Test compute_3hop_arbitrage_path with real USDT/USDC/DAI pools
    #[tokio::test]
    async fn test_compute_3hop_arbitrage_path_real() {
        use ethers::providers::{Http, Provider};

        let rpc_url =
            std::env::var("ETH_RPC_URL").unwrap_or_else(|_| "https://eth.merkle.io".to_string());
        let provider = Arc::new(
            Provider::<Http>::try_from(rpc_url.as_str()).expect("Failed to create provider"),
        );

        let usdt = "0xdac17f958d2ee523a2206206994597c13d831ec7"
            .parse::<Address>()
            .unwrap();
        let usdc = "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48"
            .parse::<Address>()
            .unwrap();
        let dai = "0x6b175474e89094c44da98b954eedeac495271d0f"
            .parse::<Address>()
            .unwrap();

        // Pool 1: USDT -> USDC (V3, 0.01%)
        let pool_usdt_usdc = TokenPriceUpdate {
            token_address: format!("{:?}", usdc).to_lowercase(),
            price_in_eth: 0.0004,
            price_in_usd: Some(1.0),
            pool_address: "0x3416cf6c708da44db2624d63ea0aaef7113527c6".to_string(),
            dex_version: "UniswapV3".to_string(),
            decimals: 6,
            last_updated: 0,
            pool_token0: usdt,
            pool_token1: usdc,
            eth_chain: eth_dex_quote::EthChain::Mainnet,
            fee_tier: Some(100),
            tick_spacing: Some(1),
            hooks: None,
            eth_price_usd: 2500.0,
        };

        // Pool 2: USDC -> DAI (V3, 0.01%)
        let pool_usdc_dai = TokenPriceUpdate {
            token_address: format!("{:?}", dai).to_lowercase(),
            price_in_eth: 0.0004,
            price_in_usd: Some(1.0),
            pool_address: "0x5777d92f208679db4b9778590fa3cab3ac9e2168".to_string(),
            dex_version: "UniswapV3".to_string(),
            decimals: 18,
            last_updated: 0,
            pool_token0: usdc,
            pool_token1: dai,
            eth_chain: eth_dex_quote::EthChain::Mainnet,
            fee_tier: Some(100),
            tick_spacing: Some(1),
            hooks: None,
            eth_price_usd: 2500.0,
        };

        // Pool 3: DAI -> USDT (V3, 0.05%)
        let pool_dai_usdt = TokenPriceUpdate {
            token_address: format!("{:?}", usdt).to_lowercase(),
            price_in_eth: 0.0004,
            price_in_usd: Some(1.0),
            pool_address: "0xc5af84701f98fa483ece78af83f11b6c38aca71d".to_string(),
            dex_version: "UniswapV3".to_string(),
            decimals: 6,
            last_updated: 0,
            pool_token0: dai,
            pool_token1: usdt,
            eth_chain: eth_dex_quote::EthChain::Mainnet,
            fee_tier: Some(500),
            tick_spacing: Some(10),
            hooks: None,
            eth_price_usd: 2500.0,
        };

        let flashloan_amount = usdt_to_u256(500.0); // 500 USDT
        let quoter_v3 = "0xb27308f9f90d607463bb33ea1bebb41c27ce5ab6"
            .parse::<Address>()
            .unwrap();
        let router_v2 = Address::zero();
        let quoter_v4 = Address::zero();

        let result = compute_3hop_arbitrage_path(
            provider,
            flashloan_amount,
            usdt,
            usdc,
            dai,
            &pool_usdt_usdc,
            &pool_usdc_dai,
            &pool_dai_usdt,
            router_v2,
            quoter_v3,
            quoter_v4,
        )
        .await;

        match result {
            Ok((amount_usdc, amount_dai, amount_usdt_final, net_profit)) => {
                let flashloan_f64 = flashloan_amount.as_u128() as f64 / 1_000_000.0;
                let usdc_f64 = amount_usdc.as_u128() as f64 / 1_000_000.0;
                let dai_f64 = amount_dai.as_u128() as f64 / 1e18;
                let final_f64 = amount_usdt_final.as_u128() as f64 / 1_000_000.0;
                let profit_f64 = net_profit as f64 / 1_000_000.0;

                println!("✅ 3-Hop Arbitrage Test:");
                println!("   Input:           {:.2} USDT", flashloan_f64);
                println!("   After USDT->USDC: {:.2} USDC", usdc_f64);
                println!("   After USDC->DAI:  {:.2} DAI", dai_f64);
                println!("   After DAI->USDT:  {:.2} USDT", final_f64);
                println!(
                    "   Net Profit:       {:.6} USDT ({:.4}%)",
                    profit_f64,
                    (profit_f64 / flashloan_f64) * 100.0
                );

                assert!(amount_usdc > U256::zero(), "USDC amount should be non-zero");
                assert!(amount_dai > U256::zero(), "DAI amount should be non-zero");
                assert!(
                    amount_usdt_final > U256::zero(),
                    "Final USDT should be non-zero"
                );

                // With 3 hops, expect cumulative fees (0.01% + 0.01% + 0.05% = 0.07%)
                let expected_min = usdt_to_u256(495.0); // Allow 1% total loss
                assert!(
                    amount_usdt_final > expected_min,
                    "Final amount too low: got {}, expected > {}",
                    amount_usdt_final,
                    expected_min
                );
            }
            Err(e) => {
                panic!("3-hop arbitrage test failed: {:?}", e);
            }
        }
    }
}
