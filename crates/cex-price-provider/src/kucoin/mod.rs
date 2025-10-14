pub mod client;
mod service;

pub use client::KucoinClient;
use serde::{Deserialize, Serialize};
pub use service::KucoinService;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Symbol {
    pub symbol: String,
    #[serde(rename = "baseCurrency")]
    pub base_currency: String,
    #[serde(rename = "quoteCurrency")]
    pub quote_currency: String,
    #[serde(rename = "enableTrading")]
    pub enable_trading: bool,
    #[serde(rename = "baseIncrement", default)]
    pub base_increment: Option<String>, // Min quantity increment (e.g., "0.01")
    #[serde(rename = "quoteIncrement", default)]
    pub quote_increment: Option<String>, // Min price increment
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolsResponse {
    pub code: String,
    pub data: Vec<Symbol>,
}
