use crate::{
    constants::is_base_token,
    pool_data_types::{GetAmmConfig, PoolUpdateEventType},
};
use borsh::{BorshDeserialize, BorshSerialize};

use orca_whirlpools_core::{
    compute_swap, AdaptiveFeeConstantsFacade, AdaptiveFeeInfo, AdaptiveFeeVariablesFacade,
    TickArrayFacade, TickArraySequence, TickFacade, WhirlpoolFacade, WhirlpoolRewardInfoFacade,
    NUM_REWARDS, TICK_ARRAY_SIZE,
};
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use solana_streamer_sdk::streaming::event_parser::protocols::orca_whirlpools::{
    parser::ORCA_WHIRLPOOL_PROGRAM_ID, types::OracleState, types::TickArrayState,
};
use std::collections::HashMap;
use std::sync::Arc;

use crate::utils::tokens_equal;

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

    pub async fn calculate_output_amount(
        &self,
        input_token: &Pubkey,
        input_amount: u64,
        _amm_config_fetcher: Arc<dyn GetAmmConfig>,
    ) -> u64 {
        if input_amount == 0 {
            return 0;
        }

        let specified_token_a = tokens_equal(input_token, &self.token_mint_a);
        let a_to_b = specified_token_a; // ExactIn: Input A -> Output B (A to B)

        // Construct WhirlpoolFacade
        let whirlpool = WhirlpoolFacade {
            fee_tier_index_seed: self.tick_spacing_seed,
            tick_spacing: self.tick_spacing,
            fee_rate: self.fee_rate,
            protocol_fee_rate: self.protocol_fee_rate,
            liquidity: self.liquidity,
            sqrt_price: self.sqrt_price,
            tick_current_index: self.tick_current_index,
            fee_growth_global_a: 0,
            fee_growth_global_b: 0,
            reward_last_updated_timestamp: 0,
            reward_infos: [WhirlpoolRewardInfoFacade::default(); NUM_REWARDS],
        };

        // Construct AdaptiveFeeInfo
        let adaptive_fee_info = if self.oracle_state.whirlpool == Pubkey::default() {
            None
        } else {
            Some(AdaptiveFeeInfo {
                constants: AdaptiveFeeConstantsFacade {
                    filter_period: self.oracle_state.adaptive_fee_constants.filter_period,
                    decay_period: self.oracle_state.adaptive_fee_constants.decay_period,
                    reduction_factor: self.oracle_state.adaptive_fee_constants.reduction_factor,
                    adaptive_fee_control_factor: self
                        .oracle_state
                        .adaptive_fee_constants
                        .adaptive_fee_control_factor,
                    max_volatility_accumulator: self
                        .oracle_state
                        .adaptive_fee_constants
                        .max_volatility_accumulator,
                    tick_group_size: self.oracle_state.adaptive_fee_constants.tick_group_size,
                    major_swap_threshold_ticks: self
                        .oracle_state
                        .adaptive_fee_constants
                        .major_swap_threshold_ticks,
                },
                variables: AdaptiveFeeVariablesFacade {
                    last_reference_update_timestamp: self
                        .oracle_state
                        .adaptive_fee_variables
                        .last_reference_update_timestamp,
                    last_major_swap_timestamp: self
                        .oracle_state
                        .adaptive_fee_variables
                        .last_major_swap_timestamp,
                    volatility_reference: self
                        .oracle_state
                        .adaptive_fee_variables
                        .volatility_reference,
                    tick_group_index_reference: self
                        .oracle_state
                        .adaptive_fee_variables
                        .tick_group_index_reference,
                    volatility_accumulator: self
                        .oracle_state
                        .adaptive_fee_variables
                        .volatility_accumulator,
                },
            })
        };

        // Construct TickArraySequence
        let start_tick_index = orca_whirlpools_core::get_tick_array_start_tick_index(
            self.tick_current_index,
            self.tick_spacing,
        );
        let offset = self.tick_spacing as i32 * TICK_ARRAY_SIZE as i32;

        // We need a sequence of tick arrays. Let's try to get 5 arrays centered around current.
        let mut tick_arrays: [Option<TickArrayFacade>; 5] = [None, None, None, None, None];

        for i in 0..5 {
            let index = start_tick_index + (i as i32 - 2) * offset;
            if let Some(state) = self.tick_array_state.get(&index) {
                // Convert TickArrayState to TickArrayFacade
                let mut ticks = [TickFacade::default(); TICK_ARRAY_SIZE];
                for (j, t) in state.ticks.iter().enumerate() {
                    if j >= TICK_ARRAY_SIZE {
                        break;
                    }
                    ticks[j] = TickFacade {
                        initialized: t.initialized,
                        liquidity_net: t.liquidity_net,
                        liquidity_gross: t.liquidity_gross,
                        fee_growth_outside_a: t.fee_growth_outside_a,
                        fee_growth_outside_b: t.fee_growth_outside_b,
                        reward_growths_outside: t.reward_growths_outside,
                    };
                }

                tick_arrays[i] = Some(TickArrayFacade {
                    start_tick_index: state.start_tick_index,
                    ticks,
                });
            }
        }

        let tick_sequence = match TickArraySequence::new(tick_arrays, self.tick_spacing) {
            Ok(seq) => seq,
            Err(_) => return 0,
        };

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let result = compute_swap(
            input_amount,
            0, // sqrt_price_limit (0 means default min/max)
            whirlpool,
            tick_sequence,
            a_to_b,
            true, // specified_input = true (ExactIn)
            timestamp,
            adaptive_fee_info,
        );

        match result {
            Ok(swap_result) => {
                if a_to_b {
                    swap_result.token_b
                } else {
                    swap_result.token_a
                }
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
