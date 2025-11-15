use crate::types::{ExchangeInfo, SymbolInfo};
use anyhow::{Context, Result};
use reqwest::Client;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

pub struct MexcClient {
    client: Client,
    base_url: String,
}

impl MexcClient {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
            base_url: "https://api.mexc.com".to_string(),
        }
    }

    pub async fn get_exchange_info(&self) -> Result<ExchangeInfo> {
        let url = format!("{}/api/v3/exchangeInfo", self.base_url);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to send request to MEXC API")?;

        let exchange_info: ExchangeInfo = response
            .json()
            .await
            .context("Failed to parse exchange info response")?;

        Ok(exchange_info)
    }

    pub async fn get_solana_usdt_pairs(&self) -> Result<Vec<SymbolInfo>> {
        let exchange_info = self.get_exchange_info().await?;

        let symbols: Vec<SymbolInfo> = exchange_info
            .symbols
            .into_iter()
            .filter(|symbol| {
                // Check if it's paired with USDT and is a Solana token
                symbol.quote_asset == "USDT"
                    && symbol.status == "1"
                    && Pubkey::from_str(&symbol.contract_address).is_ok()
                    && symbol.permissions.contains(&"SPOT".to_string())
            })
            .collect();

        log::info!(
            "Found {} Solana tokens based on contract addresses",
            symbols.len()
        );

        Ok(symbols)
    }
}
