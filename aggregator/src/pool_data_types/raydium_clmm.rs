use crate::utils::tokens_equal;
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use solana_streamer_sdk::streaming::event_parser::protocols::raydium_clmm::parser::RAYDIUM_CLMM_PROGRAM_ID;

const MIN_SQRT_PRICE_X64: u128 = 4295048016;
const MAX_SQRT_PRICE_X64: u128 = 79226673521066979257578248091;
#[derive(Clone, Debug, Copy, Default)]
pub struct TickState {
    pub tick: i32,
    pub liquidity_net: i128,
    pub liquidity_gross: u128,
}

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

#[derive(Clone, Debug)]
pub struct RadyiumClmmPoolStatePart {
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

#[derive(Clone, Debug)]
pub struct RadyiumClmmPoolReservePart {
    pub token0_reserve: u64,
    pub token1_reserve: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RadyiumClmmPoolState {
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
    pub tick_array_state: TickArrayState,
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
    pub pool_state_part: Option<RadyiumClmmPoolStatePart>,
    pub reserve_part: Option<RadyiumClmmPoolReservePart>,
    pub tick_array_state: Option<TickArrayState>,
    pub last_updated: u64,
    pub is_account_state_update: bool,
}

impl RadyiumClmmPoolState {
    pub fn get_program_id() -> Pubkey {
        Pubkey::new_from_array(*RAYDIUM_CLMM_PROGRAM_ID.as_array())
    }

    /// Calculate output amount for PumpFun bonding curve
    pub fn calculate_output_amount(&self, input_token: &Pubkey, input_amount: u64) -> u64 {
        let (token0, _) = (self.token_mint0, self.token_mint1);
        let input_is_token0 = tokens_equal(input_token, &token0);
        let sqrt_price_limit_x64 = if input_is_token0 {
            MIN_SQRT_PRICE_X64 + 1
        } else {
            MAX_SQRT_PRICE_X64 - 1
        };

        // dont take transfer tax into account for now, users should account for it un their slippage
        let real_input_amount = input_amount;
        self.get_output_amount(real_input_amount, input_is_token0, sqrt_price_limit_x64)
    }

    fn get_output_amount(
        &self,
        _input_amount: u64,
        _zero_for_one: bool,
        _sqrt_price_limit_x64: u128,
    ) -> u64 {
        // TODO: implement the actual CLMM swap logic here
        0
    }
}
