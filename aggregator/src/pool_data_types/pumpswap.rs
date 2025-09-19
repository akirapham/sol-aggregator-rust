use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PumpSwapPoolState {
    pub slot: u64,
    pub transaction_index: Option<u64>,
    pub address: Pubkey,
    pub index: u16,
    pub creator: Option<Pubkey>,
    pub base_mint: Pubkey,
    pub quote_mint: Pubkey,
    pub pool_base_token_account: Pubkey,
    pub pool_quote_token_account: Pubkey,
    pub last_updated: u64,
    pub base_reserve: u64,
    pub quote_reserve: u64,
    pub liquidity_usd: f64,
    pub is_state_keys_initialized: bool,
}

#[derive(Debug, Clone)]
pub struct PumpSwapPoolUpdate {
    pub slot: u64,
    pub transaction_index: Option<u64>,
    pub address: Pubkey,
    pub index: Option<u16>,
    pub creator: Option<Pubkey>,
    pub base_mint: Pubkey,
    pub quote_mint: Pubkey,
    pub pool_base_token_account: Pubkey,
    pub pool_quote_token_account: Pubkey,
    pub last_updated: u64,
    pub base_reserve: u64,
    pub quote_reserve: u64,
    pub is_account_state_update: bool,
}
