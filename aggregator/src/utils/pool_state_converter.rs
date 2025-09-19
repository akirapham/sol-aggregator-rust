use solana_sdk::pubkey::Pubkey;
use tokio::sync::MutexGuard;

use crate::constants::is_base_token;
use crate::pool_data_types::{
    BonkPoolState, PumpSwapPoolState, PumpfunPoolState, RadyiumClmmPoolState,
    RaydiumAmmV4PoolState, RaydiumCpmmPoolState,
};
use crate::pool_data_types::{PoolState, TickArrayState, TickState};
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

pub fn pool_update_event_to_pool_state(
    event: &PoolUpdateEvent,
    sol_price: f64,
) -> Option<PoolState> {
    match event {
        PoolUpdateEvent::PumpfunPoolUpdate(pumpfun_pool_update) => {
            Some(PoolState::PumpfunPoolState(PumpfunPoolState {
                address: pumpfun_pool_update.address,
                last_updated: pumpfun_pool_update.last_updated,
                liquidity_usd: pumpfun_pool_update.sol_reserve as f64 / 1_000_000_000_f64
                    * sol_price,
                complete: pumpfun_pool_update.complete,
                mint: pumpfun_pool_update.mint,
                sol_reserve: pumpfun_pool_update.sol_reserve,
                token_reserve: pumpfun_pool_update.token_reserve,
                real_token_reserve: pumpfun_pool_update.real_token_reserve,
                slot: pumpfun_pool_update.slot,
                transaction_index: pumpfun_pool_update.transaction_index,
                is_state_keys_initialized: pumpfun_pool_update.is_account_state_update,
            }))
        }
        PoolUpdateEvent::RaydiumPoolUpdate(raydium_pool_update) => {
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
            Some(PoolState::RaydiumAmmV4PoolState(RaydiumAmmV4PoolState {
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
            }))
        }
        PoolUpdateEvent::PumpSwapPoolUpdate(pump_swap_pool_update) => {
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
            Some(PoolState::PumpSwapPoolState(PumpSwapPoolState {
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
            }))
        }
        PoolUpdateEvent::RaydiumCpmmPoolUpdate(raydium_cpmm_pool_update) => {
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
            Some(PoolState::RaydiumCpmmPoolState(RaydiumCpmmPoolState {
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
            }))
        }
        PoolUpdateEvent::BonkPoolUpdate(bonk_pool_update) => {
            let liquidity_usd = compute_pool_liquidity_usd(
                &bonk_pool_update.base_mint,
                &bonk_pool_update.quote_vault,
                bonk_pool_update.base_reserve,
                bonk_pool_update.quote_reserve,
                sol_price,
            );
            Some(PoolState::BonkPoolState(BonkPoolState {
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
                base_mint: bonk_pool_update.base_mint,
                quote_mint: bonk_pool_update.quote_mint,
                base_vault: bonk_pool_update.base_vault,
                quote_vault: bonk_pool_update.quote_vault,
                creator: bonk_pool_update.creator,
                last_updated: bonk_pool_update.last_updated,
                is_state_keys_initialized: bonk_pool_update.is_account_state_update,
                liquidity_usd,
            }))
        }
        PoolUpdateEvent::RaydiumClmmPoolUpdate(raydium_clmm_pool_update) => {
            let mut pool_state = RadyiumClmmPoolState {
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
                tick_array_state: TickArrayState {
                    start_tick_index: 0,
                    ticks: std::array::from_fn(|i| TickState {
                        tick: i as i32,
                        liquidity_net: 0,
                        liquidity_gross: 0,
                    }),
                    initialized_tick_count: 0,
                },
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
                let start_tick_index = tick_array_update.start_tick_index;
                pool_state.tick_array_state.ticks[..60]
                    .copy_from_slice(&tick_array_update.ticks[..60]);
                pool_state.tick_array_state.start_tick_index = start_tick_index;
                pool_state.tick_array_state.initialized_tick_count =
                    tick_array_update.initialized_tick_count;
            }

            Some(PoolState::RadyiumClmmPoolState(pool_state))
        }
    }
}

pub fn update_pool_state_by_event(
    event: &PoolUpdateEvent,
    existing_state: &mut MutexGuard<PoolState>,
    sol_price: f64,
) {
    match event {
        PoolUpdateEvent::PumpfunPoolUpdate(pumpfun_pool_update) => {
            if let PoolState::PumpfunPoolState(state) = &mut **existing_state {
                // only update last_updated if it's transaction event update that we can collect reserves
                if !pumpfun_pool_update.is_account_state_update {
                    state.last_updated = pumpfun_pool_update.last_updated;
                }
                state.liquidity_usd =
                    pumpfun_pool_update.sol_reserve as f64 / 1_000_000_000_f64 * sol_price;
                state.complete = pumpfun_pool_update.complete;
                state.sol_reserve = pumpfun_pool_update.sol_reserve;
                state.token_reserve = pumpfun_pool_update.token_reserve;
                state.real_token_reserve = pumpfun_pool_update.real_token_reserve;
                state.slot = pumpfun_pool_update.slot;
                state.transaction_index = pumpfun_pool_update.transaction_index;
            }
        }
        PoolUpdateEvent::RaydiumPoolUpdate(raydium_pool_update) => {
            if let PoolState::RaydiumAmmV4PoolState(state) = &mut **existing_state {
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
                if let Some(serum_coin_vault_account) =
                    raydium_pool_update.serum_coin_vault_account
                {
                    if !tokens_equal(&serum_coin_vault_account, &state.serum_coin_vault_account)
                    {
                        state.serum_coin_vault_account = serum_coin_vault_account;
                    }
                }
                if let Some(serum_pc_vault_account) = raydium_pool_update.serum_pc_vault_account
                {
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
        PoolUpdateEvent::PumpSwapPoolUpdate(pump_swap_pool_update) => {
            if let PoolState::PumpSwapPoolState(state) = &mut **existing_state {
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
            }
        }
        PoolUpdateEvent::RaydiumCpmmPoolUpdate(raydium_cpmm_pool_update) => {
            if let PoolState::RaydiumCpmmPoolState(state) = &mut **existing_state {
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
        PoolUpdateEvent::BonkPoolUpdate(bonk_pool_update) => {
            if let PoolState::BonkPoolState(state) = &mut **existing_state {
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
        PoolUpdateEvent::RaydiumClmmPoolUpdate(raydium_clmm_pool_update) => {
            if let PoolState::RadyiumClmmPoolState(state) = &mut **existing_state {
                // only update last_updated if it's transaction event update that we can collect reserves
                if !raydium_clmm_pool_update.is_account_state_update {
                    state.last_updated = raydium_clmm_pool_update.last_updated;
                }
                state.slot = raydium_clmm_pool_update.slot;
                state.transaction_index = raydium_clmm_pool_update.transaction_index;
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
                        && !tokens_equal(
                            &pool_state_part.observation_key,
                            &state.observation_key,
                        )
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
                    let start_tick_index = tick_array_update.start_tick_index;
                    state.tick_array_state.ticks[..60]
                        .copy_from_slice(&tick_array_update.ticks[..60]);
                    state.tick_array_state.start_tick_index = start_tick_index;
                    state.tick_array_state.initialized_tick_count =
                        tick_array_update.initialized_tick_count;
                }
            }
        }
    }
}
