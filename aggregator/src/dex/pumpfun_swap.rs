use std::sync::Arc;

use async_trait::async_trait;
use rust_decimal::Decimal;
use solana_sdk::pubkey::Pubkey;
use solana_streamer_sdk::streaming::event_parser::protocols::pumpswap::parser::PUMPSWAP_PROGRAM_ID;

use crate::dex::traits::DexInterface;
use crate::error::Result;
use crate::pool_data_types::{DexType, PumpSwapPoolState};
use crate::types::{SwapParams, SwapRoute};
use crate::utils::*;

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
        Pubkey::new_from_array(PUMPSWAP_PROGRAM_ID.as_array().clone())
    }

    /// Calculate output amount for PumpFun bonding curve
    fn calculate_output_amount(&self, input_token: &Pubkey, input_amount: u64) -> u64 {
        0
    }
}

// #[async_trait]
// impl DexInterface for PumpFunDex {
//     async fn get_quote(&self, params: &SwapParams) -> Result<Option<SwapRoute>> {
//         let sol_mint = parse_pubkey("So11111111111111111111111111111111111111112")?;

//         // PumpFun only supports SOL <-> Token swaps
//         if !tokens_equal(&params.input_token.address, &sol_mint)
//             && !tokens_equal(&params.output_token.address, &sol_mint)
//         {
//             return Ok(None);
//         }

//         let output_amount = self
//             .calculate_output_amount(&params.input_token.address, params.input_amount);

//         let input_token = params.input_token.clone();
//         let output_token = params.output_token.clone();

//         // Calculate price impact (simplified)
//         let price_impact = calculate_price_impact(
//             params.input_amount,
//             output_amount,
//             Decimal::new(1, 0), // Placeholder market price
//         )?;

//         let route = SwapRoute {
//             dex: DexType::PumpFun,
//             input_token: input_token,
//             output_token: output_token,
//             input_amount: params.input_amount,
//             output_amount,
//             price_impact,
//             route_path: vec![self.pool_state.address],
//             mev_risk: crate::types::MevRisk::Medium, // MEV risk assessment
//             liquidity_depth: 0, // Bonding curve doesn't have traditional liquidity
//         };

//         Ok(Some(route))
//     }
// }
