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
    slippage_bps: u64,
) -> u64 {
    let min_output = Decimal::from(expected_output) * Decimal::from(10000 - slippage_bps) / Decimal::from(10000);
    min_output.to_u64().unwrap()
}

/// Check if two tokens are the same
pub fn tokens_equal(token_a: &Pubkey, token_b: &Pubkey) -> bool {
    token_a == token_b
}

pub fn get_sol_mint() -> Pubkey {
    parse_pubkey(WSOL_MINT).unwrap()
}
