use std::{collections::HashMap, sync::Arc};

use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use solana_streamer_sdk::streaming::event_parser::protocols::orca_whirlpools::{
    parser::ORCA_WHIRLPOOL_PROGRAM_ID, types::TickArrayState,
};

use crate::{
    constants::is_base_token,
    pool_data_types::{GetAmmConfig, PoolUpdateEventType},
    utils::tokens_equal,
};

// Whirlpool sqrt price limits (same as Raydium CLMM)
const MIN_SQRT_PRICE_X64: u128 = 4295048016;
const MAX_SQRT_PRICE_X64: u128 = 79226673521066979257578248091;

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

    /// Calculate output amount for Whirlpool swap
    pub async fn calculate_output_amount(
        &self,
        input_token: &Pubkey,
        input_amount: u64,
        amm_config_fetcher: Arc<dyn GetAmmConfig>,
    ) -> u64 {
        let (token_a, _) = (self.token_mint_a, self.token_mint_b);
        let input_is_token_a = tokens_equal(input_token, &token_a);
        let sqrt_price_limit_x64 = if input_is_token_a {
            MIN_SQRT_PRICE_X64 + 1
        } else {
            MAX_SQRT_PRICE_X64 - 1
        };

        // Don't take transfer tax into account for now, users should account for it in their slippage
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
        sqrt_price_limit_x64: u128,
        _amm_config_fetcher: Arc<dyn GetAmmConfig>,
    ) -> u64 {
        // Determine swap direction
        let a_to_b = tokens_equal(input_token, &self.token_mint_a);

        if self.sqrt_price == 0 || self.liquidity == 0 || input_amount == 0 {
            return 0;
        }

        // Based on Whirlpool SDK's compute_swap implementation
        // For routing purposes, we compute a single swap step assuming we stay in the current tick
        // This provides a reasonable estimate without full tick array traversal

        let fee_rate = self.fee_rate;
        let current_liquidity = self.liquidity;
        let current_sqrt_price = self.sqrt_price;

        // Compute single swap step (simplified from full Orca SDK compute_swap)
        match self.compute_swap_step(
            input_amount,
            fee_rate,
            current_liquidity,
            current_sqrt_price,
            sqrt_price_limit_x64,
            true, // specified_input = true
            a_to_b,
        ) {
            Ok(swap_step) => swap_step.amount_out,
            Err(_) => 0,
        }
    }

    /// Simplified swap step computation based on Orca SDK's compute_swap_step
    /// Reference: https://github.com/orca-so/whirlpools/blob/main/rust-sdk/core/src/quote/swap.rs
    fn compute_swap_step(
        &self,
        amount_remaining: u64,
        fee_rate: u16,
        current_liquidity: u128,
        current_sqrt_price: u128,
        target_sqrt_price: u128,
        specified_input: bool,
        a_to_b: bool,
    ) -> Result<SwapStepResult, &'static str> {
        if current_liquidity == 0 {
            return Err("No liquidity");
        }

        // Apply fee to input amount
        let amount_fee = if specified_input {
            ((amount_remaining as u128)
                .saturating_mul(fee_rate as u128)
                .saturating_div(1_000_000)) as u64
        } else {
            0
        };

        let amount_after_fee = amount_remaining.saturating_sub(amount_fee);

        if amount_after_fee == 0 {
            return Ok(SwapStepResult {
                amount_in: 0,
                amount_out: 0,
                next_sqrt_price: current_sqrt_price,
                fee_amount: 0,
            });
        }

        // Calculate next sqrt price from input amount
        // Using formulas from Orca SDK token math
        let next_sqrt_price = if specified_input {
            self.get_next_sqrt_price_from_input(
                current_sqrt_price,
                current_liquidity,
                amount_after_fee,
                a_to_b,
            )?
        } else {
            self.get_next_sqrt_price_from_output(
                current_sqrt_price,
                current_liquidity,
                amount_after_fee,
                a_to_b,
            )?
        };

        // Clamp to target price
        let bounded_next_sqrt_price = if a_to_b {
            next_sqrt_price.max(target_sqrt_price)
        } else {
            next_sqrt_price.min(target_sqrt_price)
        };

        // Calculate actual amounts based on price movement
        let (amount_in, amount_out) = if specified_input {
            let out = self.get_amount_delta(
                current_sqrt_price,
                bounded_next_sqrt_price,
                current_liquidity,
                !a_to_b,
            )?;
            (amount_after_fee, out)
        } else {
            let input_needed = self.get_amount_delta(
                current_sqrt_price,
                bounded_next_sqrt_price,
                current_liquidity,
                a_to_b,
            )?;
            (input_needed, amount_after_fee)
        };

        Ok(SwapStepResult {
            amount_in,
            amount_out,
            next_sqrt_price: bounded_next_sqrt_price,
            fee_amount: amount_fee,
        })
    }

    /// Calculate next sqrt price when trading token A (a_to_b = true) or token B (a_to_b = false)
    /// Based on Orca SDK's get_next_sqrt_price_from_a and get_next_sqrt_price_from_b
    fn get_next_sqrt_price_from_input(
        &self,
        sqrt_price: u128,
        liquidity: u128,
        amount: u64,
        a_to_b: bool,
    ) -> Result<u128, &'static str> {
        if amount == 0 {
            return Ok(sqrt_price);
        }

        if a_to_b {
            // Trading A for B: price decreases
            // Formula: next_sqrt_price = (sqrt_price * liquidity) / (liquidity + amount * sqrt_price)
            let product = (amount as u128).saturating_mul(sqrt_price);
            let denominator = (liquidity << 64).saturating_add(product);

            if denominator == 0 {
                return Err("Division by zero");
            }

            let numerator = (sqrt_price as u128).saturating_mul(liquidity) << 64;

            Ok(numerator.saturating_div(denominator))
        } else {
            // Trading B for A: price increases
            // Formula: next_sqrt_price = sqrt_price + (amount << 64) / liquidity
            let delta = ((amount as u128) << 64)
                .checked_div(liquidity)
                .ok_or("Division by zero")?;

            sqrt_price.checked_add(delta).ok_or("Sqrt price overflow")
        }
    }

    fn get_next_sqrt_price_from_output(
        &self,
        sqrt_price: u128,
        liquidity: u128,
        amount: u64,
        a_to_b: bool,
    ) -> Result<u128, &'static str> {
        if amount == 0 {
            return Ok(sqrt_price);
        }

        if a_to_b {
            // Want to get amount of B out: price decreases
            let delta = ((amount as u128) << 64)
                .checked_div(liquidity)
                .ok_or("Division by zero")?;

            sqrt_price.checked_sub(delta).ok_or("Sqrt price underflow")
        } else {
            // Want to get amount of A out: price increases
            let product = (amount as u128).saturating_mul(sqrt_price);
            let denominator = (liquidity << 64).saturating_sub(product);

            if denominator == 0 {
                return Err("Division by zero");
            }

            let numerator = sqrt_price.saturating_mul(liquidity) << 64;
            Ok(numerator.saturating_div(denominator))
        }
    }

    /// Calculate token amount delta between two sqrt prices
    /// Based on Orca SDK's get_amount_delta_a and get_amount_delta_b
    fn get_amount_delta(
        &self,
        sqrt_price_a: u128,
        sqrt_price_b: u128,
        liquidity: u128,
        get_token_a: bool,
    ) -> Result<u64, &'static str> {
        let (sqrt_price_lower, sqrt_price_upper) = if sqrt_price_a < sqrt_price_b {
            (sqrt_price_a, sqrt_price_b)
        } else {
            (sqrt_price_b, sqrt_price_a)
        };

        if get_token_a {
            // Amount of token A
            // Formula: liquidity * (sqrt_price_upper - sqrt_price_lower) / (sqrt_price_lower * sqrt_price_upper)
            let sqrt_price_diff = sqrt_price_upper.saturating_sub(sqrt_price_lower);
            let numerator = liquidity.saturating_mul(sqrt_price_diff) << 64;
            let denominator = sqrt_price_lower.saturating_mul(sqrt_price_upper);

            if denominator == 0 {
                return Ok(0);
            }

            Ok((numerator.saturating_div(denominator) >> 64) as u64)
        } else {
            // Amount of token B
            // Formula: liquidity * (sqrt_price_upper - sqrt_price_lower) >> 64
            let sqrt_price_diff = sqrt_price_upper.saturating_sub(sqrt_price_lower);
            Ok((liquidity.saturating_mul(sqrt_price_diff) >> 64) as u64)
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
struct SwapStepResult {
    amount_in: u64,
    amount_out: u64,
    next_sqrt_price: u128,
    fee_amount: u64,
}
