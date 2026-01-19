use crate::kucoin::client::KucoinClient;
use crate::{FilterAddressType, PriceProvider, TokenPrice};
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use dashmap::DashMap;
use futures_util::{future::try_join_all, SinkExt, StreamExt};
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message as WsMessage};

// KuCoin WebSocket requires getting connection info first
#[derive(Debug, Deserialize)]
struct BulletResponse {
    pub code: String,
    pub data: BulletData,
}

#[derive(Debug, Deserialize, Clone)]
struct BulletData {
    pub token: String,
    #[serde(rename = "instanceServers")]
    pub instance_servers: Vec<InstanceServer>,
}

#[derive(Debug, Deserialize, Clone)]
struct InstanceServer {
    pub endpoint: String,
    pub encrypt: bool,
    pub protocol: String,
    #[serde(rename = "pingInterval")]
    pub ping_interval: u64,
    #[serde(rename = "pingTimeout")]
    pub ping_timeout: u64,
}

#[derive(Debug, Deserialize)]
struct TickerMessage {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub topic: String,
    pub subject: String,
    pub data: TickerData,
}

#[derive(Debug, Deserialize)]
struct TickerData {
    pub sequence: String,
    pub price: String,
    pub size: String,
    #[serde(rename = "bestAsk")]
    pub best_ask: String,
    #[serde(rename = "bestAskSize")]
    pub best_ask_size: String,
    #[serde(rename = "bestBid")]
    pub best_bid: String,
    #[serde(rename = "bestBidSize")]
    pub best_bid_size: String,
}

#[derive(Debug, Serialize)]
struct SubscriptionRequest {
    id: String,
    #[serde(rename = "type")]
    msg_type: String,
    topic: String,
    #[serde(rename = "privateChannel")]
    private_channel: bool,
    response: bool,
}

#[derive(Debug, Serialize)]
struct PingMessage {
    id: String,
    #[serde(rename = "type")]
    msg_type: String,
}

pub struct KucoinService {
    client: KucoinClient,
    price_cache: Arc<DashMap<String, TokenPrice>>,
    symbol_to_contract: Arc<DashMap<String, String>>,
    contract_to_symbol: Arc<DashMap<String, String>>,
    token_status_cache: Arc<DashMap<String, crate::TokenStatus>>,
    symbol_precision_cache: Arc<DashMap<String, u32>>,
}

impl KucoinService {
    pub fn new(address_type: FilterAddressType) -> Self {
        Self {
            client: KucoinClient::new(address_type),
            price_cache: Arc::new(DashMap::new()),
            symbol_to_contract: Arc::new(DashMap::new()),
            contract_to_symbol: Arc::new(DashMap::new()),
            token_status_cache: Arc::new(DashMap::new()),
            symbol_precision_cache: Arc::new(DashMap::new()),
        }
    }

    pub fn with_credentials(
        address_type: FilterAddressType,
        api_key: String,
        api_secret: String,
        passphrase: String,
    ) -> Self {
        Self {
            client: KucoinClient::with_credentials(address_type, api_key, api_secret, passphrase),
            price_cache: Arc::new(DashMap::new()),
            symbol_to_contract: Arc::new(DashMap::new()),
            contract_to_symbol: Arc::new(DashMap::new()),
            token_status_cache: Arc::new(DashMap::new()),
            symbol_precision_cache: Arc::new(DashMap::new()),
        }
    }

    /// Get WebSocket connection info (bullet) from KuCoin
    async fn get_bullet(&self) -> Result<BulletData> {
        let url = format!("{}/api/v1/bullet-public", "https://api.kucoin.com");
        const MAX_RETRIES: u32 = 3;

        for attempt in 0..MAX_RETRIES {
            if attempt > 0 {
                let delay = 2u64.pow(attempt) * 1000; // Exponential backoff: 2s, 4s, 8s
                log::warn!(
                    "Retrying bullet request in {}ms (attempt {}/{})",
                    delay,
                    attempt + 1,
                    MAX_RETRIES
                );
                tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
            }

            log::info!("Requesting WebSocket bullet from: {}", url);

            let response = reqwest::Client::new()
                .post(&url)
                .send()
                .await
                .context("Failed to get KuCoin WebSocket bullet")?;

            let status = response.status();
            let response_text = response
                .text()
                .await
                .context("Failed to read bullet response text")?;

            log::debug!("Bullet response status: {}", status);
            log::debug!("Bullet response body: {}", response_text);

            // Check if we got rate limited
            if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                log::warn!("Rate limited by KuCoin, will retry...");
                continue;
            }

            let bullet_response: BulletResponse = serde_json::from_str(&response_text).context(
                format!("Failed to parse bullet response: {}", response_text),
            )?;

            if bullet_response.code == "429000" {
                log::warn!("Rate limit code in response, will retry...");
                continue;
            }

            if bullet_response.code != "200000" {
                return Err(anyhow!("KuCoin bullet API error: {}", bullet_response.code));
            }

            log::info!(
                "Bullet response - code: {}, servers: {}",
                bullet_response.code,
                bullet_response.data.instance_servers.len()
            );

            for server in &bullet_response.data.instance_servers {
                log::info!(
                    "Available server: {}, protocol: {}, ping_interval: {}ms",
                    server.endpoint,
                    server.protocol,
                    server.ping_interval
                );
            }

            return Ok(bullet_response.data);
        }

