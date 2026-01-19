use std::str::FromStr;

use crate::{
    mexc::{ExchangeInfo, SymbolInfo},
    FilterAddressType,
};
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

/// Coin information structure for MEXC
/// Requires API authentication to fetch
#[derive(Debug, Deserialize, Clone)]
pub struct CoinInfo {
    pub coin: String,
    pub name: Option<String>,
    #[serde(rename = "networkList")]
    pub network_list: Vec<NetworkInfo>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct NetworkInfo {
    #[serde(default)]
    pub coin: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub network: Option<String>,
    #[serde(rename = "netWork", default)]
    pub net_work: Option<String>,
    #[serde(default)]
    pub contract: Option<String>,
    #[serde(rename = "depositEnable")]
    pub deposit_enable: bool,
    #[serde(rename = "withdrawEnable")]
    pub withdraw_enable: bool,
    #[serde(rename = "depositDesc", default)]
    pub deposit_desc: Option<String>,
    #[serde(rename = "depositTips", default)]
    pub deposit_tips: Option<String>,
    #[serde(rename = "withdrawTips", default)]
    pub withdraw_tips: Option<String>,
    #[serde(rename = "withdrawFee", default)]
    pub withdraw_fee: Option<String>,
    #[serde(rename = "withdrawMin", default)]
    pub withdraw_min: Option<String>,
    #[serde(rename = "withdrawMax", default)]
    pub withdraw_max: Option<String>,
    #[serde(rename = "withdrawIntegerMultiple", default)]
    pub withdraw_integer_multiple: Option<String>,
    #[serde(rename = "minConfirm", default)]
    pub min_confirm: Option<u32>,
    #[serde(rename = "sameAddress", default)]
    pub same_address: Option<bool>,
}

impl NetworkInfo {
    /// Check if deposits are enabled for this network
    pub fn is_deposit_enabled(&self) -> bool {
        self.deposit_enable
    }

    /// Check if withdrawals are enabled for this network
    pub fn is_withdraw_enabled(&self) -> bool {
        self.withdraw_enable
    }
}

#[derive(Clone)]
pub struct MexcClient {
    client: Client,
    base_url: String,
    pub address_type: FilterAddressType,
    api_key: Option<String>,
    api_secret: Option<String>,
}

impl MexcClient {
    pub fn new(address_type: FilterAddressType) -> Self {
        Self {
            client: Client::new(),
            base_url: "https://api.mexc.com".to_string(),
            address_type,
            api_key: None,
            api_secret: None,
        }
    }

