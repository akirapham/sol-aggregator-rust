use crate::constants::WSOL_MINT;
use crate::error::Result;
use crate::{PoolState, PoolUpdateEvent};
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

pub fn pool_update_event_to_pool_state(event: &PoolUpdateEvent) -> PoolState {
    match event {
        PoolUpdateEvent::PumpfunPoolUpdate(pumpfun_pool_update) => PoolState {
            dex: crate::DexType::PumpFun,
            address: pumpfun_pool_update.pool_address,
            token_a: pumpfun_pool_update.mint,
            token_b: get_sol_mint(),
            reserve_a: pumpfun_pool_update.base_reserve,
            reserve_b: pumpfun_pool_update.quote_reserve,
            fee_rate: 0.01,
            last_updated: pumpfun_pool_update.last_updated,
            liquidity_usd: 0.0,
            tick_current: None,
            tick_spacing: None,
            sqrt_price: None,
            liquidity: None,
            amp_factor: None,
            bonding_curve_reserve: Some(pumpfun_pool_update.real_base_reserve),
            complete: Some(false),
        },
        PoolUpdateEvent::RaydiumPoolUpdate(raydium_pool_update) => todo!(),
    }
}
