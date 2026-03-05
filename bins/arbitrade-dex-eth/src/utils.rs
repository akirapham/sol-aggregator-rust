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
#[allow(clippy::too_many_arguments)]
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

/// Helper: Translate generic `TokenPriceUpdate` to `Hop` using Chain Config
pub fn build_hop(
    pool: &TokenPriceUpdate,
    token_in: Address,
    token_out: Address,
    chain_config: &eth_dex_quote::config::ChainConfig,
) -> Result<eth_dex_quote::quote_router::Hop> {
    let raw_dex_name = pool.dex_version.to_lowercase();
    let mapped_dex_name = match raw_dex_name.as_str() {
        "uniswapv3" => "uniswap_v3",
        "uniswapv2" => "uniswap_v2",
        "sushiswapv2" => "sushiswap_v2",
        "sushiswapv3" => "sushiswap_v3",
        "pancakeswapv3" => "pancakeswap_v3",
        "camelotv3" => "camelot_algebra",
        _ => &raw_dex_name,
    };

    let dex_cfg = chain_config
        .dexes
        .get(mapped_dex_name)
        .ok_or_else(|| anyhow!("Dex config missing for: {}", mapped_dex_name))?;

    let (pool_type, router_addr) =
        if mapped_dex_name.contains("v2") || mapped_dex_name.contains("sushiswap") {
            (
                0u8,
                dex_cfg
                    .router
                    .as_ref()
                    .ok_or_else(|| anyhow!("V2 missing router address"))?
                    .parse::<Address>()?,
            )
        } else if mapped_dex_name.contains("camelot") || mapped_dex_name.contains("algebra") {
            (
                2u8,
                dex_cfg
                    .quoter
                    .as_ref()
                    .or(dex_cfg.router.as_ref())
                    .ok_or_else(|| anyhow!("Camelot missing quoter address"))?
                    .parse::<Address>()?,
            )
        } else if mapped_dex_name.contains("v3") {
            (
                1u8,
                dex_cfg
                    .quoter
                    .as_ref()
                    .ok_or_else(|| anyhow!("V3 missing quoter address"))?
                    .parse::<Address>()?,
            )
        } else {
            return Err(anyhow!(
                "Unsupported dex for Hop Builder: {}",
                mapped_dex_name
            ));
        };

    Ok(eth_dex_quote::quote_router::Hop {
        pool_type,
        router: router_addr,
        token_in,
        token_out,
        fee: pool.fee_tier.unwrap_or(0),
    })
}

/// Helper: Translate generic `TokenPriceUpdate` to `ExecHop` which relies on executing swaps, not quotes.
pub fn build_exec_hop(
    pool: &TokenPriceUpdate,
    token_in: Address,
    token_out: Address,
    chain_config: &eth_dex_quote::config::ChainConfig,
) -> Result<eth_dex_quote::quote_router::ExecHop> {
    let raw_dex_name = pool.dex_version.to_lowercase();
    let mapped_dex_name = match raw_dex_name.as_str() {
        "uniswapv3" => "uniswap_v3",
        "uniswapv2" => "uniswap_v2",
        "sushiswapv2" => "sushiswap_v2",
        "sushiswapv3" => "sushiswap_v3",
        "pancakeswapv3" => "pancakeswap_v3",
        "camelotv3" => "camelot_algebra",
        _ => &raw_dex_name,
    };

    let dex_cfg = chain_config
        .dexes
        .get(mapped_dex_name)
        .ok_or_else(|| anyhow!("Dex config missing for: {}", mapped_dex_name))?;

    let pool_type = if mapped_dex_name.contains("v2") || mapped_dex_name.contains("sushiswap") {
        0u8
    } else if mapped_dex_name.contains("camelot") || mapped_dex_name.contains("algebra") {
        2u8
    } else if mapped_dex_name.contains("v3") {
        1u8
    } else {
        return Err(anyhow!(
            "Unsupported dex for Exec Hop Builder: {}",
            mapped_dex_name
        ));
    };

    let router_addr = dex_cfg
        .router
        .as_ref()
        .ok_or_else(|| anyhow!("Router missing for exec dex config: {}", mapped_dex_name))?
        .parse::<Address>()?;

    Ok(eth_dex_quote::quote_router::ExecHop {
        pool_type,
        router: router_addr,
        token_in,
        token_out,
        fee: pool.fee_tier.unwrap_or(0),
    })
}

