pub mod client;
mod service;

pub use client::KucoinClient;
pub use service::KucoinService;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Symbol {
    pub symbol: String,
    #[serde(rename = "baseCurrency")]
    pub base_currency: String,
    #[serde(rename = "quoteCurrency")]
    pub quote_currency: String,
    #[serde(rename = "enableTrading")]
    pub enable_trading: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolsResponse {
    pub code: String,
    pub data: Vec<Symbol>,
}