        Err(anyhow!(
            "Failed to get bullet after {} retries (rate limited)",
            MAX_RETRIES
        ))
    }

    async fn start_websocket_connection(
        connection_id: usize,
        symbols: &[String],
        price_cache: &Arc<DashMap<String, TokenPrice>>,
        symbol_to_contract: &Arc<DashMap<String, String>>,
        bullet: &BulletData,
    ) -> Result<()> {
        let server = bullet
            .instance_servers
            .first()
            .ok_or_else(|| anyhow!("No WebSocket servers available"))?;

        let ws_url = format!(
            "{}?token={}&[connectId={}]",
            server.endpoint, bullet.token, connection_id
        );

        info!(
            "Connection {}: Connecting to KuCoin WebSocket: {}",
            connection_id, server.endpoint
        );

        let (ws_stream, _) = connect_async(&ws_url)
            .await
            .context("Failed to connect to KuCoin WebSocket")?;

        let (write, mut read) = ws_stream.split();
        let write = Arc::new(tokio::sync::Mutex::new(write));

        log::info!(
            "Connection {}: WebSocket connected successfully",
            connection_id
        );

        // Wait a moment for welcome message
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        // Subscribe to ticker streams for each symbol
        // KuCoin allows comma-separated symbols in one subscription
        // Format: /market/ticker:SYMBOL1,SYMBOL2,SYMBOL3
        // But there's a limit, so we batch them
        const SYMBOLS_PER_SUBSCRIPTION: usize = 10;

        {
            let mut writer = write.lock().await;
            for (batch_num, chunk) in symbols.chunks(SYMBOLS_PER_SUBSCRIPTION).enumerate() {
                let topic = if chunk.len() == 1 {
                    format!("/market/ticker:{}", chunk[0])
                } else {
                    format!("/market/ticker:{}", chunk.join(","))
                };

                let subscription = SubscriptionRequest {
                    id: format!(
                        "{}",
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)?
                            .as_millis()
                    ),
                    msg_type: "subscribe".to_string(),
                    topic,
                    private_channel: false,
                    response: true,
                };

                let sub_msg = serde_json::to_string(&subscription)?;
                log::debug!(
                    "Connection {}: Subscribing to batch {} ({} symbols): {}",
                    connection_id,
                    batch_num + 1,
                    chunk.len(),
                    sub_msg
                );

                writer
                    .send(WsMessage::Text(sub_msg.into()))
                    .await
                    .context("Failed to send subscription message")?;

                // Small delay between subscription batches
                if batch_num + 1 < symbols.len().div_ceil(SYMBOLS_PER_SUBSCRIPTION) {
                    drop(writer); // Release lock during sleep
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                    writer = write.lock().await; // Re-acquire lock
                }
            }
        } // Release the lock

        info!(
            "Connection {}: Sent {} subscription batches for {} symbols",
            connection_id,
            symbols.len().div_ceil(SYMBOLS_PER_SUBSCRIPTION),
            symbols.len()
        );

        // Start ping task
        let ping_interval = server.ping_interval;
        let ping_write = write.clone();
        let ping_connection_id = connection_id;

        tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(tokio::time::Duration::from_millis(ping_interval));
            loop {
                interval.tick().await;
                let ping_msg = PingMessage {
                    id: format!(
                        "{}",
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_millis()
                    ),
                    msg_type: "ping".to_string(),
                };

                if let Ok(ping_str) = serde_json::to_string(&ping_msg) {
                    let mut writer = ping_write.lock().await;
                    if let Err(e) = writer.send(WsMessage::Text(ping_str.into())).await {
                        error!(
                            "Connection {}: Failed to send ping: {}",
                            ping_connection_id, e
                        );
                        break;
                    }
                    log::debug!("Connection {}: Sent ping", ping_connection_id);
                }
            }
        });

        // Process incoming messages
        log::info!(
            "Connection {}: Starting to process messages...",
            connection_id
        );

        while let Some(msg) = read.next().await {
            match msg {
                Ok(WsMessage::Text(text)) => {
                    log::debug!("Connection {}: Received text message", connection_id);
                    Self::handle_text_message(
                        connection_id,
                        &text,
                        price_cache,
                        symbol_to_contract,
                    );
                }
                Ok(WsMessage::Binary(data)) => {
                    log::debug!(
                        "Connection {}: Received binary message ({} bytes)",
                        connection_id,
                        data.len()
                    );
                }
                Ok(WsMessage::Ping(data)) => {
                    log::debug!(
                        "Connection {}: Received ping ({} bytes)",
                        connection_id,
                        data.len()
                    );
                }
                Ok(WsMessage::Pong(data)) => {
                    log::debug!(
                        "Connection {}: Received pong ({} bytes)",
                        connection_id,
                        data.len()
                    );
                }
                Ok(WsMessage::Close(frame)) => {
                    warn!(
                        "Connection {}: WebSocket closed by server: {:?}",
                        connection_id, frame
                    );
                    break;
                }
                Ok(WsMessage::Frame(_)) => {
                    log::debug!("Connection {}: Received frame", connection_id);
                }
                Err(e) => {
                    error!("Connection {}: WebSocket error: {}", connection_id, e);
                    break;
                }
            }
        }

        log::warn!("Connection {}: Message loop ended", connection_id);

        Ok(())
    }

    fn handle_text_message(
        connection_id: usize,
        text: &str,
        price_cache: &Arc<DashMap<String, TokenPrice>>,
        symbol_to_contract: &Arc<DashMap<String, String>>,
    ) {
        log::debug!("Connection {}: Received message: {}", connection_id, text);

        // Check for welcome or ack messages
        if text.contains("\"type\":\"welcome\"") {
            log::info!("Connection {}: Received welcome message", connection_id);
            return;
        }

        if text.contains("\"type\":\"ack\"") {
            log::debug!("Connection {}: Received ack message", connection_id);
            return;
        }

        if text.contains("\"type\":\"pong\"") {
            log::debug!("Connection {}: Received pong message", connection_id);
            return;
        }

        // Try to parse as ticker message
        if let Ok(ticker_msg) = serde_json::from_str::<TickerMessage>(text) {
            log::debug!(
                "Connection {}: Parsed ticker - type: {}, subject: {}, topic: {}",
                connection_id,
                ticker_msg.msg_type,
                ticker_msg.subject,
                ticker_msg.topic
            );

            if ticker_msg.msg_type == "message" && ticker_msg.subject == "trade.ticker" {
                // Extract symbol from topic: /market/ticker:BTC-USDT
                if let Some(symbol) = ticker_msg.topic.strip_prefix("/market/ticker:") {
                    if let Ok(price) = ticker_msg.data.price.parse::<f64>() {
                        // Determine the cache key: contract address if available, otherwise symbol
                        let cache_key = symbol_to_contract
                            .get(symbol)
                            .map(|entry| entry.value().clone())
                            .unwrap_or_else(|| symbol.to_lowercase());

                        log::debug!(
                            "Connection {}: Updating price for {} (key: {}) = {}",
                            connection_id,
                            symbol,
                            cache_key,
                            price
                        );

                        price_cache.insert(
                            cache_key.clone(),
                            TokenPrice {
                                symbol: cache_key,
                                price,
                            },
                        );
                    } else {
                        log::warn!(
                            "Connection {}: Failed to parse price: {}",
                            connection_id,
                            ticker_msg.data.price
                        );
                    }
                } else {
                    log::warn!(
                        "Connection {}: Invalid topic format: {}",
                        connection_id,
                        ticker_msg.topic
                    );
                }
            } else {
                log::debug!(
                    "Connection {}: Skipping message - type: {}, subject: {}",
                    connection_id,
                    ticker_msg.msg_type,
                    ticker_msg.subject
                );
            }
        } else {
            log::debug!(
                "Connection {}: Failed to parse as ticker message",
                connection_id
            );
        }
    }
}

