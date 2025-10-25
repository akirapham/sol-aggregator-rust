use std::sync::Arc;

use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use solana_streamer_sdk::streaming::event_parser::protocols::meteora_dbc::parser::DBC_PROGRAM_ID;

use crate::pool_data_types::{GetAmmConfig, PoolUpdateEventType};
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbcPoolState {
    pub slot: u64,
    pub transaction_index: Option<u64>,
    pub address: Pubkey,
    /// config key
    pub config: Pubkey,
    /// creator
    pub creator: Pubkey,
    /// base mint
    pub base_mint: Pubkey,
    /// base vault
    pub base_vault: Pubkey,
    /// quote vault
    pub quote_vault: Pubkey,
    /// base reserve
    pub base_reserve: u64,
    /// quote reserve
    pub quote_reserve: u64,
    /// protocol base fee
    pub protocol_base_fee: u64,
    /// protocol quote fee
    pub protocol_quote_fee: u64,
    /// partner base fee
    pub partner_base_fee: u64,
    /// trading quote fee
    pub partner_quote_fee: u64,
    /// current price
    pub sqrt_price: u128,
    /// Activation point
    pub activation_point: u64,
    /// pool type, spl token or token2022
    pub pool_type: u8,
    /// is migrated
    pub is_migrated: u8,
    /// is partner withdraw surplus
    pub is_partner_withdraw_surplus: u8,
    /// is protocol withdraw surplus
    pub is_protocol_withdraw_surplus: u8,
    /// migration progress
    pub migration_progress: u8,
    /// is withdraw leftover
    pub is_withdraw_leftover: u8,
    /// is creator withdraw surplus
    pub is_creator_withdraw_surplus: u8,
    /// migration fee withdraw status, first bit is for partner, second bit is for creator
    pub migration_fee_withdraw_status: u8,
    /// The time curve is finished
    pub finish_curve_timestamp: u64,
    /// creator base fee
    pub creator_base_fee: u64,
    /// creator quote fee
    pub creator_quote_fee: u64,
    pub liquidity_usd: f64,
    pub last_updated: u64,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct DbcPoolUpdate {
    pub slot: u64,
    pub transaction_index: Option<u64>,
    pub address: Pubkey,
    /// config key
    pub config: Pubkey,
    /// creator
    pub creator: Pubkey,
    /// base mint
    pub base_mint: Pubkey,
    /// base vault
    pub base_vault: Pubkey,
    /// quote vault
    pub quote_vault: Pubkey,
    /// base reserve
    pub base_reserve: u64,
    /// quote reserve
    pub quote_reserve: u64,
    /// protocol base fee
    pub protocol_base_fee: u64,
    /// protocol quote fee
    pub protocol_quote_fee: u64,
    /// partner base fee
    pub partner_base_fee: u64,
    /// trading quote fee
    pub partner_quote_fee: u64,
    /// current price
    pub sqrt_price: u128,
    /// Activation point
    pub activation_point: u64,
    /// pool type, spl token or token2022
    pub pool_type: u8,
    /// is migrated
    pub is_migrated: u8,
    /// is partner withdraw surplus
    pub is_partner_withdraw_surplus: u8,
    /// is protocol withdraw surplus
    pub is_protocol_withdraw_surplus: u8,
    /// migration progress
    pub migration_progress: u8,
    /// is withdraw leftover
    pub is_withdraw_leftover: u8,
    /// is creator withdraw surplus
    pub is_creator_withdraw_surplus: u8,
    /// migration fee withdraw status, first bit is for partner, second bit is for creator
    pub migration_fee_withdraw_status: u8,
    /// The time curve is finished
    pub finish_curve_timestamp: u64,
    /// creator base fee
    pub creator_base_fee: u64,
    /// creator quote fee
    pub creator_quote_fee: u64,
    pub is_account_state_update: bool,
    pub pool_update_event_type: PoolUpdateEventType,
    pub additional_event_type: i32,
    pub last_updated: u64,
}

#[allow(dead_code)]
impl DbcPoolState {
    pub fn get_program_id() -> Pubkey {
        DBC_PROGRAM_ID
    }

    /// Calculate output amount for PumpFun bonding curve
    pub fn calculate_output_amount(
        &self,
        _input_token: &Pubkey,
        _input_amount: u64,
        _: Arc<dyn GetAmmConfig>,
    ) -> u64 {
        // let is_buy = tokens_equal(input_token, &get_sol_mint());

        // if is_buy {
        //     get_buy_token_amount_from_sol_amount(
        //         input_amount,
        //         self.base_reserve as u128,
        //         self.quote_reserve as u128,
        //         self.real_base as u128,
        //         self.real_quote as u128,
        //         0,
        //     )
        // } else {
        //     get_sell_sol_amount_from_token_amount(
        //         input_amount,
        //         self.base_reserve as u128,
        //         self.quote_reserve as u128,
        //         self.real_base as u128,
        //         self.real_quote as u128,
        //         0,
        //     )
        // }
        0
    }

    pub fn calculate_token_prices(&self, sol_price: f64) -> (f64, f64) {
        (0.0, 0.0) // TODO
    }
}
