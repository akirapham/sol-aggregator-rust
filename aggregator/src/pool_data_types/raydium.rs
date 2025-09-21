use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use solana_streamer_sdk::streaming::event_parser::protocols::raydium_amm_v4::parser::RAYDIUM_AMM_V4_PROGRAM_ID;

use crate::utils::tokens_equal;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaydiumAmmV4PoolState {
    pub slot: u64,
    pub transaction_index: Option<u64>,
    pub address: Pubkey, // bonding curve address
    pub base_mint: Pubkey,
    pub quote_mint: Pubkey,
    pub amm_authority: Pubkey,
    pub amm_open_orders: Pubkey,
    pub amm_target_orders: Pubkey,
    pub pool_coin_token_account: Pubkey,
    pub pool_pc_token_account: Pubkey,
    pub serum_program: Pubkey,
    pub serum_market: Pubkey,
    pub serum_bids: Pubkey,
    pub serum_asks: Pubkey,
    pub serum_event_queue: Pubkey,
    pub serum_coin_vault_account: Pubkey,
    pub serum_pc_vault_account: Pubkey,
    pub serum_vault_signer: Pubkey,
    pub last_updated: u64,
    pub base_reserve: u64,
    pub quote_reserve: u64,
    pub liquidity_usd: f64,
    pub is_state_keys_initialized: bool,
}

#[derive(Debug, Clone)]
pub struct RaydiumAmmV4PoolUpdate {
    pub slot: u64,
    pub transaction_index: Option<u64>,
    pub address: Pubkey,
    pub base_mint: Pubkey,
    pub quote_mint: Pubkey,
    pub amm_authority: Pubkey,
    pub amm_open_orders: Pubkey,
    pub amm_target_orders: Pubkey,
    pub pool_coin_token_account: Pubkey,
    pub pool_pc_token_account: Pubkey,
    pub serum_program: Option<Pubkey>,
    pub serum_market: Option<Pubkey>,
    pub serum_bids: Option<Pubkey>,
    pub serum_asks: Option<Pubkey>,
    pub serum_event_queue: Option<Pubkey>,
    pub serum_coin_vault_account: Option<Pubkey>,
    pub serum_pc_vault_account: Option<Pubkey>,
    pub serum_vault_signer: Option<Pubkey>,
    pub last_updated: u64,
    pub base_reserve: u64,
    pub quote_reserve: u64,
    pub is_account_state_update: bool,
}

#[allow(dead_code)]
impl RaydiumAmmV4PoolState {
    pub fn get_program_id() -> Pubkey {
        Pubkey::new_from_array(*RAYDIUM_AMM_V4_PROGRAM_ID.as_array())
    }

    /// Calculate output amount for PumpFun bonding curve
    pub fn calculate_output_amount(&self, input_token: &Pubkey, input_amount: u64) -> u64 {
        let (base_token, _) = (self.base_mint, self.quote_mint);
        let input_is_base = tokens_equal(input_token, &base_token);
        let (input_reserve, output_reserve) = if input_is_base {
            (self.base_reserve, self.quote_reserve)
        } else {
            (self.quote_reserve, self.base_reserve)
        };
        let new_input_reserve = input_reserve as u128 + input_amount as u128;
        let new_output_reserve =
            (input_reserve as u128 * output_reserve as u128 / new_input_reserve) as u64;
        let output_amount = output_reserve - new_output_reserve;

        output_amount * 9975 / 10000 // Apply 0.25% fee
    }
}