    pub fn with_credentials(
        address_type: FilterAddressType,
        api_key: String,
        api_secret: String,
    ) -> Self {
        Self {
            client: Client::new(),
            base_url: "https://api.mexc.com".to_string(),
            address_type,
            api_key: Some(api_key),
            api_secret: Some(api_secret),
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

    /// Generate signature for MEXC API requests
    /// According to MEXC API docs, the signature is HMAC-SHA256(query_string, secret_key)
    /// where query_string includes all parameters including timestamp
    fn generate_signature(&self, query_string: &str) -> Result<String> {
        let api_secret = self
            .api_secret
            .as_ref()
            .context("API secret not configured")?;

        use hmac::{Hmac, Mac};
        use sha2::Sha256;

        type HmacSha256 = Hmac<Sha256>;
        let mut mac = HmacSha256::new_from_slice(api_secret.as_bytes())
            .map_err(|e| anyhow::anyhow!("Invalid HMAC key: {}", e))?;

        mac.update(query_string.as_bytes());
        let result = mac.finalize();
        let signature = hex::encode(result.into_bytes());

        Ok(signature)
    }

    /// Get coin information including deposit/withdrawal status
    /// Note: This endpoint requires authentication
    pub async fn get_coin_info(&self, coin: Option<&str>) -> Result<Vec<CoinInfo>> {
        if self.api_key.is_none() || self.api_secret.is_none() {
            return Err(anyhow::anyhow!(
                "MEXC coin info endpoint requires API credentials. Use with_credentials() to create client."
            ));
        }

        let api_key = self.api_key.as_ref().unwrap();

        // Build query parameters
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        let mut query_params = vec![("timestamp", timestamp.to_string())];

        if let Some(coin_name) = coin {
            query_params.push(("coin", coin_name.to_string()));
        }

        // Sort parameters and build query string
        query_params.sort_by(|a, b| a.0.cmp(b.0));
        let query_string = query_params
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join("&");

        // Generate signature (HMAC-SHA256 of the query string)
        let signature = self.generate_signature(&query_string)?;

        let url = format!(
            "{}/api/v3/capital/config/getall?{}&signature={}",
            self.base_url, query_string, signature
        );

        log::debug!(
            "MEXC coin info request URL (without signature): {}/api/v3/capital/config/getall?{}",
            self.base_url,
            query_string
        );

        let response = self
            .client
            .get(&url)
            .header("X-MEXC-APIKEY", api_key)
            .send()
            .await
            .context("Failed to send request to MEXC capital config API")?;

        let status = response.status();
        let response_text = response
            .text()
            .await
            .context("Failed to read response text")?;

        if !status.is_success() {
            log::error!("MEXC API error (status {}): {}", status, response_text);
            return Err(anyhow::anyhow!(
                "MEXC API returned error: {} - {}",
                status,
                response_text
            ));
        }

        let coin_info: Vec<CoinInfo> = serde_json::from_str(&response_text)
            .context("Failed to parse MEXC coin info response")?;

        log::info!(
            "Successfully fetched coin info for {} coins",
            coin_info.len()
        );

        Ok(coin_info)
    }

    /// Get current ticker price for a symbol (public endpoint)
    pub async fn get_ticker_price(&self, symbol: &str) -> Result<f64> {
        let url = format!("{}/api/v3/ticker/price?symbol={}", self.base_url, symbol);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to send ticker price request to MEXC API")?;

        #[derive(serde::Deserialize)]
        struct TickerPrice {
            price: String,
        }

        let ticker: TickerPrice = response
            .json()
            .await
            .context("Failed to parse ticker price response")?;

        let price = ticker
            .price
            .parse::<f64>()
            .context("Failed to parse price as f64")?;

        Ok(price)
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

    /// Get deposit address for a specific coin and network
    /// Requires authentication
    pub async fn get_deposit_address(&self, coin: &str, network: &str) -> Result<String> {
        if self.api_key.is_none() || self.api_secret.is_none() {
            return Err(anyhow::anyhow!(
                "MEXC deposit address endpoint requires API credentials"
            ));
        }

        let api_key = self.api_key.as_ref().unwrap();
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        // Build query string
        let query_string = format!("coin={}&network={}&timestamp={}", coin, network, timestamp);
        let signature = self.generate_signature(&query_string)?;

        let url = format!(
            "{}/api/v3/capital/deposit/address?{}&signature={}",
            self.base_url, query_string, signature
        );

        let response = self
            .client
            .get(&url)
            .header("X-MEXC-APIKEY", api_key)
            .send()
            .await
            .context("Failed to get deposit address from MEXC")?;

        let json: serde_json::Value = response.json().await?;
        let address = json
            .get("address")
            .and_then(|v| v.as_str())
            .context("Failed to extract deposit address from response")?
            .to_string();

        Ok(address)
    }

    /// Place a market order (buy or sell)
    /// Requires authentication
    pub async fn place_market_order(
        &self,
        symbol: &str,
        side: &str, // "BUY" or "SELL"
        quantity: f64,
    ) -> Result<serde_json::Value> {
        if self.api_key.is_none() || self.api_secret.is_none() {
            return Err(anyhow::anyhow!(
                "MEXC order endpoint requires API credentials"
            ));
        }

        let api_key = self.api_key.as_ref().unwrap();
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        // Build query string (parameters must be sorted alphabetically)
        let query_string = format!(
            "quantity={}&side={}&symbol={}&timestamp={}&type=MARKET",
            quantity, side, symbol, timestamp
        );
        let signature = self.generate_signature(&query_string)?;

        let url = format!(
            "{}/api/v3/order?{}&signature={}",
            self.base_url, query_string, signature
        );

        let response = self
            .client
            .post(&url)
            .header("X-MEXC-APIKEY", api_key)
            .send()
            .await
            .context("Failed to place order on MEXC")?;

        let json: serde_json::Value = response.json().await?;
        Ok(json)
    }

    /// Withdraw coins to an external address
    /// Requires authentication
    pub async fn withdraw(
        &self,
        coin: &str,
        address: &str,
        amount: f64,
        network: &str,
    ) -> Result<String> {
        if self.api_key.is_none() || self.api_secret.is_none() {
            return Err(anyhow::anyhow!(
                "MEXC withdrawal endpoint requires API credentials"
            ));
        }

        let api_key = self.api_key.as_ref().unwrap();
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        // Build query string (parameters must be sorted alphabetically)
        let query_string = format!(
            "address={}&amount={}&coin={}&network={}&timestamp={}",
            address, amount, coin, network, timestamp
        );
        let signature = self.generate_signature(&query_string)?;

        let url = format!(
            "{}/api/v3/capital/withdraw/apply?{}&signature={}",
            self.base_url, query_string, signature
        );

        let response = self
            .client
            .post(&url)
            .header("X-MEXC-APIKEY", api_key)
            .send()
            .await
            .context("Failed to submit withdrawal to MEXC")?;

        let json: serde_json::Value = response.json().await?;
        let withdrawal_id = json
            .get("id")
            .and_then(|v| v.as_str())
            .context("Failed to extract withdrawal ID from response")?
            .to_string();

        Ok(withdrawal_id)
    }

    /// Query order status
    pub async fn get_order(&self, symbol: &str, order_id: &str) -> Result<serde_json::Value> {
        if self.api_key.is_none() || self.api_secret.is_none() {
            return Err(anyhow::anyhow!(
                "MEXC order query endpoint requires API credentials"
            ));
        }

        let api_key = self.api_key.as_ref().unwrap();
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        let query_string = format!(
            "orderId={}&symbol={}&timestamp={}",
            order_id, symbol, timestamp
        );
        let signature = self.generate_signature(&query_string)?;

        let url = format!(
            "{}/api/v3/order?{}&signature={}",
            self.base_url, query_string, signature
        );

        let response = self
            .client
            .get(&url)
            .header("X-MEXC-APIKEY", api_key)
            .send()
            .await
            .context("Failed to query order from MEXC")?;

        let json: serde_json::Value = response.json().await?;
        Ok(json)
    }

    /// Get account information including balances
    pub async fn get_account_info(&self) -> Result<serde_json::Value> {
        if self.api_key.is_none() || self.api_secret.is_none() {
            return Err(anyhow::anyhow!(
                "MEXC account info endpoint requires API credentials"
            ));
        }

        let api_key = self.api_key.as_ref().unwrap();
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        let query_string = format!("timestamp={}", timestamp);
        let signature = self.generate_signature(&query_string)?;

        let url = format!(
            "{}/api/v3/account?{}&signature={}",
            self.base_url, query_string, signature
        );

        let response = self
            .client
            .get(&url)
            .header("X-MEXC-APIKEY", api_key)
            .send()
            .await
            .context("Failed to get account info from MEXC")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("MEXC API error ({}): {}", status, body));
        }

        let json: serde_json::Value = response.json().await?;
        Ok(json)
    }
}
