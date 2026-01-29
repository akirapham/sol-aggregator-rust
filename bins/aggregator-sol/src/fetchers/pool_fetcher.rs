//! Pool fetcher for loading configured pools from RPC
//! Used in arbitrage mode to fetch fresh pool state instead of loading from postgres
//!
//! This module follows the EXACT patterns used in our quote tests (tests/quotes/*.rs)

use crate::arbitrage_config::MonitoredPool;
use crate::fetchers::meteora_dlmm_bin_array_fetcher::MeteoraDlmmBinArrayFetcher;
use crate::fetchers::orca_tick_array_fetcher::OrcaTickArrayFetcher;
use crate::fetchers::tick_array_fetcher::TickArrayFetcher;
use crate::pool_data_types::*;
use borsh::BorshDeserialize;
use futures::stream::{self, StreamExt};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use solana_streamer_sdk::streaming::event_parser::protocols::meteora_dlmm::types::{
    BinArrayBitmapExtension as SdkBitmapExtension, LbPair as SdkLbPair,
};
use solana_streamer_sdk::streaming::event_parser::protocols::orca_whirlpools::parser::ORCA_WHIRLPOOL_PROGRAM_ID;
use solana_streamer_sdk::streaming::event_parser::protocols::orca_whirlpools::types::{
    Tick, TickArrayState, WhirlpoolPoolState as WhirlpoolStateRaw,
};
use solana_streamer_sdk::streaming::event_parser::protocols::pumpswap::types::{
    Pool as PumpSwapPoolRaw, POOL_SIZE,
};
use solana_streamer_sdk::streaming::event_parser::protocols::raydium_clmm::parser::RAYDIUM_CLMM_PROGRAM_ID;
use solana_streamer_sdk::streaming::event_parser::protocols::raydium_clmm::types::PoolState as RaydiumClmmStateRaw;
use solana_streamer_sdk::streaming::event_parser::protocols::raydium_cpmm::types::PoolState as RaydiumCpmmStateRaw;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

// ============================================================================
// LbPairRaw - copied from tests/quotes/meteora.rs for proper DLMM deserialization
// ============================================================================
#[repr(C)]
#[derive(Clone, Debug)]
struct StaticParametersRaw {
    pub base_factor: u16,
    pub filter_period: u16,
    pub decay_period: u16,
    pub reduction_factor: u16,
    pub variable_fee_control: u32,
    pub max_volatility_accumulator: u32,
    pub min_bin_id: i32,
    pub max_bin_id: i32,
    pub protocol_share: u16,
    pub padding: [u8; 6],
}

#[repr(C)]
#[derive(Clone, Debug)]
struct VariableParametersRaw {
    pub volatility_accumulator: u32,
    pub volatility_reference: u32,
    pub index_reference: i32,
    pub padding: [u8; 4],
    pub last_update_timestamp: i64,
    pub padding_1: [u8; 8],
}

#[repr(C)]
#[derive(Clone, Debug)]
struct ProtocolFeeRaw {
    pub amount_x: u64,
    pub amount_y: u64,
}

#[repr(C)]
#[derive(Clone, Debug)]
struct RewardInfoRaw {
    pub mint: Pubkey,
    pub vault: Pubkey,
    pub funder: Pubkey,
    pub reward_duration: u64,
    pub reward_duration_end: u64,
    pub reward_rate: u128,
    pub last_update_time: u64,
    pub cumulative_seconds_with_empty_liquidity_reward: u64,
}

