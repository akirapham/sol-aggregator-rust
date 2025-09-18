use solana_sdk::pubkey::Pubkey;

#[derive(Debug, Clone)]
pub struct BonkPoolState {
    pub slot: u64,
    pub transaction_index: Option<u64>,
    pub address: Pubkey,
    pub status: u8,
    pub total_base_sell: u64,
    pub base_reserve: u64,  // virtual_base
    pub quote_reserve: u64, // virtual_quote
    pub real_base: u64,
    pub real_quote: u64,
    pub quote_protocol_fee: u64,
    pub platform_fee: u64,
    pub global_config: Pubkey,
    pub platform_config: Pubkey,
    pub base_mint: Pubkey,
    pub quote_mint: Pubkey,
    pub base_vault: Pubkey,
    pub quote_vault: Pubkey,
    pub creator: Pubkey,
    pub last_updated: u64,
    pub is_state_keys_initialized: bool,
}

#[derive(Debug, Clone)]
pub struct BonkPoolUpdate {
    pub slot: u64,
    pub transaction_index: Option<u64>,
    pub address: Pubkey,
    pub status: u8,
    pub total_base_sell: u64,
    pub base_reserve: u64,  // virtual_base
    pub quote_reserve: u64, // virtual_quote
    pub real_base: u64,
    pub real_quote: u64,
    pub quote_protocol_fee: u64,
    pub platform_fee: u64,
    pub global_config: Pubkey,
    pub platform_config: Pubkey,
    pub base_mint: Pubkey,
    pub quote_mint: Pubkey,
    pub base_vault: Pubkey,
    pub quote_vault: Pubkey,
    pub creator: Pubkey,
    pub last_updated: u64,
    pub is_account_state_update: bool,
}
