use crate::FilterAddressType;
use anyhow::{Context, Result};
use reqwest::Client;
use serde::Deserialize;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

#[derive(Debug, Deserialize, Clone)]
pub struct CurrencyPair {
    pub id: String,
    pub base: String,
    pub quote: String,
    pub trade_status: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct CurrencyInfo {
    pub chain: String,
    #[serde(rename = "is_disabled")]
    pub is_disabled: i32, // 0 = enabled, 1 = disabled
    #[serde(rename = "is_deposit_disabled", default)]
    pub is_deposit_disabled: i32, // 0 = enabled, 1 = disabled
    #[serde(rename = "is_withdraw_disabled", default)]
    pub is_withdraw_disabled: i32, // 0 = enabled, 1 = disabled
    #[serde(rename = "contract_address")]
    pub contract_address: Option<String>,
}

impl CurrencyInfo {
    /// Check if deposits are enabled for this chain
    pub fn is_deposit_enabled(&self) -> bool {
        self.is_disabled == 0 && self.is_deposit_disabled == 0
    }

    /// Check if withdrawals are enabled for this chain
    pub fn is_withdraw_enabled(&self) -> bool {
        self.is_disabled == 0 && self.is_withdraw_disabled == 0
    }
}

#[derive(Debug, Deserialize)]
pub struct OrderbookResponse {
    pub id: i64,
    pub current: i64,
    pub update: i64,
    pub asks: Vec<Vec<String>>,
    pub bids: Vec<Vec<String>>,
}

pub struct GateClient {
    client: Client,
    base_url: String,
    pub address_type: FilterAddressType,
}

impl GateClient {
    pub fn new(address_type: FilterAddressType) -> Self {
        Self {
            client: Client::new(),
            base_url: "https://api.gateio.ws".to_string(),
            address_type,
        }
    }

    /// Check if a string is a valid Ethereum address
    fn is_valid_ethereum_address(address: &str) -> bool {
        let address = address.strip_prefix("0x").unwrap_or(address);
        if address.len() != 40 {
            return false;
        }
        address.chars().all(|c| c.is_ascii_hexdigit())
    }

    pub fn is_valid_address(&self, address: &str) -> bool {
        match self.address_type {
            FilterAddressType::Solana => Pubkey::from_str(address).is_ok(),
            FilterAddressType::Ethereum => Self::is_valid_ethereum_address(address),
        }
    }

    /// Get all spot currency pairs
    pub async fn get_currency_pairs(&self) -> Result<Vec<CurrencyPair>> {
        let url = format!("{}/api/v4/spot/currency_pairs", self.base_url);

        log::debug!("Fetching currency pairs from: {}", url);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to send request to Gate.io API")?;

        let response_text = response
            .text()
            .await
            .context("Failed to read response text")?;

        log::debug!("Currency pairs response length: {}", response_text.len());

        let pairs: Vec<CurrencyPair> = serde_json::from_str(&response_text)
            .context(format!("Failed to parse currency pairs response"))?;

        Ok(pairs)
    }

    /// Get USDT trading pairs
    pub async fn get_token_usdt_pairs(&self) -> Result<Vec<CurrencyPair>> {
        let pairs = self.get_currency_pairs().await?;

        let usdt_pairs: Vec<CurrencyPair> = pairs
            .into_iter()
            .filter(|p| p.quote == "USDT" && p.trade_status == "tradable")
            .collect();

        log::info!("Found {} USDT trading pairs on Gate.io", usdt_pairs.len());

        Ok(usdt_pairs)
    }

    /// Get currency chain information including contract addresses
    pub async fn get_currency_chains(&self, currency: &str) -> Result<Vec<CurrencyInfo>> {
        let url = format!(
            "{}/api/v4/wallet/currency_chains?currency={}",
            self.base_url, currency
        );

        log::debug!("Fetching currency chains for {} from: {}", currency, url);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to send request to Gate.io API")?;

        let response_text = response
            .text()
            .await
            .context("Failed to read currency chains response text")?;

        let chains: Vec<CurrencyInfo> = serde_json::from_str(&response_text).context(format!(
            "Failed to parse currency chains response for {}: {}",
            currency, response_text
        ))?;

        Ok(chains)
    }

    /// Fetch orderbook for a specific currency pair
    pub async fn get_orderbook(
        &self,
        currency_pair: &str,
        limit: u32,
    ) -> Result<OrderbookResponse> {
        let url = format!(
            "{}/api/v4/spot/order_book?currency_pair={}&limit={}",
            self.base_url, currency_pair, limit
        );

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to send orderbook request to Gate.io API")?;

        let response_text = response
            .text()
            .await
            .context("Failed to read orderbook response text")?;

        let orderbook: OrderbookResponse = serde_json::from_str(&response_text).context(
            format!("Failed to parse orderbook response: {}", response_text),
        )?;

        Ok(orderbook)
    }
}