#[repr(C)]
#[derive(Clone, Debug)]
struct LbPairRaw {
    pub parameters: StaticParametersRaw,
    pub v_parameters: VariableParametersRaw,
    pub bump_seed: [u8; 1],
    pub bin_step_seed: [u8; 2],
    pub pair_type: u8,
    pub active_id: i32,
    pub bin_step: u16,
    pub status: u8,
    pub require_base_factor_seed: u8,
    pub base_factor_seed: [u8; 2],
    pub activation_type: u8,
    pub creator_pool_on_off_control: u8,
    pub token_x_mint: Pubkey,
    pub token_y_mint: Pubkey,
    pub reserve_x: Pubkey,
    pub reserve_y: Pubkey,
    pub protocol_fee: ProtocolFeeRaw,
    pub padding_1: [u8; 32],
    pub reward_infos: [RewardInfoRaw; 2],
    pub oracle: Pubkey,
    pub bin_array_bitmap: [u64; 16],
    pub last_updated_at: i64,
    pub padding_2: [u8; 32],
    pub pre_activation_swap_address: Pubkey,
    pub base_key: Pubkey,
    pub activation_point: u64,
    pub pre_activation_duration: u64,
    pub padding_3: [u8; 8],
    pub padding_4: u64,
    pub creator: Pubkey,
    pub token_mint_x_program_flag: u8,
    pub token_mint_y_program_flag: u8,
    pub version: u8,
    pub reserved: [u8; 21],
}

impl LbPairRaw {
    fn try_from_slice(data: &[u8]) -> Result<Self, String> {
        let size = std::mem::size_of::<Self>();
        if data.len() < size + 8 {
            return Err(format!("Data too short: {} < {}", data.len(), size + 8));
        }
        let data = &data[8..]; // skip discriminator
        let ptr = data.as_ptr() as *const LbPairRaw;
        Ok(unsafe { ptr.read_unaligned() })
    }
}

// ============================================================================
// Main fetch function
// ============================================================================

/// Fetch pool states for configured pools from RPC
pub async fn fetch_configured_pools(
    rpc_client: &Arc<RpcClient>,
    pools: &[&MonitoredPool],
) -> Vec<PoolState> {
    let start = std::time::Instant::now();
    log::info!(
        "⚡ Fetching {} configured pools from RPC (parallel=5)...",
        pools.len()
    );

    let fetch_futures = pools.iter().map(|pool| {
        let rpc_client = rpc_client.clone();
        async move {
            let pubkey = match Pubkey::from_str(&pool.address) {
                Ok(p) => p,
                Err(_) => {
                    log::warn!("Invalid pool address: {}", pool.address);
                    return None;
                }
            };

            log::info!("  📦 Fetching {} pool: {}", pool.dex, pool.pair);

            let result = match pool.dex.as_str() {
                "MeteoraDlmm" => fetch_meteora_dlmm(&rpc_client, pubkey).await,
                "RaydiumClmm" => fetch_raydium_clmm(&rpc_client, pubkey).await,
                "Raydium" | "RaydiumCpmm" => fetch_raydium_cpmm(&rpc_client, pubkey).await,
                "Orca" | "OrcaWhirlpool" => fetch_orca_whirlpool(&rpc_client, pubkey).await,
                "Meteora" | "MeteoraDamm" | "MeteoraDammV2" => {
                    log::debug!("Meteora DAMM not yet supported, skip: {}", pool.pair);
                    return None;
                }
                "Pumpswap" | "PumpSwap" => fetch_pumpswap(&rpc_client, pubkey)
                    .await
                    .map_err(|e| e.to_string()),
                _ => {
                    log::warn!("Unsupported DEX type: {}", pool.dex);
                    return None; // Ensure logic consistency
                }
            };

            match result {
                Ok(pool_state) => Some(pool_state),
                Err(e) => {
                    log::warn!("Failed to fetch {} pool {}: {}", pool.dex, pool.pair, e);
                    None
                }
            }
        }
    });

    let all_pools: Vec<PoolState> = stream::iter(fetch_futures)
        .buffer_unordered(5)
        .filter_map(|res| async { res })
        .collect()
        .await;

    log::info!(
        "✅ Fetched {}/{} pools from RPC in {:?}",
        all_pools.len(),
        pools.len(),
        start.elapsed()
    );
    all_pools
}

// ============================================================================
// Meteora DLMM - exact pattern from tests/quotes/meteora.rs
// ============================================================================

