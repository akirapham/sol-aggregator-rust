use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use solana_streamer_sdk::streaming::event_parser::protocols::pumpfun::parser::PUMPFUN_PROGRAM_ID;

use crate::utils::{get_sol_mint, tokens_equal};
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PumpfunPoolState {
    pub slot: u64,
    pub transaction_index: Option<u64>,
    pub address: Pubkey, // bonding curve address
    pub mint: Pubkey,
    pub sol_reserve: u64,
    pub token_reserve: u64,
    pub real_token_reserve: u64,
    pub last_updated: u64,
    pub liquidity_usd: f64,
    pub complete: bool,
    pub is_state_keys_initialized: bool,
}

#[derive(Debug, Clone)]
pub struct PumpfunPoolUpdate {
    pub slot: u64,
    pub transaction_index: Option<u64>,
    pub address: Pubkey,
    pub mint: Pubkey,
    pub token_reserve: u64,
    pub sol_reserve: u64,
    pub real_token_reserve: u64,
    pub last_updated: u64,
    pub complete: bool,
    pub is_account_state_update: bool,
}


impl PumpfunPoolState {
    pub fn get_program_id() -> Pubkey {
        Pubkey::new_from_array(*PUMPFUN_PROGRAM_ID.as_array())
    }

    /// Calculate output amount for PumpFun bonding curve
    pub fn calculate_output_amount(&self, input_token: &Pubkey, input_amount: u64) -> u64 {
        let is_buy = tokens_equal(input_token, &get_sol_mint());
        let (token_reserve, sol_reserve) =
            (self.token_reserve, self.sol_reserve);
        let output_amount = if is_buy {
            let new_sol_reserve = sol_reserve + input_amount;
            let new_token_reserve =
                (token_reserve as u128 * sol_reserve as u128 / new_sol_reserve as u128) as u64;
            token_reserve - new_token_reserve
        } else {
            let new_token_reserve = token_reserve + input_amount;
            let new_sol_reserve =
                (sol_reserve as u128 * token_reserve as u128 / new_token_reserve as u128) as u64;
            sol_reserve - new_sol_reserve
        };

        output_amount * 99 / 100 // Apply 1% fee
    }
}
