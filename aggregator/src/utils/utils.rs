use crate::constants::WSOL_MINT;
use crate::error::DexAggregatorError;
use crate::error::Result;
use crate::types::PoolUpdateEvent;
use crate::types::Token;
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
        DexAggregatorError::PriceCalculationError(
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
        .map_err(|_| DexAggregatorError::InvalidTokenAddress(address.to_string()))
}

/// Calculate minimum output amount with slippage tolerance
pub fn calculate_min_output_amount(
    expected_output: u64,
    slippage_tolerance: Decimal,
) -> Result<u64> {
    let slippage_factor = Decimal::ONE - slippage_tolerance;
    let min_output = Decimal::from(expected_output) * slippage_factor;
    min_output.to_u64().ok_or_else(|| {
        DexAggregatorError::PriceCalculationError("Invalid slippage calculation".to_string())
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