async fn fetch_meteora_dlmm(
    rpc_client: &Arc<RpcClient>,
    pool_address: Pubkey,
) -> Result<PoolState, String> {
    // 1. Fetch pool account
    let account = rpc_client
        .get_account(&pool_address)
        .await
        .map_err(|e| format!("Failed to fetch DLMM pool: {}", e))?;

    // 2. Parse using LbPairRaw for correct field extraction
    let raw_pool = LbPairRaw::try_from_slice(&account.data)?;

    // 3. Parse SDK LbPair (skip 8 bytes discriminator)
    let size_sdk = std::mem::size_of::<SdkLbPair>();
    if account.data.len() < 8 + size_sdk {
        return Err("Account data too short for SDK LbPair".to_string());
    }
    let mut sdk_lb_pair: SdkLbPair =
        unsafe { (account.data[8..].as_ptr() as *const SdkLbPair).read_unaligned() };

    // 4. CRITICAL: Fix fields by copying from verified LbPairRaw (same as test)
    sdk_lb_pair.token_x_mint = raw_pool.token_x_mint;
    sdk_lb_pair.token_y_mint = raw_pool.token_y_mint;
    sdk_lb_pair.active_id = raw_pool.active_id;
    sdk_lb_pair.bin_step = raw_pool.bin_step;
    sdk_lb_pair.parameters.min_bin_id = raw_pool.parameters.min_bin_id;
    sdk_lb_pair.parameters.max_bin_id = raw_pool.parameters.max_bin_id;
    sdk_lb_pair.parameters.base_factor = raw_pool.parameters.base_factor;
    sdk_lb_pair.bin_array_bitmap = raw_pool.bin_array_bitmap;

    // 5. Fetch Bitmap Extension (optional)
    let program_id = Pubkey::from_str("LBUZKhRxPF3XUpBCjp4YzTKgLccjZhTSDM9YuVaPwxo").unwrap();
    let (bitmap_pubkey, _) =
        Pubkey::find_program_address(&[b"bitmap", pool_address.as_ref()], &program_id);

    let mut bitmap_extension = None;
    if let Ok(bitmap_acc) = rpc_client.get_account(&bitmap_pubkey).await {
        let size_ext = std::mem::size_of::<SdkBitmapExtension>();
        if bitmap_acc.data.len() >= 8 + size_ext {
            let ext: SdkBitmapExtension = unsafe {
                (bitmap_acc.data[8..].as_ptr() as *const SdkBitmapExtension).read_unaligned()
            };
            bitmap_extension = Some(ext);
        }
    }

    // 6. Create initial pool state
    let mut pool_state_struct = MeteoraDlmmPoolState {
        slot: 0,
        transaction_index: None,
        address: pool_address,
        lbpair: sdk_lb_pair,
        bin_arrays: HashMap::new(),
        bitmap_extension,
        reserve_x: None,
        reserve_y: None,
        liquidity_usd: 1_000_000.0,
        is_state_keys_initialized: true,
        last_updated: u64::MAX,
    };

    // 7. Fetch bin arrays using MeteoraDlmmBinArrayFetcher
    let fetcher = MeteoraDlmmBinArrayFetcher::new(rpc_client.clone());
    match fetcher
        .fetch_all_bin_arrays(pool_address, &pool_state_struct)
        .await
    {
        Ok(bin_arrays) => {
            for ba in bin_arrays {
                pool_state_struct.bin_arrays.insert(ba.index as i32, ba);
            }
            log::debug!(
                "  ✓ Fetched {} bin arrays for DLMM",
                pool_state_struct.bin_arrays.len()
            );
        }
        Err(e) => {
            log::warn!("  ⚠ Failed to fetch bin arrays: {:?}", e);
        }
    }

    Ok(PoolState::MeteoraDlmm(Box::new(pool_state_struct)))
}

// ============================================================================
// Raydium CLMM - exact pattern from tests/quotes/raydium.rs
// ============================================================================

