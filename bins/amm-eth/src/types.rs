use eth_dex_quote::{DexVersion, EthChain};
use ethers::types::Address;

use std::sync::{Arc, RwLock};

/// Configuration for Ethereum WebSocket client
#[derive(Debug, Clone)]
pub struct EthConfig {
    pub websocket_url: String,
    pub uniswap_v2_factory: Address,
    pub uniswap_v3_factory: Address,
    pub uniswap_v4_factory: Option<Address>,
    pub weth_address: Address,
    pub usdc_address: Address,
    pub usdt_address: Address,
    pub native_address: Address,
    /// Shared ETH price updated from Binance WebSocket
    pub eth_price_usd: Arc<RwLock<Option<f64>>>,
    pub eth_chain: EthChain,
}

impl Default for EthConfig {
    fn default() -> Self {
        Self {
            websocket_url: std::env::var("ETH_WEBSOCKET_URL")
                .unwrap_or_else(|_| "wss://ethereum-rpc.publicnode.com".to_string()),
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
            usdc_address: "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"
                .parse()
                .unwrap(),
            usdt_address: "0xdAC17F958D2ee523a2206206994597C13D831ec7"
                .parse()
                .unwrap(),
            native_address: "0x0000000000000000000000000000000000000000"
                .parse()
                .unwrap(),
            // ETH price will be updated from Binance WebSocket
            eth_price_usd: Arc::new(RwLock::new(None)),
            eth_chain: EthChain::Mainnet,
        }
    }
}

impl EthConfig {
    /// Create config from ETH_CHAIN environment variable
    /// Defaults to Ethereum mainnet if not set
    pub fn from_env() -> Self {
        let chain = std::env::var("ETH_CHAIN").unwrap_or_else(|_| "ethereum".to_string());
        match chain.to_lowercase().as_str() {
            "arbitrum" => Self::arbitrum(),
            _ => Self::default(),
        }
    }

    /// Arbitrum One configuration
    pub fn arbitrum() -> Self {
        Self {
            websocket_url: std::env::var("ETH_WEBSOCKET_URL")
                .unwrap_or_else(|_| "wss://arbitrum-one-rpc.publicnode.com".to_string()),
            // Arbitrum addresses — Uniswap V3 factory is same as mainnet
            uniswap_v2_factory: "0xc35DADB65012eC5796536bD9864eD8773aBc74C4"
                .parse()
                .unwrap(), // SushiSwap V2 factory on Arbitrum
            uniswap_v3_factory: "0x1F98431c8aD98523631AE4a59f267346ea31F984"
                .parse()
                .unwrap(),
            uniswap_v4_factory: None,
            weth_address: "0x82aF49447D8a07e3bd95BD0d56f35241523fBab1"
                .parse()
                .unwrap(),
            usdc_address: "0xaf88d065e77c8cC2239327C5EDb3A432268e5831"
                .parse()
                .unwrap(), // native USDC
            usdt_address: "0xFd086bC7CD5C481DCC9C85ebE478A1C0b69FCbb9"
                .parse()
                .unwrap(),
            native_address: "0x0000000000000000000000000000000000000000"
                .parse()
                .unwrap(),
            eth_price_usd: Arc::new(RwLock::new(None)),
            eth_chain: EthChain::Arbitrum,
        }
    }

    /// Get known decimals for well-known tokens to avoid RPC calls
    pub fn get_known_decimals(&self, token_address: Address) -> Option<u8> {
        if token_address == self.weth_address || token_address == self.native_address {
            Some(18)
        } else if token_address == self.usdc_address {
            Some(6)
        } else if token_address == self.usdt_address {
            Some(6)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone)]
pub struct PairInfo {
    pub pool_address: String,
    pub pool_token0: Address,
    pub pool_token1: Address,
    pub dex_version: DexVersion,
    pub decimals0: u8,
    pub decimals1: u8,
    pub factory: Address,
    pub fee_tier: Option<u32>,
    pub tick_spacing: Option<i32>,
    pub hooks: Option<Address>,
}
