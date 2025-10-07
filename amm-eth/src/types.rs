use serde::{Deserialize, Serialize};
use ethers::types::Address;

/// Token price information stored in memory
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenPrice {
    pub token_address: Address,
    pub price_in_eth: f64,
    pub price_in_usd: Option<f64>,
    pub last_updated: u64,
    pub pool_address: Address,
    pub dex_version: DexVersion,
}

/// DEX version identifier
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum DexVersion {
    UniswapV2,
    UniswapV3,
    UniswapV4,
}

impl std::fmt::Display for DexVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DexVersion::UniswapV2 => write!(f, "UniswapV2"),
            DexVersion::UniswapV3 => write!(f, "UniswapV3"),
            DexVersion::UniswapV4 => write!(f, "UniswapV4"),
        }
    }
}

use std::sync::{Arc, RwLock};

/// Configuration for Ethereum WebSocket client
#[derive(Debug, Clone)]
pub struct EthConfig {
    pub websocket_url: String,
    pub uniswap_v2_factory: Address,
    pub uniswap_v3_factory: Address,
    pub uniswap_v4_factory: Option<Address>,
    pub weth_address: Address,
    /// Shared ETH price updated from Binance WebSocket
    pub eth_price_usd: Arc<RwLock<Option<f64>>>,
}

impl Default for EthConfig {
    fn default() -> Self {
        Self {
            websocket_url: std::env::var("ETH_WEBSOCKET_URL")
                .unwrap_or_else(|_| "wss://eth-mainnet.g.alchemy.com/v2/your-api-key".to_string()),
            // Mainnet addresses
            uniswap_v2_factory: "0x5C69bEe701ef814a2B6a3EDD4B1652CB9cc5aA6f"
                .parse()
                .unwrap(),
            uniswap_v3_factory: "0x1F98431c8aD98523631AE4a59f267346ea31F984"
                .parse()
                .unwrap(),
            uniswap_v4_factory: None, // Set when V4 is deployed
            weth_address: "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"
                .parse()
                .unwrap(),
            // ETH price will be updated from Binance WebSocket
            eth_price_usd: Arc::new(RwLock::new(None)),
        }
    }
}
