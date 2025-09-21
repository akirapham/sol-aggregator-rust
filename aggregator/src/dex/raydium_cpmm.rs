use solana_sdk::pubkey::Pubkey;
use solana_streamer_sdk::streaming::event_parser::protocols::raydium_cpmm::parser::RAYDIUM_CPMM_PROGRAM_ID;
use std::sync::Arc;

use crate::{dex::DexInterface, pool_data_types::RaydiumCpmmPoolState, utils::tokens_equal};

pub struct RaydiumCpmmDex {
    pool_state: Arc<RaydiumCpmmPoolState>,
    program_id: Pubkey,
}

impl RaydiumCpmmDex {
    pub fn new(pool_state: Arc<RaydiumCpmmPoolState>) -> Self {
        Self {
            pool_state,
            program_id: Self::get_program_id(),
        }
    }
}

impl DexInterface for RaydiumCpmmDex {
    fn get_program_id() -> Pubkey {
        Pubkey::new_from_array(*RAYDIUM_CPMM_PROGRAM_ID.as_array())
    }

    /// Calculate output amount for PumpFun bonding curve
    fn calculate_output_amount(&self, input_token: &Pubkey, input_amount: u64) -> u64 {
        let (base_token, _) = (self.pool_state.token0, self.pool_state.token1);
        let input_is_base = tokens_equal(input_token, &base_token);
        let (input_reserve, output_reserve) = if input_is_base {
            (
                self.pool_state.token0_reserve,
                self.pool_state.token1_reserve,
            )
        } else {
            (
                self.pool_state.token1_reserve,
                self.pool_state.token0_reserve,
            )
        };
        let new_input_reserve = input_reserve as u128 + input_amount as u128;
        let new_output_reserve =
            (input_reserve as u128 * output_reserve as u128 / new_input_reserve) as u64;
        let output_amount = output_reserve - new_output_reserve;

        output_amount * 9975 / 10000 // Apply 0.25% fee
    }

    fn get_pool_address(&self) -> Pubkey {
        self.pool_state.address
    }

    fn get_dex(&self) -> crate::pool_data_types::DexType {
        crate::pool_data_types::DexType::RaydiumCpmm
    }
}
