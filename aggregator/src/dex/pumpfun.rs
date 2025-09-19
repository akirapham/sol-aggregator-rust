use std::sync::Arc;

use solana_sdk::pubkey::Pubkey;
use solana_streamer_sdk::streaming::event_parser::protocols::pumpfun::parser::PUMPFUN_PROGRAM_ID;

use crate::dex::DexInterface;
use crate::error::Result;
use crate::pool_data_types::PumpfunPoolState;
use crate::utils::*;

/// PumpFun DEX implementation
pub struct PumpFunDex {
    pool_state: Arc<PumpfunPoolState>,
    program_id: Pubkey,
}

impl PumpFunDex {
    pub fn new(pool_state: Arc<PumpfunPoolState>) -> Self {
        Self {
            pool_state,
            program_id: Self::get_program_id(),
        }
    }
    /// Get the bonding curve address for a token
    async fn get_bonding_curve_address(token_mint: &Pubkey) -> Result<Pubkey> {
        // PumpFun uses a deterministic bonding curve address
        // This is a simplified implementation - in reality, you'd need to derive it properly
        let (bonding_curve, _) = Pubkey::find_program_address(
            &[b"bonding-curve", token_mint.as_ref()],
            &Self::get_program_id(),
        );
        Ok(bonding_curve)
    }
}

impl DexInterface for PumpFunDex {
    fn get_program_id() -> Pubkey {
        Pubkey::new_from_array(*PUMPFUN_PROGRAM_ID.as_array())
    }

    /// Calculate output amount for PumpFun bonding curve
    fn calculate_output_amount(&self, input_token: &Pubkey, input_amount: u64) -> u64 {
        let is_buy = tokens_equal(input_token, &get_sol_mint());
        let (token_reserve, sol_reserve) =
            (self.pool_state.token_reserve, self.pool_state.sol_reserve);
        let output_amount = if is_buy {
            let new_sol_reserve = sol_reserve + input_amount;
            let new_token_reserve =
                (token_reserve as u128 * sol_reserve as u128 / new_sol_reserve as u128) as u64;
            token_reserve - new_token_reserve
        } else {
            let new_token_reserve = token_reserve + input_amount;
            let new_sol_reserve =
                (sol_reserve as u128 * token_reserve as u128 / new_token_reserve as u128) as u64;
            sol_reserve - new_sol_reserve
        };

        output_amount * 99 / 100 // Apply 1% fee
    }

    fn get_pool_address(&self) -> Pubkey {
        self.pool_state.address
    }

    fn get_dex(&self) -> crate::pool_data_types::DexType {
        crate::pool_data_types::DexType::PumpFun
    }
}
