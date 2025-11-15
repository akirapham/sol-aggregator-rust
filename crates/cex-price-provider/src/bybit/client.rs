use crate::bybit::{InstrumentInfo, InstrumentsResponse};
use crate::FilterAddressType;
use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Deserializer};
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

fn deserialize_coin_info_result<'de, D>(deserializer: D) -> Result<Vec<CoinInfo>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    struct ResultWrapper {
        rows: Vec<CoinInfo>,
    }

    let wrapper = ResultWrapper::deserialize(deserializer)?;
    Ok(wrapper.rows)
}

#[derive(Debug, Deserialize)]
pub struct OrderbookResponse {
    #[serde(rename = "retCode")]
    pub ret_code: i32,
    #[serde(rename = "retMsg")]
    pub ret_msg: String,
    pub result: OrderbookResult,
}

#[derive(Debug, Deserialize)]
pub struct OrderbookResult {
    pub s: String,           // Symbol
    pub b: Vec<[String; 2]>, // Bids: [price, size]
    pub a: Vec<[String; 2]>, // Asks: [price, size]
    pub ts: u64,             // Timestamp
    pub u: u64,              // Update ID
}

#[derive(Debug, Deserialize, Clone)]
pub struct CoinInfoResponse {
    #[serde(rename = "retCode")]
    pub ret_code: i32,
    #[serde(rename = "retMsg")]
    pub ret_msg: String,
    #[serde(deserialize_with = "deserialize_coin_info_result")]
    pub result: Vec<CoinInfo>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct CoinInfo {
    pub name: String,
    pub coin: String,
    pub chains: Vec<ChainInfo>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ChainInfo {
    pub chain: String,
    #[serde(rename = "chainType")]
    pub chain_type: String,
    #[serde(rename = "contractAddress")]
    pub contract_address: String,
    #[serde(rename = "chainDeposit")]
    pub chain_deposit: String, // "0" = disabled, "1" = enabled
    #[serde(rename = "chainWithdraw")]
    pub chain_withdraw: String, // "0" = disabled, "1" = enabled
}

impl ChainInfo {
    /// Check if deposits are enabled for this chain
    pub fn is_deposit_enabled(&self) -> bool {
        self.chain_deposit == "1"
    }

    /// Check if withdrawals are enabled for this chain
    pub fn is_withdraw_enabled(&self) -> bool {
        self.chain_withdraw == "1"
    }
}

#[derive(Clone)]
pub struct BybitClient {
    client: Client,
    base_url: String,
    pub(crate) address_type: FilterAddressType,
    api_key: Option<String>,
    api_secret: Option<String>,
}

impl BybitClient {
    pub fn new(address_type: FilterAddressType) -> Self {
        Self {
            client: Client::new(),
            base_url: "https://api.bybit.com".to_string(),
            address_type,
            api_key: None,
            api_secret: None,
        }
    }

    /// Create client with API credentials for authenticated endpoints
    pub fn with_credentials(
        address_type: FilterAddressType,
        api_key: String,
        api_secret: String,
    ) -> Self {
        Self {
            client: Client::new(),
            base_url: "https://api.bybit.com".to_string(),
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

    /// Check if a contract address is valid for the configured address type
    pub fn is_valid_contract_address(&self, address: &str) -> bool {
        match self.address_type {
            FilterAddressType::Solana => Pubkey::from_str(address).is_ok(),
            FilterAddressType::Ethereum => Self::is_valid_ethereum_address(address),
        }
    }

    /// Get all spot instruments from Bybit
    pub async fn get_instruments(&self, cursor: Option<&str>) -> Result<InstrumentsResponse> {
        let mut url = format!(
            "{}/v5/market/instruments-info?category=spot&limit=1000",
            self.base_url
        );

        if let Some(cursor_val) = cursor {
            url.push_str(&format!("&cursor={}", cursor_val));
        }

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to send request to Bybit API")?;

        let instruments: InstrumentsResponse = response
            .json()
            .await
            .context("Failed to parse instruments response")?;

        Ok(instruments)
    }

    /// Get all USDT spot trading pairs
    ///
    /// Note: This endpoint does NOT return contract addresses. Bybit's instruments API
    /// only provides trading symbols (BTCUSDT, ETHUSDT, etc.) with base/quote coins.
    ///
    /// To get contract addresses, you need to:
    /// 1. Use `with_credentials()` to create an authenticated client
    /// 2. Call `get_coin_info()` to fetch coin details including contract addresses per chain
    ///
    /// Without authentication, this method returns ALL USDT pairs regardless of the
    /// address_type filter setting.
    pub async fn get_token_usdt_pairs(&self) -> Result<Vec<InstrumentInfo>> {
        let mut all_instruments = Vec::new();
        let mut cursor: Option<String> = None;

        // Bybit uses pagination, so we need to fetch all pages
        loop {
            let response = self.get_instruments(cursor.as_deref()).await?;

            if response.ret_code != 0 {
                return Err(anyhow::anyhow!(
                    "Bybit API error: {} - {}",
                    response.ret_code,
                    response.ret_msg
                ));
            }

            let filtered: Vec<InstrumentInfo> = response
                .result
                .list
                .into_iter()
                .filter(|instrument| {
                    // Filter for USDT pairs with "Trading" status
                    // Note: Cannot filter by contract address here as it's not available
                    instrument.quote_coin == "USDT" && instrument.status == "Trading"
                })
                .collect();

            all_instruments.extend(filtered);

            // Check if there are more pages
            if let Some(next_cursor) = response.result.next_page_cursor {
                if next_cursor.is_empty() {
                    break;
                }
                cursor = Some(next_cursor);
            } else {
                break;
            }
        }

        log::info!(
            "Found {} USDT trading pairs on Bybit",
            all_instruments.len()
        );

        Ok(all_instruments)
    }

    /// Fetch orderbook for a specific symbol
    /// category: spot, linear, inverse, option
    /// limit: 1-500 for linear/inverse, 1-200 for spot, 1-25 for option
    pub async fn get_orderbook(&self, symbol: &str, limit: u32) -> Result<OrderbookResponse> {
        let url = format!(
            "{}/v5/market/orderbook?category=spot&symbol={}&limit={}",
            self.base_url, symbol, limit
        );

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to send orderbook request to Bybit API")?;

        let orderbook: OrderbookResponse = response
            .json()
            .await
            .context("Failed to parse orderbook response")?;

        if orderbook.ret_code != 0 {
            return Err(anyhow::anyhow!(
                "Bybit orderbook API error: {} - {}",
                orderbook.ret_code,
                orderbook.ret_msg
            ));
        }

        Ok(orderbook)
    }

    /// Generate authentication signature for Bybit API
    fn generate_signature(&self, timestamp: u64, recv_window_params: &str) -> Result<String> {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;

        let api_secret = self
            .api_secret
            .as_ref()
            .context("API secret not configured")?;

        let api_key = self.api_key.as_ref().unwrap();
        let sign_str = format!("{}{}{}", timestamp, api_key, recv_window_params);

        log::debug!("Signature string: {}", sign_str);

        type HmacSha256 = Hmac<Sha256>;
        let mut mac = HmacSha256::new_from_slice(api_secret.as_bytes())
            .map_err(|e| anyhow::anyhow!("Invalid HMAC key: {}", e))?;

        mac.update(sign_str.as_bytes());
        let result = mac.finalize();
        let signature = hex::encode(result.into_bytes());

        Ok(signature)
    }

    /// Get coin information including contract addresses (requires authentication)
    /// If coin is None, returns all coins
    pub async fn get_coin_info(&self, coin: Option<&str>) -> Result<CoinInfoResponse> {
        if self.api_key.is_none() || self.api_secret.is_none() {
            return Err(anyhow::anyhow!(
                "API credentials required for get_coin_info. Use with_credentials() to create client."
            ));
        }

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis() as u64;

        let recv_window = "5000";

        // Build query parameters
        let mut query_params = vec![];
        if let Some(coin_val) = coin {
            query_params.push(format!("coin={}", coin_val));
        }

        let params_str = query_params.join("&");

        // For signature, we need: timestamp + api_key + recv_window + params
        let sign_params = format!("{}{}", recv_window, params_str);
        let signature = self.generate_signature(timestamp, &sign_params)?;

        let url = if params_str.is_empty() {
            format!("{}/v5/asset/coin/query-info", self.base_url)
        } else {
            format!("{}/v5/asset/coin/query-info?{}", self.base_url, params_str)
        };

        let response = self
            .client
            .get(&url)
            .header("X-BAPI-API-KEY", self.api_key.as_ref().unwrap())
            .header("X-BAPI-TIMESTAMP", timestamp.to_string())
            .header("X-BAPI-SIGN", signature)
            .header("X-BAPI-RECV-WINDOW", recv_window)
            .send()
            .await
            .context("Failed to send coin info request to Bybit API")?;

        // Debug: print response status and headers
        log::debug!("Coin info response status: {}", response.status());
        log::debug!("Coin info response headers: {:?}", response.headers());

        let response_text = response
            .text()
            .await
            .context("Failed to read coin info response text")?;

        let coin_info: CoinInfoResponse = serde_json::from_str(&response_text).context(format!(
            "Failed to parse coin info response. Raw response: {}",
            response_text
        ))?;

        if coin_info.ret_code != 0 {
            return Err(anyhow::anyhow!(
                "Bybit coin info API error: {} - {}",
                coin_info.ret_code,
                coin_info.ret_msg
            ));
        }

        Ok(coin_info)
    }

    /// Get account wallet balance
    /// Requires authentication
    pub async fn get_account_balance(&self, account_type: &str) -> Result<serde_json::Value> {
        if self.api_key.is_none() || self.api_secret.is_none() {
            return Err(anyhow::anyhow!(
                "Bybit account balance endpoint requires API credentials"
            ));
        }

        let api_key = self.api_key.as_ref().unwrap();
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let recv_window = "5000";

        // Build query string (params format for signature generation)
        let recv_window_params = format!("{}accountType={}", recv_window, account_type);

        // Generate signature
        let signature = self.generate_signature(timestamp, &recv_window_params)?;

        let url = format!(
            "{}/v5/account/wallet-balance?accountType={}",
            self.base_url, account_type
        );

        let response = self
            .client
            .get(&url)
            .header("X-BAPI-API-KEY", api_key)
            .header("X-BAPI-TIMESTAMP", timestamp.to_string())
            .header("X-BAPI-SIGN", signature)
            .header("X-BAPI-RECV-WINDOW", recv_window)
            .send()
            .await
            .context("Failed to get account balance from Bybit")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Bybit API error ({}): {}", status, body));
        }

        let json: serde_json::Value = response.json().await?;
        Ok(json)
    }

    /// Get asset info for funding account
    /// This uses a different endpoint than the unified account balance
    pub async fn get_funding_balance(&self, coin: Option<&str>) -> Result<serde_json::Value> {
        if self.api_key.is_none() || self.api_secret.is_none() {
            return Err(anyhow::anyhow!(
                "Bybit funding balance endpoint requires API credentials"
            ));
        }

        let api_key = self.api_key.as_ref().unwrap();
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let recv_window = "5000";

        // Build query string
        let query_string = if let Some(coin_name) = coin {
            format!("accountType=FUND&coin={}", coin_name)
        } else {
            "accountType=FUND".to_string()
        };

        // Build params for signature
        let recv_window_params = format!("{}{}", recv_window, query_string);

        // Generate signature
        let signature = self.generate_signature(timestamp, &recv_window_params)?;

        let url = format!(
            "{}/v5/asset/transfer/query-account-coins-balance?{}",
            self.base_url, query_string
        );

        let response = self
            .client
            .get(&url)
            .header("X-BAPI-API-KEY", api_key)
            .header("X-BAPI-TIMESTAMP", timestamp.to_string())
            .header("X-BAPI-SIGN", signature)
            .header("X-BAPI-RECV-WINDOW", recv_window)
            .send()
            .await
            .context("Failed to get funding balance from Bybit")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Bybit API error ({}): {}", status, body));
        }

        let json: serde_json::Value = response.json().await?;
        Ok(json)
    }

    /// Place a market order on Bybit
    /// Requires authentication
    pub async fn place_market_order(
        &self,
        symbol: &str,
        side: &str, // "Buy" or "Sell"
        qty: f64,
    ) -> Result<serde_json::Value> {
        if self.api_key.is_none() || self.api_secret.is_none() {
            return Err(anyhow::anyhow!(
                "Bybit place order endpoint requires API credentials"
            ));
        }

        let api_key = self.api_key.as_ref().unwrap();
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
            .to_string();

        let recv_window = "5000";

        // Build request parameters
        let params = serde_json::json!({
            "category": "spot",
            "symbol": symbol,
            "side": side,
            "orderType": "Market",
            "qty": qty.to_string(),
            "timeInForce": "IOC", // Immediate or Cancel
        });

        let params_str = serde_json::to_string(&params)?;

        // Generate signature: timestamp + api_key + recv_window + params_str
        let timestamp_u64: u64 = timestamp.parse()?;
        let recv_window_params = format!("{}{}", recv_window, params_str);
        let signature = self.generate_signature(timestamp_u64, &recv_window_params)?;

        let url = format!("{}/v5/order/create", self.base_url);

        let response = self
            .client
            .post(&url)
            .header("X-BAPI-API-KEY", api_key)
            .header("X-BAPI-SIGN", signature)
            .header("X-BAPI-TIMESTAMP", &timestamp)
            .header("X-BAPI-RECV-WINDOW", recv_window)
            .header("Content-Type", "application/json")
            .body(params_str)
            .send()
            .await
            .context("Failed to place order on Bybit")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Bybit API error ({}): {}", status, body));
        }

        let json: serde_json::Value = response.json().await?;
        Ok(json)
    }

    /// Query order details
    pub async fn get_order(&self, order_id: &str, symbol: &str) -> Result<serde_json::Value> {
        if self.api_key.is_none() || self.api_secret.is_none() {
            return Err(anyhow::anyhow!(
                "Bybit get order endpoint requires API credentials"
            ));
        }

        let api_key = self.api_key.as_ref().unwrap();
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
            .to_string();

        let recv_window = "5000";

        // Build query parameters
        let query_params = format!("category=spot&orderId={}&symbol={}", order_id, symbol);

        // Generate signature: timestamp + api_key + recv_window + query_params
        let timestamp_u64: u64 = timestamp.parse()?;
        let recv_window_params = format!("{}{}", recv_window, query_params);
        let signature = self.generate_signature(timestamp_u64, &recv_window_params)?;

        let url = format!("{}/v5/order/realtime?{}", self.base_url, query_params);

        let response = self
            .client
            .get(&url)
            .header("X-BAPI-API-KEY", api_key)
            .header("X-BAPI-SIGN", signature)
            .header("X-BAPI-TIMESTAMP", &timestamp)
            .header("X-BAPI-RECV-WINDOW", recv_window)
            .send()
            .await
            .context("Failed to query order from Bybit")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Bybit API error ({}): {}", status, body));
        }

        let json: serde_json::Value = response.json().await?;
        Ok(json)
    }

