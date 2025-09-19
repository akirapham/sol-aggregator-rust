use solana_sdk::pubkey::Pubkey;

pub trait DexInterface {
    /// Get the program ID of the DEX
    fn get_program_id() -> Pubkey;

    fn calculate_output_amount(&self, input_token: &Pubkey, input_amount: u64) -> u64;
}
