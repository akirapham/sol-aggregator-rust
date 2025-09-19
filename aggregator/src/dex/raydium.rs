use std::sync::Arc;
use solana_sdk::pubkey::Pubkey;
use solana_streamer_sdk::streaming::event_parser::protocols::raydium_amm_v4::parser::RAYDIUM_AMM_V4_PROGRAM_ID;

use crate::{dex::DexInterface, pool_data_types::RaydiumAmmV4PoolState, utils::tokens_equal};

pub struct RaydiumAmmV4Dex {
    pool_state: Arc<RaydiumAmmV4PoolState>,
    program_id: Pubkey,
}

impl RaydiumAmmV4Dex {
    pub fn new(pool_state: Arc<RaydiumAmmV4PoolState>) -> Self {
        Self {
            pool_state,
            program_id: Self::get_program_id(),
        }
    }
}

impl DexInterface for RaydiumAmmV4Dex {
    fn get_program_id() -> Pubkey {
        Pubkey::new_from_array(*RAYDIUM_AMM_V4_PROGRAM_ID.as_array())
    }

    /// Calculate output amount for PumpFun bonding curve
    fn calculate_output_amount(&self, input_token: &Pubkey, input_amount: u64) -> u64 {
        let (base_token, quote_token) = (self.pool_state.base_mint, self.pool_state.quote_mint);
        let input_is_base = tokens_equal(input_token, &base_token);
        let (input_reserve, output_reserve) = if input_is_base {
            (self.pool_state.base_reserve, self.pool_state.quote_reserve)
        } else {
            (self.pool_state.quote_reserve, self.pool_state.base_reserve)
        };
        let new_input_reserve = input_reserve as u128 + input_amount as u128;
        let new_output_reserve =
                (input_reserve as u128 * output_reserve as u128 / new_input_reserve) as u64;
        let output_amount = output_reserve - new_output_reserve;

        output_amount * 9975 / 10000 // Apply 0.25% fee
    }
}