#[async_trait]
impl PriceProvider for KucoinService {
    async fn get_price(&self, key: &str) -> Option<TokenPrice> {
        self.price_cache
            .get(&key.to_lowercase())
            .map(|entry| entry.value().clone())
    }

    async fn get_all_prices(&self) -> Vec<TokenPrice> {
        self.price_cache
            .iter()
            .map(|entry| entry.value().clone())
            .collect()
    }

    async fn get_prices(&self, keys: &Vec<String>) -> Vec<Option<TokenPrice>> {
        let mut result = Vec::new();
        for key in keys {
            result.push(self.get_price(&key.to_lowercase()).await);
        }
        result
    }

    async fn start(&self) -> Result<()> {
        // Initial token status refresh (with network and deposit verification)
        info!("KuCoin: Performing initial token status verification...");
        let safe_market_symbols = match self.refresh_token_status().await {
            Ok(symbols) => {
                info!(
                    "KuCoin: Successfully verified {} safe tokens",
                    symbols.len()
                );
                symbols
            }
            Err(e) => {
                warn!("KuCoin: Initial token status refresh failed: {}", e);
                return Ok(());
            }
        };

        if safe_market_symbols.is_empty() {
            warn!("KuCoin: No safe tokens to subscribe to after filtering");
            return Ok(());
        }

        info!(
            "KuCoin: Subscribing to {} verified safe tokens",
            safe_market_symbols.len()
        );

        // Get WebSocket connection info
        let bullet = self.get_bullet().await?;

        // Split symbols into chunks for multiple connections
        // Since we're batching 10 symbols per subscription, 100 symbols = 10 subscription requests
        // This is conservative to avoid overwhelming the connection
        const MAX_SYMBOLS_PER_CONNECTION: usize = 50;
        let connection_chunks: Vec<Vec<String>> = safe_market_symbols
            .chunks(MAX_SYMBOLS_PER_CONNECTION)
            .map(|chunk| chunk.to_vec())
            .collect();

        log::info!("Starting {} WebSocket connections", connection_chunks.len());

        // Start multiple WebSocket connections concurrently
        let mut connection_handles = Vec::new();

        for (connection_id, chunk) in connection_chunks.into_iter().enumerate() {
            let price_cache = self.price_cache.clone();
            let symbol_to_contract = self.symbol_to_contract.clone();
            let bullet_clone = bullet.clone();

            let handle = tokio::spawn(async move {
                loop {
                    info!(
                        "Starting WebSocket connection {} for {} symbols",
                        connection_id,
                        chunk.len()
                    );

                    if let Err(e) = Self::start_websocket_connection(
                        connection_id,
                        &chunk,
                        &price_cache,
                        &symbol_to_contract,
                        &bullet_clone,
                    )
                    .await
                    {
                        error!("WebSocket connection {} failed: {}", connection_id, e);
                        info!("Reconnecting connection {} in 5 seconds...", connection_id);
                        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                        continue;
                    }

                    info!(
                        "WebSocket connection {} ended, reconnecting in 5 seconds...",
                        connection_id
                    );
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                }
            });

            connection_handles.push(handle);
        }

        // Start a background task to refresh token status every 12 hours
        let refresh_service = Arc::new(Self {
            client: self.client.clone(),
            price_cache: self.price_cache.clone(),
            symbol_to_contract: self.symbol_to_contract.clone(),
            contract_to_symbol: self.contract_to_symbol.clone(),
            token_status_cache: self.token_status_cache.clone(),
            symbol_precision_cache: self.symbol_precision_cache.clone(),
        });
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(12 * 3600)); // 12 hours
            interval.tick().await; // Skip first immediate tick

