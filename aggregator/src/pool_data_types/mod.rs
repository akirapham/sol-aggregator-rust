pub mod pool_state;
pub mod pumpfun;
pub mod pumpswap;
pub mod raydium;
pub mod raydium_cpmm;
pub mod raydium_clmm;
pub mod bonk;

pub use pool_state::*;
pub use pumpfun::*;
pub use pumpswap::*;
pub use raydium::*;
pub use raydium_cpmm::*;
pub use raydium_clmm::*;
pub use bonk::*;

use serde::{Deserialize, Serialize};

/// Different types of DEXs supported
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DexType {
    PumpFun,
    PumpFunSwap,
    Raydium,
    RaydiumCpmm,
    Orca,
    Bonk
}
