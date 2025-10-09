use crate::bybit::client::BybitClient;
use crate::{FilterAddressType, PriceProvider, TokenPrice};
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use axum::body::Bytes;
use dashmap::DashMap;
use futures_util::{future::try_join_all, SinkExt, StreamExt};
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use std::sync::Arc;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message as WsMessage};

// Bybit WebSocket endpoints
pub const BYBIT_WS_URL_SPOT: &str = "wss://stream.bybit.com/v5/public/spot";

#[derive(Debug, Deserialize, Serialize)]
struct TickerData {
    symbol: String,
    #[serde(rename = "lastPrice")]
    last_price: String,
    #[serde(rename = "bid1Price")]
    bid1_price: Option<String>,
    #[serde(rename = "ask1Price")]
    ask1_price: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct TickerMessage {
    topic: String,
    #[serde(rename = "type")]
    msg_type: String,
    data: TickerData,
    ts: u64,
}

#[derive(Debug, Serialize)]
struct SubscriptionRequest {
    op: String,
    args: Vec<String>,
}

pub struct BybitService {
    client: BybitClient,
    price_cache: Arc<DashMap<String, TokenPrice>>,  // Maps contract_address -> TokenPrice
    symbol_to_contract: Arc<DashMap<String, String>>, // Maps symbol -> contract_address
    contract_to_symbol: Arc<DashMap<String, String>>, // Maps contract_address -> symbol
}

impl BybitService {
    /// Create service without authentication - will not filter by contract address
    pub fn new(address_type: FilterAddressType) -> Self {
        Self {
            client: BybitClient::new(address_type),
            price_cache: Arc::new(DashMap::new()),
            symbol_to_contract: Arc::new(DashMap::new()),
            contract_to_symbol: Arc::new(DashMap::new()),
        }
    }

    /// Create service with authentication - can filter by contract address
    pub fn with_credentials(
        address_type: FilterAddressType,
        api_key: String,
        api_secret: String,
    ) -> Self {
        Self {
            client: BybitClient::with_credentials(address_type, api_key, api_secret),
            price_cache: Arc::new(DashMap::new()),
            symbol_to_contract: Arc::new(DashMap::new()),
            contract_to_symbol: Arc::new(DashMap::new()),
        }
    }

