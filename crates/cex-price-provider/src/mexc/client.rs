use std::str::FromStr;

use crate::{mexc::{ ExchangeInfo, SymbolInfo }, FilterAddressType};
use anyhow::{Context, Result};
use reqwest::Client;
use serde::Deserialize;
use solana_sdk::pubkey::Pubkey;

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct OrderbookResponse {
    pub bids: Vec<[String; 2]>, // [price, quantity]
    pub asks: Vec<[String; 2]>, // [price, quantity]
}

pub struct MexcClient {
    client: Client,
    base_url: String,
    address_type: FilterAddressType,
}

impl MexcClient {
    pub fn new(address_type: FilterAddressType) -> Self {
        Self {
            client: Client::new(),
            base_url: "https://api.mexc.com".to_string(),
            address_type,
        }
    }

    /// Check if a string is a valid Ethereum address
    fn is_valid_ethereum_address(address: &str) -> bool {
        // Remove 0x prefix if present
        let address = address.strip_prefix("0x").unwrap_or(address);

        // Ethereum addresses should be 40 hex characters
        if address.len() != 40 {
            return false;
        }

        // Check if all characters are valid hex
        address.chars().all(|c| c.is_ascii_hexdigit())
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

    pub async fn get_token_usdt_pairs(&self) -> Result<Vec<SymbolInfo>> {
        let exchange_info = self.get_exchange_info().await?;

        let symbols: Vec<SymbolInfo> = exchange_info
            .symbols
            .into_iter()
            .filter(|symbol| {
                // Check if it's paired with USDT and is a valid ethereum contract address
                symbol.quote_asset == "USDT"
                    && symbol.status == "1"
                    && self.is_valid_address(&symbol.contract_address)
                    && symbol.permissions.contains(&"SPOT".to_string())
            })
            .collect();

        log::info!(
            "Found {} Ethereum tokens based on contract addresses",
            symbols.len()
        );

        Ok(symbols)
    }

    fn is_valid_address(&self, address: &str) -> bool {
        match self.address_type {
            FilterAddressType::Solana => Pubkey::from_str(address).is_ok(),
            FilterAddressType::Ethereum => Self::is_valid_ethereum_address(address),
        }
    }

    /// Fetch orderbook for a specific symbol
    pub async fn get_orderbook(&self, symbol: &str, limit: u32) -> Result<OrderbookResponse> {
        let url = format!(
            "{}/api/v3/depth?symbol={}&limit={}",
            self.base_url, symbol, limit
        );

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to send orderbook request to MEXC API")?;

        let orderbook: OrderbookResponse = response
            .json()
            .await
            .context("Failed to parse orderbook response")?;

        Ok(orderbook)
    }
}
