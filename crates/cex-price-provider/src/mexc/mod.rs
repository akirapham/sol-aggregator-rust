mod client;
mod service;
use serde::{Deserialize, Serialize};
pub use service::MexcService;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolInfo {
    pub symbol: String,
    #[serde(rename = "baseAsset")]
    pub base_asset: String,
    #[serde(rename = "quoteAsset")]
    pub quote_asset: String,
    pub status: String, // "1" = enabled, "0" = disabled
    #[serde(rename = "contractAddress")]
    pub contract_address: String,
    pub permissions: Vec<String>,
    #[serde(rename = "isSpotTradingAllowed", default)]
    pub is_spot_trading_allowed: bool,
    #[serde(rename = "baseAssetPrecision", default)]
    pub base_asset_precision: Option<u32>, // Precision for base asset (quantity)
    #[serde(rename = "quoteAssetPrecision", default)]
    pub quote_asset_precision: Option<u32>, // Precision for quote asset (price)
}

impl SymbolInfo {
    /// Check if trading is enabled for this symbol
    pub fn is_trading_enabled(&self) -> bool {
        self.status == "1" && self.is_spot_trading_allowed
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExchangeInfo {
    pub timezone: String,
    #[serde(rename = "serverTime")]
    pub server_time: u64,
    pub symbols: Vec<SymbolInfo>,
}