    /// Transfer assets between accounts (e.g., FUNDING to UNIFIED)
    pub async fn transfer_between_accounts(
        &self,
        coin: &str,
        amount: f64,
        from_account: &str, // "FUND", "UNIFIED", "SPOT", etc.
        to_account: &str,
    ) -> Result<serde_json::Value> {
        if self.api_key.is_none() || self.api_secret.is_none() {
            return Err(anyhow::anyhow!(
                "Bybit transfer endpoint requires API credentials"
            ));
        }

        let api_key = self.api_key.as_ref().unwrap();
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
            .to_string();

        let recv_window = "5000";

        // Generate a UUID for the transfer ID
        let transfer_id = uuid::Uuid::new_v4().to_string();

        // Build request parameters
        let params = serde_json::json!({
            "transferId": transfer_id,
            "coin": coin,
            "amount": amount.to_string(),
            "fromAccountType": from_account,
            "toAccountType": to_account,
        });

        let params_str = serde_json::to_string(&params)?;

        // Generate signature
        let timestamp_u64: u64 = timestamp.parse()?;
        let recv_window_params = format!("{}{}", recv_window, params_str);
        let signature = self.generate_signature(timestamp_u64, &recv_window_params)?;

        let url = format!("{}/v5/asset/transfer/inter-transfer", self.base_url);

        let response = self
            .client
            .post(&url)
            .header("X-BAPI-API-KEY", api_key)
            .header("X-BAPI-SIGN", signature)
            .header("X-BAPI-TIMESTAMP", &timestamp)
            .header("X-BAPI-RECV-WINDOW", recv_window)
            .header("Content-Type", "application/json")
            .body(params_str)
            .send()
            .await
            .context("Failed to transfer assets on Bybit")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Bybit API error ({}): {}", status, body));
        }

        let json: serde_json::Value = response.json().await?;
        Ok(json)
    }
}
