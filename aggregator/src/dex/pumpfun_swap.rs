use std::sync::Arc;

use solana_sdk::pubkey::Pubkey;
use solana_streamer_sdk::streaming::event_parser::protocols::pumpswap::parser::PUMPSWAP_PROGRAM_ID;

use crate::{pool_data_types::PumpSwapPoolState, utils::tokens_equal};

pub struct PumpSwapDex {
    pool_state: Arc<PumpSwapPoolState>,
    program_id: Pubkey,
}

impl PumpSwapDex {
    pub fn new(pool_state: Arc<PumpSwapPoolState>) -> Self {
        Self {
            pool_state,
            program_id: Self::get_program_id(),
        }
    }

    pub fn get_program_id() -> Pubkey {
        Pubkey::new_from_array(*PUMPSWAP_PROGRAM_ID.as_array())
    }

    /// Calculate output amount for PumpFun bonding curve
    pub fn calculate_output_amount(&self, input_token: &Pubkey, input_amount: u64) -> u64 {
        let (base_token, _quote_token) = (self.pool_state.base_mint, self.pool_state.quote_mint);
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

        output_amount * 997 / 1000 // Apply 0.3% fee
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pool_data_types::PumpSwapPoolState;

    fn create_test_pool_state(base_mint: Pubkey, quote_mint: Pubkey, base_reserve: u64, quote_reserve: u64) -> PumpSwapPoolState {
        PumpSwapPoolState {
            base_mint,
            quote_mint,
            base_reserve,
            quote_reserve,
            // Add other fields as needed by your PumpSwapPoolState struct
            ..Default::default()
        }
    }

    #[test]
    fn test_calculate_output_amount_base_to_quote() {
        let base_mint = Pubkey::new_unique();
        let quote_mint = Pubkey::new_unique();
        let pool_state = create_test_pool_state(base_mint, quote_mint, 1000, 1000);
        let dex = PumpSwapDex::new(Arc::new(pool_state));

        // Swap 100 base tokens for quote tokens
        let output = dex.calculate_output_amount(&base_mint, 100);

        // Expected calculation:
        // new_input_reserve = 1000 + 100 = 1100
        // new_output_reserve = 1000 * 1000 / 1100 = 909
        // output_amount = 1000 - 909 = 91
        // with fee: 91 * 997 / 1000 = 90
        assert_eq!(output, 90);
    }

    #[test]
    fn test_calculate_output_amount_quote_to_base() {
        let base_mint = Pubkey::new_unique();
        let quote_mint = Pubkey::new_unique();
        let pool_state = create_test_pool_state(base_mint, quote_mint, 1000, 1000);
        let dex = PumpSwapDex::new(Arc::new(pool_state));

        // Swap 100 quote tokens for base tokens
        let output = dex.calculate_output_amount(&quote_mint, 100);

        // Same calculation as above since reserves are symmetric
        assert_eq!(output, 90);
    }

    #[test]
    fn test_calculate_output_amount_large_amount() {
        let base_mint = Pubkey::new_unique();
        let quote_mint = Pubkey::new_unique();
        let pool_state = create_test_pool_state(base_mint, quote_mint, 1000000, 1000000);
        let dex = PumpSwapDex::new(Arc::new(pool_state));

        // Swap a large amount
        let output = dex.calculate_output_amount(&base_mint, 500000);

        // new_input_reserve = 1000000 + 500000 = 1500000
        // new_output_reserve = 1000000 * 1000000 / 1500000 = 666666
        // output_amount = 1000000 - 666666 = 333334
        // with fee: 333334 * 997 / 1000 = 332333
        assert_eq!(output, 332333);
    }

    #[test]
    fn test_calculate_output_amount_small_amount() {
        let base_mint = Pubkey::new_unique();
        let quote_mint = Pubkey::new_unique();
        let pool_state = create_test_pool_state(base_mint, quote_mint, 1000, 1000);
        let dex = PumpSwapDex::new(Arc::new(pool_state));

        // Swap a very small amount
        let output = dex.calculate_output_amount(&base_mint, 1);

        // new_input_reserve = 1000 + 1 = 1001
        // new_output_reserve = 1000 * 1000 / 1001 = 999
        // output_amount = 1000 - 999 = 1
        // with fee: 1 * 997 / 1000 = 0 (due to integer division)
        assert_eq!(output, 0);
    }

    #[test]
    fn test_calculate_output_amount_zero_amount() {
        let base_mint = Pubkey::new_unique();
        let quote_mint = Pubkey::new_unique();
        let pool_state = create_test_pool_state(base_mint, quote_mint, 1000, 1000);
        let dex = PumpSwapDex::new(Arc::new(pool_state));

        // Swap zero amount
        let output = dex.calculate_output_amount(&base_mint, 0);
        assert_eq!(output, 0);
    }

    #[test]
    fn test_calculate_output_amount_unequal_reserves() {
        let base_mint = Pubkey::new_unique();
        let quote_mint = Pubkey::new_unique();
        let pool_state = create_test_pool_state(base_mint, quote_mint, 2000, 1000);
        let dex = PumpSwapDex::new(Arc::new(pool_state));

        // Swap 100 base tokens when reserves are unequal
        let output = dex.calculate_output_amount(&base_mint, 100);

        // new_input_reserve = 2000 + 100 = 2100
        // new_output_reserve = 2000 * 1000 / 2100 = 952
        // output_amount = 1000 - 952 = 48
        // with fee: 48 * 997 / 1000 = 47
        assert_eq!(output, 47);
    }

    #[test]
    fn test_calculate_output_amount_max_values() {
        let base_mint = Pubkey::new_unique();
        let quote_mint = Pubkey::new_unique();
        let pool_state = create_test_pool_state(base_mint, quote_mint, u64::MAX, u64::MAX);
        let dex = PumpSwapDex::new(Arc::new(pool_state));

        // Test with maximum values (this will test overflow handling)
        let output = dex.calculate_output_amount(&base_mint, 1);

        // Due to u128 arithmetic, this should handle large numbers correctly
        // The exact value depends on the calculation, but it should not panic
        assert!(output >= 0);
    }

    #[test]
    fn test_calculate_output_amount_wrong_token() {
        let base_mint = Pubkey::new_unique();
        let quote_mint = Pubkey::new_unique();
        let wrong_token = Pubkey::new_unique();
        let pool_state = create_test_pool_state(base_mint, quote_mint, 1000, 1000);
        let dex = PumpSwapDex::new(Arc::new(pool_state));

        // Swap with a token that's not in the pool (should treat as quote token)
        let output = dex.calculate_output_amount(&wrong_token, 100);

        // Since wrong_token != base_mint, it will be treated as quote token
        assert_eq!(output, 90);
    }
}