    async fn start_websocket_connection(
        connection_id: usize,
        symbols: &[String],
        price_cache: &Arc<DashMap<String, TokenPrice>>,
        symbol_to_contract: &Arc<DashMap<String, String>>,
    ) -> Result<()> {
        let ws_url = BYBIT_WS_URL_SPOT;

        info!(
            "Connection {}: Connecting to Bybit WebSocket: {}",
            connection_id, ws_url
        );

        let (ws_stream, _) = connect_async(ws_url)
            .await
            .context("Failed to connect to Bybit WebSocket")?;

        let (mut write, mut read) = ws_stream.split();

        // Subscribe to ticker streams
        // Bybit allows up to 10 args per subscription for spot
        const MAX_SYMBOLS_PER_SUBSCRIPTION: usize = 10;

        for chunk in symbols.chunks(MAX_SYMBOLS_PER_SUBSCRIPTION) {
            let topics: Vec<String> = chunk
                .iter()
                .map(|symbol| format!("tickers.{}", symbol))
                .collect();

            let subscribe_msg = SubscriptionRequest {
                op: "subscribe".to_string(),
                args: topics,
            };

            let msg_json = serde_json::to_string(&subscribe_msg)
                .context("Failed to serialize subscription message")?;

            info!(
                "Connection {}: Subscribing to {} symbols",
                connection_id,
                chunk.len()
            );

            let msg = WsMessage::Text(msg_json.into());
            if let Err(e) = write.send(msg).await {
                error!(
                    "Connection {}: Failed to send subscription: {}",
                    connection_id, e
                );
            }

            // Small delay between subscription batches
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }

        // Create a ping interval timer (Bybit recommends every 20 seconds)
        let mut ping_interval = tokio::time::interval(tokio::time::Duration::from_secs(20));
        ping_interval.tick().await; // Skip the first immediate tick

        // Handle incoming messages and periodic pings
        loop {
            tokio::select! {
                // Handle incoming WebSocket messages
                msg = read.next() => {
                    match msg {
                        Some(Ok(WsMessage::Text(text))) => {
                            if let Err(e) = Self::handle_text_message(
                                &text,
                                price_cache,
                                symbol_to_contract,
                                connection_id,
                            ) {
                                log::debug!(
                                    "Connection {}: Error handling message: {}",
                                    connection_id, e
                                );
                            }
                        }
                        Some(Ok(WsMessage::Binary(data))) => {
                            log::debug!(
                                "Connection {}: Received binary message (length: {})",
                                connection_id,
                                data.len()
                            );
                        }
                        Some(Ok(WsMessage::Ping(data))) => {
                            log::debug!("Connection {}: Received ping, sending pong", connection_id);
                            if let Err(e) = write.send(WsMessage::Pong(data)).await {
                                error!(
                                    "Connection {}: Failed to send pong: {}",
                                    connection_id, e
                                );
                            }
                        }
                        Some(Ok(WsMessage::Pong(_))) => {
                            log::debug!("Connection {}: Received pong", connection_id);
                        }
                        Some(Ok(WsMessage::Close(frame))) => {
                            warn!(
                                "Connection {}: WebSocket connection closed: {:?}",
                                connection_id, frame
                            );
                            break;
                        }
                        Some(Ok(WsMessage::Frame(_))) => {
                            warn!("Connection {}: Received raw frame - unexpected", connection_id);
                        }
                        Some(Err(e)) => {
                            error!("Connection {}: WebSocket error: {}", connection_id, e);
                            break;
                        }
                        None => {
                            warn!("Connection {}: WebSocket stream ended", connection_id);
                            break;
                        }
                    }
                }
                // Send periodic ping
                _ = ping_interval.tick() => {
                    let ping_msg = serde_json::json!({
                        "op": "ping"
                    });
                    if let Err(e) = write.send(WsMessage::Text(ping_msg.to_string().into())).await {
                        error!("Connection {}: Failed to send ping: {}", connection_id, e);
                        break;
                    }
                    log::debug!("Connection {}: Sent ping", connection_id);
                }
            }
        }

        // Connection ended, return to allow reconnection in the loop
        warn!("Connection {}: WebSocket connection ended", connection_id);
        Ok(())
    }

    fn handle_text_message(
        text: &str,
        price_cache: &Arc<DashMap<String, TokenPrice>>,
        symbol_to_contract: &Arc<DashMap<String, String>>,
        connection_id: usize,
    ) -> Result<()> {
        // Try to parse as TickerMessage
        if let Ok(ticker_msg) = serde_json::from_str::<TickerMessage>(text) {
            if ticker_msg.topic.starts_with("tickers.") {
                let symbol = ticker_msg.data.symbol.clone();

                // Check if we have contract address mapping for this symbol
                if let Some(contract_entry) = symbol_to_contract.get(&symbol) {
                    let contract_address = contract_entry.value().clone();

                    if let Ok(price) = f64::from_str(&ticker_msg.data.last_price) {
                        let token_price = TokenPrice {
                            symbol: symbol.clone(),
                            price,
                        };

                        // Store by contract address (lowercased for consistency)
                        price_cache.insert(contract_address.to_lowercase(), token_price);

                        log::debug!(
                            "Connection {}: Updated price for {} (contract: {}): ${}",
                            connection_id,
                            symbol,
                            contract_address,
                            price
                        );
                    }
                } else {
                    // No contract mapping - this means we're running without authentication
                    // Store by symbol as fallback (for testing without API keys)
                    if let Ok(price) = f64::from_str(&ticker_msg.data.last_price) {
                        let token_price = TokenPrice {
                            symbol: symbol.clone(),
                            price,
                        };

                        // Use symbol as key when no contract address available
                        price_cache.insert(symbol.to_lowercase(), token_price);
                    }
                }
            }
            return Ok(());
        }

        // Log subscription confirmations and other messages at debug level
        log::debug!(
            "Connection {}: Received message: {}",
            connection_id,
            text
        );
        Ok(())
    }
}

#[async_trait]
impl PriceProvider for BybitService {
    async fn get_price(&self, symbol: &str) -> Option<TokenPrice> {
        self.price_cache
            .get(symbol)
            .map(|entry| entry.value().clone())
    }

