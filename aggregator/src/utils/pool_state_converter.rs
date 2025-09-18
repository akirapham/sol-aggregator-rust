use solana_sdk::pubkey::Pubkey;

use crate::pool_data_types::PoolState;
use crate::types::PoolUpdateEvent;
use crate::utils::use_input_or_existing;
use crate::pool_data_types::{ RaydiumCpmmPoolState, RaydiumAmmV4PoolState, PumpSwapPoolState, BonkPoolState, PumpfunPoolState };

// TODO: refactor this, we can update the input event directly instead of creating a new struct
pub fn pool_update_event_to_pool_state(
    event: &PoolUpdateEvent,
    existing_state: Option<PoolState>,
) -> PoolState {
    match event {
        PoolUpdateEvent::PumpfunPoolUpdate(pumpfun_pool_update) => {
                        PoolState::PumpfunPoolState(PumpfunPoolState {
                            address: pumpfun_pool_update.address,
                            last_updated: pumpfun_pool_update.last_updated,
                            liquidity_usd: 0.0,
                            complete: pumpfun_pool_update.complete,
                            mint: pumpfun_pool_update.mint,
                            sol_reserve: pumpfun_pool_update.sol_reserve,
                            token_reserve: pumpfun_pool_update.token_reserve,
                            real_token_reserve: pumpfun_pool_update.real_token_reserve,
                            slot: pumpfun_pool_update.slot,
                            transaction_index: pumpfun_pool_update.transaction_index,
                        })
            }
        PoolUpdateEvent::RaydiumPoolUpdate(raydium_pool_update) => {
                let existing_raydium_state = match existing_state {
                    Some(PoolState::RaydiumAmmV4PoolState(state)) => Some(state),
                    _ => None,
                };
                let (
                    existing_serum_program,
                    existing_serum_market,
                    existing_serum_bids,
                    existing_serum_asks,
                    existing_serum_event_queue,
                    existing_serum_coin_vault_account,
                    existing_serum_pc_vault_account,
                    existing_serum_vault_signer,
                ) = if let Some(state) = existing_raydium_state {
                    (
                        state.serum_program,
                        state.serum_market,
                        state.serum_bids,
                        state.serum_asks,
                        state.serum_event_queue,
                        state.serum_coin_vault_account,
                        state.serum_pc_vault_account,
                        state.serum_vault_signer,
                    )
                } else {
                    (
                        Pubkey::default(),
                        Pubkey::default(),
                        Pubkey::default(),
                        Pubkey::default(),
                        Pubkey::default(),
                        Pubkey::default(),
                        Pubkey::default(),
                        Pubkey::default(),
                    )
                };
                PoolState::RaydiumAmmV4PoolState(RaydiumAmmV4PoolState {
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
                    serum_program: use_input_or_existing(
                        &raydium_pool_update.serum_program,
                        &existing_serum_program,
                    ),
                    serum_market: use_input_or_existing(
                        &raydium_pool_update.serum_market,
                        &existing_serum_market,
                    ),
                    serum_bids: use_input_or_existing(
                        &raydium_pool_update.serum_bids,
                        &existing_serum_bids,
                    ),
                    serum_asks: use_input_or_existing(
                        &raydium_pool_update.serum_asks,
                        &existing_serum_asks,
                    ),
                    serum_event_queue: use_input_or_existing(
                        &raydium_pool_update.serum_event_queue,
                        &existing_serum_event_queue,
                    ),
                    serum_coin_vault_account: use_input_or_existing(
                        &raydium_pool_update.serum_coin_vault_account,
                        &existing_serum_coin_vault_account,
                    ),
                    serum_pc_vault_account: use_input_or_existing(
                        &raydium_pool_update.serum_pc_vault_account,
                        &existing_serum_pc_vault_account,
                    ),
                    serum_vault_signer: use_input_or_existing(
                        &raydium_pool_update.serum_vault_signer,
                        &existing_serum_vault_signer,
                    ),
                    last_updated: raydium_pool_update.last_updated,
                    base_reserve: raydium_pool_update.base_reserve,
                    quote_reserve: raydium_pool_update.quote_reserve,
                })
            }
        PoolUpdateEvent::PumpSwapPoolUpdate(pump_swap_pool_update) => {
                let existing_pump_swap_state = match existing_state {
                    Some(PoolState::PumpSwapPoolState(state)) => Some(state),
                    _ => None,
                };
                let (existing_index, existing_creator) = if let Some(state) = existing_pump_swap_state {
                    (state.index, state.creator)
                } else {
                    (0, None)
                };
                PoolState::PumpSwapPoolState(PumpSwapPoolState {
                    address: pump_swap_pool_update.address,
                    index: pump_swap_pool_update.index.unwrap_or(existing_index),
                    creator: if existing_creator.is_some() {
                        existing_creator
                    } else {
                        pump_swap_pool_update.creator
                    },
                    last_updated: pump_swap_pool_update.last_updated,
                    base_reserve: pump_swap_pool_update.base_reserve,
                    quote_reserve: pump_swap_pool_update.quote_reserve,
                    slot: pump_swap_pool_update.slot,
                    transaction_index: pump_swap_pool_update.transaction_index,
                    base_mint: pump_swap_pool_update.base_mint,
                    quote_mint: pump_swap_pool_update.quote_mint,
                    pool_base_token_account: pump_swap_pool_update.pool_base_token_account,
                    pool_quote_token_account: pump_swap_pool_update.pool_quote_token_account,
                })
            }
        PoolUpdateEvent::RaydiumCpmmPoolUpdate(raydium_cpmm_pool_update) => {
                let existing_raydium_state = match existing_state {
                    Some(PoolState::RaydiumCpmmPoolState(state)) => Some(state),
                    _ => None,
                };
                let is_reserve_updated = raydium_cpmm_pool_update.token0_reserve != 0
                    && raydium_cpmm_pool_update.token1_reserve != 0;
                let (token0_reserve, token1_reserve) = if is_reserve_updated {
                    (
                        raydium_cpmm_pool_update.token0_reserve,
                        raydium_cpmm_pool_update.token1_reserve,
                    )
                } else if let Some(ref state) = existing_raydium_state {
                    (state.token0_reserve, state.token1_reserve)
                } else {
                    (0, 0)
                };
                let updated_status = if let Some(status) = raydium_cpmm_pool_update.status.clone() {
                    status
                } else if let Some(ref state) = existing_raydium_state {
                    state.status
                } else {
                    0
                };
                let (existing_amm_config, existing_observation_state) =
                    if let Some(ref state) = existing_raydium_state {
                        (state.amm_config, state.observation_state)
                    } else {
                        (Pubkey::default(), Pubkey::default())
                    };
                PoolState::RaydiumCpmmPoolState(RaydiumCpmmPoolState {
                    slot: raydium_cpmm_pool_update.slot,
                    transaction_index: raydium_cpmm_pool_update.transaction_index,
                    address: raydium_cpmm_pool_update.address,
                    status: updated_status,
                    token0: raydium_cpmm_pool_update.token0,
                    token1: raydium_cpmm_pool_update.token1,
                    token0_vault: raydium_cpmm_pool_update.token0_vault,
                    token1_vault: raydium_cpmm_pool_update.token1_vault,
                    token0_reserve,
                    token1_reserve,
                    amm_config: use_input_or_existing(
                        &raydium_cpmm_pool_update.amm_config,
                        &existing_amm_config,
                    ),
                    observation_state: use_input_or_existing(
                        &raydium_cpmm_pool_update.observation_state,
                        &existing_observation_state,
                    ),
                    last_updated: raydium_cpmm_pool_update.last_updated,
                    liquidity_usd: 0.0,
                })
            }
        PoolUpdateEvent::BonkPoolUpdate(bonk_pool_update) => {
                PoolState::BonkPoolState(BonkPoolState {
                    slot: bonk_pool_update.slot,
                    transaction_index: bonk_pool_update.transaction_index,
                    address: bonk_pool_update.address,
                    status: bonk_pool_update.status,
                    base_decimals: bonk_pool_update.base_decimals,
                    quote_decimals: bonk_pool_update.quote_decimals,
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
                })
            }
        PoolUpdateEvent::RaydiumClmmPoolUpdate(raydium_clmm_pool_update) => todo!(),
    }
}
