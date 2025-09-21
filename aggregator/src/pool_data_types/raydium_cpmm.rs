use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use solana_streamer_sdk::streaming::event_parser::protocols::raydium_cpmm::parser::RAYDIUM_CPMM_PROGRAM_ID;

use crate::utils::tokens_equal;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaydiumCpmmPoolState {
    pub slot: u64,
    pub transaction_index: Option<u64>,
    pub status: u8,
    pub address: Pubkey,
    pub token0: Pubkey,
    pub token1: Pubkey,
    pub token0_vault: Pubkey,
    pub token1_vault: Pubkey,
    pub token0_reserve: u64,
    pub token1_reserve: u64,
    pub amm_config: Pubkey,
    pub observation_state: Pubkey,
    pub last_updated: u64,
    pub liquidity_usd: f64,
    pub is_state_keys_initialized: bool,
}

#[derive(Debug, Clone)]
pub struct RaydiumCpmmPoolUpdate {
    pub slot: u64,
    pub transaction_index: Option<u64>,
    pub status: Option<u8>,
    pub address: Pubkey,
    pub token0: Pubkey,
    pub token1: Pubkey,
    pub token0_vault: Pubkey,
    pub token1_vault: Pubkey,
    pub token0_reserve: u64,
    pub token1_reserve: u64,
    pub amm_config: Pubkey,
    pub observation_state: Pubkey,
    pub last_updated: u64,
    pub is_account_state_update: bool,
}

impl RaydiumCpmmPoolState {
    pub fn get_program_id() -> Pubkey {
        Pubkey::new_from_array(*RAYDIUM_CPMM_PROGRAM_ID.as_array())
    }

    /// Calculate output amount for PumpFun bonding curve
    pub fn calculate_output_amount(&self, input_token: &Pubkey, input_amount: u64) -> u64 {
        let (base_token, _) = (self.token0, self.token1);
        let input_is_base = tokens_equal(input_token, &base_token);
        let (input_reserve, output_reserve) = if input_is_base {
            (self.token0_reserve, self.token1_reserve)
        } else {
            (self.token1_reserve, self.token0_reserve)
        };
        let new_input_reserve = input_reserve as u128 + input_amount as u128;
        let new_output_reserve =
            (input_reserve as u128 * output_reserve as u128 / new_input_reserve) as u64;
        let output_amount = output_reserve - new_output_reserve;

        output_amount * 9975 / 10000 // Apply 0.25% fee
    }
}
