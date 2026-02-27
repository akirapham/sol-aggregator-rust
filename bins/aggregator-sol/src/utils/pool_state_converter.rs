#![allow(clippy::manual_div_ceil)]
use std::collections::HashMap;

use solana_sdk::pubkey::Pubkey;
use tokio::sync::MutexGuard;

use crate::constants::is_base_token;
use crate::pool_data_types::{
    BonkPoolState, DbcPoolState, MeteoraDammV2PoolState, MeteoraDlmmPoolState, PumpSwapPoolState,
    PumpfunPoolState, RaydiumAmmV4PoolState, RaydiumClmmPoolState, RaydiumCpmmPoolState,
};
use crate::pool_data_types::{PoolState, PoolUpdateEventType, WhirlpoolPoolState};
use crate::types::PoolUpdateEvent;
use crate::utils::{get_sol_mint, tokens_equal};

fn compute_pool_liquidity_usd(
    token0: &Pubkey,
    token1: &Pubkey,
    reserve0: u64,
    reserve1: u64,
    sol_price: f64,
) -> f64 {
    let sol_mint = get_sol_mint();
    if tokens_equal(token0, &sol_mint) {
        reserve0 as f64 / 1_000_000_000_f64 * sol_price
    } else if tokens_equal(token1, &sol_mint) {
        reserve1 as f64 / 1_000_000_000_f64 * sol_price
    } else if is_base_token(&token0.to_string()) {
        reserve0 as f64 / 1_000_000_f64
    } else if is_base_token(&token1.to_string()) {
        reserve1 as f64 / 1_000_000_f64
    } else {
        0.0
    }
}

use uint::construct_uint;

construct_uint! {
    pub struct U256(4);
}

/// Calculate token reserves from Meteora DAMM V2 pool state
/// Uses the same formulas as the on-chain program:
/// - reserve_b = L * (√P - √P_min) / 2^128
/// - reserve_a = L * (√P_max - √P) / (√P_max * √P)
fn calculate_damm_v2_reserves(
    liquidity: u128,
    sqrt_price: u128,
    sqrt_min_price: u128,
    sqrt_max_price: u128,
) -> (u64, u64) {
    // Convert inputs to U256 to prevent overflow during intermediate calculations
    let liquidity_256 = U256::from(liquidity);
    let sqrt_price_256 = U256::from(sqrt_price);
    let sqrt_min_price_256 = U256::from(sqrt_min_price);
    let sqrt_max_price_256 = U256::from(sqrt_max_price);

    // Calculate reserve_b: L * (√P - √P_min) / 2^128
    let delta_sqrt_price_b = sqrt_price_256.saturating_sub(sqrt_min_price_256);
    let product_b = liquidity_256.saturating_mul(delta_sqrt_price_b);

    // Divide by 2^128 (shift right by 128 bits)
    let reserve_b = (product_b >> 128).as_u64();

    // Calculate reserve_a: L * (√P_max - √P) / (√P_max * √P)
    let numerator = liquidity_256.saturating_mul(sqrt_max_price_256.saturating_sub(sqrt_price_256));
    let denominator = sqrt_max_price_256.saturating_mul(sqrt_price_256);

    let reserve_a = if !denominator.is_zero() {
        (numerator / denominator).as_u64()
    } else {
        0
    };

    (reserve_a, reserve_b)
}

