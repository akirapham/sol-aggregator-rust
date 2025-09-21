use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use solana_streamer_sdk::streaming::event_parser::protocols::bonk::parser::BONK_PROGRAM_ID;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BonkPoolState {
    pub slot: u64,
    pub transaction_index: Option<u64>,
    pub address: Pubkey,
    pub status: u8,
    pub total_base_sell: u64,
    pub base_reserve: u64,  // virtual_base
    pub quote_reserve: u64, // virtual_quote
    pub liquidity_usd: f64, // base liquidity, one side
    pub real_base: u64,
    pub real_quote: u64,
    pub quote_protocol_fee: u64,
    pub platform_fee: u64,
    pub global_config: Pubkey,
    pub platform_config: Pubkey,
    pub base_mint: Pubkey,
    pub quote_mint: Pubkey,
    pub base_vault: Pubkey,
    pub quote_vault: Pubkey,
    pub creator: Pubkey,
    pub last_updated: u64,
    pub is_state_keys_initialized: bool,
}

#[derive(Debug, Clone)]
pub struct BonkPoolUpdate {
    pub slot: u64,
    pub transaction_index: Option<u64>,
    pub address: Pubkey,
    pub status: u8,
    pub total_base_sell: u64,
    pub base_reserve: u64,  // virtual_base
    pub quote_reserve: u64, // virtual_quote
    pub real_base: u64,
    pub real_quote: u64,
    pub quote_protocol_fee: u64,
    pub platform_fee: u64,
    pub global_config: Pubkey,
    pub platform_config: Pubkey,
    pub base_mint: Pubkey,
    pub quote_mint: Pubkey,
    pub base_vault: Pubkey,
    pub quote_vault: Pubkey,
    pub creator: Pubkey,
    pub last_updated: u64,
    pub is_account_state_update: bool,
}

impl BonkPoolState {
    pub fn get_program_id() -> Pubkey {
        Pubkey::new_from_array(*BONK_PROGRAM_ID.as_array())
    }

    /// Calculate output amount for PumpFun bonding curve
    pub fn calculate_output_amount(&self, _: &Pubkey, _: u64) -> u64 {
        // let (base_token, quote_token) = (self.base_mint, self.quote_mint);
        // let input_is_base = tokens_equal(input_token, &base_token);
        // let (input_reserve, output_reserve) = if input_is_base {
        //     (self.base_reserve, self.quote_reserve)
        // } else {
        //     (self.quote_reserve, self.base_reserve)
        // };
        // let new_input_reserve = input_reserve as u128 + input_amount as u128;
        // let new_output_reserve =
        //         (input_reserve as u128 * output_reserve as u128 / new_input_reserve) as u64;
        // let output_amount = output_reserve - new_output_reserve;

        // output_amount * 9975 / 10000 // Apply 0.25% fee
        0 // TODO
    }
}