/// Execute a 2-hop arbitrage path: X -> A -> X
#[allow(clippy::too_many_arguments)]
pub async fn compute_arbitrage_path<P: ethers::providers::Middleware + 'static>(
    provider: Arc<P>,
    flashloan_amount: U256,
    token_x: Address,
    token_a: Address,
    buy_pool: &TokenPriceUpdate,
    sell_pool: &TokenPriceUpdate,
    chain_config: &eth_dex_quote::config::ChainConfig,
    router: &eth_dex_quote::quote_router::QuoteRouterClient<P>,
) -> Result<(U256, U256, i128)> {
    let other_token_in_sell = if token_a == sell_pool.pool_token0 {
        sell_pool.pool_token1
    } else {
        sell_pool.pool_token0
    };

    // Fast-path: Unified Quote Router single batch RPC
    if let (Ok(hop1), Ok(hop2)) = (
        build_hop(buy_pool, token_x, token_a, chain_config),
        build_hop(sell_pool, token_a, other_token_in_sell, chain_config),
    ) {
        match router
            .quote_arbitrage_2_hop(hop1, hop2, flashloan_amount)
            .await
        {
            Ok((amount_out, profit)) => {
                // Convert profit dynamically
                let profit_i128 = if profit.is_negative() {
                    let abs_raw = (!profit.into_raw())
                        .overflowing_add(ethers::types::U256::one())
                        .0;
                    -(abs_raw.as_u128() as i128)
                } else {
                    profit.into_raw().as_u128() as i128
                };
                return Ok((U256::zero(), amount_out, profit_i128));
            }
            Err(e) => log::warn!("QuoteRouter 2-hop failed: {:?}", e),
        }
    }

    // Fallback path: Sequential RPC resolution
    // Fallbacks fetch the router addresses via `build_hop` dynamically depending on specific `dex_version`
    // ... we'll patch this dynamically in the loop instead of static arguments.
    let hop1_cfg = build_hop(buy_pool, token_x, token_a, chain_config)?;
    let hop2_cfg = build_hop(sell_pool, token_a, other_token_in_sell, chain_config)?;

    let current_amount = flashloan_amount;

    // Hop 1 (X -> A)
    let amount_a = compute_swap_output(
        provider.clone(),
        current_amount,
        token_x,
        token_a,
        buy_pool,
        if hop1_cfg.pool_type == 0 {
            hop1_cfg.router
        } else {
            Address::zero()
        },
        if hop1_cfg.pool_type == 1 || hop1_cfg.pool_type == 2 {
            hop1_cfg.router
        } else {
            Address::zero()
        },
        Address::zero(), // quoter_v4 unused yet
    )
    .await?;

    // Hop 2 (A -> X)
    let amount_x_final = compute_swap_output(
        provider.clone(),
        amount_a,
        token_a,
        other_token_in_sell,
        sell_pool,
        if hop2_cfg.pool_type == 0 {
            hop2_cfg.router
        } else {
            Address::zero()
        },
        if hop2_cfg.pool_type == 1 || hop2_cfg.pool_type == 2 {
            hop2_cfg.router
        } else {
            Address::zero()
        },
        Address::zero(), // quoter_v4 unused yet
    )
    .await?;

    let net_profit = if amount_x_final >= flashloan_amount {
        (amount_x_final - flashloan_amount).as_u128() as i128
    } else {
        -((flashloan_amount - amount_x_final).as_u128() as i128)
    };

    if amount_x_final.is_zero() {
        return Err(anyhow!("Swap output is zero, arbitrage failed"));
    }

    Ok((amount_a, amount_x_final, net_profit))
}