pub fn pool_update_event_to_pool_state(
    event: &PoolUpdateEvent,
    sol_price: f64,
    dbc_configs: Option<
        &std::collections::HashMap<
            solana_sdk::pubkey::Pubkey,
            crate::pool_data_types::dbc::PoolConfig,
        >,
    >,
) -> (Option<PoolState>, bool) {
    match event {
        PoolUpdateEvent::Pumpfun(pumpfun_pool_update) => (
            Some(PoolState::Pumpfun(PumpfunPoolState {
                address: pumpfun_pool_update.address,
                last_updated: pumpfun_pool_update.last_updated,
                liquidity_usd: pumpfun_pool_update.virtual_sol_reserves as f64 / 1_000_000_000_f64
                    * sol_price,
                complete: pumpfun_pool_update.complete,
                mint: pumpfun_pool_update.mint,
                virtual_sol_reserves: pumpfun_pool_update.virtual_sol_reserves,
                virtual_token_reserves: pumpfun_pool_update.virtual_token_reserves,
                real_sol_reserves: pumpfun_pool_update.real_sol_reserves,
                real_token_reserves: pumpfun_pool_update.real_token_reserves,
                creator: pumpfun_pool_update.creator,
                is_mayhem_mode: pumpfun_pool_update.is_mayhem_mode,
                slot: pumpfun_pool_update.slot,
                transaction_index: pumpfun_pool_update.transaction_index,
                is_state_keys_initialized: pumpfun_pool_update.is_account_state_update,
                is_cashback: pumpfun_pool_update.is_cashback.unwrap_or(false),
            })),
            false,
        ),
        PoolUpdateEvent::Raydium(raydium_pool_update) => {
            let liquidity_usd = if raydium_pool_update.is_account_state_update {
                0.0
            } else {
                compute_pool_liquidity_usd(
                    &raydium_pool_update.base_mint,
                    &raydium_pool_update.quote_mint,
                    raydium_pool_update.base_reserve,
                    raydium_pool_update.quote_reserve,
                    sol_price,
                )
            };
            (
                Some(PoolState::RaydiumAmmV4(RaydiumAmmV4PoolState {
                    slot: raydium_pool_update.slot,
                    transaction_index: raydium_pool_update.transaction_index,
                    address: raydium_pool_update.address,
                    base_mint: raydium_pool_update.base_mint,
                    quote_mint: raydium_pool_update.quote_mint,
                    amm_authority: raydium_pool_update.amm_authority,
                    amm_open_orders: raydium_pool_update.amm_open_orders,
                    amm_target_orders: raydium_pool_update.amm_target_orders,
                    pool_coin_token_account: raydium_pool_update.pool_coin_token_account,
                    pool_pc_token_account: raydium_pool_update.pool_pc_token_account,
                    serum_program: raydium_pool_update.serum_program.unwrap_or_default(),
                    serum_market: raydium_pool_update.serum_market.unwrap_or_default(),
                    serum_bids: raydium_pool_update.serum_bids.unwrap_or_default(),
                    serum_asks: raydium_pool_update.serum_asks.unwrap_or_default(),
                    serum_event_queue: raydium_pool_update.serum_event_queue.unwrap_or_default(),
                    serum_coin_vault_account: raydium_pool_update
                        .serum_coin_vault_account
                        .unwrap_or_default(),
                    serum_pc_vault_account: raydium_pool_update
                        .serum_pc_vault_account
                        .unwrap_or_default(),
                    serum_vault_signer: raydium_pool_update.serum_vault_signer.unwrap_or_default(),
                    last_updated: raydium_pool_update.last_updated,
                    base_reserve: raydium_pool_update.base_reserve,
                    quote_reserve: raydium_pool_update.quote_reserve,
                    is_state_keys_initialized: raydium_pool_update.is_account_state_update,
                    liquidity_usd,
                })),
                false,
            )
        }
        PoolUpdateEvent::PumpSwap(pump_swap_pool_update) => {
            let liquidity_usd = if pump_swap_pool_update.is_account_state_update {
                0.0
            } else {
                compute_pool_liquidity_usd(
                    &pump_swap_pool_update.base_mint,
                    &pump_swap_pool_update.quote_mint,
                    pump_swap_pool_update.base_reserve,
                    pump_swap_pool_update.quote_reserve,
                    sol_price,
                )
            };
            (
                Some(PoolState::PumpSwap(PumpSwapPoolState {
                    address: pump_swap_pool_update.address,
                    index: pump_swap_pool_update.index.unwrap_or_default(),
                    creator: pump_swap_pool_update.creator,
                    last_updated: pump_swap_pool_update.last_updated,
                    base_reserve: pump_swap_pool_update.base_reserve,
                    quote_reserve: pump_swap_pool_update.quote_reserve,
                    slot: pump_swap_pool_update.slot,
                    transaction_index: pump_swap_pool_update.transaction_index,
                    base_mint: pump_swap_pool_update.base_mint,
                    quote_mint: pump_swap_pool_update.quote_mint,
                    pool_base_token_account: pump_swap_pool_update.pool_base_token_account,
                    pool_quote_token_account: pump_swap_pool_update.pool_quote_token_account,
                    is_state_keys_initialized: pump_swap_pool_update.is_account_state_update,
                    liquidity_usd,
                    coin_creator: pump_swap_pool_update.coin_creator,
                    protocol_fee_recipient: pump_swap_pool_update.protocol_fee_recipient,
                    is_cashback: pump_swap_pool_update.is_cashback.unwrap_or(false),
                })),
                false,
            )
        }
        PoolUpdateEvent::RaydiumCpmm(raydium_cpmm_pool_update) => {
            let liquidity_usd = if raydium_cpmm_pool_update.is_account_state_update {
                0.0
            } else {
                compute_pool_liquidity_usd(
                    &raydium_cpmm_pool_update.token0,
                    &raydium_cpmm_pool_update.token1,
                    raydium_cpmm_pool_update.token0_reserve,
                    raydium_cpmm_pool_update.token1_reserve,
                    sol_price,
                )
            };
            (
                Some(PoolState::RaydiumCpmm(RaydiumCpmmPoolState {
                    slot: raydium_cpmm_pool_update.slot,
                    transaction_index: raydium_cpmm_pool_update.transaction_index,
                    address: raydium_cpmm_pool_update.address,
                    status: raydium_cpmm_pool_update.status.unwrap_or_default(),
                    token0: raydium_cpmm_pool_update.token0,
                    token1: raydium_cpmm_pool_update.token1,
                    token0_vault: raydium_cpmm_pool_update.token0_vault,
                    token1_vault: raydium_cpmm_pool_update.token1_vault,
                    token0_reserve: raydium_cpmm_pool_update.token0_reserve,
                    token1_reserve: raydium_cpmm_pool_update.token1_reserve,
                    amm_config: raydium_cpmm_pool_update.amm_config,
                    observation_state: raydium_cpmm_pool_update.observation_state,
                    last_updated: raydium_cpmm_pool_update.last_updated,
                    liquidity_usd,
                    is_state_keys_initialized: raydium_cpmm_pool_update.is_account_state_update,
                })),
                false,
            )
        }
        PoolUpdateEvent::Bonk(bonk_pool_update) => {
            let liquidity_usd = compute_pool_liquidity_usd(
                &bonk_pool_update.base_mint,
                &bonk_pool_update.quote_vault,
                bonk_pool_update.base_reserve,
                bonk_pool_update.quote_reserve,
                sol_price,
            );
            (
                Some(PoolState::Bonk(BonkPoolState {
                    slot: bonk_pool_update.slot,
                    transaction_index: bonk_pool_update.transaction_index,
                    address: bonk_pool_update.address,
                    status: bonk_pool_update.status,
                    total_base_sell: bonk_pool_update.total_base_sell,
                    base_reserve: bonk_pool_update.base_reserve,
                    quote_reserve: bonk_pool_update.quote_reserve,
                    real_base: bonk_pool_update.real_base,
                    real_quote: bonk_pool_update.real_quote,
                    quote_protocol_fee: bonk_pool_update.quote_protocol_fee,
                    platform_fee: bonk_pool_update.platform_fee,
                    global_config: bonk_pool_update.global_config,
                    platform_config: bonk_pool_update.platform_config,
                    platform_fee_wallet: Pubkey::default(),
                    base_mint: bonk_pool_update.base_mint,
                    quote_mint: bonk_pool_update.quote_mint,
                    base_vault: bonk_pool_update.base_vault,
                    quote_vault: bonk_pool_update.quote_vault,
                    creator: bonk_pool_update.creator,
                    last_updated: bonk_pool_update.last_updated,
                    is_state_keys_initialized: bonk_pool_update.is_account_state_update,
                    liquidity_usd,
                })),
                false,
            )
        }
        PoolUpdateEvent::RaydiumClmm(raydium_clmm_pool_update) => {
            let mut pool_state = RaydiumClmmPoolState {
                slot: raydium_clmm_pool_update.slot,
                transaction_index: raydium_clmm_pool_update.transaction_index,
                address: raydium_clmm_pool_update.address,
                amm_config: Pubkey::default(),
                token_mint0: Pubkey::default(),
                token_mint1: Pubkey::default(),
                token_vault0: Pubkey::default(),
                token_vault1: Pubkey::default(),
                observation_key: Pubkey::default(),
                tick_spacing: 0,
                liquidity: 0,
                sqrt_price_x64: 0,
                tick_current_index: 0,
                status: 0,
                tick_array_bitmap: [0; 16],
                open_time: 0,
                tick_array_state: HashMap::new(),
                tick_array_bitmap_extension: None,
                last_updated: raydium_clmm_pool_update.last_updated,
                token0_reserve: 0,
                token1_reserve: 0,
                is_state_keys_initialized: raydium_clmm_pool_update.is_account_state_update,
                liquidity_usd: 0.0,
            };
            pool_state.slot = raydium_clmm_pool_update.slot;
            pool_state.transaction_index = raydium_clmm_pool_update.transaction_index;
            pool_state.last_updated = raydium_clmm_pool_update.last_updated;

            if let Some(ref pool_state_part) = raydium_clmm_pool_update.pool_state_part {
                pool_state.amm_config = pool_state_part.amm_config;
                pool_state.token_mint0 = pool_state_part.token_mint0;
                pool_state.token_mint1 = pool_state_part.token_mint1;
                pool_state.token_vault0 = pool_state_part.token_vault0;
                pool_state.token_vault1 = pool_state_part.token_vault1;
                pool_state.observation_key = pool_state_part.observation_key;
                pool_state.tick_spacing = pool_state_part.tick_spacing;
                pool_state.liquidity = pool_state_part.liquidity;
                pool_state.sqrt_price_x64 = pool_state_part.sqrt_price_x64;
                pool_state.tick_current_index = pool_state_part.tick_current_index;
                pool_state.status = pool_state_part.status;
                pool_state.tick_array_bitmap = pool_state_part.tick_array_bitmap;
                pool_state.open_time = pool_state_part.open_time;
            }

            if let Some(ref token_reserves) = raydium_clmm_pool_update.reserve_part {
                pool_state.token0_reserve = token_reserves.token0_reserve;
                pool_state.token1_reserve = token_reserves.token1_reserve;

                pool_state.liquidity_usd = compute_pool_liquidity_usd(
                    &pool_state.token_mint0,
                    &pool_state.token_mint1,
                    pool_state.token0_reserve,
                    pool_state.token1_reserve,
                    sol_price,
                );
            }

            if let Some(ref tick_array_update) = raydium_clmm_pool_update.tick_array_state {
                pool_state.tick_array_state.insert(
                    tick_array_update.start_tick_index,
                    tick_array_update.clone(),
                );
            }

            if raydium_clmm_pool_update
                .tick_array_bitmap_extension
                .is_some()
            {
                pool_state.tick_array_bitmap_extension =
                    raydium_clmm_pool_update.tick_array_bitmap_extension.clone();
            }

            (Some(PoolState::RadyiumClmm(Box::new(pool_state))), true)
        }
        PoolUpdateEvent::Whirlpool(whirlpool_update) => {
            let mut pool_state = WhirlpoolPoolState {
                slot: whirlpool_update.slot,
                transaction_index: whirlpool_update.transaction_index,
                address: whirlpool_update.address,
                last_updated: whirlpool_update.last_updated,
                is_state_keys_initialized: whirlpool_update.is_account_state_update,
                ..Default::default()
            };

            if let Some(ref pool_state_part) = whirlpool_update.pool_state_part {
                pool_state.whirlpool_config = pool_state_part.whirlpool_config;
                pool_state.token_mint_a = pool_state_part.token_mint_a;
                pool_state.token_mint_b = pool_state_part.token_mint_b;
                pool_state.token_vault_a = pool_state_part.token_vault_a;
                pool_state.token_vault_b = pool_state_part.token_vault_b;
                pool_state.tick_spacing = pool_state_part.tick_spacing;
                pool_state.liquidity = pool_state_part.liquidity;
                pool_state.sqrt_price = pool_state_part.sqrt_price;
                pool_state.tick_current_index = pool_state_part.tick_current_index;
                pool_state.fee_rate = pool_state_part.fee_rate;
                pool_state.protocol_fee_rate = pool_state_part.protocol_fee_rate;
                pool_state.tick_spacing_seed = pool_state_part.tick_spacing_seed;
            }

            if let Some(ref token_reserves) = whirlpool_update.reserve_part {
                pool_state.token_a_reserve = token_reserves.token_a_reserve;
                pool_state.token_b_reserve = token_reserves.token_b_reserve;

                pool_state.liquidity_usd = compute_pool_liquidity_usd(
                    &pool_state.token_mint_a,
                    &pool_state.token_mint_b,
                    pool_state.token_a_reserve,
                    pool_state.token_b_reserve,
                    sol_price,
                );
            }

            if let Some(ref tick_array_update) = whirlpool_update.tick_array_state {
                pool_state.tick_array_state.insert(
                    tick_array_update.start_tick_index,
                    tick_array_update.clone(),
                );
            }

            if let Some(ref oracle_state) = whirlpool_update.oracle_state {
                pool_state.oracle_state = oracle_state.clone();
            }

            (Some(PoolState::OrcaWhirlpool(pool_state)), true)
        }
        PoolUpdateEvent::MeteoraDbc(dbc_pool_update) => {
            // Get pool config from update or from dbc_configs parameter
            let pool_config = dbc_pool_update
                .pool_config
                .as_ref()
                .or_else(|| dbc_configs.and_then(|configs| configs.get(&dbc_pool_update.config)));

            let liquidity_usd = if let Some(config) = pool_config {
                if dbc_pool_update.is_account_state_update && !dbc_pool_update.is_config_update {
                    compute_pool_liquidity_usd(
                        &dbc_pool_update.base_mint,
                        &config.quote_mint,
                        dbc_pool_update.base_reserve,
                        dbc_pool_update.quote_reserve,
                        sol_price,
                    )
                } else {
                    0.0
                }
            } else {
                0.0
            };

            (
                Some(PoolState::MeteoraDbc(Box::new(DbcPoolState {
                    slot: dbc_pool_update.slot,
                    transaction_index: dbc_pool_update.transaction_index,
                    address: dbc_pool_update.address,
                    config: dbc_pool_update.config,
                    creator: dbc_pool_update.creator,
                    base_mint: dbc_pool_update.base_mint,
                    base_vault: dbc_pool_update.base_vault,
                    quote_vault: dbc_pool_update.quote_vault,
                    base_reserve: dbc_pool_update.base_reserve,
                    quote_reserve: dbc_pool_update.quote_reserve,
                    protocol_base_fee: dbc_pool_update.protocol_base_fee,
                    protocol_quote_fee: dbc_pool_update.protocol_quote_fee,
                    partner_base_fee: dbc_pool_update.partner_base_fee,
                    partner_quote_fee: dbc_pool_update.partner_quote_fee,
                    sqrt_price: dbc_pool_update.sqrt_price,
                    activation_point: dbc_pool_update.activation_point,
                    pool_type: dbc_pool_update.pool_type,
                    is_migrated: dbc_pool_update.is_migrated,
                    is_partner_withdraw_surplus: dbc_pool_update.is_partner_withdraw_surplus,
                    is_protocol_withdraw_surplus: dbc_pool_update.is_protocol_withdraw_surplus,
                    migration_progress: dbc_pool_update.migration_progress,
                    is_withdraw_leftover: dbc_pool_update.is_withdraw_leftover,
                    is_creator_withdraw_surplus: dbc_pool_update.is_creator_withdraw_surplus,
                    migration_fee_withdraw_status: dbc_pool_update.migration_fee_withdraw_status,
                    finish_curve_timestamp: dbc_pool_update.finish_curve_timestamp,
                    creator_base_fee: dbc_pool_update.creator_base_fee,
                    creator_quote_fee: dbc_pool_update.creator_quote_fee,
                    liquidity_usd,
                    last_updated: dbc_pool_update.last_updated,

                    // Config fields - use from update or from dbc_configs parameter
                    pool_config: pool_config.cloned(),

                    // Volatility Tracker
                    volatility_tracker: dbc_pool_update.volatility_tracker.clone(),
                }))),
                false,
            )
        }
        PoolUpdateEvent::MeteoraDammV2(meteora_dammv2_pool_update) => {
            // Calculate reserves from pool state using DAMM V2 formulas
            let (token_a_reserve, token_b_reserve) =
                if !meteora_dammv2_pool_update.is_account_state_update {
                    calculate_damm_v2_reserves(
                        meteora_dammv2_pool_update.liquidity,
                        meteora_dammv2_pool_update.sqrt_price,
                        meteora_dammv2_pool_update.sqrt_min_price,
                        meteora_dammv2_pool_update.sqrt_max_price,
                    )
                } else {
                    (0, 0)
                };

            let liquidity_usd = if !meteora_dammv2_pool_update.is_account_state_update {
                compute_pool_liquidity_usd(
                    &meteora_dammv2_pool_update.token_a_mint,
                    &meteora_dammv2_pool_update.token_b_mint,
                    token_a_reserve,
                    token_b_reserve,
                    sol_price,
                )
            } else {
                0.0
            };

            (
                Some(PoolState::MeteoraDammV2(Box::new(MeteoraDammV2PoolState {
                    slot: meteora_dammv2_pool_update.slot,
                    transaction_index: meteora_dammv2_pool_update.transaction_index,
                    address: meteora_dammv2_pool_update.address,
                    pool_fees: meteora_dammv2_pool_update.pool_fees.clone(),
                    token_a_mint: meteora_dammv2_pool_update.token_a_mint,
                    token_b_mint: meteora_dammv2_pool_update.token_b_mint,
                    token_a_vault: meteora_dammv2_pool_update.token_a_vault,
                    token_b_vault: meteora_dammv2_pool_update.token_b_vault,
                    whitelisted_vault: meteora_dammv2_pool_update.whitelisted_vault,
                    partner: meteora_dammv2_pool_update.partner,
                    liquidity: meteora_dammv2_pool_update.liquidity,
                    protocol_a_fee: meteora_dammv2_pool_update.protocol_a_fee,
                    protocol_b_fee: meteora_dammv2_pool_update.protocol_b_fee,
                    partner_a_fee: meteora_dammv2_pool_update.partner_a_fee,
                    partner_b_fee: meteora_dammv2_pool_update.partner_b_fee,
                    sqrt_min_price: meteora_dammv2_pool_update.sqrt_min_price,
                    sqrt_max_price: meteora_dammv2_pool_update.sqrt_max_price,
                    sqrt_price: meteora_dammv2_pool_update.sqrt_price,
                    activation_point: meteora_dammv2_pool_update.activation_point,
                    activation_type: meteora_dammv2_pool_update.activation_type,
                    pool_status: meteora_dammv2_pool_update.pool_status,
                    token_a_flag: meteora_dammv2_pool_update.token_a_flag,
                    token_b_flag: meteora_dammv2_pool_update.token_b_flag,
                    collect_fee_mode: meteora_dammv2_pool_update.collect_fee_mode,
                    pool_type: meteora_dammv2_pool_update.pool_type,
                    version: meteora_dammv2_pool_update.version,
                    fee_a_per_liquidity: meteora_dammv2_pool_update.fee_a_per_liquidity,
                    fee_b_per_liquidity: meteora_dammv2_pool_update.fee_b_per_liquidity,
                    permanent_lock_liquidity: meteora_dammv2_pool_update.permanent_lock_liquidity,
                    metrics: meteora_dammv2_pool_update.metrics.clone(),
                    creator: meteora_dammv2_pool_update.creator,
                    reward_infos: meteora_dammv2_pool_update.reward_infos.clone(),
                    liquidity_usd,
                    last_updated: meteora_dammv2_pool_update.last_updated,
                }))),
                false,
            )
        }
        PoolUpdateEvent::MeteoraDlmm(meteora_dlmm_pool_update) => {
            let liquidity_usd = if let (Some(reserve_x), Some(reserve_y)) = (
                meteora_dlmm_pool_update.reserve_x,
                meteora_dlmm_pool_update.reserve_y,
            ) {
                compute_pool_liquidity_usd(
                    &meteora_dlmm_pool_update.lbpair.token_x_mint,
                    &meteora_dlmm_pool_update.lbpair.token_y_mint,
                    reserve_x,
                    reserve_y,
                    sol_price,
                )
            } else {
                0.0
            };

            (
                Some(PoolState::MeteoraDlmm(Box::new(MeteoraDlmmPoolState {
                    slot: meteora_dlmm_pool_update.slot,
                    transaction_index: meteora_dlmm_pool_update.transaction_index,
                    address: meteora_dlmm_pool_update.address,
                    lbpair: meteora_dlmm_pool_update.lbpair.clone(),
                    bin_arrays: meteora_dlmm_pool_update
                        .bin_arrays
                        .clone()
                        .unwrap_or_default(),
                    bitmap_extension: meteora_dlmm_pool_update.bitmap_extension.clone(),
                    is_state_keys_initialized: true,
                    reserve_x: meteora_dlmm_pool_update.reserve_x,
                    reserve_y: meteora_dlmm_pool_update.reserve_y,
                    liquidity_usd,
                    last_updated: meteora_dlmm_pool_update.last_updated,
                }))),
                false,
            )
        }
    }
}

