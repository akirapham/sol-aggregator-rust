use std::{collections::HashMap, sync::Arc};

use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use solana_streamer_sdk::streaming::event_parser::protocols::orca_whirlpools::{
    parser::ORCA_WHIRLPOOL_PROGRAM_ID, types::TickArrayState,
};

use crate::pool_data_types::{GetAmmConfig, PoolUpdateEventType};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WhirlpoolPoolState {
    pub slot: u64,
    pub transaction_index: Option<u64>,
    pub address: Pubkey,
    pub whirlpool_config: Pubkey,
    pub tick_spacing: u16,
    pub tick_spacing_seed: [u8; 2],
    pub fee_rate: u16,
    pub protocol_fee_rate: u16,
    pub liquidity: u128,
    pub liquidity_usd: f64,
    pub sqrt_price: u128,
    pub tick_current_index: i32,
    pub token_mint_a: Pubkey,
    pub token_vault_a: Pubkey,
    pub token_mint_b: Pubkey,
    pub token_vault_b: Pubkey,

    #[serde(skip)]
    pub tick_array_state: HashMap<i32, TickArrayState>,
    pub last_updated: u64, // Unix timestamp
    pub token_a_reserve: u64,
    pub token_b_reserve: u64,
    pub is_state_keys_initialized: bool,
}

#[derive(Clone, Debug)]
pub struct WhirlpoolPoolStatePart {
    pub whirlpool_config: Pubkey,
    pub tick_spacing: u16,
    pub tick_spacing_seed: [u8; 2],
    pub fee_rate: u16,
    pub protocol_fee_rate: u16,
    pub liquidity: u128,
    pub sqrt_price: u128,
    pub tick_current_index: i32,
    pub token_mint_a: Pubkey,
    pub token_vault_a: Pubkey,
    pub token_mint_b: Pubkey,
    pub token_vault_b: Pubkey,
}

#[derive(Clone, Debug)]
pub struct WhirlpoolPoolReservePart {
    pub token_a_reserve: u64,
    pub token_b_reserve: u64,
}

#[derive(Debug, Clone)]
pub struct WhirlpoolPoolUpdate {
    pub slot: u64,
    pub transaction_index: Option<u64>,
    pub address: Pubkey,
    pub pool_state_part: Option<WhirlpoolPoolStatePart>,
    pub reserve_part: Option<WhirlpoolPoolReservePart>,
    pub tick_array_state: Option<TickArrayState>,
    pub last_updated: u64,
    pub is_account_state_update: bool,
    pub pool_update_event_type: PoolUpdateEventType,
    pub additional_event_type: i32, // for tick array index tracking, 0 for others
}

impl WhirlpoolPoolState {
    pub fn get_program_id() -> Pubkey {
        Pubkey::new_from_array(*ORCA_WHIRLPOOL_PROGRAM_ID.as_array())
    }

    /// Calculate output amount for PumpFun bonding curve
    pub async fn calculate_output_amount(
        &self,
        input_token: &Pubkey,
        input_amount: u64,
        amm_config_fetcher: Arc<dyn GetAmmConfig>,
    ) -> u64 {
        // let (token0, _) = (self.token_mint0, self.token_mint1);
        // let input_is_token0 = tokens_equal(input_token, &token0);
        // let sqrt_price_limit_x64 = if input_is_token0 {
        //     MIN_SQRT_PRICE_X64 + 1
        // } else {
        //     MAX_SQRT_PRICE_X64 - 1
        // };

        // // dont take transfer tax into account for now, users should account for it un their slippage
        // let real_input_amount = input_amount;
        // self.get_output_amount(
        //     real_input_amount,
        //     input_token,
        //     sqrt_price_limit_x64,
        //     amm_config_fetcher,
        // )
        // .await
        0
    }

    async fn get_output_amount(
        &self,
        input_amount: u64,
        input_token: &Pubkey,
        _sqrt_price_limit_x64: u128,
        amm_config_fetcher: Arc<dyn GetAmmConfig>,
    ) -> u64 {
        // create pool info
        // let pool_info = ComputeClmmPoolInfo::new(
        //     self.address,
        //     Self::get_program_id(),
        //     self,
        //     self.tick_array_bitmap_extension.as_ref(),
        //     amm_config_fetcher
        //         .get_raydium_clmm_amm_config(&self.amm_config)
        //         .await
        //         .unwrap_or(None),
        // );
        // let result = PoolUtils::get_output_amount_and_remain_accounts(
        //     &pool_info,
        //     &self.tick_array_state,
        //     input_token,
        //     rug::Integer::from(input_amount),
        // );
        // match result {
        //     Ok(output) => output.expected_amount_out.abs().to_u64().unwrap_or(0),
        //     Err(_) => 0,
        // }
        0
    }

    pub fn calculate_token_prices(
        &self,
        sol_price: f64,
        base_decimals: u8,
        quote_decimals: u8,
    ) -> (f64, f64) {
        // // For concentrated liquidity (CLMM), price is derived from sqrt_price_x64
        // // sqrt_price_x64 is in Q64 format (fixed point with 64 fractional bits)
        // // price = (sqrt_price_x64 / 2^64)^2 * (10^(quote_decimals - base_decimals))

        // if self.sqrt_price_x64 == 0 {
        //     return (0.0, 0.0);
        // }

        // let token0_str = self.token_mint0.to_string();
        // let token1_str = self.token_mint1.to_string();

        // let is_token0_a_base_token = is_base_token(&token0_str);
        // let is_token1_a_base_token = is_base_token(&token1_str);

        // // Convert sqrt_price_x64 from Q64 to float (Q64 == 2^64)
        // let q64 = 2f64.powi(64);
        // let sqrt_price = self.sqrt_price_x64 as f64 / q64;

        // // Price = sqrt_price^2 * (10^(quote_decimals - base_decimals))
        // let decimal_scale = 10_f64.powi(quote_decimals as i32 - base_decimals as i32);
        // let price_ratio = sqrt_price * sqrt_price * decimal_scale;

        // // If token1 is a base token (like USDC, SOL), use its price
        // if is_token1_a_base_token {
        //     let token1_price = if token1_str == "So11111111111111111111111111111111111111112" {
        //         sol_price // SOL
        //     } else {
        //         1.0 // Assume USDC/USDT are ~$1
        //     };

        //     let token0_price = price_ratio * token1_price;
        //     (token0_price, token1_price)
        // } else if is_token0_a_base_token {
        //     // If token0 is a base token, use its price
        //     let token0_price = if token0_str == "So11111111111111111111111111111111111111112" {
        //         sol_price // SOL
        //     } else {
        //         1.0 // Assume USDC/USDT are ~$1
        //     };

        //     let token1_price = token0_price / price_ratio;
        //     (token0_price, token1_price)
        // } else {
        //     // Neither token is a base token, assume relative pricing
        //     (price_ratio, 1.0)
        // }
        (0.0, 0.0)
    }
}
