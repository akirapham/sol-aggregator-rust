use eth_dex_quote::EthChain;
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
