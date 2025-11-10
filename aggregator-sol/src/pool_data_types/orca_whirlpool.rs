use std::{collections::HashMap, sync::Arc, time::{SystemTime, UNIX_EPOCH}};
use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use solana_streamer_sdk::streaming::event_parser::protocols::orca_whirlpools::{
    parser::ORCA_WHIRLPOOL_PROGRAM_ID, types::TickArrayState, types::OracleState
};

use crate::{
    constants::is_base_token,
    pool_data_types::{
        GetAmmConfig, PoolUpdateEventType, orca::fee_rate_manager::FeeRateManager
    },
    utils::tokens_equal,
};

use crate::pool_data_types::orca::{
    math::*,
    state::*,
};

// Whirlpool sqrt price limits (same as Raydium CLMM)
const MIN_SQRT_PRICE_X64: u128 = 4295048016;
const MAX_SQRT_PRICE_X64: u128 = 79226673521066979257578248091;

#[derive(Debug, Clone, Default, Serialize, Deserialize, BorshDeserialize, BorshSerialize)]
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
    #[serde(skip)]
    pub oracle_state: OracleState,
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
    pub oracle_state: Option<OracleState>,
    pub last_updated: u64,
    pub is_account_state_update: bool,
    pub pool_update_event_type: PoolUpdateEventType,
    pub additional_event_type: i32, // for tick array index tracking, 0 for others
}

impl WhirlpoolPoolState {
    pub fn get_program_id() -> Pubkey {
        Pubkey::new_from_array(*ORCA_WHIRLPOOL_PROGRAM_ID.as_array())
    }

    /// Calculate output amount for Whirlpool swap based on Orca SDK quote_swap
    pub async fn calculate_output_amount(
        &self,
        input_token: &Pubkey,
        input_amount: u64,
        _amm_config_fetcher: Arc<dyn GetAmmConfig>,
    ) -> u64 {
        let a_to_b = tokens_equal(input_token, &self.token_mint_a);
        if self.sqrt_price == 0 || self.liquidity == 0 || input_amount == 0 {
            return 0;
        }
        // log::info!("YYYYYYYYYYYYYYYYYYYYY oracle_state {:?}", self.oracle_state.whirlpool.to_string());

        let adjusted_sqrt_price_limit = if a_to_b {
                MIN_SQRT_PRICE_X64
            } else {
                MAX_SQRT_PRICE_X64
            };

        if a_to_b && adjusted_sqrt_price_limit >= self.sqrt_price
            || !a_to_b && adjusted_sqrt_price_limit <= self.sqrt_price
        {
            return 0;
        }
        let fee_rate = self.fee_rate;
        let amount_remaining: u64 = input_amount;
        let sqrt_price_current = self.sqrt_price;
        let curr_liquidity = self.liquidity;

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let adaptive_fee_info: Option<AdaptiveFeeInfo>;
        if self.oracle_state.whirlpool.to_string() == "11111111111111111111111111111111" {
            adaptive_fee_info = None;
        } else {
            adaptive_fee_info = Some(AdaptiveFeeInfo {
                constants: AdaptiveFeeConstants {
                    filter_period: self.oracle_state.adaptive_fee_constants.filter_period,
                    decay_period: self.oracle_state.adaptive_fee_constants.decay_period,
                    reduction_factor: self.oracle_state.adaptive_fee_constants.reduction_factor,
                    adaptive_fee_control_factor: self.oracle_state.adaptive_fee_constants.adaptive_fee_control_factor,
                    max_volatility_accumulator: self.oracle_state.adaptive_fee_constants.max_volatility_accumulator,
                    tick_group_size: self.oracle_state.adaptive_fee_constants.tick_group_size,
                    major_swap_threshold_ticks: self.oracle_state.adaptive_fee_constants.major_swap_threshold_ticks,
                    reserved: self.oracle_state.adaptive_fee_constants.reserved,
                },
                variables: AdaptiveFeeVariables {
                    last_reference_update_timestamp: self.oracle_state.adaptive_fee_variables.last_reference_update_timestamp,
                    last_major_swap_timestamp: self.oracle_state.adaptive_fee_variables.last_major_swap_timestamp,
                    volatility_reference: self.oracle_state.adaptive_fee_variables.volatility_reference,
                    tick_group_index_reference: self.oracle_state.adaptive_fee_variables.tick_group_index_reference,
                    volatility_accumulator: self.oracle_state.adaptive_fee_variables.volatility_accumulator,
                    reserved: self.oracle_state.adaptive_fee_variables.reserved,
                },
            });
        }

        let mut fee_rate_manager = match FeeRateManager::new(
            a_to_b,
            self.tick_current_index, // note:  -1 shift is acceptable
            timestamp,
            fee_rate,
            &adaptive_fee_info,
        ) {
            Ok(manager) => manager,
            Err(_) => {
                log::warn!("Failed to create FeeRateManager, falling back to simple calculation");
                return 0;
            }
        };

        let _ = fee_rate_manager.update_volatility_accumulator();
        let total_fee_rate = fee_rate_manager.get_total_fee_rate();
        let (bounded_sqrt_price_target, _adaptive_fee_update_skipped) =
            fee_rate_manager.get_bounded_sqrt_price_target(adjusted_sqrt_price_limit, curr_liquidity);

        let swap_computation = compute_swap(
            amount_remaining,
            total_fee_rate,
            curr_liquidity,
            sqrt_price_current,
            bounded_sqrt_price_target,
            true, // amount_specified_is_input
            a_to_b,
        );
        match swap_computation {
            Ok(computation) => computation.amount_out,
            Err(e) => {
                log::warn!("Swap computation failed: {:?}", e);
                0 // Return 0 or handle error appropriately
            }
        }
    }

