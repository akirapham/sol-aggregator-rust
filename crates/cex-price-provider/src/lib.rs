pub mod bitget;
pub mod bybit;
pub mod gate;
pub mod kucoin;
pub mod mexc;
use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterAddressType {
    Ethereum,
    Solana,
}

/// Represents the trading and deposit status of a token on an exchange
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenStatus {
    pub symbol: String,                // Trading pair symbol (e.g., "BTCUSDT")
    pub base_asset: String,             // Base currency (e.g., "BTC")
    pub contract_address: Option<String>, // Token contract address
    pub is_trading: bool,               // Whether trading is enabled
    pub is_deposit_enabled: bool,       // Whether deposits are enabled on the correct network
    pub network_verified: bool,         // Whether network matches filter (ERC20 for Ethereum, Solana chain for Solana)
    pub last_updated: u64,              // Unix timestamp of last update
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenPrice {
    pub symbol: String,
    pub price: f64,
}

#[async_trait]
#[allow(dead_code)]
pub trait PriceProvider {
    async fn get_price(&self, symbol: &str) -> Option<TokenPrice>;
    async fn get_all_prices(&self) -> Vec<TokenPrice>;
    async fn get_prices(&self, mints: &Vec<String>) -> Vec<Option<TokenPrice>>;
    async fn start(&self) -> Result<()>;
    fn get_price_provider_name(&self) -> &'static str;

    /// Check if a token is safe for arbitrage:
    /// - Trading must be enabled
    /// - Deposits must be enabled on the correct network (ERC20 for Ethereum, Solana chain for Solana)
    /// Returns true only if ALL conditions are met
    async fn is_token_safe_for_arbitrage(&self, symbol: &str, contract_address: Option<&str>) -> bool;

    /// Get the status of a token (trading, deposit, network verification)
    async fn get_token_status(&self, symbol: &str, contract_address: Option<&str>) -> Option<TokenStatus>;

    /// Refresh the token status cache and return list of safe trading pair symbols
    /// (should be called periodically, e.g., every 12 hours)
    /// Returns: Vec of trading pair symbols that passed all safety checks
    async fn refresh_token_status(&self) -> Result<Vec<String>>;
}
