pub mod bybit;
pub mod kucoin;
pub mod mexc;
use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub enum FilterAddressType {
    Ethereum,
    Solana,
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
}
