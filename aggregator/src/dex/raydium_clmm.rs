use solana_sdk::pubkey::Pubkey;
use solana_streamer_sdk::streaming::event_parser::protocols::raydium_clmm::parser::RAYDIUM_CLMM_PROGRAM_ID;
use std::sync::Arc;

use crate::{dex::DexInterface, pool_data_types::RaydiumClmmPoolState, utils::tokens_equal};

const MIN_SQRT_PRICE_X64: u128 = 4295048016;
const MAX_SQRT_PRICE_X64: u128 = 79226673521066979257578248091;

pub struct RaydiumClmmDex {
    pool_state: Arc<RaydiumClmmPoolState>,
    program_id: Pubkey,
}

impl RaydiumClmmDex {
    pub fn new(pool_state: Arc<RaydiumClmmPoolState>) -> Self {
        Self {
            pool_state,
            program_id: Self::get_program_id(),
        }
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

impl DexInterface for RaydiumClmmDex {
    fn get_program_id() -> Pubkey {
        Pubkey::new_from_array(*RAYDIUM_CLMM_PROGRAM_ID.as_array())
    }

    /// Calculate output amount for PumpFun bonding curve
    fn calculate_output_amount(&self, input_token: &Pubkey, input_amount: u64) -> u64 {
        let (token0, _) = (self.pool_state.token_mint0, self.pool_state.token_mint1);
        let input_is_token0 = tokens_equal(input_token, &token0);
        let sqrt_price_limit_x64 = if input_is_token0 {
            MIN_SQRT_PRICE_X64 + 1
        } else {
            79226673521066979257578248091 - 1
        };

        // dont take transfer tax into account for now, users should account for it un their slippage
        let real_input_amount = input_amount;
        self.get_output_amount(real_input_amount, input_is_token0, sqrt_price_limit_x64)
    }

    fn get_pool_address(&self) -> Pubkey {
        self.pool_state.address
    }

    fn get_dex(&self) -> crate::pool_data_types::DexType {
        crate::pool_data_types::DexType::RaydiumClmm
    }
}
