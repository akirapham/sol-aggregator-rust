pub mod client;
mod service;

pub use client::BybitClient;
use serde::{Deserialize, Serialize};
pub use service::BybitService;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstrumentInfo {
    pub symbol: String,
    #[serde(rename = "baseCoin")]
    pub base_coin: String,
    #[serde(rename = "quoteCoin")]
    pub quote_coin: String,
    pub status: String,
    #[serde(rename = "lotSizeFilter", default)]
    pub lot_size_filter: Option<LotSizeFilter>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LotSizeFilter {
    #[serde(rename = "basePrecision")]
    pub base_precision: Option<String>,
    #[serde(rename = "quotePrecision")]
    pub quote_precision: Option<String>,
    #[serde(rename = "minOrderQty")]
    pub min_order_qty: Option<String>,
    #[serde(rename = "maxOrderQty")]
    pub max_order_qty: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstrumentsResponse {
    #[serde(rename = "retCode")]
    pub ret_code: i32,
    #[serde(rename = "retMsg")]
    pub ret_msg: String,
    pub result: InstrumentsResult,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstrumentsResult {
    pub category: String,
    pub list: Vec<InstrumentInfo>,
    #[serde(rename = "nextPageCursor")]
    pub next_page_cursor: Option<String>,
}