/// Execute a 3-hop arbitrage path: X -> A -> B -> X
#[allow(clippy::too_many_arguments)]
pub async fn compute_3hop_arbitrage_path<P: ethers::providers::Middleware + 'static>(
    provider: Arc<P>,
    flashloan_amount: U256,
    token_x: Address,
    token_a: Address,
    token_b: Address,
    pool_x_to_a: &TokenPriceUpdate,
    pool_a_to_b: &TokenPriceUpdate,
    pool_b_to_x: &TokenPriceUpdate,
    chain_config: &eth_dex_quote::config::ChainConfig,
    router: &eth_dex_quote::quote_router::QuoteRouterClient<P>,
) -> Result<(U256, U256, U256, i128)> {
    // Fast path: Unified Quote Router single batch RPC
    if let (Ok(hop1), Ok(hop2), Ok(hop3)) = (
        build_hop(pool_x_to_a, token_x, token_a, chain_config),
        build_hop(pool_a_to_b, token_a, token_b, chain_config),
        build_hop(pool_b_to_x, token_b, token_x, chain_config),
    ) {
        let hops = vec![hop1, hop2, hop3];
        match router.quote_single_path(hops, flashloan_amount).await {
            Ok(final_amount) => {
                let profit = if final_amount >= flashloan_amount {
                    (final_amount - flashloan_amount).as_u128() as i128
                } else {
                    -((flashloan_amount - final_amount).as_u128() as i128)
                };
                return Ok((U256::zero(), U256::zero(), final_amount, profit));
            }
            Err(e) => log::warn!("QuoteRouter 3-hop failed: {:?}", e),
        }
    }

    // Fallback path: Sequential execution
    let hop1_cfg = build_hop(pool_x_to_a, token_x, token_a, chain_config)?;
    let hop2_cfg = build_hop(pool_a_to_b, token_a, token_b, chain_config)?;
    let hop3_cfg = build_hop(pool_b_to_x, token_b, token_x, chain_config)?;

    let amount_a = compute_swap_output(
        provider.clone(),
        flashloan_amount,
        token_x,
        token_a,
        pool_x_to_a,
        if hop1_cfg.pool_type == 0 {
            hop1_cfg.router
        } else {
            Address::zero()
        },
        if hop1_cfg.pool_type == 1 || hop1_cfg.pool_type == 2 {
            hop1_cfg.router
        } else {
            Address::zero()
        },
        Address::zero(),
    )
    .await?;

    let amount_b = compute_swap_output(
        provider.clone(),
        amount_a,
        token_a,
        token_b,
        pool_a_to_b,
        if hop2_cfg.pool_type == 0 {
            hop2_cfg.router
        } else {
            Address::zero()
        },
        if hop2_cfg.pool_type == 1 || hop2_cfg.pool_type == 2 {
            hop2_cfg.router
        } else {
            Address::zero()
        },
        Address::zero(),
    )
    .await?;

    let final_amount = compute_swap_output(
        provider.clone(),
        amount_b,
        token_b,
        token_x,
        pool_b_to_x,
        if hop3_cfg.pool_type == 0 {
            hop3_cfg.router
        } else {
            Address::zero()
        },
        if hop3_cfg.pool_type == 1 || hop3_cfg.pool_type == 2 {
            hop3_cfg.router
        } else {
            Address::zero()
        },
        Address::zero(),
    )
    .await?;

    let net_profit = if final_amount >= flashloan_amount {
        (final_amount - flashloan_amount).as_u128() as i128
    } else {
        -((flashloan_amount - final_amount).as_u128() as i128)
    };

    if final_amount.is_zero() {
        return Err(anyhow!("Swap output is zero, 3-hop arbitrage failed"));
    }

    Ok((amount_a, amount_b, final_amount, net_profit))
}
/// Helper function to compute swap output based on pool's dex_version
#[allow(clippy::too_many_arguments)]
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
            // Try off-chain math first if reserves are available (zero RPC calls)
            if let (Some(r0_str), Some(r1_str)) = (&pool.reserve0, &pool.reserve1) {
                if let (Some(r0), Some(r1)) = (
                    eth_dex_quote::parse_reserve(r0_str),
                    eth_dex_quote::parse_reserve(r1_str),
                ) {
                    if let Some(amount_out) = eth_dex_quote::compute_v2_swap(
                        amount_in,
                        token_in,
                        pool.pool_token0,
                        r0,
                        r1,
                        30, // standard 0.3% fee
                    ) {
                        info!(
                            "V2 off-chain quote: {} -> {} (pool {})",
                            amount_in, amount_out, pool.pool_address
                        );
                        return Ok(amount_out);
                    }
                }
            }
            // Fallback to on-chain router quote if reserves unavailable
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

