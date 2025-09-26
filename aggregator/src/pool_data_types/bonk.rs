use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sol_trade_sdk::utils::calc::bonk::{
    get_buy_token_amount_from_sol_amount, get_sell_sol_amount_from_token_amount,
};
use solana_sdk::pubkey::Pubkey;
use solana_streamer_sdk::streaming::event_parser::protocols::bonk::parser::BONK_PROGRAM_ID;

use crate::{
    pool_data_types::{GetAmmConfig, PoolUpdateEventType},
    utils::{get_sol_mint, tokens_equal},
};
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
    pub pool_update_event_type: PoolUpdateEventType,
    pub additional_event_type: i32, // for tick array index tracking, 0 for others
}

#[allow(dead_code)]
impl BonkPoolState {
    pub fn get_program_id() -> Pubkey {
        Pubkey::new_from_array(*BONK_PROGRAM_ID.as_array())
    }

    /// Calculate output amount for PumpFun bonding curve
    pub fn calculate_output_amount(
        &self,
        input_token: &Pubkey,
        input_amount: u64,
        _: Arc<dyn GetAmmConfig>,
    ) -> u64 {
        let is_buy = tokens_equal(&input_token, &get_sol_mint());

        if is_buy {
            get_buy_token_amount_from_sol_amount(
                input_amount,
                self.base_reserve as u128,
                self.quote_reserve as u128,
                self.real_base as u128,
                self.real_quote as u128,
                0,
            )
        } else {
            get_sell_sol_amount_from_token_amount(
                input_amount,
                self.base_reserve as u128,
                self.quote_reserve as u128,
                self.real_base as u128,
                self.real_quote as u128,
                0,
            )
        }
    }
}