            loop {
                interval.tick().await;
                info!("KuCoin: Starting scheduled token status refresh (every 12 hours)...");
                if let Err(e) = refresh_service.refresh_token_status().await {
                    warn!("KuCoin: Scheduled token status refresh failed: {}", e);
                }
            }
        });

        // Start statistics logging task
        let stats_price_cache = self.price_cache.clone();
        let stats_symbol_map = self.symbol_to_contract.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
            loop {
                interval.tick().await;

                let token_count = stats_price_cache.len();
                let symbol_count = stats_symbol_map.len();

                info!(
                    "KuCoin Service Stats - Tokens with prices: {}, Contracts mapped: {}",
                    token_count, symbol_count
                );
            }
        });

        // Wait for all connections
        let results: Result<Vec<_>, _> = try_join_all(connection_handles).await;
        results.context("One or more WebSocket connections failed")?;

        Ok(())
    }

    fn get_price_provider_name(&self) -> &'static str {
        "KuCoin"
    }

    async fn is_token_safe_for_arbitrage(
        &self,
        symbol: &str,
        contract_address: Option<&str>,
    ) -> bool {
        let status = self.get_token_status(symbol, contract_address).await;
        match status {
            Some(status) => {
                status.is_trading && status.is_deposit_enabled && status.network_verified
            }
            None => false,
        }
    }

    async fn get_token_status(
        &self,
        symbol: &str,
        contract_address: Option<&str>,
    ) -> Option<crate::TokenStatus> {
        // Try to get from cache first using market symbol (e.g., "LINK-USDT")
        if let Some(status) = self.token_status_cache.get(symbol) {
            return Some(status.clone());
        }

        // If not in cache and we have a contract address, try to find by contract
        if let Some(contract_addr) = contract_address {
            let normalized_addr = contract_addr.to_lowercase();
            if let Some(market_symbol) = self.contract_to_symbol.get(&normalized_addr) {
                return self
                    .token_status_cache
                    .get(market_symbol.value())
                    .map(|s| s.clone());
            }
        }

        None
    }

    async fn refresh_token_status(&self) -> Result<Vec<String>> {
        info!("KuCoin: Refreshing token status cache...");

        // Get all trading pairs
        let pairs = self.client.get_token_usdt_pairs().await?;

        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut verified_count = 0;
        let mut failed_count = 0;

        for pair in pairs {
            let market_symbol = pair.symbol.clone(); // e.g., "LINK-USDT"
            let base_asset = pair.base_currency.clone();

            // Extract and cache precision from base_increment
            if let Some(ref base_increment) = pair.base_increment {
                if let Ok(increment_val) = base_increment.parse::<f64>() {
                    let precision = if increment_val >= 1.0 {
                        0 // No decimals needed
                    } else {
                        // Count decimal places: e.g., "0.01" = 2, "0.0001" = 4
                        base_increment
                            .split('.')
                            .nth(1)
                            .map(|s| s.len() as u32)
                            .unwrap_or(8)
                    };
                    self.symbol_precision_cache
                        .insert(base_asset.clone(), precision);
                }
            }

            // Default status: trading enabled (from exchange info), but need to verify deposits
            let mut status = crate::TokenStatus {
                symbol: market_symbol.clone(),
                base_asset: base_asset.clone(),
                contract_address: None,
                is_trading: pair.enable_trading,
                is_deposit_enabled: false,
                network_verified: false,
                last_updated: current_time,
            };

            // Get currency detail to check chain information
            match self.client.get_currency_detail(&base_asset).await {
                Ok(currency_detail) => {
                    log::debug!(
                        "KuCoin: Checking token {} with {} chains",
                        base_asset,
                        currency_detail.chains.len()
                    );

                    // Check if there's a network that matches our requirements
                    for chain_detail in &currency_detail.chains {
                        let chain_name = chain_detail.chain_name.as_str();

                        let is_correct_network = match self.client.address_type {
                            FilterAddressType::Ethereum => {
                                // KuCoin uses chain names like "ETH", "ERC20", or "Ethereum"
                                let chain_lower = chain_name.to_lowercase();
                                (chain_lower.contains("eth") || chain_lower.contains("erc20"))
                                    && !chain_lower.contains("bsc")
                                    && !chain_lower.contains("arb")
                                    && !chain_lower.contains("polygon")
                                    && !chain_lower.contains("optimism")
                            }
                            FilterAddressType::Solana => {
                                let chain_lower = chain_name.to_lowercase();
                                chain_lower.contains("sol") && !chain_lower.contains("bsc")
                            }
                        };

                        if is_correct_network && chain_detail.is_deposit_enabled {
                            let contract = &chain_detail.contract_address;
                            if !contract.is_empty() && self.client.is_valid_address(contract) {
                                let normalized_contract = contract.to_lowercase();
                                status.contract_address = Some(normalized_contract.clone());
                                status.is_deposit_enabled = true;
                                status.network_verified = true;

                                if status.is_trading
                                    && status.is_deposit_enabled
                                    && status.network_verified
                                {
                                    verified_count += 1;
                                    log::debug!(
                                        "KuCoin: ✓ Verified {} - trading:{} deposit:{} network:{}",
                                        base_asset,
                                        status.is_trading,
                                        status.is_deposit_enabled,
                                        status.network_verified
                                    );
                                }

                                // Store contract mapping
                                self.contract_to_symbol
                                    .insert(normalized_contract.clone(), market_symbol.clone());
                                self.symbol_to_contract
                                    .insert(market_symbol.clone(), normalized_contract);
                                break;
                            }
                        }
                    }
                }
                Err(e) => {
                    log::debug!(
                        "KuCoin: Failed to get currency detail for {}: {}",
                        base_asset,
                        e
                    );
                }
            }

            if !status.network_verified {
                failed_count += 1;
                log::debug!(
                    "KuCoin: Token {} - network verification failed or deposits disabled",
                    base_asset
                );
            }

            // Store in cache
            self.token_status_cache.insert(market_symbol, status);
        }

        info!(
            "KuCoin: Token status refresh complete. Verified: {}, Failed: {}, Total: {}",
            verified_count,
            failed_count,
            verified_count + failed_count
        );

        // Return list of verified safe market symbols
        let safe_symbols: Vec<String> = self
            .token_status_cache
            .iter()
            .filter_map(|entry| {
                let status = entry.value();
                if status.is_trading && status.is_deposit_enabled && status.network_verified {
                    Some(status.symbol.clone())
                } else {
                    None
                }
            })
            .collect();

        info!(
            "KuCoin: Returning {} safe symbols for WebSocket subscription",
            safe_symbols.len()
        );
        Ok(safe_symbols)
    }

    async fn get_deposit_address(
        &self,
        _symbol: &str,
        _address_type: crate::FilterAddressType,
    ) -> Result<String> {
        Err(anyhow::anyhow!(
            "KuCoin: get_deposit_address not yet implemented"
        ))
    }

    async fn sell_token_for_usdt(&self, symbol: &str, amount: f64) -> Result<(String, f64, f64)> {
        self.sell_token_for_usdt_impl(symbol, amount).await
    }

    async fn withdraw_usdt(
        &self,
        _address: &str,
        _amount: f64,
        _address_type: crate::FilterAddressType,
    ) -> Result<String> {
        Err(anyhow::anyhow!("KuCoin: withdraw_usdt not yet implemented"))
    }

    async fn get_portfolio(&self) -> Result<crate::Portfolio> {
        self.get_portfolio_impl().await
    }

    async fn transfer_all_to_trading(&self, coin: Option<&str>) -> Result<u32> {
        self.transfer_all_to_trading_impl(coin).await
    }

    async fn transfer_all_to_funding(&self, coin: Option<&str>) -> Result<u32> {
        self.transfer_all_to_funding_impl(coin).await
    }

    async fn get_token_symbol_for_contract_address(
        &self,
        contract_address: &str,
    ) -> Option<String> {
        // Get the market symbol (e.g., "LINK-USDT") and extract base asset
        let market_symbol = self
            .contract_to_symbol
            .get(&contract_address.to_lowercase())
            .map(|entry| entry.value().clone())?;

        // KuCoin uses dash separator, split on "-USDT"
        if let Some(base) = market_symbol.strip_suffix("-USDT") {
            Some(base.to_string())
        } else {
            Some(market_symbol)
        }
    }

    async fn get_quantity_precision(&self, symbol: &str) -> Result<u32> {
        // Check cache first
        if let Some(precision) = self.symbol_precision_cache.get(symbol) {
            return Ok(*precision);
        }

        // If not in cache, refresh token status and try again
        log::info!(
            "KuCoin: Precision not in cache for {}, refreshing...",
            symbol
        );
        self.refresh_token_status().await?;

        // Check cache again after refresh
        if let Some(precision) = self.symbol_precision_cache.get(symbol) {
            return Ok(*precision);
        }

        // If still not found, return default
        log::warn!(
            "KuCoin: Could not find precision for {}, using default (8)",
            symbol
        );
        Ok(8)
    }
}

