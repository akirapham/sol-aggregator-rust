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

// FEE_RATE denominator from Orca SDK
const FEE_RATE_MUL_VALUE: u32 = 1_000_000;

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

        log::debug!(
            "Orca swap: input={}, a_to_b={}, fee_rate={}, liquidity={}, sqrt_price={}",
            input_amount,
            a_to_b,
            self.fee_rate,
            self.liquidity,
            self.sqrt_price
        );

        // NOTE: This is a simplified single-step calculation. For accurate quotes, 
        // we would need to traverse through all affected tick arrays.
        // The Orca SDK's compute_swap requires the full tick sequence.
        
        // Apply fee to input amount
        let fee_rate_u128 = self.fee_rate as u128;
        let amount_after_fee = (input_amount as u128)
            .checked_mul(1_000_000 - fee_rate_u128)
            .and_then(|v| Some(v / 1_000_000))
            .unwrap_or(0) as u64;

        // For single-step: estimate output using constant product-like formula
        // output ≈ liquidity * (sqrt_price - sqrt_price_after) / (sqrt_price * sqrt_price_after)
        // This is a rough approximation for small swaps
        
        if amount_after_fee == 0 {
            return 0;
        }

        // Simplified formula assuming single tick
        // For token B output (B = amount * sqrt_price in Q64 scale)
        if a_to_b {
            // Swapping A for B: output is approximate
            // B ≈ (liquidity * ΔA) / (liquidity + ΔA * sqrt_price)
            let numerator = (self.liquidity as u128)
                .checked_mul(amount_after_fee as u128)
                .unwrap_or(u128::MAX);
            let denominator = (self.liquidity as u128)
                .checked_add((amount_after_fee as u128).checked_mul(self.sqrt_price).unwrap_or(u128::MAX))
                .unwrap_or(u128::MAX);
            
            let output = numerator / denominator;
            let output_u64 = if output > u64::MAX as u128 { u64::MAX } else { output as u64 };
            
            log::debug!("Orca swap result: amount_in={}, amount_out={}", input_amount, output_u64);
            output_u64
        } else {
            // Swapping B for A: output is approximate
            // A ≈ amount / sqrt_price (in Q64 scale)
            let amount_x64 = (amount_after_fee as u128) << 64;
            let output = amount_x64 / self.sqrt_price;
            let output_u64 = if output > u64::MAX as u128 { u64::MAX } else { output as u64 };
            
            log::debug!("Orca swap result: amount_in={}, amount_out={}", input_amount, output_u64);
            output_u64
        }
    }

    /// Swap step computation exactly matching Orca SDK's compute_swap
    /// Reference: https://github.com/orca-so/whirlpools/blob/main/programs/whirlpool/src/math/swap_math.rs
    fn compute_swap_step(
        &self,
        amount_remaining: u64,
        fee_rate: u16,
        liquidity: u128,
        sqrt_price_current: u128,
        sqrt_price_target: u128,
        specified_input: bool,
        a_to_b: bool,
    ) -> Result<SwapStepResult, &'static str> {
        if liquidity == 0 {
            return Err("No liquidity");
        }

        // Calculate initial fixed delta to determine if we hit max swap
        let initial_amount_fixed_delta = self.try_get_amount_fixed_delta(
            sqrt_price_current,
            sqrt_price_target,
            liquidity,
            specified_input,
            a_to_b,
        )?;

        // Calculate amount after fee application (only for ExactIn)
        let mut amount_calc = amount_remaining;
        if specified_input {
            // Apply fee: amount_calc = amount_remaining * (FEE_RATE_MUL_VALUE - fee_rate) / FEE_RATE_MUL_VALUE
            // FEE_RATE_MUL_VALUE = 1_000_000
            let fee_rate_u128 = fee_rate as u128;
            let numerator = (amount_remaining as u128)
                .checked_mul(1_000_000 - fee_rate_u128)
                .ok_or("Multiplication overflow")?;
            amount_calc = (numerator / 1_000_000) as u64;
        }

        log::debug!(
            "compute_swap_step: amount_remaining={}, amount_calc={}, initial_fixed_delta={}",
            amount_remaining,
            amount_calc,
            initial_amount_fixed_delta
        );

        log::debug!(
            "compute_swap_step: amount_remaining={}, amount_calc={}, initial_fixed_delta={}",
            amount_remaining,
            amount_calc,
            initial_amount_fixed_delta
        );

        // Determine next sqrt price
        let next_sqrt_price = if initial_amount_fixed_delta <= amount_calc {
            // We can reach the target price
            log::debug!("Can reach target price");
            sqrt_price_target
        } else {
            // We can't reach target, calculate intermediate price
            log::debug!("Cannot reach target, calculating intermediate price");
            self.get_next_sqrt_price(
                sqrt_price_current,
                liquidity,
                amount_calc,
                specified_input,
                a_to_b,
            )?
        };

        log::debug!("next_sqrt_price: {}, target: {}", next_sqrt_price, sqrt_price_target);

        let is_max_swap = next_sqrt_price == sqrt_price_target;

        // Calculate amount unfixed delta (the "other" amount not specified)
        let amount_unfixed_delta = self.get_amount_unfixed_delta(
            sqrt_price_current,
            next_sqrt_price,
            liquidity,
            specified_input,
            a_to_b,
        )?;

        // Calculate fixed delta for the new price (might differ due to precision)
        let amount_fixed_delta = if !is_max_swap {
            self.get_amount_fixed_delta(
                sqrt_price_current,
                next_sqrt_price,
                liquidity,
                specified_input,
                a_to_b,
            )?
        } else {
            initial_amount_fixed_delta
        };

        // Determine which is input and which is output
        let (amount_in, amount_out) = if specified_input {
            (amount_fixed_delta, amount_unfixed_delta)
        } else {
            (amount_unfixed_delta, amount_fixed_delta)
        };

        // Cap output amount if using output
        let amount_out = if !specified_input && amount_out > amount_remaining {
            amount_remaining
        } else {
            amount_out
        };

        // Calculate fee amount
        let fee_amount = if specified_input && !is_max_swap {
            // Fee is the remaining amount not used for input
            amount_remaining.saturating_sub(amount_in)
        } else if specified_input {
            // For max swap with exact input, apply fee formula
            // fee = amount_in * fee_rate / (FEE_RATE_MUL_VALUE - fee_rate)
            let fee_rate_u128 = fee_rate as u128;
            let numerator = (amount_in as u128)
                .checked_mul(fee_rate_u128)
                .ok_or("Multiplication overflow")?;
            (numerator / (1_000_000 - fee_rate_u128)) as u64
        } else {
            // ExactOut - no fee calculation needed here
            0
        };

        Ok(SwapStepResult {
            amount_in,
            amount_out,
            next_sqrt_price,
            fee_amount,
        })
    }

    /// Get fixed delta amount - the amount that is specified in the swap direction
    /// Matches Orca SDK's try_get_amount_fixed_delta
    fn try_get_amount_fixed_delta(
        &self,
        sqrt_price_current: u128,
        sqrt_price_target: u128,
        liquidity: u128,
        specified_input: bool,
        a_to_b: bool,
    ) -> Result<u64, &'static str> {
        if a_to_b == specified_input {
            // We're fixing A (either swapping A as input or getting A as output)
            self.get_amount_delta_a(
                sqrt_price_current,
                sqrt_price_target,
                liquidity,
                specified_input,
            )
        } else {
            // We're fixing B
            self.get_amount_delta_b(
                sqrt_price_current,
                sqrt_price_target,
                liquidity,
                specified_input,
            )
        }
    }

    /// Get unfixed delta amount - the amount that is not specified in the swap direction
    /// Matches Orca SDK's get_amount_unfixed_delta
    fn get_amount_unfixed_delta(
        &self,
        sqrt_price_current: u128,
        sqrt_price_target: u128,
        liquidity: u128,
        specified_input: bool,
        a_to_b: bool,
    ) -> Result<u64, &'static str> {
        if a_to_b == specified_input {
            // We're fixing A, so unfixed is B
            self.get_amount_delta_b(
                sqrt_price_current,
                sqrt_price_target,
                liquidity,
                !specified_input,
            )
        } else {
            // We're fixing B, so unfixed is A
            self.get_amount_delta_a(
                sqrt_price_current,
                sqrt_price_target,
                liquidity,
                !specified_input,
            )
        }
    }

    /// Get fixed delta - same as try_get_amount_fixed_delta but for non-target price
    fn get_amount_fixed_delta(
        &self,
        sqrt_price_current: u128,
        sqrt_price_target: u128,
        liquidity: u128,
        specified_input: bool,
        a_to_b: bool,
    ) -> Result<u64, &'static str> {
        if a_to_b == specified_input {
            self.get_amount_delta_a(
                sqrt_price_current,
                sqrt_price_target,
                liquidity,
                specified_input,
            )
        } else {
            self.get_amount_delta_b(
                sqrt_price_current,
                sqrt_price_target,
                liquidity,
                specified_input,
            )
        }
    }

    /// Calculate next sqrt price given an input amount
    /// Matches Orca SDK's get_next_sqrt_price
    fn get_next_sqrt_price(
        &self,
        current_sqrt_price: u128,
        liquidity: u128,
        amount_calc: u64,
        specified_input: bool,
        a_to_b: bool,
    ) -> Result<u128, &'static str> {
        if specified_input == a_to_b {
            // We're swapping the fixed token in the input direction
            self.get_next_sqrt_price_from_a_round_up(
                current_sqrt_price,
                liquidity,
                amount_calc,
                specified_input,
            )
        } else {
            self.get_next_sqrt_price_from_b_round_down(
                current_sqrt_price,
                liquidity,
                amount_calc,
                specified_input,
            )
        }
    }

    /// Calculate next sqrt price when token A is being swapped
    /// Δ(1/sqrt_p) = ΔtokenA / liquidity
    /// sqrt_price_new = 1 / (1/sqrt_price + amount/liquidity)
    fn get_next_sqrt_price_from_a_round_up(
        &self,
        sqrt_price: u128,
        liquidity: u128,
        amount: u64,
        _round_up: bool,
    ) -> Result<u128, &'static str> {
        if amount == 0 {
            return Ok(sqrt_price);
        }

        let amount_u128 = amount as u128;

        let denominator = (liquidity << 64).checked_add(amount_u128.saturating_mul(sqrt_price))
            .ok_or("Overflow in denominator")?;
        let numerator = (sqrt_price as u128)
            .checked_mul(liquidity)
            .ok_or("Overflow in numerator")?
            .checked_shl(64)
            .ok_or("Overflow in shift")?;

        numerator.checked_div(denominator).ok_or("Division by zero")
    }

    /// Calculate next sqrt price when token B is being swapped
    /// Δ(sqrt_p) = ΔtokenB / liquidity
    /// sqrt_price_new = sqrt_price + amount/liquidity
    fn get_next_sqrt_price_from_b_round_down(
        &self,
        sqrt_price: u128,
        liquidity: u128,
        amount: u64,
        _round_down: bool,
    ) -> Result<u128, &'static str> {
        if amount == 0 {
            return Ok(sqrt_price);
        }

        let amount_x64 = (amount as u128) << 64;
        let delta = amount_x64.checked_div(liquidity).ok_or("Division by zero")?;

        sqrt_price.checked_add(delta).ok_or("Sqrt price overflow")
    }

    /// Calculate token A amount delta between two sqrt prices
    /// ΔtokenA = liquidity * (sqrt_price_upper - sqrt_price_lower) / (sqrt_price_lower * sqrt_price_upper) / 2^64
    fn get_amount_delta_a(
        &self,
        sqrt_price_0: u128,
        sqrt_price_1: u128,
        liquidity: u128,
        round_up: bool,
    ) -> Result<u64, &'static str> {
        let (sqrt_price_lower, sqrt_price_upper) = if sqrt_price_0 <= sqrt_price_1 {
            (sqrt_price_0, sqrt_price_1)
        } else {
            (sqrt_price_1, sqrt_price_0)
        };

        if sqrt_price_lower == sqrt_price_upper || liquidity == 0 {
            return Ok(0);
        }

        let sqrt_price_diff = sqrt_price_upper.checked_sub(sqrt_price_lower)
            .ok_or("Underflow in sqrt_price_diff")?;

        // numerator = liquidity * (sqrt_price_upper - sqrt_price_lower) << 64
        let numerator = liquidity
            .checked_mul(sqrt_price_diff)
            .ok_or("Overflow in numerator")?
            .checked_shl(64)
            .ok_or("Overflow in shift")?;

        // denominator = sqrt_price_lower * sqrt_price_upper
        let denominator = sqrt_price_lower
            .checked_mul(sqrt_price_upper)
            .ok_or("Overflow in denominator")?;

        let quotient = numerator / denominator;
        let remainder = numerator % denominator;

        let result = if round_up && remainder != 0 {
            quotient.checked_add(1).ok_or("Overflow in result")?
        } else {
            quotient
        };

        // Result should fit in u64
        if result > u64::MAX as u128 {
            Err("Amount delta exceeds u64")
        } else {
            Ok(result as u64)
        }
    }

    /// Calculate token B amount delta between two sqrt prices
    /// ΔtokenB = liquidity * (sqrt_price_upper - sqrt_price_lower) >> 64
    fn get_amount_delta_b(
        &self,
        sqrt_price_0: u128,
        sqrt_price_1: u128,
        liquidity: u128,
        round_up: bool,
    ) -> Result<u64, &'static str> {
        let (sqrt_price_lower, sqrt_price_upper) = if sqrt_price_0 <= sqrt_price_1 {
            (sqrt_price_0, sqrt_price_1)
        } else {
            (sqrt_price_1, sqrt_price_0)
        };

        if sqrt_price_lower == sqrt_price_upper || liquidity == 0 {
            return Ok(0);
        }

        let sqrt_price_diff = sqrt_price_upper.checked_sub(sqrt_price_lower)
            .ok_or("Underflow in sqrt_price_diff")?;

        // product = liquidity * sqrt_price_diff
        let product = liquidity
            .checked_mul(sqrt_price_diff)
            .ok_or("Overflow in product")?;

        let quotient = product >> 64;
        let remainder = product & ((1u128 << 64) - 1);

        let should_round = round_up && remainder > 0;
        let result = if should_round {
            quotient.checked_add(1).ok_or("Overflow in result")?
        } else {
            quotient
        };

        // Result should fit in u64
        if result > u64::MAX as u128 {
            Err("Amount delta exceeds u64")
        } else {
            Ok(result as u64)
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
