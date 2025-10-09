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
}

impl KucoinService {
    pub fn new(address_type: FilterAddressType) -> Self {
        Self {
            client: KucoinClient::new(address_type),
            price_cache: Arc::new(DashMap::new()),
            symbol_to_contract: Arc::new(DashMap::new()),
            contract_to_symbol: Arc::new(DashMap::new()),
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

            let bullet_response: BulletResponse = serde_json::from_str(&response_text)
                .context(format!("Failed to parse bullet response: {}", response_text))?;

            if bullet_response.code == "429000" {
                log::warn!("Rate limit code in response, will retry...");
                continue;
            }

            if bullet_response.code != "200000" {
                return Err(anyhow!(
                    "KuCoin bullet API error: {}",
                    bullet_response.code
                ));
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

        Err(anyhow!("Failed to get bullet after {} retries (rate limited)", MAX_RETRIES))
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

        log::info!("Connection {}: WebSocket connected successfully", connection_id);

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
                    id: format!("{}", std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)?
                        .as_millis()),
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
                if batch_num + 1 < (symbols.len() + SYMBOLS_PER_SUBSCRIPTION - 1) / SYMBOLS_PER_SUBSCRIPTION {
                    drop(writer); // Release lock during sleep
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                    writer = write.lock().await; // Re-acquire lock
                }
            }
        } // Release the lock

        info!(
            "Connection {}: Sent {} subscription batches for {} symbols",
            connection_id,
            (symbols.len() + SYMBOLS_PER_SUBSCRIPTION - 1) / SYMBOLS_PER_SUBSCRIPTION,
            symbols.len()
        );

        // Start ping task
        let ping_interval = server.ping_interval;
        let ping_write = write.clone();
        let ping_connection_id = connection_id;

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(ping_interval));
            loop {
                interval.tick().await;
                let ping_msg = PingMessage {
                    id: format!("{}", std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_millis()),
                    msg_type: "ping".to_string(),
                };

                if let Ok(ping_str) = serde_json::to_string(&ping_msg) {
                    let mut writer = ping_write.lock().await;
                    if let Err(e) = writer.send(WsMessage::Text(ping_str.into())).await {
                        error!("Connection {}: Failed to send ping: {}", ping_connection_id, e);
                        break;
                    }
                    log::debug!("Connection {}: Sent ping", ping_connection_id);
                }
            }
        });

        // Process incoming messages
        log::info!("Connection {}: Starting to process messages...", connection_id);

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
                    log::debug!("Connection {}: Received binary message ({} bytes)", connection_id, data.len());
                }
                Ok(WsMessage::Ping(data)) => {
                    log::debug!("Connection {}: Received ping ({} bytes)", connection_id, data.len());
                }
                Ok(WsMessage::Pong(data)) => {
                    log::debug!("Connection {}: Received pong ({} bytes)", connection_id, data.len());
                }
                Ok(WsMessage::Close(frame)) => {
                    warn!("Connection {}: WebSocket closed by server: {:?}", connection_id, frame);
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
                        log::warn!("Connection {}: Failed to parse price: {}", connection_id, ticker_msg.data.price);
                    }
                } else {
                    log::warn!("Connection {}: Invalid topic format: {}", connection_id, ticker_msg.topic);
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
            log::debug!("Connection {}: Failed to parse as ticker message", connection_id);
        }
    }
}

