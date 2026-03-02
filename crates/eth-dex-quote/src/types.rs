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
    pub tick_spacing: Option<i32>,
    pub eth_price_usd: f64,
    pub hooks: Option<Address>,
    /// V2 reserve0 (serialized U256) - only populated for V2 pools
    pub reserve0: Option<String>,
    /// V2 reserve1 (serialized U256) - only populated for V2 pools
    pub reserve1: Option<String>,
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
    CamelotAlgebra,
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
            DexVersion::CamelotAlgebra => "camelot_algebra",
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
            "camelot_algebra" => Ok(DexVersion::CamelotAlgebra),
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

/// Token price update message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenPriceUpdate {
    pub token_address: String,
    pub price_in_eth: f64,
    pub price_in_usd: Option<f64>,
    pub last_updated: u64,
    pub pool_address: String,
    pub dex_version: String,
    pub decimals: u8,
    pub pool_token0: Address,
    pub pool_token1: Address,
    pub eth_chain: EthChain,
    pub fee_tier: Option<u32>,
    pub tick_spacing: Option<i32>,
    pub eth_price_usd: f64,
    pub hooks: Option<Address>,
    /// V2 reserve0 (serialized U256) - only populated for V2 pools
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reserve0: Option<String>,
    /// V2 reserve1 (serialized U256) - only populated for V2 pools
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reserve1: Option<String>,
}

impl From<TokenPrice> for TokenPriceUpdate {
    fn from(price: TokenPrice) -> Self {
        Self {
            token_address: format!("{:?}", price.token_address),
            price_in_eth: price.price_in_eth,
            price_in_usd: price.price_in_usd,
            last_updated: price.last_updated,
            pool_address: price.pool_address.to_string(),
            dex_version: format!("{:?}", price.dex_version),
            decimals: price.decimals,
            pool_token0: price.pool_token0,
            pool_token1: price.pool_token1,
            eth_chain: price.eth_chain,
            fee_tier: price.fee_tier,
            tick_spacing: price.tick_spacing,
            eth_price_usd: price.eth_price_usd,
            hooks: price.hooks,
            reserve0: price.reserve0,
            reserve1: price.reserve1,
        }
    }
}
