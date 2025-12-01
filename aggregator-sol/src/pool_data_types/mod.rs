pub mod bonk;
pub mod clmm;
pub mod common;
pub mod dbc;
pub mod meteora_dammv2;
pub mod orca_whirlpool;
pub mod pool_state;
pub mod pumpf;
pub mod pumpfun;
pub mod pumpswap;
pub mod raydium;
pub mod raydium_clmm;
pub mod raydium_cpmm;
pub mod traits;

pub use bonk::*;
pub use dbc::*;
pub use meteora_dammv2::*;
pub use orca_whirlpool::*;
pub use pool_state::*;

pub use pumpfun::*;
pub use pumpswap::*;
pub use raydium::*;
pub use raydium_clmm::*;
pub use raydium_cpmm::*;
pub use traits::*;

use serde::{Deserialize, Serialize};

/// Different types of DEXs supported
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DexType {
    PumpFun,
    PumpFunSwap,
    Raydium,
    RaydiumCpmm,
    Orca,
    Bonk,
    RaydiumClmm,
    MeteoraDbc,
    MeteoraDammv2,
}