async fn fetch_raydium_clmm(
    rpc_client: &Arc<RpcClient>,
    pool_address: Pubkey,
) -> Result<PoolState, String> {
    // 1. Fetch pool account
    let account = rpc_client
        .get_account(&pool_address)
        .await
        .map_err(|e| format!("Failed to fetch CLMM pool: {}", e))?;

    if account.data.len() < 8 {
        return Err("Account data too short".to_string());
    }

    // 2. Deserialize (skip 8-byte discriminator)
    let clmm_state = RaydiumClmmStateRaw::try_from_slice(&account.data[8..])
        .map_err(|e| format!("Raydium CLMM deserialize error: {}", e))?;

    // 3. Fetch vault reserves
    let vault_a_account = rpc_client.get_account(&clmm_state.token_vault0).await;
    let vault_b_account = rpc_client.get_account(&clmm_state.token_vault1).await;

    let vault_a_reserve = vault_a_account
        .ok()
        .and_then(|a| a.data.get(64..72).and_then(|b| b.try_into().ok()))
        .map(u64::from_le_bytes)
        .unwrap_or(0);
    let vault_b_reserve = vault_b_account
        .ok()
        .and_then(|a| a.data.get(64..72).and_then(|b| b.try_into().ok()))
        .map(u64::from_le_bytes)
        .unwrap_or(0);

    // 4. Create initial pool state
    let mut pool_state = RaydiumClmmPoolState {
        slot: 0,
        transaction_index: None,
        address: pool_address,
        amm_config: clmm_state.amm_config,
        token_mint0: clmm_state.token_mint0,
        token_mint1: clmm_state.token_mint1,
        token_vault0: clmm_state.token_vault0,
        token_vault1: clmm_state.token_vault1,
        observation_key: clmm_state.observation_key,
        tick_spacing: clmm_state.tick_spacing,
        liquidity: clmm_state.liquidity,
        sqrt_price_x64: clmm_state.sqrt_price_x64,
        tick_current_index: clmm_state.tick_current,
        status: clmm_state.status,
        tick_array_bitmap: clmm_state.tick_array_bitmap,
        open_time: clmm_state.open_time,
        tick_array_state: HashMap::new(),
        tick_array_bitmap_extension: None,
        liquidity_usd: 1_000_000.0,
        last_updated: u64::MAX,
        token0_reserve: vault_a_reserve,
        token1_reserve: vault_b_reserve,
        is_state_keys_initialized: true,
    };

    // 5. Fetch tick arrays
    let tick_fetcher = TickArrayFetcher::new(
        rpc_client.clone(),
        Pubkey::new_from_array(*RAYDIUM_CLMM_PROGRAM_ID.as_array()),
    );

    if let Ok(tick_arrays) = tick_fetcher
        .fetch_all_tick_arrays(pool_address, &pool_state)
        .await
    {
        pool_state.tick_array_state = tick_arrays
            .into_iter()
            .map(|ta| (ta.start_tick_index, ta.clone()))
            .collect();
        log::debug!(
            "  ✓ Fetched {} tick arrays for CLMM",
            pool_state.tick_array_state.len()
        );
    }

    Ok(PoolState::RadyiumClmm(Box::new(pool_state)))
}

// ============================================================================
// Raydium CPMM - exact pattern from tests/quotes/raydium.rs
// ============================================================================

async fn fetch_raydium_cpmm(
    rpc_client: &Arc<RpcClient>,
    pool_address: Pubkey,
) -> Result<PoolState, String> {
    // 1. Fetch pool account
    let account = rpc_client
        .get_account(&pool_address)
        .await
        .map_err(|e| format!("Failed to fetch CPMM pool: {}", e))?;

    if account.data.len() < 8 {
        return Err("Account data too short".to_string());
    }

    // 2. Use unsafe pointer read like tests do
    let cpmm_state: RaydiumCpmmStateRaw =
        unsafe { (account.data[8..].as_ptr() as *const RaydiumCpmmStateRaw).read_unaligned() };

    // 3. Fetch vault reserves
    let vault_a_account = rpc_client.get_account(&cpmm_state.token0_vault).await;
    let vault_b_account = rpc_client.get_account(&cpmm_state.token1_vault).await;

    let vault_a_reserve = vault_a_account
        .ok()
        .and_then(|a| a.data.get(64..72).and_then(|b| b.try_into().ok()))
        .map(u64::from_le_bytes)
        .unwrap_or(0);
    let vault_b_reserve = vault_b_account
        .ok()
        .and_then(|a| a.data.get(64..72).and_then(|b| b.try_into().ok()))
        .map(u64::from_le_bytes)
        .unwrap_or(0);

    // 4. Create pool state
    let pool_state = RaydiumCpmmPoolState {
        slot: 0,
        transaction_index: None,
        status: cpmm_state.status,
        address: pool_address,
        token0: cpmm_state.token0_mint,
        token1: cpmm_state.token1_mint,
        token0_vault: cpmm_state.token0_vault,
        token1_vault: cpmm_state.token1_vault,
        token0_reserve: vault_a_reserve,
        token1_reserve: vault_b_reserve,
        amm_config: cpmm_state.amm_config,
        observation_state: cpmm_state.observation_key,
        last_updated: u64::MAX,
        liquidity_usd: 1_000_000.0,
        is_state_keys_initialized: true,
    };

    Ok(PoolState::RaydiumCpmm(pool_state))
}

