use crate::constants::WSOL_MINT;
use crate::error::DexAggregatorError;
use crate::error::Result;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use solana_sdk::instruction::Instruction;
use solana_sdk::pubkey::Pubkey;

/// Parse a base58 string to Pubkey
pub fn parse_pubkey(address: &str) -> Result<Pubkey> {
    address
        .parse()
        .map_err(|_| DexAggregatorError::InvalidTokenAddress(address.to_string()).into())
}

/// Calculate minimum output amount with slippage tolerance
pub fn calculate_min_output_amount(expected_output: u64, slippage_bps: u64) -> u64 {
    let min_output =
        Decimal::from(expected_output) * Decimal::from(10000 - slippage_bps) / Decimal::from(10000);
    min_output.to_u64().unwrap()
}

/// Check if two tokens are the same
pub fn tokens_equal(token_a: &Pubkey, token_b: &Pubkey) -> bool {
    token_a == token_b
}

pub fn get_sol_mint() -> Pubkey {
    parse_pubkey(WSOL_MINT).unwrap()
}

/// Iterates through instructions and replaces a specific public key with another in all AccountMeta.
/// This is used to replace the dummy signer from SDK with the actual user wallet.
pub fn replace_key_in_instructions(instructions: &mut [Instruction], from: &Pubkey, to: &Pubkey) {
    for ix in instructions.iter_mut() {
        for account in ix.accounts.iter_mut() {
            if account.pubkey == *from {
                account.pubkey = *to;
            }
        }
    }
}
