use solana_sdk::pubkey::Pubkey;
use serde::{Deserialize, Serialize};

use crate::{
    constants::wsol,
    pool_data_types::{
        BonkPoolState, DexType, PumpSwapPoolState, PumpfunPoolState, RadyiumClmmPoolState,
        RaydiumAmmV4PoolState, RaydiumCpmmPoolState,
    },
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PoolState {
    PumpfunPoolState(PumpfunPoolState),
    PumpSwapPoolState(PumpSwapPoolState),
    RaydiumAmmV4PoolState(RaydiumAmmV4PoolState),
    RaydiumCpmmPoolState(RaydiumCpmmPoolState),
    BonkPoolState(BonkPoolState),
    RadyiumClmmPoolState(RadyiumClmmPoolState),
}

#[derive(Debug, Clone)]
pub struct PoolStateMetadata {
    pub slot: u64,
    pub transaction_index: Option<u64>,
}

impl PoolState {
    pub fn last_updated(&self) -> u64 {
        match self {
            PoolState::PumpfunPoolState(state) => state.last_updated,
            PoolState::PumpSwapPoolState(state) => state.last_updated,
            PoolState::RaydiumAmmV4PoolState(state) => state.last_updated,
            PoolState::RaydiumCpmmPoolState(state) => state.last_updated,
            PoolState::BonkPoolState(state) => state.last_updated,
            PoolState::RadyiumClmmPoolState(state) => state.last_updated,
        }
    }

    pub fn address(&self) -> Pubkey {
        match self {
            PoolState::PumpfunPoolState(state) => state.address,
            PoolState::PumpSwapPoolState(state) => state.address,
            PoolState::RaydiumAmmV4PoolState(state) => state.address,
            PoolState::RaydiumCpmmPoolState(state) => state.address,
            PoolState::BonkPoolState(state) => state.address,
            PoolState::RadyiumClmmPoolState(state) => state.address,
        }
    }

    pub fn get_tokens(&self) -> (Pubkey, Pubkey) {
        match self {
            PoolState::PumpfunPoolState(state) => (state.mint, wsol()),
            PoolState::PumpSwapPoolState(state) => (state.base_mint, state.quote_mint),
            PoolState::RaydiumAmmV4PoolState(state) => (state.base_mint, state.quote_mint),
            PoolState::RaydiumCpmmPoolState(state) => (state.token0, state.token1),
            PoolState::BonkPoolState(state) => (state.base_mint, state.quote_mint),
            PoolState::RadyiumClmmPoolState(state) => (state.token_mint0, state.token_mint1),
        }
    }

    pub fn dex(&self) -> DexType {
        match self {
            PoolState::PumpfunPoolState(_) => DexType::PumpFun,
            PoolState::PumpSwapPoolState(_) => DexType::PumpFunSwap,
            PoolState::RaydiumAmmV4PoolState(_) => DexType::Raydium,
            PoolState::RaydiumCpmmPoolState(_) => DexType::RaydiumCpmm,
            PoolState::BonkPoolState(_) => DexType::Bonk,
            PoolState::RadyiumClmmPoolState(_) => DexType::RaydiumClmm,
        }
    }

    pub fn get_metadata(&self) -> PoolStateMetadata {
        match self {
            PoolState::PumpfunPoolState(state) => PoolStateMetadata {
                slot: state.slot,
                transaction_index: state.transaction_index,
            },
            PoolState::PumpSwapPoolState(state) => PoolStateMetadata {
                slot: state.slot,
                transaction_index: state.transaction_index,
            },
            PoolState::RaydiumAmmV4PoolState(state) => PoolStateMetadata {
                slot: state.slot,
                transaction_index: state.transaction_index,
            },
            PoolState::RaydiumCpmmPoolState(state) => PoolStateMetadata {
                slot: state.slot,
                transaction_index: state.transaction_index,
            },
            PoolState::BonkPoolState(state) => PoolStateMetadata {
                slot: state.slot,
                transaction_index: state.transaction_index,
            },
            PoolState::RadyiumClmmPoolState(state) => PoolStateMetadata {
                slot: state.slot,
                transaction_index: state.transaction_index,
            },
        }
    }
    pub fn get_reserves(&self) -> (u64, u64) {
        match self {
            PoolState::PumpfunPoolState(state) => (state.token_reserve, state.sol_reserve),
            PoolState::PumpSwapPoolState(state) => (state.base_reserve, state.quote_reserve),
            PoolState::RaydiumAmmV4PoolState(state) => (state.base_reserve, state.quote_reserve),
            PoolState::RaydiumCpmmPoolState(state) => (state.token0_reserve, state.token1_reserve),
            PoolState::BonkPoolState(state) => (state.base_reserve, state.quote_reserve),
            PoolState::RadyiumClmmPoolState(state) => (state.token0_reserve, state.token1_reserve),
        }
    }
}