pub fn update_pool_state_by_event(
    event: &PoolUpdateEvent,
    existing_state: &mut MutexGuard<PoolState>,
    sol_price: f64,
) -> bool {
    let mut is_pool_with_ticks = false;
    match event {
        PoolUpdateEvent::Pumpfun(pumpfun_pool_update) => {
            if let PoolState::Pumpfun(state) = &mut **existing_state {
                // only update last_updated if it's transaction event update that we can collect reserves
                if !pumpfun_pool_update.is_account_state_update {
                    state.last_updated = pumpfun_pool_update.last_updated;
                }
                state.liquidity_usd =
                    pumpfun_pool_update.virtual_sol_reserves as f64 / 1_000_000_000_f64 * sol_price;
                state.complete = pumpfun_pool_update.complete;
                state.virtual_sol_reserves = pumpfun_pool_update.virtual_sol_reserves;
                state.virtual_token_reserves = pumpfun_pool_update.virtual_token_reserves;
                state.real_sol_reserves = pumpfun_pool_update.real_sol_reserves;
                state.real_token_reserves = pumpfun_pool_update.real_token_reserves;
                state.slot = pumpfun_pool_update.slot;
                state.transaction_index = pumpfun_pool_update.transaction_index;
                if pumpfun_pool_update.creator != Pubkey::default() {
                    state.creator = pumpfun_pool_update.creator;
                }
                if pumpfun_pool_update.is_mayhem_mode {
                    state.is_mayhem_mode = pumpfun_pool_update.is_mayhem_mode;
                }
                if let Some(is_cashback) = pumpfun_pool_update.is_cashback {
                    state.is_cashback = is_cashback;
                }
            }
        }
        PoolUpdateEvent::Raydium(raydium_pool_update) => {
            if let PoolState::RaydiumAmmV4(state) = &mut **existing_state {
                // only update last_updated if it's transaction event update that we can collect reserves
                if !raydium_pool_update.is_account_state_update {
                    state.last_updated = raydium_pool_update.last_updated;
                }

                state.slot = raydium_pool_update.slot;
                state.transaction_index = raydium_pool_update.transaction_index;
                if let Some(serum_program) = raydium_pool_update.serum_program {
                    if !tokens_equal(&serum_program, &state.serum_program) {
                        state.serum_program = serum_program;
                    }
                }
                if let Some(serum_market) = raydium_pool_update.serum_market {
                    if !tokens_equal(&serum_market, &state.serum_market) {
                        state.serum_market = serum_market;
                    }
                }
                if let Some(serum_bids) = raydium_pool_update.serum_bids {
                    if !tokens_equal(&serum_bids, &state.serum_bids) {
                        state.serum_bids = serum_bids;
                    }
                }
                if let Some(serum_asks) = raydium_pool_update.serum_asks {
                    if !tokens_equal(&serum_asks, &state.serum_asks) {
                        state.serum_asks = serum_asks;
                    }
                }
                if let Some(serum_event_queue) = raydium_pool_update.serum_event_queue {
                    if !tokens_equal(&serum_event_queue, &state.serum_event_queue) {
                        state.serum_event_queue = serum_event_queue;
                    }
                }
                if let Some(serum_coin_vault_account) = raydium_pool_update.serum_coin_vault_account
                {
                    if !tokens_equal(&serum_coin_vault_account, &state.serum_coin_vault_account) {
                        state.serum_coin_vault_account = serum_coin_vault_account;
                    }
                }
                if let Some(serum_pc_vault_account) = raydium_pool_update.serum_pc_vault_account {
                    if !tokens_equal(&serum_pc_vault_account, &state.serum_pc_vault_account) {
                        state.serum_pc_vault_account = serum_pc_vault_account;
                    }
                }
                if let Some(serum_vault_signer) = raydium_pool_update.serum_vault_signer {
                    if !tokens_equal(&serum_vault_signer, &state.serum_vault_signer) {
                        state.serum_vault_signer = serum_vault_signer;
                    }
                }
                if !raydium_pool_update.is_account_state_update {
                    state.base_reserve = raydium_pool_update.base_reserve;
                    state.quote_reserve = raydium_pool_update.quote_reserve;
                    state.liquidity_usd = compute_pool_liquidity_usd(
                        &raydium_pool_update.base_mint,
                        &raydium_pool_update.quote_mint,
                        raydium_pool_update.base_reserve,
                        raydium_pool_update.quote_reserve,
                        sol_price,
                    );
                }
            }
        }
        PoolUpdateEvent::PumpSwap(pump_swap_pool_update) => {
            if let PoolState::PumpSwap(state) = &mut **existing_state {
                // only update last_updated if it's transaction event update that we can collect reserves
                if !pump_swap_pool_update.is_account_state_update {
                    state.last_updated = pump_swap_pool_update.last_updated;
                }
                state.slot = pump_swap_pool_update.slot;
                state.transaction_index = pump_swap_pool_update.transaction_index;
                state.index = pump_swap_pool_update.index.unwrap_or(state.index);
                if let Some(creator) = pump_swap_pool_update.creator {
                    if state.creator.is_none() {
                        state.creator = Some(creator);
                    }
                }
                if !pump_swap_pool_update.is_account_state_update {
                    state.base_reserve = pump_swap_pool_update.base_reserve;
                    state.quote_reserve = pump_swap_pool_update.quote_reserve;
                    state.liquidity_usd = compute_pool_liquidity_usd(
                        &pump_swap_pool_update.base_mint,
                        &pump_swap_pool_update.quote_mint,
                        pump_swap_pool_update.base_reserve,
                        pump_swap_pool_update.quote_reserve,
                        sol_price,
                    );
                }
                if pump_swap_pool_update.coin_creator != Pubkey::default() {
                    state.coin_creator = pump_swap_pool_update.coin_creator;
                }
                if pump_swap_pool_update.protocol_fee_recipient != Pubkey::default() {
                    state.protocol_fee_recipient = pump_swap_pool_update.protocol_fee_recipient;
                }
                if let Some(is_cashback) = pump_swap_pool_update.is_cashback {
                    state.is_cashback = is_cashback;
                }
            }
        }
        PoolUpdateEvent::RaydiumCpmm(raydium_cpmm_pool_update) => {
            if let PoolState::RaydiumCpmm(state) = &mut **existing_state {
                // only update last_updated if it's transaction event update that we can collect reserves
                if !raydium_cpmm_pool_update.is_account_state_update {
                    state.last_updated = raydium_cpmm_pool_update.last_updated;
                }
                state.slot = raydium_cpmm_pool_update.slot;
                state.transaction_index = raydium_cpmm_pool_update.transaction_index;
                state.status = raydium_cpmm_pool_update.status.unwrap_or(state.status);
                if !raydium_cpmm_pool_update.is_account_state_update {
                    if raydium_cpmm_pool_update.token0_reserve != 0 {
                        state.token0_reserve = raydium_cpmm_pool_update.token0_reserve;
                        state.liquidity_usd = compute_pool_liquidity_usd(
                            &raydium_cpmm_pool_update.token0,
                            &raydium_cpmm_pool_update.token1,
                            state.token0_reserve,
                            state.token1_reserve,
                            sol_price,
                        );
                    }
                    if raydium_cpmm_pool_update.token1_reserve != 0 {
                        state.token1_reserve = raydium_cpmm_pool_update.token1_reserve;
                    }
                }
                if !tokens_equal(&raydium_cpmm_pool_update.amm_config, &state.amm_config) {
                    state.amm_config = raydium_cpmm_pool_update.amm_config;
                }
                if !tokens_equal(
                    &raydium_cpmm_pool_update.observation_state,
                    &state.observation_state,
                ) {
                    state.observation_state = raydium_cpmm_pool_update.observation_state;
                }
            }
        }
        PoolUpdateEvent::Bonk(bonk_pool_update) => {
            if let PoolState::Bonk(state) = &mut **existing_state {
                // only update last_updated if it's transaction event update that we can collect reserves
                if !bonk_pool_update.is_account_state_update {
                    state.last_updated = bonk_pool_update.last_updated;
                }
                state.slot = bonk_pool_update.slot;
                state.transaction_index = bonk_pool_update.transaction_index;
                state.status = bonk_pool_update.status;
                state.total_base_sell = bonk_pool_update.total_base_sell;
                state.base_reserve = bonk_pool_update.base_reserve;
                state.quote_reserve = bonk_pool_update.quote_reserve;
                state.real_base = bonk_pool_update.real_base;
                state.real_quote = bonk_pool_update.real_quote;
                state.quote_protocol_fee = bonk_pool_update.quote_protocol_fee;
                state.platform_fee = bonk_pool_update.platform_fee;
                state.global_config = bonk_pool_update.global_config;
                state.platform_config = bonk_pool_update.platform_config;
                state.liquidity_usd = compute_pool_liquidity_usd(
                    &bonk_pool_update.base_mint,
                    &bonk_pool_update.quote_mint,
                    bonk_pool_update.base_reserve,
                    bonk_pool_update.quote_reserve,
                    sol_price,
                );
            }
        }
        PoolUpdateEvent::RaydiumClmm(raydium_clmm_pool_update) => {
            if let PoolState::RadyiumClmm(state) = &mut **existing_state {
                is_pool_with_ticks = true;
                // only update last_updated if it's transaction event update that we can collect reserves
                if !raydium_clmm_pool_update.is_account_state_update {
                    state.last_updated = raydium_clmm_pool_update.last_updated;
                    state.slot = raydium_clmm_pool_update.slot;
                    state.transaction_index = raydium_clmm_pool_update.transaction_index;
                }

                let default_pubkey = Pubkey::default();
                if let Some(ref pool_state_part) = raydium_clmm_pool_update.pool_state_part {
                    if !tokens_equal(&pool_state_part.amm_config, &default_pubkey)
                        && !tokens_equal(&pool_state_part.amm_config, &state.amm_config)
                    {
                        state.amm_config = pool_state_part.amm_config;
                    }
                    if !tokens_equal(&pool_state_part.token_mint0, &default_pubkey)
                        && !tokens_equal(&pool_state_part.token_mint0, &state.token_mint0)
                    {
                        state.token_mint0 = pool_state_part.token_mint0;
                    }
                    if !tokens_equal(&pool_state_part.token_mint1, &default_pubkey)
                        && !tokens_equal(&pool_state_part.token_mint1, &state.token_mint1)
                    {
                        state.token_mint1 = pool_state_part.token_mint1;
                    }
                    if !tokens_equal(&pool_state_part.token_vault0, &default_pubkey)
                        && !tokens_equal(&pool_state_part.token_vault0, &state.token_vault0)
                    {
                        state.token_vault0 = pool_state_part.token_vault0;
                    }
                    if !tokens_equal(&pool_state_part.token_vault1, &default_pubkey)
                        && !tokens_equal(&pool_state_part.token_vault1, &state.token_vault1)
                    {
                        state.token_vault1 = pool_state_part.token_vault1;
                    }
                    if !tokens_equal(&pool_state_part.observation_key, &default_pubkey)
                        && !tokens_equal(&pool_state_part.observation_key, &state.observation_key)
                    {
                        state.observation_key = pool_state_part.observation_key;
                    }

                    state.tick_spacing = if pool_state_part.tick_spacing != 0 {
                        pool_state_part.tick_spacing
                    } else {
                        state.tick_spacing
                    };
                    state.liquidity = if pool_state_part.liquidity != 0 {
                        pool_state_part.liquidity
                    } else {
                        state.liquidity
                    };
                    state.sqrt_price_x64 = if pool_state_part.sqrt_price_x64 != 0 {
                        pool_state_part.sqrt_price_x64
                    } else {
                        state.sqrt_price_x64
                    };
                    state.tick_current_index = if pool_state_part.tick_current_index != 0 {
                        pool_state_part.tick_current_index
                    } else {
                        state.tick_current_index
                    };
                    state.status = if pool_state_part.status != 0 {
                        pool_state_part.status
                    } else {
                        state.status
                    };
                    if pool_state_part.tick_array_bitmap != [0; 16] {
                        state.tick_array_bitmap = pool_state_part.tick_array_bitmap;
                    }
                    state.open_time = if pool_state_part.open_time != 0 {
                        pool_state_part.open_time
                    } else {
                        state.open_time
                    };
                }

                if let Some(ref token_reserves) = raydium_clmm_pool_update.reserve_part {
                    if token_reserves.token0_reserve != 0 {
                        state.token0_reserve = token_reserves.token0_reserve;
                    }
                    if token_reserves.token1_reserve != 0 {
                        state.token1_reserve = token_reserves.token1_reserve;
                    }

                    state.liquidity_usd = compute_pool_liquidity_usd(
                        &state.token_mint0,
                        &state.token_mint1,
                        state.token0_reserve,
                        state.token1_reserve,
                        sol_price,
                    );
                }
                if let Some(ref tick_array_update) = raydium_clmm_pool_update.tick_array_state {
                    state.tick_array_state.insert(
                        tick_array_update.start_tick_index,
                        tick_array_update.clone(),
                    );
                }

                if raydium_clmm_pool_update
                    .tick_array_bitmap_extension
                    .is_some()
                {
                    state.tick_array_bitmap_extension =
                        raydium_clmm_pool_update.tick_array_bitmap_extension.clone();
                }
            }
        }
        PoolUpdateEvent::Whirlpool(whirlpool_update) => {
            if let PoolState::OrcaWhirlpool(state) = &mut **existing_state {
                is_pool_with_ticks = true;
                // only update last_updated if it's transaction event update that we can collect reserves
                if !whirlpool_update.is_account_state_update {
                    state.last_updated = whirlpool_update.last_updated;
                    state.slot = whirlpool_update.slot;
                    state.transaction_index = whirlpool_update.transaction_index;
                }

                let default_pubkey = Pubkey::default();
                if let Some(ref pool_state_part) = whirlpool_update.pool_state_part {
                    if !tokens_equal(&pool_state_part.whirlpool_config, &default_pubkey)
                        && !tokens_equal(&pool_state_part.whirlpool_config, &state.whirlpool_config)
                    {
                        state.whirlpool_config = pool_state_part.whirlpool_config;
                    }
                    if !tokens_equal(&pool_state_part.token_mint_a, &default_pubkey)
                        && !tokens_equal(&pool_state_part.token_mint_a, &state.token_mint_a)
                    {
                        state.token_mint_a = pool_state_part.token_mint_a;
                    }
                    if !tokens_equal(&pool_state_part.token_mint_b, &default_pubkey)
                        && !tokens_equal(&pool_state_part.token_mint_b, &state.token_mint_b)
                    {
                        state.token_mint_b = pool_state_part.token_mint_b;
                    }
                    if !tokens_equal(&pool_state_part.token_vault_a, &default_pubkey)
                        && !tokens_equal(&pool_state_part.token_vault_a, &state.token_vault_a)
                    {
                        state.token_vault_a = pool_state_part.token_vault_a;
                    }
                    if !tokens_equal(&pool_state_part.token_vault_b, &default_pubkey)
                        && !tokens_equal(&pool_state_part.token_vault_b, &state.token_vault_b)
                    {
                        state.token_vault_b = pool_state_part.token_vault_b;
                    }

                    state.tick_spacing = pool_state_part.tick_spacing;
                    state.liquidity = pool_state_part.liquidity;
                    state.sqrt_price = pool_state_part.sqrt_price;
                    state.tick_current_index = pool_state_part.tick_current_index;
                    state.fee_rate = pool_state_part.fee_rate;
                    state.protocol_fee_rate = pool_state_part.protocol_fee_rate;
                    state.tick_spacing_seed = pool_state_part.tick_spacing_seed;
                }

                if let Some(ref token_reserves) = whirlpool_update.reserve_part {
                    if token_reserves.token_a_reserve != 0 {
                        state.token_a_reserve = token_reserves.token_a_reserve;
                    }
                    if token_reserves.token_b_reserve != 0 {
                        state.token_b_reserve = token_reserves.token_b_reserve;
                    }

                    state.liquidity_usd = compute_pool_liquidity_usd(
                        &state.token_mint_a,
                        &state.token_mint_b,
                        state.token_a_reserve,
                        state.token_b_reserve,
                        sol_price,
                    );
                }
                if let Some(ref tick_array_update) = whirlpool_update.tick_array_state {
                    state.tick_array_state.insert(
                        tick_array_update.start_tick_index,
                        tick_array_update.clone(),
                    );
                }

                if let Some(ref oracle_state) = whirlpool_update.oracle_state {
                    state.oracle_state = oracle_state.clone();
                }
            }
        }
        PoolUpdateEvent::MeteoraDbc(dbc_pool_update) => {
            if let PoolState::MeteoraDbc(state) = &mut **existing_state {
                state.last_updated = dbc_pool_update.last_updated;
                state.slot = dbc_pool_update.slot;
                state.transaction_index = dbc_pool_update.transaction_index;

                if dbc_pool_update.is_config_update {
                    // Update config fields
                    state.pool_config = dbc_pool_update.pool_config.clone();
                } else {
                    // Update pool fields
                    state.base_reserve = dbc_pool_update.base_reserve;
                    state.quote_reserve = dbc_pool_update.quote_reserve;
                    state.protocol_base_fee = dbc_pool_update.protocol_base_fee;
                    state.protocol_quote_fee = dbc_pool_update.protocol_quote_fee;
                    state.partner_base_fee = dbc_pool_update.partner_base_fee;
                    state.partner_quote_fee = dbc_pool_update.partner_quote_fee;
                    state.sqrt_price = dbc_pool_update.sqrt_price;
                    state.activation_point = dbc_pool_update.activation_point;
                    state.pool_type = dbc_pool_update.pool_type;
                    state.is_migrated = dbc_pool_update.is_migrated;
                    state.is_partner_withdraw_surplus = dbc_pool_update.is_partner_withdraw_surplus;
                    state.is_protocol_withdraw_surplus =
                        dbc_pool_update.is_protocol_withdraw_surplus;
                    state.migration_progress = dbc_pool_update.migration_progress;
                    state.is_withdraw_leftover = dbc_pool_update.is_withdraw_leftover;
                    state.is_creator_withdraw_surplus = dbc_pool_update.is_creator_withdraw_surplus;
                    state.migration_fee_withdraw_status =
                        dbc_pool_update.migration_fee_withdraw_status;
                    state.finish_curve_timestamp = dbc_pool_update.finish_curve_timestamp;
                    state.creator_base_fee = dbc_pool_update.creator_base_fee;
                    state.creator_quote_fee = dbc_pool_update.creator_quote_fee;

                    // Update Volatility Tracker
                    state.volatility_tracker = dbc_pool_update.volatility_tracker.clone();

                    // Recalculate liquidity USD
                    if let Some(config) = &state.pool_config {
                        state.liquidity_usd = compute_pool_liquidity_usd(
                            &state.base_mint,
                            &config.quote_mint,
                            state.base_reserve,
                            state.quote_reserve,
                            sol_price,
                        );
                    }
                }
            }
        }
        PoolUpdateEvent::MeteoraDammV2(meteora_dammv2_pool_update) => {
            if let PoolState::MeteoraDammV2(state) = &mut **existing_state {
                // Update slot and timestamp
                state.last_updated = meteora_dammv2_pool_update.last_updated;

                state.slot = meteora_dammv2_pool_update.slot;
                state.transaction_index = meteora_dammv2_pool_update.transaction_index;

                // Update pool state fields
                state.pool_fees = meteora_dammv2_pool_update.pool_fees.clone();
                state.whitelisted_vault = meteora_dammv2_pool_update.whitelisted_vault;
                state.partner = meteora_dammv2_pool_update.partner;
                state.liquidity = meteora_dammv2_pool_update.liquidity;
                state.protocol_a_fee = meteora_dammv2_pool_update.protocol_a_fee;
                state.protocol_b_fee = meteora_dammv2_pool_update.protocol_b_fee;
                state.partner_a_fee = meteora_dammv2_pool_update.partner_a_fee;
                state.partner_b_fee = meteora_dammv2_pool_update.partner_b_fee;
                state.sqrt_min_price = meteora_dammv2_pool_update.sqrt_min_price;
                state.sqrt_max_price = meteora_dammv2_pool_update.sqrt_max_price;
                state.sqrt_price = meteora_dammv2_pool_update.sqrt_price;
                state.activation_point = meteora_dammv2_pool_update.activation_point;
                state.activation_type = meteora_dammv2_pool_update.activation_type;
                state.pool_status = meteora_dammv2_pool_update.pool_status;
                state.collect_fee_mode = meteora_dammv2_pool_update.collect_fee_mode;
                state.pool_type = meteora_dammv2_pool_update.pool_type;
                state.version = meteora_dammv2_pool_update.version;
                state.fee_a_per_liquidity = meteora_dammv2_pool_update.fee_a_per_liquidity;
                state.fee_b_per_liquidity = meteora_dammv2_pool_update.fee_b_per_liquidity;
                state.permanent_lock_liquidity =
                    meteora_dammv2_pool_update.permanent_lock_liquidity;
                state.metrics = meteora_dammv2_pool_update.metrics.clone();
                state.reward_infos = meteora_dammv2_pool_update.reward_infos.clone();

                // Recalculate liquidity USD from pool state
                let (token_a_reserve, token_b_reserve) = calculate_damm_v2_reserves(
                    state.liquidity,
                    state.sqrt_price,
                    state.sqrt_min_price,
                    state.sqrt_max_price,
                );
                state.liquidity_usd = compute_pool_liquidity_usd(
                    &state.token_a_mint,
                    &state.token_b_mint,
                    token_a_reserve,
                    token_b_reserve,
                    sol_price,
                );
            }
        }
        PoolUpdateEvent::MeteoraDlmm(meteora_dlmm_pool_update) => {
            if let PoolState::MeteoraDlmm(state) = &mut **existing_state {
                is_pool_with_ticks = true;

                state.slot = meteora_dlmm_pool_update.slot;
                state.transaction_index = meteora_dlmm_pool_update.transaction_index;
                state.last_updated = meteora_dlmm_pool_update.last_updated;

                if let (Some(reserve_x), Some(reserve_y)) = (
                    meteora_dlmm_pool_update.reserve_x,
                    meteora_dlmm_pool_update.reserve_y,
                ) {
                    state.liquidity_usd = compute_pool_liquidity_usd(
                        &state.lbpair.token_x_mint,
                        &state.lbpair.token_y_mint,
                        reserve_x,
                        reserve_y,
                        sol_price,
                    );
                }

                if meteora_dlmm_pool_update.pool_update_event_type
                    == PoolUpdateEventType::MeteoraDlmmLbPairAccount
                {
                    state.lbpair = meteora_dlmm_pool_update.lbpair.clone();
                } else if meteora_dlmm_pool_update.pool_update_event_type
                    == PoolUpdateEventType::MeteoraDlmmBinArrayAccount
                {
                    if let Some(ref bin_arrays) = meteora_dlmm_pool_update.bin_arrays {
                        for (index, bin_array) in bin_arrays {
                            state.bin_arrays.insert(*index, bin_array.clone());
                        }
                    }
                } else if meteora_dlmm_pool_update.pool_update_event_type
                    == PoolUpdateEventType::MeteoraDlmmBinArrayBitmapExtensionAccount
                {
                    if let Some(ref bitmap_ext) = meteora_dlmm_pool_update.bitmap_extension {
                        state.bitmap_extension = Some(bitmap_ext.clone());
                    }
                }
            }
        }
    }
    is_pool_with_ticks
}