    pub fn calculate_token_prices(
        &self,
        sol_price: f64,
        base_decimals: u8,
        quote_decimals: u8,
    ) -> (f64, f64) {
        // For concentrated liquidity (CLMM), price is derived from sqrt_price
        // sqrt_price is in Q64 format (fixed point with 64 fractional bits)
        // price = (sqrt_price / 2^64)^2 * (10^(quote_decimals - base_decimals))

        if self.sqrt_price == 0 {
            return (0.0, 0.0);
        }

        let token_a_str = self.token_mint_a.to_string();
        let token_b_str = self.token_mint_b.to_string();

        let is_token_a_base_token = is_base_token(&token_a_str);
        let is_token_b_base_token = is_base_token(&token_b_str);

        // Convert sqrt_price from Q64 to float (Q64 == 2^64)
        let q64 = 2f64.powi(64);
        let sqrt_price = self.sqrt_price as f64 / q64;

        // Price = sqrt_price^2 * (10^(quote_decimals - base_decimals))
        let decimal_scale = 10_f64.powi(quote_decimals as i32 - base_decimals as i32);
        let price_ratio = sqrt_price * sqrt_price * decimal_scale;

        // If token_b is a base token (like USDC, SOL), use its price
        if is_token_b_base_token {
            let token_b_price = if token_b_str == "So11111111111111111111111111111111111111112" {
                sol_price // SOL
            } else {
                1.0 // Assume USDC/USDT are ~$1
            };

            let token_a_price = price_ratio * token_b_price;
            (token_a_price, token_b_price)
        } else if is_token_a_base_token {
            // If token_a is a base token, use its price
            let token_a_price = if token_a_str == "So11111111111111111111111111111111111111112" {
                sol_price // SOL
            } else {
                1.0 // Assume USDC/USDT are ~$1
            };

            let token_b_price = token_a_price / price_ratio;
            (token_a_price, token_b_price)
        } else {
            // Neither token is a base token, assume relative pricing
            (price_ratio, 1.0)
        }
    }
}

/// Result of a single swap step computation
#[allow(unused)]
struct SwapStepResult {
    amount_in: u64,
    amount_out: u64,
    next_sqrt_price: u128,
    fee_amount: u64,
}