// ============================================================================
// Orca Whirlpool - exact pattern from tests/quotes/orca.rs
// ============================================================================

async fn fetch_orca_whirlpool(
    rpc_client: &Arc<RpcClient>,
    pool_address: Pubkey,
) -> Result<PoolState, String> {
    // 1. Fetch pool account
    let account = rpc_client
        .get_account(&pool_address)
        .await
        .map_err(|e| format!("Failed to fetch Whirlpool: {}", e))?;

    if account.data.len() < 8 {
        return Err("Account data too short".to_string());
    }

    let whirlpool_state = WhirlpoolStateRaw::try_from_slice(&account.data[8..]).map_err(|e| {
        format!(
            "Whirlpool deserialize error: {} (data len: {})",
            e,
            account.data.len()
        )
    })?;

    // 3. Fetch vault reserves
    let vault_a_account = rpc_client.get_account(&whirlpool_state.token_vault_a).await;
    let vault_b_account = rpc_client.get_account(&whirlpool_state.token_vault_b).await;

    let token_a_reserve = vault_a_account
        .ok()
        .and_then(|a| a.data.get(64..72).and_then(|b| b.try_into().ok()))
        .map(u64::from_le_bytes)
        .unwrap_or(0);
    let token_b_reserve = vault_b_account
        .ok()
        .and_then(|a| a.data.get(64..72).and_then(|b| b.try_into().ok()))
        .map(u64::from_le_bytes)
        .unwrap_or(0);

    // 4. Create temporary pool state for tick array fetching
    let temp_pool_state = WhirlpoolPoolState {
        slot: 0,
        transaction_index: None,
        address: pool_address,
        whirlpool_config: whirlpool_state.whirlpools_config,
        tick_spacing: whirlpool_state.tick_spacing,
        tick_spacing_seed: whirlpool_state.tick_spacing_seed,
        fee_rate: whirlpool_state.fee_rate,
        protocol_fee_rate: whirlpool_state.protocol_fee_rate,
        liquidity: whirlpool_state.liquidity,
        liquidity_usd: 1_000_000.0,
        sqrt_price: whirlpool_state.sqrt_price,
        tick_current_index: whirlpool_state.tick_current_index,
        token_mint_a: whirlpool_state.token_mint_a,
        token_vault_a: whirlpool_state.token_vault_a,
        token_mint_b: whirlpool_state.token_mint_b,
        token_vault_b: whirlpool_state.token_vault_b,
        tick_array_state: HashMap::new(),
        last_updated: u64::MAX,
        token_a_reserve,
        token_b_reserve,
        is_state_keys_initialized: true,
        oracle_state: Default::default(),
    };

    // 5. Fetch tick arrays using OrcaTickArrayFetcher - exact pattern from test
    let tick_fetcher = OrcaTickArrayFetcher::new(
        rpc_client.clone(),
        Pubkey::new_from_array(*ORCA_WHIRLPOOL_PROGRAM_ID.as_array()),
    );

    let mut tick_array_state_map = HashMap::new();

    if let Ok(tick_arrays) = tick_fetcher
        .fetch_all_tick_arrays(pool_address, &temp_pool_state)
        .await
    {
        // Convert fetched tick arrays - exact pattern from test
        for tick_array in tick_arrays {
            let ticks = tick_array.ticks();
            if ticks.len() != 88 {
                continue; // Skip malformed tick arrays
            }
            let ticks_array: [Tick; 88] = ticks
                .iter()
                .map(|t| Tick {
                    initialized: t.initialized,
                    liquidity_net: t.liquidity_net,
                    liquidity_gross: t.liquidity_gross,
                    fee_growth_outside_a: t.fee_growth_outside_a,
                    fee_growth_outside_b: t.fee_growth_outside_b,
                    reward_growths_outside: t.reward_growths_outside,
                })
                .collect::<Vec<_>>()
                .try_into()
                .unwrap();

            let converted = TickArrayState {
                start_tick_index: tick_array.start_tick_index(),
                ticks: ticks_array,
                whirlpool: pool_address,
            };

            tick_array_state_map.insert(tick_array.start_tick_index(), converted);
        }

        log::debug!(
            "  ✓ Fetched {} tick arrays for Whirlpool",
            tick_array_state_map.len()
        );
    }

    // 6. Create final pool state
    let pool_state = WhirlpoolPoolState {
        slot: 0,
        transaction_index: None,
        address: pool_address,
        whirlpool_config: whirlpool_state.whirlpools_config,
        tick_spacing: whirlpool_state.tick_spacing,
        tick_spacing_seed: whirlpool_state.tick_spacing_seed,
        fee_rate: whirlpool_state.fee_rate,
        protocol_fee_rate: whirlpool_state.protocol_fee_rate,
        liquidity: whirlpool_state.liquidity,
        liquidity_usd: 1_000_000.0,
        sqrt_price: whirlpool_state.sqrt_price,
        tick_current_index: whirlpool_state.tick_current_index,
        token_mint_a: whirlpool_state.token_mint_a,
        token_vault_a: whirlpool_state.token_vault_a,
        token_mint_b: whirlpool_state.token_mint_b,
        token_vault_b: whirlpool_state.token_vault_b,
        tick_array_state: tick_array_state_map,
        last_updated: u64::MAX,
        token_a_reserve,
        token_b_reserve,
        is_state_keys_initialized: true,
        oracle_state: Default::default(),
    };

    Ok(PoolState::OrcaWhirlpool(pool_state))
}