impl KucoinService {
    /// Estimate how much USDT you'd get by selling a certain amount of tokens on KuCoin
    pub async fn estimate_sell_output(
        &self,
        contract_address: &str,
        token_amount: f64,
    ) -> Result<f64> {
        let symbol = self
            .contract_to_symbol
            .get(&contract_address.to_lowercase())
            .map(|entry| entry.value().clone())
            .context("Contract address not found in KuCoin markets")?;

        let orderbook = self.client.get_orderbook(&symbol, 100).await?;

        let mut remaining_tokens = token_amount;
        let mut total_usdt = 0.0;

        for bid in orderbook.data.bids {
            if remaining_tokens <= 0.0 {
                break;
            }

            let price: f64 = bid[0].parse().context("Failed to parse bid price")?;
            let quantity: f64 = bid[1].parse().context("Failed to parse bid quantity")?;

            let tokens_to_sell = remaining_tokens.min(quantity);
            total_usdt += tokens_to_sell * price;
            remaining_tokens -= tokens_to_sell;
        }

        if remaining_tokens > 0.0 {
            warn!(
                "Orderbook depth insufficient for {} {} (contract: {}), {} tokens remaining unsold",
                token_amount, symbol, contract_address, remaining_tokens
            );
        }

        Ok(total_usdt)
    }

