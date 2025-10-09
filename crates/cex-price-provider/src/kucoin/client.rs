use crate::kucoin::{Symbol, SymbolsResponse};
use crate::FilterAddressType;
use anyhow::{Context, Result};
use reqwest::Client;
use serde::Deserialize;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

#[derive(Debug, Deserialize)]
pub struct OrderbookResponse {
    pub code: String,
    pub data: OrderbookData,
}

#[derive(Debug, Deserialize)]
pub struct OrderbookData {
    pub sequence: String,
    pub bids: Vec<[String; 2]>, // [price, size]
    pub asks: Vec<[String; 2]>, // [price, size]
}

#[derive(Debug, Deserialize, Clone)]
pub struct CurrencyDetailResponse {
    pub code: String,
    pub data: CurrencyDetail,
}

#[derive(Debug, Deserialize, Clone)]
pub struct CurrencyDetail {
    pub currency: String,
    pub name: String,
    pub chains: Vec<ChainDetail>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ChainDetail {
    #[serde(rename = "chainName")]
    pub chain_name: String,
    #[serde(rename = "contractAddress")]
    pub contract_address: String,
    #[serde(rename = "isWithdrawEnabled")]
    pub is_withdraw_enabled: bool,
    #[serde(rename = "isDepositEnabled")]
    pub is_deposit_enabled: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct CurrenciesResponse {
    pub code: String,
    pub data: Vec<CurrencyInfo>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct CurrencyInfo {
    pub currency: String,
    pub name: String,
    #[serde(rename = "fullName")]
    pub full_name: String,
    pub precision: u8,
}

pub struct KucoinClient {
    client: Client,
    base_url: String,
    pub(crate) address_type: FilterAddressType,
}

impl KucoinClient {
    pub fn new(address_type: FilterAddressType) -> Self {
        Self {
            client: Client::new(),
            base_url: "https://api.kucoin.com".to_string(),
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

    pub fn is_valid_address(&self, address: &str) -> bool {
        match self.address_type {
            FilterAddressType::Solana => Pubkey::from_str(address).is_ok(),
            FilterAddressType::Ethereum => Self::is_valid_ethereum_address(address),
        }
    }

    /// Get all trading symbols from KuCoin
    pub async fn get_symbols(&self) -> Result<Vec<Symbol>> {
        let url = format!("{}/api/v1/symbols", self.base_url);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to send request to KuCoin API")?;

        let symbols_response: SymbolsResponse = response
            .json()
            .await
            .context("Failed to parse symbols response")?;

        if symbols_response.code != "200000" {
            return Err(anyhow::anyhow!(
                "KuCoin API error: {}",
                symbols_response.code
            ));
        }

        Ok(symbols_response.data)
    }

    /// Get all USDT spot trading pairs
    pub async fn get_token_usdt_pairs(&self) -> Result<Vec<Symbol>> {
        let all_symbols = self.get_symbols().await?;

        let filtered: Vec<Symbol> = all_symbols
            .into_iter()
            .filter(|symbol| symbol.quote_currency == "USDT" && symbol.enable_trading)
            .collect();

        log::info!("Found {} USDT trading pairs on KuCoin", filtered.len());

        Ok(filtered)
    }

    /// Fetch orderbook for a specific symbol
    pub async fn get_orderbook(&self, symbol: &str, depth: u32) -> Result<OrderbookResponse> {
        let url = format!(
            "{}/api/v1/market/orderbook/level2_{}?symbol={}",
            self.base_url, depth, symbol
        );

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to send orderbook request to KuCoin API")?;

        let orderbook: OrderbookResponse = response
            .json()
            .await
            .context("Failed to parse orderbook response")?;

        if orderbook.code != "200000" {
            return Err(anyhow::anyhow!(
                "KuCoin orderbook API error: {}",
                orderbook.code
            ));
        }

        Ok(orderbook)
    }

    /// Get list of all currencies (public endpoint, no auth needed)
    pub async fn get_currencies(&self) -> Result<Vec<CurrencyInfo>> {
        let url = format!("{}/api/v3/currencies", self.base_url);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to send currencies request to KuCoin API")?;

        let response_text = response
            .text()
            .await
            .context("Failed to read currencies response text")?;

        log::debug!("Currencies raw response: {}", response_text);

        let currencies_response: CurrenciesResponse = serde_json::from_str(&response_text)
            .context(format!(
                "Failed to parse currencies response. Raw: {}",
                response_text
            ))?;

        if currencies_response.code != "200000" {
            return Err(anyhow::anyhow!(
                "KuCoin currencies API error: {}",
                currencies_response.code
            ));
        }

        Ok(currencies_response.data)
    }

    /// Get currency detail including contract addresses (public endpoint)
    pub async fn get_currency_detail(&self, currency: &str) -> Result<CurrencyDetail> {
        let url = format!("{}/api/v3/currencies/{}", self.base_url, currency);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to send currency detail request to KuCoin API")?;

        let response_text = response
            .text()
            .await
            .context("Failed to read currency detail response text")?;

        log::debug!("Currency detail raw response: {}", response_text);

        let detail_response: CurrencyDetailResponse = serde_json::from_str(&response_text)
            .context(format!(
                "Failed to parse currency detail response. Raw: {}",
                response_text
            ))?;

        if detail_response.code != "200000" {
            return Err(anyhow::anyhow!(
                "KuCoin currency detail API error: {}",
                detail_response.code
            ));
        }

        Ok(detail_response.data)
    }
}