// ============================================================================
// Pumpswap Fetcher
// ============================================================================
pub async fn fetch_pumpswap(
    rpc_client: &Arc<RpcClient>,
    pool_address: Pubkey,
) -> Result<PoolState, Box<dyn std::error::Error + Send + Sync>> {
    // 1. Fetch account
    let account = rpc_client.get_account(&pool_address).await?;

    // 2. Deserialize PumpSwapPoolRaw
    let pumpswap_pool = PumpSwapPoolRaw::try_from_slice(&account.data[8..8 + POOL_SIZE])?;

    // 3. Fetch Vaults
    let vault_base_account = rpc_client
        .get_account(&pumpswap_pool.pool_base_token_account)
        .await?;
    let vault_quote_account = rpc_client
        .get_account(&pumpswap_pool.pool_quote_token_account)
        .await?;

    // 4. Parse Reserves
    let base_reserve = u64::from_le_bytes(vault_base_account.data[64..72].try_into().unwrap());
    let quote_reserve = u64::from_le_bytes(vault_quote_account.data[64..72].try_into().unwrap());

    // 5. Construct State
    let pool_state = PumpSwapPoolState {
        slot: 100, // Placeholder
        transaction_index: None,
        address: pool_address,
        index: pumpswap_pool.index,
        creator: Some(pumpswap_pool.creator),
        base_mint: pumpswap_pool.base_mint,
        quote_mint: pumpswap_pool.quote_mint,
        pool_base_token_account: pumpswap_pool.pool_base_token_account,
        pool_quote_token_account: pumpswap_pool.pool_quote_token_account,
        last_updated: u64::MAX,
        base_reserve,
        quote_reserve,
        liquidity_usd: 0.0,
        is_state_keys_initialized: true,
        coin_creator: pumpswap_pool.coin_creator,
        protocol_fee_recipient: Pubkey::from_str("62qc2CNXwrYqQScmEdiZFFAnJR262PxWEuNQtxfafNgV")
            .unwrap(),
    };

    Ok(PoolState::PumpSwap(pool_state))
}
