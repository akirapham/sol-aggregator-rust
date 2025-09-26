use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sol_trade_sdk::utils::calc::pumpfun::{
    get_buy_token_amount_from_sol_amount, get_sell_sol_amount_from_token_amount,
};
use solana_sdk::pubkey::Pubkey;
use solana_streamer_sdk::streaming::event_parser::protocols::pumpfun::parser::PUMPFUN_PROGRAM_ID;

use crate::{
    pool_data_types::{GetAmmConfig, PoolUpdateEventType},
    utils::{get_sol_mint, tokens_equal},
};
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
    pub pool_update_event_type: PoolUpdateEventType,
}

#[allow(dead_code)]
impl PumpfunPoolState {
    pub fn get_program_id() -> Pubkey {
        Pubkey::new_from_array(*PUMPFUN_PROGRAM_ID.as_array())
    }

    /// Calculate output amount for PumpFun bonding curve
    pub fn calculate_output_amount(
        &self,
        input_token: &Pubkey,
        input_amount: u64,
        _: Arc<dyn GetAmmConfig>,
    ) -> u64 {
        let is_buy = tokens_equal(input_token, &get_sol_mint());
        if is_buy {
            get_buy_token_amount_from_sol_amount(
                self.token_reserve as u128,
                self.sol_reserve as u128,
                self.real_token_reserve as u128,
                Default::default(),
                input_amount,
            )
        } else {
            get_sell_sol_amount_from_token_amount(
                self.token_reserve as u128,
                self.sol_reserve as u128,
                Default::default(),
                input_amount,
            )
        }
    }
}