#[async_trait]
impl PriceProvider for KucoinService {
    async fn get_price(&self, key: &str) -> Option<TokenPrice> {
        self.price_cache.get(&key.to_lowercase()).map(|entry| entry.value().clone())
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
        info!("Starting KuCoin Service");

        // Get all USDT trading pairs
        let pairs = self
            .client
            .get_token_usdt_pairs()
            .await
            .context("Failed to fetch KuCoin trading pairs")?;

        info!("Found {} USDT trading pairs", pairs.len());

        // Fetch contract addresses for each currency in parallel
        // Note: KuCoin's public API provides contract addresses without authentication!
        log::info!("Fetching currency details for {} pairs in parallel...", pairs.len());

        // Create a set of unique base currencies to avoid duplicate API calls
        let unique_currencies: std::collections::HashSet<String> = pairs
            .iter()
            .map(|p| p.base_currency.clone())
            .collect();

        log::info!("Found {} unique currencies to query", unique_currencies.len());

        // Fetch all currency details concurrently in batches
        // Use smaller batch size and add delays to respect rate limits
        const BATCH_SIZE: usize = 10;
        const BATCH_DELAY_MS: u64 = 1000; // 1 second between batches

        let currencies: Vec<String> = unique_currencies.into_iter().collect();
        let mut all_currency_details = Vec::new();

        for (batch_num, chunk) in currencies.chunks(BATCH_SIZE).enumerate() {
            log::info!(
                "Fetching batch {}/{} ({} currencies)...",
                batch_num + 1,
                (currencies.len() + BATCH_SIZE - 1) / BATCH_SIZE,
                chunk.len()
            );

            let futures: Vec<_> = chunk
                .iter()
                .map(|currency| {
                    let client = &self.client;
                    let currency = currency.clone();
                    async move {
                        (currency.clone(), client.get_currency_detail(&currency).await)
                    }
                })
                .collect();

            let results = futures_util::future::join_all(futures).await;
            all_currency_details.extend(results);

            log::info!(
                "Batch {} complete ({}/{})",
                batch_num + 1,
                ((batch_num + 1) * BATCH_SIZE).min(currencies.len()),
                currencies.len()
            );

            // Add delay between batches to avoid rate limiting
            if batch_num + 1 < (currencies.len() + BATCH_SIZE - 1) / BATCH_SIZE {
                tokio::time::sleep(tokio::time::Duration::from_millis(BATCH_DELAY_MS)).await;
            }
        }

        log::info!("Fetched {} currency details", all_currency_details.len());

        // Build a map of currency -> contract addresses
        let mut currency_contracts: std::collections::HashMap<String, Vec<(String, String)>> =
            std::collections::HashMap::new();

        for (currency, result) in all_currency_details {
            if let Ok(currency_detail) = result {
                let mut contracts = Vec::new();
                for chain in &currency_detail.chains {
                    if !chain.contract_address.is_empty() {
                        contracts.push((chain.chain_name.clone(), chain.contract_address.clone()));
                    }
                }
                if !contracts.is_empty() {
                    currency_contracts.insert(currency, contracts);
                }
            }
        }

        log::info!(
            "Found contract addresses for {} currencies",
            currency_contracts.len()
        );

        // Now map trading pairs to contract addresses
        let mut contract_count = 0;
        let mut filtered_count = 0;

        for pair in &pairs {
            if let Some(contracts) = currency_contracts.get(&pair.base_currency) {
                for (chain_name, contract_address) in contracts {
                    let is_target_chain = match self.client.address_type {
                        FilterAddressType::Ethereum => {
                            chain_name.to_lowercase().contains("eth") ||
                            chain_name == "ERC20"
                        }
                        FilterAddressType::Solana => {
                            chain_name.to_lowercase().contains("sol") ||
                            chain_name == "SPL"
                        }
                    };

                    if is_target_chain && self.client.is_valid_address(contract_address) {
                        self.symbol_to_contract.insert(
                            pair.symbol.clone(),
                            contract_address.clone(),
                        );
                        self.contract_to_symbol.insert(
                            contract_address.to_lowercase(),
                            pair.symbol.clone(),
                        );
                        contract_count += 1;
                        break;
                    } else if is_target_chain {
                        filtered_count += 1;
                        log::debug!(
                            "Filtered out {} with invalid contract address: {}",
                            pair.symbol,
                            contract_address
                        );
                        break;
                    }
                }
            }
        }

        log::info!(
            "Mapped {} trading pairs to contract addresses ({} filtered out as invalid)",
            contract_count,
            filtered_count
        );

        // Filter pairs: only subscribe to those with valid contract addresses
        let symbols: Vec<String> = if contract_count > 0 {
            pairs
                .iter()
                .filter(|pair| self.symbol_to_contract.contains_key(&pair.symbol))
                .map(|pair| pair.symbol.clone())
                .collect()
        } else {
            log::warn!("No contract addresses found, subscribing to all USDT pairs");
            pairs.iter().map(|pair| pair.symbol.clone()).collect()
        };

        if symbols.is_empty() {
            log::warn!("No symbols to subscribe to after filtering");
            return Ok(());
        }

        log::info!(
            "Subscribing to {} symbols (filtered from {} total)",
            symbols.len(),
            pairs.len()
        );

        // Get WebSocket connection info
        let bullet = self.get_bullet().await?;

        // Split symbols into chunks for multiple connections
        // Since we're batching 10 symbols per subscription, 100 symbols = 10 subscription requests
        // This is conservative to avoid overwhelming the connection
        const MAX_SYMBOLS_PER_CONNECTION: usize = 50;
        let connection_chunks: Vec<Vec<String>> = symbols
            .chunks(MAX_SYMBOLS_PER_CONNECTION)
            .map(|chunk| chunk.to_vec())
            .collect();

        log::info!(
            "Starting {} WebSocket connections",
            connection_chunks.len()
        );

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
}
