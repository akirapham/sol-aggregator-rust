pub mod pool_state;
pub mod pumpfun;
pub mod pumpswap;
pub mod raydium;

use std::str::FromStr;

pub use pool_state::*;
pub use pumpfun::*;
pub use pumpswap::*;
pub use raydium::*;
use solana_sdk::pubkey::Pubkey;

use serde::{Deserialize, Serialize};

/// Different types of DEXs supported
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DexType {
    PumpFun,
    PumpFunSwap,
    Raydium,
    RaydiumCpmm,
    Orca,
}
