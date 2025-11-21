use std::{collections::HashMap, sync::Arc};

use crate::{
    constants::is_base_token,
    pool_data_types::{
        clmm::{pool::PoolUtils, tpe::ComputeClmmPoolInfo},
        GetAmmConfig, PoolUpdateEventType,
    },
    utils::tokens_equal,
};
use borsh::BorshDeserialize;
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use solana_streamer_sdk::streaming::event_parser::protocols::raydium_clmm::parser::RAYDIUM_CLMM_PROGRAM_ID;
use solana_client::nonblocking::rpc_client::RpcClient;
const MIN_SQRT_PRICE_X64: u128 = 4295048016;
const MAX_SQRT_PRICE_X64: u128 = 79226673521066979257578248091;
#[derive(Clone, Debug, Copy, Default)]
#[allow(dead_code)]
pub struct TickState {
    pub tick: i32,
    pub liquidity_net: i128,
    pub liquidity_gross: u128,
}

#[allow(unused)]
#[derive(Clone, Debug)]
pub struct TickArrayState {
    pub start_tick_index: i32,
    pub ticks: [TickState; 60],
    pub initialized_tick_count: u8,
}

impl Default for TickArrayState {
    fn default() -> Self {
        Self {
            start_tick_index: 0,
            ticks: [TickState::default(); 60],
            initialized_tick_count: 0,
        }
    }
}

const EXTENSION_TICKARRAY_BITMAP_SIZE: usize = 14;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TickArrayBitmapExtension {
    /// Packed initialized tick array state for start_tick_index is positive
    pub positive_tick_array_bitmap: [[u64; 8]; EXTENSION_TICKARRAY_BITMAP_SIZE],
    /// Packed initialized tick array state for start_tick_index is negitive
    pub negative_tick_array_bitmap: [[u64; 8]; EXTENSION_TICKARRAY_BITMAP_SIZE],
}

impl Default for TickArrayBitmapExtension {
    fn default() -> Self {
        Self {
            positive_tick_array_bitmap: core::array::from_fn(|_| [0u64; 8]),
            negative_tick_array_bitmap: core::array::from_fn(|_| [0u64; 8]),
        }
    }
}

#[derive(Clone, Debug)]
pub struct RaydiumClmmPoolStatePart {
    pub amm_config: Pubkey,
    pub token_mint0: Pubkey,
    pub token_mint1: Pubkey,
    pub token_vault0: Pubkey,
    pub token_vault1: Pubkey,
    pub observation_key: Pubkey,
    pub tick_spacing: u16,
    pub liquidity: u128,
    pub sqrt_price_x64: u128,
    pub tick_current_index: i32,
    pub status: u8,
    pub tick_array_bitmap: [u64; 16],
    pub open_time: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, BorshDeserialize)]
pub struct RaydiumClmmAmmConfig {
    pub bump: u8,
    pub index: u16,
    pub owner: Pubkey,
    pub protocol_fee_rate: u32,
    pub trade_fee_rate: u32,
    pub tick_spacing: u16,
    pub fund_fee_rate: u32,
    pub padding_u32: u32,
    pub fund_owner: Pubkey,
    pub padding: [u64; 3],
}

#[derive(Clone, Debug)]
pub struct RaydiumClmmPoolReservePart {
    pub token0_reserve: u64,
    pub token1_reserve: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaydiumClmmPoolState {
    pub slot: u64,
    pub transaction_index: Option<u64>,
    pub address: Pubkey,
    pub amm_config: Pubkey,
    pub token_mint0: Pubkey,
    pub token_mint1: Pubkey,
    pub token_vault0: Pubkey,
    pub token_vault1: Pubkey,
    pub observation_key: Pubkey,
    pub tick_spacing: u16,
    pub liquidity: u128,
    pub liquidity_usd: f64,
    pub sqrt_price_x64: u128,
    pub tick_current_index: i32,
    pub status: u8,
    pub tick_array_bitmap: [u64; 16],
    pub open_time: u64,
    #[serde(skip)]
    pub tick_array_state: HashMap<i32, TickArrayState>,
    pub tick_array_bitmap_extension: Option<TickArrayBitmapExtension>,
    pub last_updated: u64, // Unix timestamp
    pub token0_reserve: u64,
    pub token1_reserve: u64,
    pub is_state_keys_initialized: bool,
}

#[derive(Debug, Clone)]
pub struct RaydiumClmmPoolUpdate {
    pub slot: u64,
    pub transaction_index: Option<u64>,
    pub address: Pubkey,
    pub pool_state_part: Option<RaydiumClmmPoolStatePart>,
    pub reserve_part: Option<RaydiumClmmPoolReservePart>,
    pub tick_array_state: Option<TickArrayState>,
    pub tick_array_bitmap_extension: Option<TickArrayBitmapExtension>,
    pub last_updated: u64,
    pub is_account_state_update: bool,
    pub pool_update_event_type: PoolUpdateEventType,
    pub additional_event_type: i32, // for tick array index tracking, 0 for others
}

impl RaydiumClmmPoolState {
    pub fn get_program_id() -> Pubkey {
        Pubkey::new_from_array(*RAYDIUM_CLMM_PROGRAM_ID.as_array())
    }

