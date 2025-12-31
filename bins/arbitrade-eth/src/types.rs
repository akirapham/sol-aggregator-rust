use serde::{Deserialize, Serialize};

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
