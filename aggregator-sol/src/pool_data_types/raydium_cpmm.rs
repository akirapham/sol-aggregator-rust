use std::sync::Arc;

use crate::{
    constants::is_base_token,
    pool_data_types::{GetAmmConfig, PoolUpdateEventType},
    utils::tokens_equal,
};
use borsh::BorshDeserialize;
use serde::{Deserialize, Serialize};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use solana_streamer_sdk::streaming::event_parser::protocols::raydium_cpmm::parser::RAYDIUM_CPMM_PROGRAM_ID;
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

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, BorshDeserialize)]
pub struct RaydiumCpmmAmmConfig {
    pub bump: u8,
    pub disable_create_pool: bool,
    pub index: u16,
    pub trade_fee_rate: u64,
    pub protocol_fee_rate: u64,
    pub fund_fee_rate: u64,
    pub create_pool_fee: u64,
    pub protocol_owner: Pubkey,
    pub fund_owner: Pubkey,
    pub padding: [u64; 16],
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
    pub pool_update_event_type: PoolUpdateEventType,
    pub additional_event_type: i32, // for tick array index tracking, 0 for others
}

#[allow(dead_code)]
impl RaydiumCpmmPoolState {
    pub fn get_program_id() -> Pubkey {
        Pubkey::new_from_array(*RAYDIUM_CPMM_PROGRAM_ID.as_array())
    }

    /// Calculate output amount for PumpFun bonding curve
    pub fn calculate_output_amount(
        &self,
        input_token: &Pubkey,
        input_amount: u64,
        _: Arc<dyn GetAmmConfig>,
    ) -> u64 {
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

    pub fn calculate_token_prices(
        &self,
        sol_price: f64,
        base_decimals: u8,
        quote_decimals: u8,
    ) -> (f64, f64) {
        if self.token1_reserve == 0 || self.token0_reserve == 0 {
            return (0.0, 0.0);
        }

        let token0_str = self.token0.to_string();
        let token1_str = self.token1.to_string();

        let is_token0_a_base_token = is_base_token(&token0_str);
        let is_token1_a_base_token = is_base_token(&token1_str);

        let decimal_scale = 10_f64.powi(base_decimals as i32 - quote_decimals as i32);

        // If token1 is a base token (like USDC, SOL), use its price
        if is_token1_a_base_token {
            let token1_price = if token1_str == "So11111111111111111111111111111111111111112" {
                sol_price // SOL
            } else {
                1.0 // Assume USDC/USDT are ~$1
            };

            let token0_price = (self.token1_reserve as f64 / self.token0_reserve as f64)
                * decimal_scale
                * token1_price;
            (token0_price, token1_price)
        } else if is_token0_a_base_token {
            // If token0 is a base token, use its price
            let token0_price = if token0_str == "So11111111111111111111111111111111111111112" {
                sol_price // SOL
            } else {
                1.0 // Assume USDC/USDT are ~$1
            };

            let token1_price = (self.token0_reserve as f64 / self.token1_reserve as f64)
                * (1.0 / decimal_scale)
                * token0_price;
            (token0_price, token1_price)
        } else {
            // Neither token is a base token, assume relative pricing
            let token0_price =
                (self.token1_reserve as f64 / self.token0_reserve as f64) * decimal_scale * 1.0;
            (token0_price, 1.0)
        }
    }
}
