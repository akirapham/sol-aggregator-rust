use crate::FilterAddressType;
use anyhow::{Context, Result};
use reqwest::Client;
use serde::Deserialize;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

#[derive(Debug, Deserialize, Clone)]
pub struct BitgetResponse<T> {
    pub code: String,
    pub msg: String,
    pub data: T,
}

#[derive(Debug, Deserialize, Clone)]
pub struct SymbolInfo {
    pub symbol: String,
    #[serde(rename = "baseCoin")]
    pub base_coin: String,
    #[serde(rename = "quoteCoin")]
    pub quote_coin: String,
    pub status: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct CurrencyInfo {
    pub coin: String,
    pub chains: Vec<ChainInfo>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ChainInfo {
    pub chain: String,
    #[serde(rename = "contractAddress")]
    pub contract_address: Option<String>, // null for native tokens like ETH, BNB, SOL
    #[serde(rename = "needTag")]
    pub need_tag: String,
    #[serde(rename = "rechargeable", default)]
    pub rechargeable: String, // "true" or "false" - deposits enabled
    #[serde(rename = "withdrawable", default)]
    pub withdrawable: String, // "true" or "false" - withdrawals enabled
}

impl ChainInfo {
    /// Check if deposits are enabled for this chain
    pub fn is_deposit_enabled(&self) -> bool {
        self.rechargeable == "true"
    }

    /// Check if withdrawals are enabled for this chain
    pub fn is_withdraw_enabled(&self) -> bool {
        self.withdrawable == "true"
    }
}

#[derive(Debug, Deserialize)]
pub struct OrderbookData {
    pub asks: Vec<Vec<String>>,
    pub bids: Vec<Vec<String>>,
    pub ts: String,
}

#[derive(Clone)]
pub struct BitgetClient {
    client: Client,
    base_url: String,
    pub address_type: FilterAddressType,
    api_key: Option<String>,
    api_secret: Option<String>,
    api_passphrase: Option<String>,
}

impl BitgetClient {
    pub fn new(address_type: FilterAddressType) -> Self {
        Self {
            client: Client::new(),
            base_url: "https://api.bitget.com".to_string(),
            address_type,
            api_key: None,
            api_secret: None,
            api_passphrase: None,
        }
    }

    pub fn with_credentials(
        address_type: FilterAddressType,
        api_key: String,
        api_secret: String,
        api_passphrase: String,
    ) -> Self {
        Self {
            client: Client::new(),
            base_url: "https://api.bitget.com".to_string(),
            address_type,
            api_key: Some(api_key),
            api_secret: Some(api_secret),
            api_passphrase: Some(api_passphrase),
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

    /// Get all spot trading symbols
    pub async fn get_symbols(&self) -> Result<Vec<SymbolInfo>> {
        let url = format!("{}/api/v2/spot/public/symbols", self.base_url);

        log::debug!("Fetching symbols from: {}", url);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to send request to Bitget API")?;

        let response_text = response
            .text()
            .await
            .context("Failed to read response text")?;

        log::debug!("Symbols response: {}", response_text);

        let bitget_response: BitgetResponse<Vec<SymbolInfo>> = serde_json::from_str(&response_text)
            .context(format!(
                "Failed to parse symbols response: {}",
                response_text
            ))?;

        if bitget_response.code != "00000" {
            return Err(anyhow::anyhow!(
                "Bitget API error: {} - {}",
                bitget_response.code,
                bitget_response.msg
            ));
        }

        Ok(bitget_response.data)
    }

    /// Get USDT trading pairs
    pub async fn get_token_usdt_pairs(&self) -> Result<Vec<SymbolInfo>> {
        let symbols = self.get_symbols().await?;

        let usdt_pairs: Vec<SymbolInfo> = symbols
            .into_iter()
            .filter(|s| s.quote_coin == "USDT" && s.status == "online")
            .collect();

        log::info!("Found {} USDT trading pairs on Bitget", usdt_pairs.len());

        Ok(usdt_pairs)
    }

    /// Get currency information including contract addresses
    pub async fn get_coin_info(&self, coin: &str) -> Result<Vec<CurrencyInfo>> {
        let url = format!("{}/api/v2/spot/public/coins?coin={}", self.base_url, coin);

        log::debug!("Fetching coin info for {} from: {}", coin, url);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to send request to Bitget API")?;

        let response_text = response
            .text()
            .await
            .context("Failed to read coin info response text")?;

        log::debug!("Coin info raw response: {}", response_text);

        let bitget_response: BitgetResponse<Vec<CurrencyInfo>> =
            serde_json::from_str(&response_text).context(format!(
                "Failed to parse coin info response: {}",
                response_text
            ))?;

        if bitget_response.code != "00000" {
            return Err(anyhow::anyhow!(
                "Bitget coin info API error: {} - {}",
                bitget_response.code,
                bitget_response.msg
            ));
        }

        Ok(bitget_response.data)
    }

    /// Fetch orderbook for a specific symbol
    pub async fn get_orderbook(&self, symbol: &str, limit: u32) -> Result<OrderbookData> {
        let url = format!(
            "{}/api/v2/spot/market/orderbook?symbol={}&type=step0&limit={}",
            self.base_url, symbol, limit
        );

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to send orderbook request to Bitget API")?;

        let response_text = response
            .text()
            .await
            .context("Failed to read orderbook response text")?;

        let bitget_response: BitgetResponse<OrderbookData> = serde_json::from_str(&response_text)
            .context(format!(
            "Failed to parse orderbook response: {}",
            response_text
        ))?;

        if bitget_response.code != "00000" {
            return Err(anyhow::anyhow!(
                "Bitget orderbook API error: {} - {}",
                bitget_response.code,
                bitget_response.msg
            ));
        }

        Ok(bitget_response.data)
    }

    /// Get account assets (spot account balances)
    /// Requires authentication
    pub async fn get_account_assets(&self) -> Result<serde_json::Value> {
        if self.api_key.is_none() || self.api_secret.is_none() || self.api_passphrase.is_none() {
            return Err(anyhow::anyhow!(
                "Bitget account assets endpoint requires API credentials"
            ));
        }

        let api_key = self.api_key.as_ref().unwrap();
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
            .to_string();

        let endpoint = "/api/v2/spot/account/assets";
        let query_string = "";

        // Generate signature: timestamp + method + endpoint + query_string
        let pre_hash = format!("{}{}{}{}", timestamp, "GET", endpoint, query_string);
        let signature = self.generate_signature(&pre_hash)?;

        let url = format!("{}{}", self.base_url, endpoint);

        let response = self
            .client
            .get(&url)
            .header("ACCESS-KEY", api_key)
            .header("ACCESS-SIGN", signature)
            .header("ACCESS-TIMESTAMP", &timestamp)
            .header("ACCESS-PASSPHRASE", self.api_passphrase.as_ref().unwrap())
            .header("Content-Type", "application/json")
            .send()
            .await
            .context("Failed to get account assets from Bitget")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Bitget API error ({}): {}", status, body));
        }

        let json: serde_json::Value = response.json().await?;
        Ok(json)
    }

    /// Generate HMAC SHA256 signature for Bitget API
    fn generate_signature(&self, pre_hash: &str) -> Result<String> {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;

        let api_secret = self
            .api_secret
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("API secret not set"))?;

        let mut mac = Hmac::<Sha256>::new_from_slice(api_secret.as_bytes())
            .map_err(|e| anyhow::anyhow!("Invalid HMAC key: {}", e))?;

        mac.update(pre_hash.as_bytes());
        let result = mac.finalize();
        let signature = base64::encode(result.into_bytes());

        Ok(signature)
    }
}
