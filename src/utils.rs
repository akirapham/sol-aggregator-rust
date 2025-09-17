use crate::constants::WSOL_MINT;
use crate::error::Result;
use crate::pool_data_types::{
    PoolState, PumpSwapPoolState, PumpfunPoolState, RaydiumAmmV4PoolState,
};
use crate::PoolUpdateEvent;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use solana_sdk::pubkey::Pubkey;

/// Convert lamports to token amount based on decimals
pub fn lamports_to_amount(lamports: u64, decimals: u8) -> Decimal {
    let divisor = 10_u64.pow(decimals as u32);
    Decimal::from(lamports) / Decimal::from(divisor)
}

/// Convert token amount to lamports based on decimals
pub fn amount_to_lamports(amount: Decimal, decimals: u8) -> Result<u64> {
    let multiplier = 10_u64.pow(decimals as u32);
    let lamports = amount * Decimal::from(multiplier);
    lamports.to_u64().ok_or_else(|| {
        crate::error::DexAggregatorError::PriceCalculationError(
            "Amount too large to convert to lamports".to_string(),
        )
    })
}

/// Calculate price impact for a swap
pub fn calculate_price_impact(
    input_amount: u64,
    output_amount: u64,
    market_price: Decimal,
) -> Result<Decimal> {
    let expected_output = Decimal::from(input_amount) * market_price;
    let actual_output = Decimal::from(output_amount);

    if expected_output.is_zero() {
        return Ok(Decimal::ZERO);
    }

    let impact = (expected_output - actual_output) / expected_output;
    Ok(impact.abs())
}

/// Calculate slippage percentage
pub fn calculate_slippage(expected_amount: u64, actual_amount: u64) -> Decimal {
    if expected_amount == 0 {
        return Decimal::ZERO;
    }

    let expected = Decimal::from(expected_amount);
    let actual = Decimal::from(actual_amount);

    ((expected - actual) / expected).abs()
}

/// Parse a base58 string to Pubkey
pub fn parse_pubkey(address: &str) -> Result<Pubkey> {
    address
        .parse()
        .map_err(|_| crate::error::DexAggregatorError::InvalidTokenAddress(address.to_string()))
}

/// Convert a Pubkey to base58 string
pub fn pubkey_to_string(pubkey: &Pubkey) -> String {
    bs58::encode(pubkey.as_ref()).into_string()
}

/// Calculate minimum output amount with slippage tolerance
pub fn calculate_min_output_amount(
    expected_output: u64,
    slippage_tolerance: Decimal,
) -> Result<u64> {
    let slippage_factor = Decimal::ONE - slippage_tolerance;
    let min_output = Decimal::from(expected_output) * slippage_factor;
    min_output.to_u64().ok_or_else(|| {
        crate::error::DexAggregatorError::PriceCalculationError(
            "Invalid slippage calculation".to_string(),
        )
    })
}

/// Check if two tokens are the same
pub fn tokens_equal(token_a: &Pubkey, token_b: &Pubkey) -> bool {
    token_a == token_b
}

/// Calculate the fee for a given amount and fee rate
pub fn calculate_fee(amount: u64, fee_rate: f64) -> u64 {
    let fee = amount as f64 * fee_rate as f64;
    fee as u64
}

pub fn get_sol_mint() -> Pubkey {
    parse_pubkey(WSOL_MINT).unwrap()
}

pub fn use_input_or_existing(input_key: &Pubkey, &existing: &Pubkey) -> Pubkey {
    if *input_key != Pubkey::default() {
        input_key.clone()
    } else {
        existing.clone()
    }
}

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
    }
}