/// Fetch available B->X pools from amm-eth API (no RPC calls)
/// Returns vector of (pool, amount_out) for each pool (amount_out will be computed via quoteMultiPaths)
pub async fn fetch_b_to_x_pools(
    http_client: reqwest::Client,
    dex_pair_api_url: &str,
    token_b: Address,
    token_x: Address,
    _chain_config: &eth_dex_quote::config::ChainConfig,
) -> Result<Vec<(TokenPriceUpdate, U256)>> {
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
    let mut pools: Vec<(TokenPriceUpdate, U256)> = Vec::new();

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

        // Check if this pair can swap B -> X
        let token_out_str;
        let decimals_out;
        if token0 == token_b_str && token1 == token_x_str {
            token_out_str = token1.to_string();
            decimals_out = decimals1;
        } else if token1 == token_b_str && token0 == token_x_str {
            token_out_str = token0.to_string();
            decimals_out = decimals0;
        } else {
            continue;
        };

        let pool_update = TokenPriceUpdate {
            token_address: token_out_str,
            price_in_eth: 0.0,
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
            eth_price_usd: 2500.0,
            reserve0: None,
            reserve1: None,
        };

        // Just push the pool - amount_out will be computed via quoteMultiPaths
        pools.push((pool_update, U256::zero()));
    }

    if pools.is_empty() {
        return Err(anyhow!("No valid B->X pools found"));
    }

    Ok(pools)
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
#[allow(clippy::too_many_arguments)]
pub async fn find_best_b_to_x_swap<P: ethers::providers::Middleware + 'static>(
    http_client: reqwest::Client,
    dex_pair_api_url: &str,
    token_b: Address,
    token_x: Address,
    amount_b: U256,
    provider: Arc<P>,
    chain_config: &eth_dex_quote::config::ChainConfig,
    _quote_router_client: &eth_dex_quote::quote_router::QuoteRouterClient<P>,
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
            reserve0: None,
            reserve1: None,
        };
        // Try to build a Hop just to get the router addresses right
        let hop_cfg = match build_hop(&pool_update, token_b, token_x, chain_config) {
            Ok(h) => h,
            Err(_) => continue, // Skip unsupported Dexes
        };

        match compute_swap_output(
            provider.clone(),
            amount_b,
            token_b,
            token_x,
            &pool_update,
            if hop_cfg.pool_type == 0 {
                hop_cfg.router
            } else {
                Address::zero()
            },
            if hop_cfg.pool_type == 1 || hop_cfg.pool_type == 2 {
                hop_cfg.router
            } else {
                Address::zero()
            },
            Address::zero(),
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

/// Helper to convert f64 USDC amount to U256 with 6 decimals (USDC standard)
pub fn usdc_to_u256(amount: f64) -> U256 {
    let decimals = 6; // USDC has 6 decimals
    let multiplier = 10_f64.powi(decimals);
    let amount_with_decimals = amount * multiplier;
    U256::from(amount_with_decimals as u64)
}

use std::collections::HashSet;

/// Build ALL possible 2-hop and 3-hop arbitrage paths for all base tokens
/// and check them in a single RPC multicall
#[allow(clippy::too_many_arguments)]
pub async fn check_all_arbitrage_paths<P: ethers::providers::Middleware + 'static>(
    http_client: reqwest::Client,
    dex_pair_api_url: &str,
    provider: Arc<P>,
    base_tokens: Vec<(Address, bool)>, // (token_address, is_stable)
    flashloan_amount: u64,
    chain_config: &eth_dex_quote::config::ChainConfig,
    quote_router_client: &eth_dex_quote::quote_router::QuoteRouterClient<P>,
    eth_price: f64,
) -> Result<Option<(Vec<eth_dex_quote::quote_router::ExecHop>, i128, Address, f64)>> {
    let mut all_paths: Vec<eth_dex_quote::quote_router::PathQuote> = Vec::new();
    // Metadata: (path_index, flashloan_token, is_stable, flashloan_wei)
    let mut path_metadata: Vec<(usize, Address, bool, u64)> = Vec::new();

    // Limit total paths to avoid RPC overload and invalid paths
    const MAX_PATHS: usize = 200;

    let mut path_count = 0;

    for (token_x, is_stable) in &base_tokens {
        if path_count >= MAX_PATHS {
            break;
        }
        
        let token_x_str = format!("{:?}", token_x).to_lowercase();
        let url = format!("{}/pairs/{}", dex_pair_api_url, token_x_str);

        let response = match http_client.get(&url).send().await {
            Ok(r) => r,
            Err(e) => {
                log::debug!("Failed to fetch pairs for {}: {}", token_x_str, e);
                continue;
            }
        };

        let pairs_data: serde_json::Value = match response.json().await {
            Ok(v) => v,
            Err(_) => continue,
        };

        let pairs_array = match pairs_data.get("pairs").and_then(|p| p.as_array()) {
            Some(arr) => arr,
            None => continue,
        };

        // Collect all pools involving token_x
        let mut pools_with_x: Vec<serde_json::Value> = Vec::new();
        for pair in pairs_array {
            let token0 = pair.get("pool_token0").and_then(|t| t.as_str()).unwrap_or("");
            let token1 = pair.get("pool_token1").and_then(|t| t.as_str()).unwrap_or("");
            if token0 == token_x_str || token1 == token_x_str {
                pools_with_x.push(pair.clone());
            }
        }

        // For each pool with X, get intermediate tokens and build 2-hop and 3-hop paths
        for pool_with_x in &pools_with_x {
            let token0 = pool_with_x.get("pool_token0").and_then(|t| t.as_str()).unwrap_or("");
            let token1 = pool_with_x.get("pool_token1").and_then(|t| t.as_str()).unwrap_or("");
            let pool_address = pool_with_x.get("pool_address").and_then(|t| t.as_str()).unwrap_or("");
            let dex_version = pool_with_x.get("dex_version").and_then(|t| t.as_str()).unwrap_or("UniswapV3");
            let fee_tier = pool_with_x.get("fee_tier").and_then(|t| t.as_u64()).map(|f| f as u32);
            let tick_spacing = pool_with_x.get("tick_spacing").and_then(|t| t.as_i64()).map(|t| t as i32);
            let hooks = pool_with_x.get("hooks").and_then(|t| t.as_str()).and_then(|s| s.parse::<Address>().ok());

            // Determine which token is the intermediate (not token_x)
            let token_a_str = if token0 == token_x_str { token1 } else { token0 };
            let token_a: Address = match token_a_str.parse() {
                Ok(a) => a,
                Err(_) => continue,
            };

            // Build TokenPriceUpdate for this pool
            let pool_update = TokenPriceUpdate {
                token_address: token_a_str.to_string(),
                price_in_eth: 0.0,
                price_in_usd: None,
                pool_address: pool_address.to_string(),
                dex_version: dex_version.to_string(),
                decimals: 18,
                last_updated: 0,
                pool_token0: token0.parse().unwrap_or_default(),
                pool_token1: token1.parse().unwrap_or_default(),
                eth_chain: eth_dex_quote::EthChain::Mainnet,
                fee_tier,
                tick_spacing,
                hooks,
                eth_price_usd: eth_price,
                reserve0: None,
                reserve1: None,
            };

            // Build hop for X -> A
            let hop1 = match build_hop(&pool_update, *token_x, token_a, chain_config) {
                Ok(h) => h,
                Err(_) => continue,
            };

            // 2-hop path: X -> A -> X
            let hop1_rev = eth_dex_quote::quote_router::Hop {
                pool_type: hop1.pool_type,
                router: hop1.router,
                token_in: token_a,
                token_out: *token_x,
                fee: hop1.fee,
            };

            let flashloan_wei = if *is_stable {
                (flashloan_amount as f64 * 1_000_000f64) as u64
            } else {
                (flashloan_amount as f64 / eth_price * 1e18f64) as u64
            };
            let amount_in = U256::from(flashloan_wei);

            let path_2hop = eth_dex_quote::quote_router::PathQuote {
                hops: vec![hop1.clone(), hop1_rev],
                amount_in,
            };
            all_paths.push(path_2hop);
            path_metadata.push((all_paths.len() - 1, *token_x, *is_stable, flashloan_wei));
            path_count += 1;

            if path_count >= MAX_PATHS {
                break;
            }

            // Fetch pools for token A to build 3-hop paths
            let url_a = format!("{}/pairs/{}", dex_pair_api_url, token_a_str);
            if let Ok(response_a) = http_client.get(&url_a).send().await {
                if let Ok(pairs_data_a) = response_a.json::<serde_json::Value>().await {
                    if let Some(pairs_array_a) = pairs_data_a.get("pairs").and_then(|p| p.as_array()) {
                        // Use HashSet to avoid duplicate intermediate tokens
                        let mut seen_tokens: HashSet<String> = HashSet::new();

                        for pair_a in pairs_array_a {
                            let token0_a = pair_a.get("pool_token0").and_then(|t| t.as_str()).unwrap_or("");
                            let token1_a = pair_a.get("pool_token1").and_then(|t| t.as_str()).unwrap_or("");
                            
                            // Skip if both tokens are X or A
                            if (token0_a == token_x_str && token1_a == token_a_str) || 
                               (token1_a == token_x_str && token0_a == token_a_str) {
                                continue;
                            }

                            // Get intermediate token B (not X, not A)
                            let token_b_str = if token0_a == token_a_str { token1_a } else if token1_a == token_a_str { token0_a } else { continue };
                            
                            // Skip if we've already processed this token B for this path
                            if !seen_tokens.insert(token_b_str.to_string()) {
                                continue;
                            }

                            let token_b: Address = match token_b_str.parse() {
                                Ok(b) => b,
                                Err(_) => continue,
                            };

                            let pool_address_b = pair_a.get("pool_address").and_then(|t| t.as_str()).unwrap_or("");
                            let dex_version_b = pair_a.get("dex_version").and_then(|t| t.as_str()).unwrap_or("UniswapV3");
                            let fee_tier_b = pair_a.get("fee_tier").and_then(|t| t.as_u64()).map(|f| f as u32);
                            let tick_spacing_b = pair_a.get("tick_spacing").and_then(|t| t.as_i64()).map(|t| t as i32);
                            let hooks_b = pair_a.get("hooks").and_then(|t| t.as_str()).and_then(|s| s.parse::<Address>().ok());

                            let pool_update_b = TokenPriceUpdate {
                                token_address: token_b_str.to_string(),
                                price_in_eth: 0.0,
                                price_in_usd: None,
                                pool_address: pool_address_b.to_string(),
                                dex_version: dex_version_b.to_string(),
                                decimals: 18,
                                last_updated: 0,
                                pool_token0: token0_a.parse().unwrap_or_default(),
                                pool_token1: token1_a.parse().unwrap_or_default(),
                                eth_chain: eth_dex_quote::EthChain::Mainnet,
                                fee_tier: fee_tier_b,
                                tick_spacing: tick_spacing_b,
                                hooks: hooks_b,
                                eth_price_usd: eth_price,
                                reserve0: None,
                                reserve1: None,
                            };

                            // Build hop for A -> B
                            let hop2 = match build_hop(&pool_update_b, token_a, token_b, chain_config) {
                                Ok(h) => h,
                                Err(_) => continue,
                            };

                            // Build hop for B -> X
                            // First need to find a pool with B -> X
                            let url_bx = format!("{}/pairs/{}", dex_pair_api_url, token_b_str);
                            if let Ok(response_bx) = http_client.get(&url_bx).send().await {
                                if let Ok(pairs_data_bx) = response_bx.json::<serde_json::Value>().await {
                                    if let Some(pairs_array_bx) = pairs_data_bx.get("pairs").and_then(|p| p.as_array()) {
                                        for pair_bx in pairs_array_bx {
                                            let token0_bx = pair_bx.get("pool_token0").and_then(|t| t.as_str()).unwrap_or("");
                                            let token1_bx = pair_bx.get("pool_token1").and_then(|t| t.as_str()).unwrap_or("");
                                            
                                            // Check if this pool connects B to X
                                            let pool_bx = if token0_bx == token_b_str && token1_bx == token_x_str {
                                                pair_bx
                                            } else if token1_bx == token_b_str && token0_bx == token_x_str {
                                                pair_bx
                                            } else {
                                                continue;
                                            };

                                            let pool_address_bx = pool_bx.get("pool_address").and_then(|t| t.as_str()).unwrap_or("");
                                            let dex_version_bx = pool_bx.get("dex_version").and_then(|t| t.as_str()).unwrap_or("UniswapV3");
                                            let fee_tier_bx = pool_bx.get("fee_tier").and_then(|t| t.as_u64()).map(|f| f as u32);
                                            let tick_spacing_bx = pool_bx.get("tick_spacing").and_then(|t| t.as_i64()).map(|t| t as i32);
                                            let hooks_bx = pool_bx.get("hooks").and_then(|t| t.as_str()).and_then(|s| s.parse::<Address>().ok());

                                            let pool_update_bx = TokenPriceUpdate {
                                                token_address: token_x_str.clone(),
                                                price_in_eth: 0.0,
                                                price_in_usd: None,
                                                pool_address: pool_address_bx.to_string(),
                                                dex_version: dex_version_bx.to_string(),
                                                decimals: 18,
                                                last_updated: 0,
                                                pool_token0: token0_bx.parse().unwrap_or_default(),
                                                pool_token1: token1_bx.parse().unwrap_or_default(),
                                                eth_chain: eth_dex_quote::EthChain::Mainnet,
                                                fee_tier: fee_tier_bx,
                                                tick_spacing: tick_spacing_bx,
                                                hooks: hooks_bx,
                                                eth_price_usd: eth_price,
                                                reserve0: None,
                                                reserve1: None,
                                            };

                                            let hop3 = match build_hop(&pool_update_bx, token_b, *token_x, chain_config) {
                                                Ok(h) => h,
                                                Err(_) => continue,
                                            };

                                            // 3-hop path: X -> A -> B -> X
                                            let path_3hop = eth_dex_quote::quote_router::PathQuote {
                                                hops: vec![hop1.clone(), hop2.clone(), hop3],
                                                amount_in,
                                            };
                                            all_paths.push(path_3hop);
                                            path_metadata.push((all_paths.len() - 1, *token_x, *is_stable, flashloan_wei));
                                            path_count += 1;

                                            if path_count >= MAX_PATHS {
                                                break;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        
        if path_count >= MAX_PATHS {
            break;
        }
    }

    if all_paths.is_empty() {
        return Ok(None);
    }

    log::info!("Checking {} total arbitrage paths (capped at {})", all_paths.len(), MAX_PATHS);

    // Single RPC call to get all quotes
    let results = quote_router_client.quote_multi_paths(all_paths).await
        .map_err(|e| anyhow!("quote_multi_paths failed: {:?}", e))?;

    // Find best profitable path
    let mut best_idx: Option<usize> = None;
    let mut best_profit: i128 = 0;
    let mut best_flashloan_token = Address::zero();
    let mut best_is_stable = false;

    for (i, result) in results.iter().enumerate() {
        if !result.success || result.amount_out.is_zero() {
            continue;
        }

        let (_, flashloan_token, is_stable, flashloan_wei_val) = path_metadata.get(i).cloned()
            .unwrap_or((0, Address::zero(), false, 0));

        let profit = if result.amount_out >= U256::from(flashloan_wei_val) {
            (result.amount_out - U256::from(flashloan_wei_val)).as_u128() as i128
        } else {
            -((U256::from(flashloan_wei_val) - result.amount_out).as_u128() as i128)
        };

        if profit > best_profit {
            best_profit = profit;
            best_idx = Some(i);
            best_flashloan_token = flashloan_token;
            best_is_stable = is_stable;
        }
    }

    if let Some(idx) = best_idx {
        if best_profit > 0 {
            let profit_usd = if best_is_stable {
                best_profit as f64 / 1_000_000f64
            } else {
                best_profit as f64 / 1e18f64 * eth_price
            };

            log::info!("💰 Found profitable path! Profit: {:.2} USD", profit_usd);

            // Build execution hops for the best path
            // Reconstruct the path using the metadata
            let (_, token_x, is_stable, _) = path_metadata.get(idx).cloned()
                .unwrap_or((0, Address::zero(), false, 0));
            
            // For now, return None - actual execution would need to rebuild the exact hops
            // This is the comprehensive check function - execution can be added later
            return Ok(Some((Vec::new(), best_profit, token_x, profit_usd)));
        }
    }

    Ok(None)
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
            reserve0: None,
            reserve1: None,
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
            reserve0: None,
            reserve1: None,
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
            reserve0: None,
            reserve1: None,
        };

        let flashloan_amount = usdt_to_u256(1000.0); // 1000 USDT
        let _quoter_v3 = "0xb27308f9f90d607463bb33ea1bebb41c27ce5ab6"
            .parse::<Address>()
            .unwrap();
        let mut dexes = std::collections::HashMap::new();
        dexes.insert(
            "uniswapv3".to_string(),
            eth_dex_quote::config::DexConfig {
                router: None,
                factory: None,
                quoter: Some("0xb27308f9f90d607463bb33ea1bebb41c27ce5ab6".to_string()),
                vault: None,
                position_manager: None,
                fee_tiers: vec![100, 500, 3000, 10000],
                fee_bps: 1,
            },
        );

        let mock_chain_config = eth_dex_quote::config::ChainConfig {
            chain_id: 1,
            chain_name: "ethereum".to_string(),
            rpc_url: rpc_url.clone(),
            base_tokens: vec![],
            quote_router: None,
            dexes,
        };

        let result = compute_arbitrage_path(
            provider,
            flashloan_amount,
            usdt,
            usdc,
            &buy_pool,
            &sell_pool,
            &mock_chain_config,
            None,
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
            reserve0: None,
            reserve1: None,
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
            reserve0: None,
            reserve1: None,
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
            reserve0: None,
            reserve1: None,
        };

        let flashloan_amount = usdt_to_u256(500.0); // 500 USDT
        let mut dexes = std::collections::HashMap::new();
        dexes.insert(
            "uniswapv3".to_string(),
            eth_dex_quote::config::DexConfig {
                router: None,
                factory: None,
                quoter: Some("0xb27308f9f90d607463bb33ea1bebb41c27ce5ab6".to_string()),
                vault: None,
                position_manager: None,
                fee_tiers: vec![100, 500, 3000, 10000],
                fee_bps: 1,
            },
        );

        let mock_chain_config = eth_dex_quote::config::ChainConfig {
            chain_id: 1,
            chain_name: "ethereum".to_string(),
            rpc_url: rpc_url.clone(),
            base_tokens: vec![],
            quote_router: None,
            dexes,
        };

        let result = compute_3hop_arbitrage_path(
            provider,
            flashloan_amount,
            usdt,
            usdc,
            dai,
            &pool_usdt_usdc,
            &pool_usdc_dai,
            &pool_dai_usdt,
            &mock_chain_config,
            None,
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
