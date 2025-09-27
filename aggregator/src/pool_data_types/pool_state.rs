use std::sync::Arc;

use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;

use crate::{
    constants::wsol,
    pool_data_types::{
        BonkPoolState, DexType, GetAmmConfig, PumpSwapPoolState, PumpfunPoolState,
        RaydiumAmmV4PoolState, RaydiumClmmPoolState, RaydiumCpmmPoolState,
    },
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PoolState {
    Pumpfun(PumpfunPoolState),
    PumpSwap(PumpSwapPoolState),
    RaydiumAmmV4(RaydiumAmmV4PoolState),
    RaydiumCpmm(RaydiumCpmmPoolState),
    Bonk(BonkPoolState),
    RadyiumClmm(RaydiumClmmPoolState),
}

#[derive(Debug, Clone, Copy, Eq, Hash, PartialEq)]
pub enum PoolUpdateEventType {
    PumpFunTrade,
    PumpFunMigrate,
    PumpFunCreateToken,
    PumpSwapBuy,
    PumpSwapSell,
    PumpSwapCreatePool,
    PumpSwapDeposit,
    PumpSwapWithdraw,
    RaydiumCpmmSwap,
    RaydiumCpmmDeposit,
    RaydiumCpmmInitialize,
    RaydiumCpmmWithdraw,
    RaydiumClmmSwap,
    RaydiumClmmSwapV2,
    RaydiumClmmClosePosition,
    RaydiumClmmDecreaseLiquidityV2,
    RaydiumClmmIncreaseLiquidityV2,
    RaydiumClmmOpenPositionWithToken22Nft,
    RaydiumClmmOpenPositionV2,
    RaydiumAmmV4Swap,
    RaydiumAmmV4Deposit,
    RaydiumAmmV4Initialize2,
    RaydiumAmmV4Withdraw,
    RaydiumAmmV4WithdrawPnl,
    BonkPoolStateAccount,
    PumpSwapPoolAccount,
    RaydiumClmmPoolStateAccount,
    RaydiumClmmTickArrayStateAccount,
    RaydiumClmmTickArrayBitmapExtensionAccount,
    RaydiumCpmmPoolStateAccount,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct PoolStateMetadata {
    pub slot: u64,
    pub transaction_index: Option<u64>,
}

impl PoolState {
    pub fn last_updated(&self) -> u64 {
        match self {
            PoolState::Pumpfun(state) => state.last_updated,
            PoolState::PumpSwap(state) => state.last_updated,
            PoolState::RaydiumAmmV4(state) => state.last_updated,
            PoolState::RaydiumCpmm(state) => state.last_updated,
            PoolState::Bonk(state) => state.last_updated,
            PoolState::RadyiumClmm(state) => state.last_updated,
        }
    }

    pub fn address(&self) -> Pubkey {
        match self {
            PoolState::Pumpfun(state) => state.address,
            PoolState::PumpSwap(state) => state.address,
            PoolState::RaydiumAmmV4(state) => state.address,
            PoolState::RaydiumCpmm(state) => state.address,
            PoolState::Bonk(state) => state.address,
            PoolState::RadyiumClmm(state) => state.address,
        }
    }

    pub fn get_tokens(&self) -> (Pubkey, Pubkey) {
        match self {
            PoolState::Pumpfun(state) => (state.mint, wsol()),
            PoolState::PumpSwap(state) => (state.base_mint, state.quote_mint),
            PoolState::RaydiumAmmV4(state) => (state.base_mint, state.quote_mint),
            PoolState::RaydiumCpmm(state) => (state.token0, state.token1),
            PoolState::Bonk(state) => (state.base_mint, state.quote_mint),
            PoolState::RadyiumClmm(state) => (state.token_mint0, state.token_mint1),
        }
    }

    pub fn dex(&self) -> DexType {
        match self {
            PoolState::Pumpfun(_) => DexType::PumpFun,
            PoolState::PumpSwap(_) => DexType::PumpFunSwap,
            PoolState::RaydiumAmmV4(_) => DexType::Raydium,
            PoolState::RaydiumCpmm(_) => DexType::RaydiumCpmm,
            PoolState::Bonk(_) => DexType::Bonk,
            PoolState::RadyiumClmm(_) => DexType::RaydiumClmm,
        }
    }

    pub fn get_metadata(&self) -> PoolStateMetadata {
        match self {
            PoolState::Pumpfun(state) => PoolStateMetadata {
                slot: state.slot,
                transaction_index: state.transaction_index,
            },
            PoolState::PumpSwap(state) => PoolStateMetadata {
                slot: state.slot,
                transaction_index: state.transaction_index,
            },
            PoolState::RaydiumAmmV4(state) => PoolStateMetadata {
                slot: state.slot,
                transaction_index: state.transaction_index,
            },
            PoolState::RaydiumCpmm(state) => PoolStateMetadata {
                slot: state.slot,
                transaction_index: state.transaction_index,
            },
            PoolState::Bonk(state) => PoolStateMetadata {
                slot: state.slot,
                transaction_index: state.transaction_index,
            },
            PoolState::RadyiumClmm(state) => PoolStateMetadata {
                slot: state.slot,
                transaction_index: state.transaction_index,
            },
        }
    }
    pub fn get_reserves(&self) -> (u64, u64) {
        match self {
            PoolState::Pumpfun(state) => (state.token_reserve, state.sol_reserve),
            PoolState::PumpSwap(state) => (state.base_reserve, state.quote_reserve),
            PoolState::RaydiumAmmV4(state) => (state.base_reserve, state.quote_reserve),
            PoolState::RaydiumCpmm(state) => (state.token0_reserve, state.token1_reserve),
            PoolState::Bonk(state) => (state.base_reserve, state.quote_reserve),
            PoolState::RadyiumClmm(state) => (state.token0_reserve, state.token1_reserve),
        }
    }

    pub fn get_liquidity_usd(&self) -> f64 {
        match self {
            PoolState::Pumpfun(state) => state.liquidity_usd,
            PoolState::PumpSwap(state) => state.liquidity_usd,
            PoolState::RaydiumAmmV4(state) => state.liquidity_usd,
            PoolState::RaydiumCpmm(state) => state.liquidity_usd,
            PoolState::Bonk(state) => state.liquidity_usd,
            PoolState::RadyiumClmm(state) => state.liquidity_usd,
        }
    }

    pub async fn calculate_output_amount(
        &self,
        input_token: &Pubkey,
        input_amount: u64,
        amm_confi_fetcher: Arc<dyn GetAmmConfig>,
    ) -> u64 {
        match self {
            PoolState::Pumpfun(state) => {
                state.calculate_output_amount(input_token, input_amount, amm_confi_fetcher)
            }
            PoolState::PumpSwap(state) => {
                state.calculate_output_amount(input_token, input_amount, amm_confi_fetcher)
            }
            PoolState::RaydiumAmmV4(state) => {
                state.calculate_output_amount(input_token, input_amount, amm_confi_fetcher)
            }
            PoolState::RaydiumCpmm(state) => {
                state.calculate_output_amount(input_token, input_amount, amm_confi_fetcher)
            }
            PoolState::Bonk(state) => {
                state.calculate_output_amount(input_token, input_amount, amm_confi_fetcher)
            }
            PoolState::RadyiumClmm(state) => {
                state
                    .calculate_output_amount(input_token, input_amount, amm_confi_fetcher)
                    .await
            }
        }
    }
}
