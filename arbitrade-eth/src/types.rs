use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenPrice {
    pub symbol: String,
    pub price: f64,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenPriceUpdate {
    pub token_address: String,
    pub price_in_eth: f64,
    pub price_in_usd: f64,
    pub last_updated: u64,
    pub pool_address: String,
    pub dex_version: String,
    pub decimals: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DexPriceMessage {
    #[serde(rename = "type")]
    pub r#type: String,
    pub data: TokenPriceUpdate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DexSubscriptionMessage {
    pub topics: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExchangeInfo {
    pub timezone: String,
    #[serde(rename = "serverTime")]
    pub server_time: u64,
    pub symbols: Vec<SymbolInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolInfo {
    pub symbol: String,
    #[serde(rename = "baseAsset")]
    pub base_asset: String,
    #[serde(rename = "quoteAsset")]
    pub quote_asset: String,
    pub status: String,
    #[serde(rename = "contractAddress")]
    pub contract_address: String,
    pub permissions: Vec<String>,
}

use async_trait::async_trait;

#[async_trait]
#[allow(dead_code)]
pub trait PriceProvider {
    async fn get_price(&self, symbol: &str) -> Option<TokenPrice>;
    async fn get_all_prices(&self) -> Vec<TokenPrice>;
    async fn get_prices(&self, mints: &Vec<String>) -> Vec<Option<TokenPrice>>;
}