    /// Calculate output amount using Raydium CLMM pool state
    pub async fn calculate_output_amount(
        &self,
        input_token: &Pubkey,
        input_amount: u64,
        amm_config_fetcher: Arc<dyn GetAmmConfig>,
        _rpc_client: &RpcClient,
    ) -> u64 {
        if input_amount == 0 {
            return 0;
        }

        let (token0, _) = (self.token_mint0, self.token_mint1);
        let input_is_token0 = tokens_equal(input_token, &token0);
        let sqrt_price_limit_x64 = if input_is_token0 {
            MIN_SQRT_PRICE_X64 + 1
        } else {
            MAX_SQRT_PRICE_X64 - 1
        };

        let real_input_amount = input_amount;
        self.get_output_amount(
            real_input_amount,
            input_token,
            sqrt_price_limit_x64,
            amm_config_fetcher,
        )
        .await
    }

    async fn get_output_amount(
        &self,
        input_amount: u64,
        input_token: &Pubkey,
        _sqrt_price_limit_x64: u128,
        amm_config_fetcher: Arc<dyn GetAmmConfig>,
    ) -> u64 {
        let amm_config = match amm_config_fetcher
            .get_raydium_clmm_amm_config(&self.amm_config)
            .await
        {
            Ok(Some(config)) => config,
            _ => return 0,
        };

        let pool_info = ComputeClmmPoolInfo::new(
            self.address,
            Self::get_program_id(),
            self,
            self.tick_array_bitmap_extension.as_ref(),
            Some(amm_config),
        );

        match PoolUtils::get_output_amount_and_remain_accounts(
            &pool_info,
            &self.tick_array_state,
            input_token,
            rug::Integer::from(input_amount),
        ) {
            Ok(result) => {
                result
                    .expected_amount_out
                    .abs()
                    .to_u64()
                    .unwrap_or(0)
            }
            Err(_) => 0,
        }
    }

    pub fn calculate_token_prices(
        &self,
        sol_price: f64,
        base_decimals: u8,
        quote_decimals: u8,
    ) -> (f64, f64) {
        // For concentrated liquidity (CLMM), price is derived from sqrt_price_x64
        // sqrt_price_x64 is in Q64 format (fixed point with 64 fractional bits)
        // price = (sqrt_price_x64 / 2^64)^2 * (10^(quote_decimals - base_decimals))

        if self.sqrt_price_x64 == 0 {
            return (0.0, 0.0);
        }

        let token0_str = self.token_mint0.to_string();
        let token1_str = self.token_mint1.to_string();

        let is_token0_a_base_token = is_base_token(&token0_str);
        let is_token1_a_base_token = is_base_token(&token1_str);

        // Convert sqrt_price_x64 from Q64 to float (Q64 == 2^64)
        let q64 = 2f64.powi(64);
        let sqrt_price = self.sqrt_price_x64 as f64 / q64;

        // Price = sqrt_price^2 * (10^(quote_decimals - base_decimals))
        let decimal_scale = 10_f64.powi(quote_decimals as i32 - base_decimals as i32);
        let price_ratio = sqrt_price * sqrt_price * decimal_scale;

        // If token1 is a base token (like USDC, SOL), use its price
        if is_token1_a_base_token {
            let token1_price = if token1_str == "So11111111111111111111111111111111111111112" {
                sol_price // SOL
            } else {
                1.0 // Assume USDC/USDT are ~$1
            };

            let token0_price = price_ratio * token1_price;
            (token0_price, token1_price)
        } else if is_token0_a_base_token {
            // If token0 is a base token, use its price
            let token0_price = if token0_str == "So11111111111111111111111111111111111111112" {
                sol_price // SOL
            } else {
                1.0 // Assume USDC/USDT are ~$1
            };

            let token1_price = token0_price / price_ratio;
            (token0_price, token1_price)
        } else {
            // Neither token is a base token, assume relative pricing
            (price_ratio, 1.0)
        }
    }
}
