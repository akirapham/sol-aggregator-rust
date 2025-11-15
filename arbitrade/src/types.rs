use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenPrice {
    pub symbol: String,
    pub price: f64,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenPriceUpdate {
    pub price_in_usd: f64,
    pub price_in_native: f64,
    pub token: String,
    pub dex_program_id: String,
    pub pair_address: String,
    pub timestamp: i64,
    pub sol_reserve: String,
    pub token_reserve: String,
    pub index: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DexPriceMessage {
    #[serde(rename = "type")]
    pub message_type: String,
    pub payload: DexPricePayload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DexPricePayload {
    pub data: Vec<TokenPriceUpdate>,
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
pub trait PriceProvider {
    async fn get_price(&self, symbol: &str) -> Option<TokenPrice>;
    async fn get_all_prices(&self) -> Vec<TokenPrice>;
    async fn get_prices(&self, mints: &Vec<String>) -> Vec<Option<TokenPrice>>;
}