    async fn get_portfolio_impl(&self) -> Result<crate::Portfolio> {
        let account_data = self.client.get_account_balance().await?;

        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut trading_balances: Vec<crate::Balance> = Vec::new();
        let mut trading_usdt_value = 0.0;
        let mut funding_balances: Vec<crate::Balance> = Vec::new();
        let mut funding_usdt_value = 0.0;

        // Parse KuCoin response structure
        // KuCoin has: trade (trading), main (funding), margin
        if let Some(data) = account_data.get("data").and_then(|v| v.as_array()) {
            for account in data {
                let asset = account
                    .get("currency")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                let account_type = account.get("type").and_then(|v| v.as_str()).unwrap_or("");

                let available: f64 = account
                    .get("available")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0.0);

                let holds: f64 = account
                    .get("holds")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0.0);

                let total = available + holds;

                // Only include non-zero balances
                if total > 0.0 {
                    // Calculate USDT value
                    let usdt_value = if asset == "USDT" {
                        total
                    } else {
                        // Try to get the current price for this asset
                        let symbol = format!("{}-USDT", asset);
                        if let Some(price_info) = self.get_price(&symbol.to_lowercase()).await {
                            total * price_info.price
                        } else {
                            0.0
                        }
                    };

                    log::info!(
                        "KuCoin {}: {} - {} (free: {}, locked: {}, USD: ${})",
                        account_type,
                        asset,
                        total,
                        available,
                        holds,
                        usdt_value
                    );

                    let balance = crate::Balance {
                        asset,
                        free: available,
                        locked: holds,
                        total,
                    };

                    // Categorize: trade + margin = trading, main = funding
                    match account_type {
                        "trade" | "margin" => {
                            trading_usdt_value += usdt_value;
                            trading_balances.push(balance);
                        }
                        "main" => {
                            funding_usdt_value += usdt_value;
                            funding_balances.push(balance);
                        }
                        _ => {
                            log::debug!("Unknown KuCoin account type: {}", account_type);
                        }
                    }
                }
            }
        }

        let total_usdt_value = trading_usdt_value + funding_usdt_value;

        log::info!(
            "KuCoin: Portfolio - Trading: ${:.2}, Funding: ${:.2}, Total: ${:.2}",
            trading_usdt_value,
            funding_usdt_value,
            total_usdt_value
        );

        Ok(crate::Portfolio {
            exchange: "KuCoin".to_string(),
            trading: crate::AccountBalances {
                balances: trading_balances,
                total_usdt_value: trading_usdt_value,
            },
            funding: crate::AccountBalances {
                balances: funding_balances,
                total_usdt_value: funding_usdt_value,
            },
            total_usdt_value,
            timestamp: current_time,
        })
    }

    /// Sell tokens for USDT using market order
    /// Will sell all available balance of the token across all accounts
    /// Checks trade, main, and margin accounts and transfers to trade if needed
    async fn sell_token_for_usdt_impl(
        &self,
        symbol: &str,
        amount: f64,
    ) -> Result<(String, f64, f64)> {
        // KuCoin uses dash-separated symbols (e.g., LINK-USDT)
        let symbol_pair = format!("{}-USDT", symbol);

        log::info!(
            "KuCoin: Checking balances across all account types for {}",
            symbol
        );

        // Get all account balances
        let account_data = self.client.get_account_balance().await?;

        let mut trade_balance = 0.0;
        let mut main_balance = 0.0;
        let mut margin_balance = 0.0;

        // Parse account balances by type
        if let Some(data) = account_data.get("data").and_then(|v| v.as_array()) {
            for account in data {
                let currency = account
                    .get("currency")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                if currency != symbol {
                    continue;
                }

                let account_type = account.get("type").and_then(|v| v.as_str()).unwrap_or("");

                let available: f64 = account
                    .get("available")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0.0);

                match account_type {
                    "trade" => trade_balance = available,
                    "main" => main_balance = available,
                    "margin" => margin_balance = available,
                    _ => {}
                }

                log::info!(
                    "KuCoin: Found {} balance in {} account: {}",
                    symbol,
                    account_type,
                    available
                );
            }
        }

        let total_balance = trade_balance + main_balance + margin_balance;
        log::info!(
            "KuCoin: Total {} balance: {} (trade: {}, main: {}, margin: {})",
            symbol,
            total_balance,
            trade_balance,
            main_balance,
            margin_balance
        );

        if total_balance < amount {
            return Err(anyhow::anyhow!(
                "Insufficient {} balance: have {}, need {}",
                symbol,
                total_balance,
                amount
            ));
        }

        // Transfer from main to trade if needed
        if main_balance > 0.0 && trade_balance < amount {
            let transfer_amount = main_balance.min(amount - trade_balance);
            log::info!(
                "KuCoin: Transferring {} {} from main to trade account",
                transfer_amount,
                symbol
            );

            let transfer_result = self
                .client
                .transfer_between_accounts(symbol, "main", "trade", &transfer_amount.to_string())
                .await?;

            log::info!(
                "KuCoin: Transfer result: {}",
                serde_json::to_string_pretty(&transfer_result)?
            );

            trade_balance += transfer_amount;

            // Wait for transfer to complete
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        }

        // Transfer from margin to trade if still needed
        if margin_balance > 0.0 && trade_balance < amount {
            let transfer_amount = margin_balance.min(amount - trade_balance);
            log::info!(
                "KuCoin: Transferring {} {} from margin to trade account",
                transfer_amount,
                symbol
            );

            let transfer_result = self
                .client
                .transfer_between_accounts(symbol, "margin", "trade", &transfer_amount.to_string())
                .await?;

            log::info!(
                "KuCoin: Transfer result: {}",
                serde_json::to_string_pretty(&transfer_result)?
            );

            trade_balance += transfer_amount;

            // Wait for transfer to complete
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        }

        log::info!(
            "KuCoin: Ready to sell {} {} from trade account",
            amount,
            symbol
        );

        // Place market sell order
        let order_result = self
            .client
            .place_market_order(&symbol_pair, "sell", amount)
            .await?;

        log::info!(
            "KuCoin order placement response: {}",
            serde_json::to_string_pretty(&order_result)?
        );

        // Extract order ID from response
        let order_id = order_result
            .get("data")
            .and_then(|d| d.get("orderId"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("No orderId in response"))?
            .to_string();

        // Wait a moment for order to execute
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        // Query order to get execution details
        let order_status = self.client.get_order(&order_id).await?;
        log::info!(
            "KuCoin order status response: {}",
            serde_json::to_string_pretty(&order_status)?
        );

        // Extract execution details from the data object
        let data = order_status
            .get("data")
            .ok_or_else(|| anyhow::anyhow!("No data in order status response"))?;

        // dealSize = executed quantity in base currency (tokens sold)
        let executed_qty = data
            .get("dealSize")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<f64>().ok())
            .or_else(|| data.get("dealSize").and_then(|v| v.as_f64()))
            .unwrap_or(0.0);

        // dealFunds = total value in quote currency (USDT received)
        let usdt_received = data
            .get("dealFunds")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<f64>().ok())
            .or_else(|| data.get("dealFunds").and_then(|v| v.as_f64()))
            .unwrap_or(0.0);

        Ok((order_id, executed_qty, usdt_received))
    }

    /// Transfer all assets from main/margin accounts to trade account
    /// This prepares assets for trading
    pub async fn transfer_all_to_trading_impl(&self, coin: Option<&str>) -> Result<u32> {
        println!("KuCoin: Transferring all assets to trading account...");

        // Get all accounts
        let accounts_resp = self.client.get_account_balance().await?;
        let accounts = accounts_resp
            .get("data")
            .and_then(|d| d.as_array())
            .ok_or_else(|| anyhow::anyhow!("Invalid accounts response"))?;

        let mut transfer_count = 0u32;

        // Group balances by currency and account type
        let mut balances: std::collections::HashMap<
            String,
            std::collections::HashMap<String, f64>,
        > = std::collections::HashMap::new();

        for account in accounts {
            let account_obj = account.as_object().unwrap();
            let currency = account_obj
                .get("currency")
                .and_then(|c| c.as_str())
                .unwrap_or("");
            let account_type = account_obj
                .get("type")
                .and_then(|t| t.as_str())
                .unwrap_or("");
            let available = account_obj
                .get("available")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(0.0);

            if available > 0.0 {
                balances
                    .entry(currency.to_string())
                    .or_default()
                    .insert(account_type.to_string(), available);
            }
        }

        // Transfer from main and margin to trade
        for (currency, accounts) in balances.iter() {
            // Filter by coin if specified
            if let Some(target_coin) = coin {
                if currency != target_coin {
                    continue;
                }
            }

            // Transfer from main to trade
            if let Some(&amount) = accounts.get("main") {
                if amount > 0.0 {
                    println!("  Transferring {} {} from main to trade", amount, currency);

                    match self
                        .client
                        .transfer_between_accounts(currency, "main", "trade", &amount.to_string())
                        .await
                    {
                        Ok(_) => {
                            transfer_count += 1;
                            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                        }
                        Err(e) => {
                            eprintln!(
                                "  Failed to transfer {} {} from main: {}",
                                currency, amount, e
                            );
                        }
                    }
                }
            }

            // Transfer from margin to trade
            if let Some(&amount) = accounts.get("margin") {
                if amount > 0.0 {
                    println!(
                        "  Transferring {} {} from margin to trade",
                        amount, currency
                    );

                    match self
                        .client
                        .transfer_between_accounts(currency, "margin", "trade", &amount.to_string())
                        .await
                    {
                        Ok(_) => {
                            transfer_count += 1;
                            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                        }
                        Err(e) => {
                            eprintln!(
                                "  Failed to transfer {} {} from margin: {}",
                                currency, amount, e
                            );
                        }
                    }
                }
            }
        }

        if transfer_count > 0 {
            println!(
                "KuCoin: Transferred {} assets to trading account",
                transfer_count
            );
        } else {
            println!("KuCoin: No assets to transfer to trading account");
        }

        Ok(transfer_count)
    }

    /// Transfer all assets from trade/margin accounts to main account (funding)
    /// This prepares assets for withdrawal
    pub async fn transfer_all_to_funding_impl(&self, coin: Option<&str>) -> Result<u32> {
        println!("KuCoin: Transferring all assets to funding account (main)...");

        // Get all accounts
        let accounts_resp = self.client.get_account_balance().await?;
        let accounts = accounts_resp
            .get("data")
            .and_then(|d| d.as_array())
            .ok_or_else(|| anyhow::anyhow!("Invalid accounts response"))?;

        let mut transfer_count = 0u32;

        // Group balances by currency and account type
        let mut balances: std::collections::HashMap<
            String,
            std::collections::HashMap<String, f64>,
        > = std::collections::HashMap::new();

        for account in accounts {
            let account_obj = account.as_object().unwrap();
            let currency = account_obj
                .get("currency")
                .and_then(|c| c.as_str())
                .unwrap_or("");
            let account_type = account_obj
                .get("type")
                .and_then(|t| t.as_str())
                .unwrap_or("");
            let available = account_obj
                .get("available")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(0.0);

            if available > 0.0 {
                balances
                    .entry(currency.to_string())
                    .or_default()
                    .insert(account_type.to_string(), available);
            }
        }

        // Transfer from trade and margin to main
        for (currency, accounts) in balances.iter() {
            // Filter by coin if specified
            if let Some(target_coin) = coin {
                if currency != target_coin {
                    continue;
                }
            }

            // Transfer from trade to main
            if let Some(&amount) = accounts.get("trade") {
                if amount > 0.0 {
                    println!("  Transferring {} {} from trade to main", amount, currency);

                    match self
                        .client
                        .transfer_between_accounts(currency, "trade", "main", &amount.to_string())
                        .await
                    {
                        Ok(_) => {
                            transfer_count += 1;
                            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                        }
                        Err(e) => {
                            eprintln!(
                                "  Failed to transfer {} {} from trade: {}",
                                currency, amount, e
                            );
                        }
                    }
                }
            }

            // Transfer from margin to main
            if let Some(&amount) = accounts.get("margin") {
                if amount > 0.0 {
                    println!("  Transferring {} {} from margin to main", amount, currency);

                    match self
                        .client
                        .transfer_between_accounts(currency, "margin", "main", &amount.to_string())
                        .await
                    {
                        Ok(_) => {
                            transfer_count += 1;
                            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                        }
                        Err(e) => {
                            eprintln!(
                                "  Failed to transfer {} {} from margin: {}",
                                currency, amount, e
                            );
                        }
                    }
                }
            }
        }

        if transfer_count > 0 {
            println!(
                "KuCoin: Transferred {} assets to funding account",
                transfer_count
            );
        } else {
            println!("KuCoin: No assets to transfer to funding account");
        }

        Ok(transfer_count)
    }
}
