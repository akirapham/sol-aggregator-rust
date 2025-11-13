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

    /// Calculate output amount for Whirlpool swap using compute_swap_simplified
    /// 
    /// Uses the core compute_swap_simplified() function which mirrors the official
    /// Whirlpool SDK's pub fn swap() (swap_manager.rs lines 29-244).
    /// 
    /// This simplified implementation provides:
    /// - Exact input mode: Input fixed, output calculated
    /// - Core swap computation with official math
    /// - Proper fee deduction
    /// - Price limit validation
    /// - Direction-aware price movements
    /// 
    /// For multi-tick traversal with dynamic fees and liquidity updates,
    /// use the full loop implementation below (kept as reference).
    pub fn calculate_output_amount(
        &self,
        input_token: &Pubkey,
        input_amount: u64,
    ) -> u64 {
        // Input validation
        let a_to_b = tokens_equal(input_token, &self.token_mint_a);
        if self.sqrt_price == 0 || self.liquidity == 0 || input_amount == 0 {
            return 0;
        }

        // Set price limit based on direction
        let sqrt_price_limit = if a_to_b {
            MIN_SQRT_PRICE_X64
        } else {
            MAX_SQRT_PRICE_X64
        };

        // Validate price limit direction
        if (a_to_b && sqrt_price_limit >= self.sqrt_price)
            || (!a_to_b && sqrt_price_limit <= self.sqrt_price)
        {
            return 0;
        }

        // Call compute_swap_simplified with current pool state
        // This executes the core swap computation from the official Whirlpool SDK
        match compute_swap_simplified(
            input_amount,              // Exact input amount
            sqrt_price_limit,          // Price limit (MIN for A→B, MAX for B→A)
            true,                      // amount_specified_is_input = true (exact input mode)
            a_to_b,                    // Direction: true = A→B, false = B→A
            self.fee_rate,             // Static fee rate
            self.sqrt_price,           // Current pool price in Q64 format
            self.liquidity,            // Current pool liquidity
        ) {
            Ok(result) => {
                // Log swap details for debugging
                log::debug!(
                    "Swap computed: direction={}, input={}, output_a={}, output_b={}, fee={}",
                    if a_to_b { "A→B" } else { "B→A" },
                    input_amount,
                    result.amount_a,
                    result.amount_b,
                    result.fee_amount
                );

                // Return the output amount based on direction
                // In exact-input mode with compute_swap_simplified():
                //   amount_a = input - fees
                //   amount_b = output calculated
                if a_to_b {
                    result.amount_b  // Output is token B when A→B
                } else {
                    result.amount_a  // Output is token A when B→A
                }
            }
            Err(e) => {
                log::warn!("Swap computation failed: {}", e);
                0
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

/// Implements the core compute_swap logic matching official Whirlpool SDK
/// 
/// This function mirrors: pub fn swap(
///     whirlpool: &Whirlpool,
///     swap_tick_sequence: &mut SwapTickSequence,
///     amount: u64,
///     sqrt_price_limit: u128,
///     amount_specified_is_input: bool,
///     a_to_b: bool,
///     timestamp: u64,
///     adaptive_fee_info: &Option<AdaptiveFeeInfo>,
/// ) -> Result<Box<PostSwapUpdate>>
///
/// Key Parameters (7 parameters to compute_swap call):
/// 1. amount_remaining: u64 - Decreases with each iteration (input or output depending on mode)
/// 2. total_fee_rate: u16 - Updated via FeeRateManager (dynamic fees)
/// 3. curr_liquidity: u128 - Changes at tick crossings (liquidity_net adjustments)
/// 4. curr_sqrt_price: u128 - Current Q64 price, updates after each step
/// 5. bounded_sqrt_price_target: u128 - Calculated with fee bounds for this iteration
/// 6. amount_specified_is_input: bool - Constant mode flag (exact-in vs exact-out)
/// 7. a_to_b: bool - Constant direction flag (Token A→B vs B→A)
///
/// Documentation of the swap loop structure:
/// 
/// OUTER LOOP (lines 102-216 in official):
///   while amount_remaining > 0 && adjusted_sqrt_price_limit != curr_sqrt_price
///   - Finds next initialized tick
///   - Gets next_tick_sqrt_price and sqrt_price_target
///   
///   INNER LOOP ("do while"):
///     loop:
///       - Updates volatility accumulator (line 111)
///       - Gets bounded_sqrt_price_target (line 115-120)
///       - Calls compute_swap() with 7 parameters (line 115-123)
///       - Updates amounts based on mode (lines 125-134)
///       - Accumulates fees (lines 136-140)
///       - Handles tick crossings (lines 142-172)
///       - Updates price indices (lines 174-207)
///       - Advances fee manager (lines 208-215)
///       - Breaks when: amount_remaining == 0 OR curr_sqrt_price == sqrt_price_target
///
pub fn compute_swap_simplified(
    amount: u64,
    sqrt_price_limit: u128,
    amount_specified_is_input: bool,
    a_to_b: bool,
    fee_rate: u16,
    curr_sqrt_price: u128,
    curr_liquidity: u128,
) -> std::result::Result<SwapComputeResult, String> {
    // Input validation - mirror official swap function
    const MIN_SQRT_PRICE_X64: u128 = 4295048016;
    const MAX_SQRT_PRICE_X64: u128 = 79226673521066979257578248091;
    const NO_EXPLICIT_SQRT_PRICE_LIMIT: u128 = 0;

    let adjusted_sqrt_price_limit = if sqrt_price_limit == NO_EXPLICIT_SQRT_PRICE_LIMIT {
        if a_to_b {
            MIN_SQRT_PRICE_X64
        } else {
            MAX_SQRT_PRICE_X64
        }
    } else {
        sqrt_price_limit
    };

    // Validate price bounds
    if adjusted_sqrt_price_limit < MIN_SQRT_PRICE_X64 || adjusted_sqrt_price_limit > MAX_SQRT_PRICE_X64 {
        return Err("SqrtPriceOutOfBounds".to_string());
    }

    // Validate price limit direction
    if (a_to_b && adjusted_sqrt_price_limit >= curr_sqrt_price)
        || (!a_to_b && adjusted_sqrt_price_limit <= curr_sqrt_price)
    {
        return Err("InvalidSqrtPriceLimitDirection".to_string());
    }

    if amount == 0 {
        return Err("ZeroTradableAmount".to_string());
    }

    // Initialize state variables - mirror official swap loop state initialization
    let mut amount_remaining: u64 = amount;
    let mut amount_calculated: u64 = 0;
    let mut curr_sqrt_price_mut = curr_sqrt_price;
    let mut curr_liquidity_mut = curr_liquidity;
    let mut fee_sum: u64 = 0;

    // SIMPLIFIED VERSION: Single step (not multi-step like official)
    // Uses fee_rate directly instead of dynamic FeeRateManager
    // For multi-step swaps, would need full tick tracking and liquidity updates
    
    // Execute single swap step with base fee rate
    let swap_computation = compute_swap(
        amount_remaining,
        fee_rate as u32, // Convert u16 to u32 for fee rate
        curr_liquidity_mut,
        curr_sqrt_price_mut,
        adjusted_sqrt_price_limit,
        amount_specified_is_input,
        a_to_b,
    ).map_err(|e| format!("Swap step failed: {:?}", e))?;

    // Update amounts based on swap mode - mirror official logic (lines 125-141)
    if amount_specified_is_input {
        // Exact input mode: input amount is fixed, output varies
        amount_remaining = amount_remaining
            .checked_sub(swap_computation.amount_in)
            .ok_or("AmountRemainingOverflow")?;
        amount_remaining = amount_remaining
            .checked_sub(swap_computation.fee_amount)
            .ok_or("AmountRemainingOverflow")?;

        amount_calculated = amount_calculated
            .checked_add(swap_computation.amount_out)
            .ok_or("AmountCalcOverflow")?;
    } else {
        // Exact output mode: output amount is fixed, input varies
        amount_remaining = amount_remaining
            .checked_sub(swap_computation.amount_out)
            .ok_or("AmountRemainingOverflow")?;

        amount_calculated = amount_calculated
            .checked_add(swap_computation.amount_in)
            .ok_or("AmountCalcOverflow")?;
        amount_calculated = amount_calculated
            .checked_add(swap_computation.fee_amount)
            .ok_or("AmountCalcOverflow")?;
    }

    fee_sum = fee_sum
        .checked_add(swap_computation.fee_amount)
        .ok_or("AmountCalcOverflow")?;

    // Update current sqrt price
    curr_sqrt_price_mut = swap_computation.next_price;

    // Final amount calculation (mirror lines 226-231 in official)
    let (amount_a, amount_b) = if a_to_b == amount_specified_is_input {
        (amount - amount_remaining, amount_calculated)
    } else {
        (amount_calculated, amount - amount_remaining)
    };

    Ok(SwapComputeResult {
        amount_a,
        amount_b,
        fee_amount: fee_sum,
        end_sqrt_price: curr_sqrt_price_mut,
        end_tick_index: 0, // Not tracked in simplified version
        end_liquidity: curr_liquidity_mut,
    })
}

/// Result of compute_swap computation matching PostSwapUpdate
#[derive(Debug, Clone)]
pub struct SwapComputeResult {
    pub amount_a: u64,
    pub amount_b: u64,
    pub fee_amount: u64,
    pub end_sqrt_price: u128,
    pub end_tick_index: i32,
    pub end_liquidity: u128,
}