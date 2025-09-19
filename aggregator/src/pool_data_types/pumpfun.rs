use solana_sdk::pubkey::Pubkey;
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PumpfunPoolState {
    pub slot: u64,
    pub transaction_index: Option<u64>,
    pub address: Pubkey, // bonding curve address
    pub mint: Pubkey,
    pub sol_reserve: u64,
    pub token_reserve: u64,
    pub real_token_reserve: u64,
    pub last_updated: u64,
    pub liquidity_usd: f64,
    pub complete: bool,
    pub is_state_keys_initialized: bool,
}

#[derive(Debug, Clone)]
pub struct PumpfunPoolUpdate {
    pub slot: u64,
    pub transaction_index: Option<u64>,
    pub address: Pubkey,
    pub mint: Pubkey,
    pub token_reserve: u64,
    pub sol_reserve: u64,
    pub real_token_reserve: u64,
    pub last_updated: u64,
    pub complete: bool,
    pub is_account_state_update: bool,
}
