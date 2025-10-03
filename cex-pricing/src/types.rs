use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenPrice {
    pub symbol: String,
    pub price: f64,
    pub timestamp: i64,
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
pub trait PriceProvider {
    async fn get_price(&self, symbol: &str) -> Option<TokenPrice>;
    async fn get_all_prices(&self) -> Vec<TokenPrice>;
}
