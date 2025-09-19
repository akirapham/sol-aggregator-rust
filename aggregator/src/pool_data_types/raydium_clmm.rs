use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
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
