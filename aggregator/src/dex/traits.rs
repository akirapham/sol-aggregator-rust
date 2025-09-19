use solana_sdk::pubkey::Pubkey;

use crate::pool_data_types::DexType;

pub trait DexInterface {
    /// Get the program ID of the DEX
    fn get_program_id() -> Pubkey;

    fn calculate_output_amount(&self, input_token: &Pubkey, input_amount: u64) -> u64;

    fn get_pool_address(&self) -> Pubkey;
    fn get_dex(&self) -> DexType;
}

