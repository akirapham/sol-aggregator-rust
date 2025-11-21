use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sol_trade_sdk::utils::calc::pumpfun::{
    get_buy_token_amount_from_sol_amount, get_sell_sol_amount_from_token_amount,
};
use solana_sdk::pubkey::Pubkey;
use solana_streamer_sdk::streaming::event_parser::protocols::pumpfun::parser::PUMPFUN_PROGRAM_ID;
use solana_client::nonblocking::rpc_client::RpcClient;
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
    pub additional_event_type: i32, // for tick array index tracking, 0 for others
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
        _rpc_client: &RpcClient,
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

    pub fn calculate_token_prices(
        &self,
        sol_price: f64,
        base_decimals: u8,
        quote_decimals: u8,
    ) -> (f64, f64) {
        // For Pumpfun: mint price in USD, sol price in USD
        // Price ratio needs to account for decimal scaling:
        // token_price_usd = (sol_reserve / token_reserve) * (10^base_decimals / 10^quote_decimals) * sol_price_usd

        if self.token_reserve == 0 {
            return (0.0, sol_price);
        }

        let decimal_scale = 10_f64.powi(base_decimals as i32 - quote_decimals as i32);
        let token_price =
            (self.sol_reserve as f64 / self.token_reserve as f64) * decimal_scale * sol_price;

        (token_price, sol_price)
    }
}