    async fn get_prices(&self, symbols: &Vec<String>) -> Vec<Option<TokenPrice>> {
        symbols
            .iter()
            .map(|symbol| {
                self.price_cache
                    .get(symbol)
                    .map(|entry| entry.value().clone())
            })
            .collect()
    }

    async fn get_all_prices(&self) -> Vec<TokenPrice> {
        self.price_cache
            .iter()
            .map(|entry| entry.value().clone())
            .collect()
    }

    async fn start(&self) -> Result<()> {
        // Get Token/USDT pairs
        let pairs = self.client.get_token_usdt_pairs().await?;

        if pairs.is_empty() {
            return Ok(());
        }

        // Try to get contract addresses if authenticated
        // This will build the contract address mappings
        let has_contract_mapping = match self.client.get_coin_info(None).await {
            Ok(coin_info) => {
                log::info!("Successfully fetched coin info with contract addresses");

                // Build mappings: symbol -> contract_address and contract_address -> symbol
                for coin in &coin_info.result {
                    // Find Ethereum chain (or Solana if that's the filter type)
                    for chain in &coin.chains {
                        let is_target_chain = match self.client.address_type {
                            FilterAddressType::Ethereum => chain.chain_type == "Ethereum",
                            FilterAddressType::Solana => chain.chain_type == "Solana" || chain.chain == "SOL",
                        };

                        // Only include if:
                        // 1. It's the target chain type (Ethereum or Solana)
                        // 2. Contract address is not empty
                        // 3. Contract address is valid for the chain type
                        if is_target_chain && !chain.contract_address.is_empty() {
                            // Validate the contract address format
                            if !self.client.is_valid_contract_address(&chain.contract_address) {
                                log::debug!(
                                    "Skipping invalid contract address for {}: {}",
                                    coin.coin,
                                    chain.contract_address
                                );
                                continue;
                            }

                            // Map all trading symbols for this coin to its contract
                            for pair in &pairs {
                                if pair.base_coin == coin.coin {
                                    self.symbol_to_contract.insert(
                                        pair.symbol.clone(),
                                        chain.contract_address.clone()
                                    );
                                    self.contract_to_symbol.insert(
                                        chain.contract_address.to_lowercase(),
                                        pair.symbol.clone()
                                    );

                                    log::debug!(
                                        "Mapped {} ({}) to contract address: {}",
                                        pair.symbol,
                                        coin.coin,
                                        chain.contract_address
                                    );
                                }
                            }
                            break; // Found target chain, no need to check others
                        }
                    }
                }

                let mapped_count = self.symbol_to_contract.len();
                log::info!(
                    "Successfully mapped {} trading pairs to valid {} contract addresses",
                    mapped_count,
                    match self.client.address_type {
                        FilterAddressType::Ethereum => "Ethereum",
                        FilterAddressType::Solana => "Solana",
                    }
                );

                if mapped_count == 0 {
                    log::warn!(
                        "No trading pairs found with valid {} contract addresses. Will not subscribe to any symbols.",
                        match self.client.address_type {
                            FilterAddressType::Ethereum => "Ethereum",
                            FilterAddressType::Solana => "Solana",
                        }
                    );
                }

                mapped_count > 0
            }
            Err(e) => {
                log::warn!(
                    "Could not fetch coin info (API auth required): {}. Running without contract address filtering.",
                    e
                );
                log::warn!(
                    "Price cache will use symbol names as keys instead of contract addresses."
                );
                false
            }
        };

        // Filter pairs based on whether we have contract mappings
        let symbols: Vec<String> = if has_contract_mapping {
            // WITH AUTH: Only subscribe to coins with valid contract addresses
            let filtered: Vec<String> = pairs
                .iter()
                .filter(|pair| self.symbol_to_contract.contains_key(&pair.symbol))
                .map(|pair| pair.symbol.clone())
                .collect();

            log::info!(
                "Filtered to {} symbols (out of {} total) that have valid {} contract addresses",
                filtered.len(),
                pairs.len(),
                match self.client.address_type {
                    FilterAddressType::Ethereum => "Ethereum",
                    FilterAddressType::Solana => "Solana",
                }
            );

            filtered
        } else {
            // WITHOUT AUTH: Subscribe to all pairs (no contract address filtering)
            log::warn!(
                "Running without authentication - subscribing to all {} USDT pairs without contract address filtering",
                pairs.len()
            );
            log::warn!("Prices will be cached by symbol name instead of contract address");

            pairs.iter().map(|pair| pair.symbol.clone()).collect()
        };

        if symbols.is_empty() {
            log::error!("No symbols to subscribe to after filtering. This means:");
            log::error!("  - No coins have valid {} contract addresses on Bybit",
                match self.client.address_type {
                    FilterAddressType::Ethereum => "Ethereum",
                    FilterAddressType::Solana => "Solana",
                }
            );
            log::error!("  - Or API authentication failed");
            return Ok(());
        }

        // Split symbols into chunks for multiple WebSocket connections
        // Bybit allows max 10 args per subscription, but we can have multiple connections
        const MAX_SYMBOLS_PER_CONNECTION: usize = 100; // Conservative limit
        let connection_chunks: Vec<Vec<String>> = symbols
            .chunks(MAX_SYMBOLS_PER_CONNECTION)
            .map(|chunk| chunk.to_vec())
            .collect();

        log::info!(
            "Subscribing to {} symbols across {} WebSocket connections",
            symbols.len(),
            connection_chunks.len()
        );

        // Start multiple WebSocket connections concurrently
        let mut connection_handles = Vec::new();

        for (connection_id, chunk) in connection_chunks.into_iter().enumerate() {
            let price_cache = self.price_cache.clone();
            let symbol_to_contract = self.symbol_to_contract.clone();

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

        // Start a background task to log statistics periodically
        let stats_price_cache = self.price_cache.clone();
        let stats_symbol_map = self.symbol_to_contract.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
            loop {
                interval.tick().await;

                let token_count = stats_price_cache.len();
                let symbol_count = stats_symbol_map.len();

                info!(
                    "Bybit Service Stats - Tokens with prices: {}, Contracts mapped: {}",
                    token_count, symbol_count
                );
            }
        });

        // Wait for all connections (they should run indefinitely)
        let results: Result<Vec<_>, _> = try_join_all(connection_handles).await;
        results.context("One or more WebSocket connections failed")?;

        Ok(())
    }

    fn get_price_provider_name(&self) -> &'static str {
        "Bybit"
    }
}

impl BybitService {
    /// Estimate how much USDT you'd get by selling a certain amount of tokens on Bybit
    /// Uses the orderbook to simulate market sell order
    ///
    /// `contract_address` - The Ethereum/Solana contract address (or symbol if running without auth)
    pub async fn estimate_sell_output(
        &self,
        contract_address: &str,
        token_amount: f64,
    ) -> Result<f64> {
        // Get the trading symbol for this contract address
        let symbol = self
            .contract_to_symbol
            .get(&contract_address.to_lowercase())
            .map(|entry| entry.value().clone())
            .context("Contract address not found in Bybit markets")?;

        // Fetch orderbook (bids = buy orders, we want to sell into these)
        let orderbook = self.client.get_orderbook(&symbol, 200).await?;

        let mut remaining_tokens = token_amount;
        let mut total_usdt = 0.0;

        // Iterate through bids (buy orders) from highest to lowest price
        for bid in orderbook.result.b {
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
