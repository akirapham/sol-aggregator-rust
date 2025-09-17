use solana_sdk::pubkey::Pubkey;

#[derive(Debug, Clone)]
pub struct PumpSwapPoolState {
    pub slot: u64,
    pub transaction_index: Option<u64>,
    pub address: Pubkey, // bonding curve address
    pub index: u16,
    pub creator: Option<Pubkey>,
    pub base_mint: Pubkey,
    pub quote_mint: Pubkey,
    pub pool_base_token_account: Pubkey,
    pub pool_quote_token_account: Pubkey,
    pub last_updated: u64, // Unix timestamp
    pub base_reserve: u64,
    pub quote_reserve: u64,
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
    pub last_updated: u64, // Unix timestamp
    pub base_reserve: u64,
    pub quote_reserve: u64,
}
