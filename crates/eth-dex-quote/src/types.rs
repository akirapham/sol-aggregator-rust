use ethers::types::{Address, U256};
use serde::{Deserialize, Serialize};
use std::str::FromStr;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reserve {
    pub reserve0: U256,
    pub reserve1: U256,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DexType {
    UniswapV2,
    UniswapV3,
    UniswapV4,
}

#[derive(Debug, Clone)]
pub struct SwapQuote {
    pub amount_out: U256,
    pub route: Vec<Address>,
    pub dex: DexType,
}

#[derive(Debug, thiserror::Error)]
pub enum QuoteError {
    #[error("RPC error: {0}")]
    RpcError(String),

    #[error("Contract error: {0}")]
    ContractError(String),

    #[error("Invalid path")]
    InvalidPath,

    #[error("No liquidity")]
    NoLiquidity,

    #[error("Computation error: {0}")]
    ComputationError(String),
}

pub type Result<T> = std::result::Result<T, QuoteError>;

/// Token price information stored in memory
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenPrice {
    pub token_address: Address,
    pub price_in_eth: f64,
    pub price_in_usd: Option<f64>,
    pub last_updated: u64,
    pub pool_address: String, // use String to store cover both address type and pool id in V4
    pub dex_version: DexVersion,
    pub decimals: u8,
    pub pool_token0: Address,
    pub pool_token1: Address,
    pub eth_chain: EthChain,
    pub fee_tier: Option<u32>,
}

/// DEX type identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DexVersion {
    UniswapV2,
    UniswapV3,
    UniswapV4,
    SushiswapV2,
    SushiswapV3,
    PancakeswapV2,
    PancakeswapV3,
}

impl DexVersion {
    pub fn as_str(&self) -> &'static str {
        match self {
            DexVersion::UniswapV2 => "uniswap_v2",
            DexVersion::UniswapV3 => "uniswap_v3",
            DexVersion::UniswapV4 => "uniswap_v4",
            DexVersion::SushiswapV2 => "sushiswap_v2",
            DexVersion::SushiswapV3 => "sushiswap_v3",
            DexVersion::PancakeswapV2 => "pancakeswap_v2",
            DexVersion::PancakeswapV3 => "pancakeswap_v3",
        }
    }
}

impl FromStr for DexVersion {
    type Err = QuoteError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "uniswap_v2" => Ok(DexVersion::UniswapV2),
            "uniswap_v3" => Ok(DexVersion::UniswapV3),
            "uniswap_v4" => Ok(DexVersion::UniswapV4),
            "sushiswap_v2" => Ok(DexVersion::SushiswapV2),
            "sushiswap_v3" => Ok(DexVersion::SushiswapV3),
            "pancakeswap_v2" => Ok(DexVersion::PancakeswapV2),
            "pancakeswap_v3" => Ok(DexVersion::PancakeswapV3),
            _ => Err(QuoteError::ContractError(format!(
                "Unknown DexVersion: {}",
                s
            ))),
        }
    }
}

// Chain specifier
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum EthChain {
    Mainnet,
    Base,
    Arbitrum,
}

impl std::fmt::Display for EthChain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EthChain::Mainnet => write!(f, "Mainnet"),
            EthChain::Base => write!(f, "Base"),
            EthChain::Arbitrum => write!(f, "Arbitrum"),
        }
    }
}
