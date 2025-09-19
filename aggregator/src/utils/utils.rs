use crate::constants::WSOL_MINT;
use crate::error::DexAggregatorError;
use crate::error::Result;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use solana_sdk::pubkey::Pubkey;

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

pub fn get_sol_mint() -> Pubkey {
    parse_pubkey(WSOL_MINT).unwrap()
}
