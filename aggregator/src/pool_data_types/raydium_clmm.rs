use solana_sdk::pubkey::Pubkey;

#[derive(Clone, Debug)]
pub struct TickState {
    pub tick: i32,
    pub liquidity_net: i128,
    pub liquidity_gross: u128
}

#[derive(Clone, Debug)]
pub struct TickArrayState {
    pub pool_id: Pubkey,
    pub start_tick_index: i32,
    #[serde(with = "serde_big_array::BigArray")]
    pub ticks: [TickState; 60],
    pub initialized_tick_count: u8,
}

#[derive(Debug, Clone)]
pub struct RadyiumClmmPoolState {
    pub slot: u64,
    pub transaction_index: Option<u64>,
    pub address: Pubkey, // bonding curve address
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
    pub tick_array_state: TickArrayState,
    pub last_updated: u64, // Unix timestamp
    pub token0_reserve: u64,
    pub token1_reserve: u64,
}

#[derive(Debug, Clone)]
pub struct RaydiumClmmPoolUpdate {
    pub slot: u64,
    pub transaction_index: Option<u64>,
    pub address: Pubkey,
}
