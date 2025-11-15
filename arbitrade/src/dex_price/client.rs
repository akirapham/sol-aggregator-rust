use crate::types::{DexPriceMessage, DexSubscriptionMessage, TokenPriceUpdate};
use anyhow::{Context, Result};
use axum::body::Bytes;
use futures_util::{SinkExt, StreamExt};
use log::{error, info, warn};
use serde_json;
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message as WsMessage};

/// Configuration for DEX price WebSocket client
#[derive(Debug, Clone)]
pub struct DexPriceConfig {
    pub websocket_url: String,
    pub subscription_topic: String,
    pub reconnect_delay_secs: u64,
    pub ping_interval_secs: u64,
    pub batch_size: usize,
    pub batch_timeout_ms: u64,
}

impl DexPriceConfig {
    /// Create a new config from environment variables
    pub fn from_env() -> Self {
        Self::default()
    }
}

impl Default for DexPriceConfig {
    fn default() -> Self {
        Self {
            websocket_url: std::env::var("DEX_PRICE_STREAM")
                .unwrap_or_else(|_| "ws://localhost:8080/ws".to_string()),
            subscription_topic: std::env::var("DEX_SUBSCRIPTION_TOPIC")
                .unwrap_or_else(|_| "token_price".to_string()),
            reconnect_delay_secs: std::env::var("DEX_RECONNECT_DELAY_SECS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(5),
            ping_interval_secs: std::env::var("DEX_PING_INTERVAL_SECS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(30),
            batch_size: std::env::var("DEX_BATCH_SIZE")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(100),
            batch_timeout_ms: std::env::var("DEX_BATCH_TIMEOUT_MS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(1000),
        }
    }
}

/// DEX price WebSocket client
pub struct DexPriceClient {
    config: DexPriceConfig,
    price_sender: mpsc::UnboundedSender<Vec<TokenPriceUpdate>>,
}

impl DexPriceClient {
    /// Create a new DEX price client
    pub fn new(config: DexPriceConfig) -> (Self, mpsc::UnboundedReceiver<Vec<TokenPriceUpdate>>) {
        let (price_sender, price_receiver) = mpsc::unbounded_channel();

        let client = Self {
            config,
            price_sender,
        };

        (client, price_receiver)
    }

    /// Start the WebSocket client with automatic reconnection
    pub async fn start(&self) -> Result<()> {
        info!(
            "Starting DEX price WebSocket client for: {}",
            self.config.websocket_url
        );

        loop {
            if let Err(e) = self.connect_and_stream().await {
                error!("WebSocket connection failed: {}", e);
                info!(
                    "Reconnecting in {} seconds...",
                    self.config.reconnect_delay_secs
                );
                tokio::time::sleep(Duration::from_secs(self.config.reconnect_delay_secs)).await;
                continue;
            }

            info!(
                "WebSocket connection ended, reconnecting in {} seconds...",
                self.config.reconnect_delay_secs
            );
            tokio::time::sleep(Duration::from_secs(self.config.reconnect_delay_secs)).await;
        }
    }

    /// Connect to WebSocket and handle streaming
    async fn connect_and_stream(&self) -> Result<()> {
        info!(
            "Connecting to DEX price WebSocket: {}",
            self.config.websocket_url
        );

        let (ws_stream, response) = connect_async(&self.config.websocket_url)
            .await
            .context("Failed to connect to DEX price WebSocket")?;

        info!("WebSocket connected. Response: {:?}", response.status());

        let (mut write, mut read) = ws_stream.split();

        // Send subscription message
        let subscription_msg = DexSubscriptionMessage {
            topics: self.config.subscription_topic.clone(),
        };

        let subscription_json = serde_json::to_string(&subscription_msg)
            .context("Failed to serialize subscription message")?;

        info!("Sending subscription message: {}", subscription_json);

        write
            .send(WsMessage::Text(subscription_json.into()))
            .await
            .context("Failed to send subscription message")?;

        // Create ping interval timer
        let mut ping_interval =
            tokio::time::interval(Duration::from_secs(self.config.ping_interval_secs));
        ping_interval.tick().await; // Skip first immediate tick

        // Create batching mechanism
        let mut batch_buffer = Vec::with_capacity(self.config.batch_size);
        let mut batch_timer =
            tokio::time::interval(Duration::from_millis(self.config.batch_timeout_ms));
        batch_timer.tick().await; // Skip first immediate tick

        // Main message loop
        loop {
            tokio::select! {
                // Handle incoming WebSocket messages
                msg = read.next() => {
                    match msg {
                        Some(Ok(WsMessage::Text(text))) => {
                            if let Err(e) = self.handle_text_message(&text, &mut batch_buffer).await {
                                error!("Error handling text message: {}", e);
                            }

                            // Check if batch is full
                            if batch_buffer.len() >= self.config.batch_size {
                                self.send_batch(&mut batch_buffer).await;
                            }
                        }
                        Some(Ok(WsMessage::Binary(_))) => {
                            warn!("Received unexpected binary message");
                        }
                        Some(Ok(WsMessage::Ping(data))) => {
                            info!("Received ping, sending pong");
                            if let Err(e) = write.send(WsMessage::Pong(data)).await {
                                error!("Failed to send pong: {}", e);
                            }
                        }
                        Some(Ok(WsMessage::Pong(_))) => {
                            log::debug!("Received pong");
                        }
                        Some(Ok(WsMessage::Close(frame))) => {
                            warn!("WebSocket connection closed: {:?}", frame);
                            break;
                        }
                        Some(Ok(WsMessage::Frame(_))) => {
                            warn!("Received raw frame - unexpected");
                        }
                        Some(Err(e)) => {
                            error!("WebSocket error: {}", e);
                            break;
                        }
                        None => {
                            warn!("WebSocket stream ended");
                            break;
                        }
                    }
                }
                // Send periodic ping
                _ = ping_interval.tick() => {
                    log::debug!("Sending ping to keep connection alive");
                    if let Err(e) = write.send(WsMessage::Ping(Bytes::new())).await {
                        error!("Failed to send ping: {}", e);
                        break;
                    }
                }
                // Send batch on timeout
                _ = batch_timer.tick() => {
                    if !batch_buffer.is_empty() {
                        self.send_batch(&mut batch_buffer).await;
                    }
                }
            }
        }

        // Send any remaining items in the batch before disconnecting
        if !batch_buffer.is_empty() {
            self.send_batch(&mut batch_buffer).await;
        }

        warn!("WebSocket connection ended");
        Ok(())
    }

    /// Handle incoming text message and parse token price updates
    async fn handle_text_message(
        &self,
        text: &str,
        batch_buffer: &mut Vec<TokenPriceUpdate>,
    ) -> Result<()> {
        // Try to parse as DexPriceMessage (the actual structure from the stream)
        if let Ok(dex_message) = serde_json::from_str::<DexPriceMessage>(text) {
            if dex_message.message_type == "data" {
                // Filter out updates with less than 20 SOL reserve
                let filtered_updates: Vec<TokenPriceUpdate> = dex_message
                    .payload
                    .data
                    .into_iter()
                    .filter(|update| self.passes_filters(update))
                    .collect();

                batch_buffer.extend(filtered_updates);
                return Ok(());
            }
        }

        // Try to parse as single TokenPriceUpdate (fallback for compatibility)
        if let Ok(price_update) = serde_json::from_str::<TokenPriceUpdate>(text) {
            if self.passes_filters(&price_update) {
                log::debug!("Received price update for token: {}", price_update.token);
                batch_buffer.push(price_update);
            } else {
                log::debug!(
                    "Filtered out price update for token {} due to filters",
                    price_update.token
                );
            }
            return Ok(());
        }

        // Try to parse as array of TokenPriceUpdate (fallback for compatibility)
        if let Ok(price_updates) = serde_json::from_str::<Vec<TokenPriceUpdate>>(text) {
            let filtered_updates: Vec<TokenPriceUpdate> = price_updates
                .into_iter()
                .filter(|update| self.passes_filters(update))
                .collect();

            batch_buffer.extend(filtered_updates);
            return Ok(());
        }

        // Log as debug since it might be subscription confirmation or other message
        log::info!("Received non-price message: {}", text);
        Ok(())
    }

    /// Send batched price updates to the channel
    async fn send_batch(&self, batch_buffer: &mut Vec<TokenPriceUpdate>) {
        if batch_buffer.is_empty() {
            return;
        }

        // Deduplicate prices by (token, pair_address) - keep the most recent price for each unique pair
        let mut deduped_updates = HashMap::new();
        for update in batch_buffer.drain(..) {
            let key = (update.token.clone(), update.pair_address.clone());
            deduped_updates.insert(key, update);
        }

        let batch: Vec<TokenPriceUpdate> = deduped_updates
            .into_iter()
            .map(|(_, update)| update)
            .collect();
        let batch_size = batch.len();

        if let Err(e) = self.price_sender.send(batch) {
            error!("Failed to send price batch to channel: {}", e);
        } else {
            log::debug!(
                "Sent deduplicated batch of {} price updates to channel",
                batch_size
            );
        }
    }

    /// Check if the price update passes all filters (minimum SOL reserve and DEX exclusions)
    fn passes_filters(&self, update: &TokenPriceUpdate) -> bool {
        // Filter out PumpFun DEX
        if update.dex_program_id == "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA" {
            return false;
        }

        // Check minimum SOL reserve (>= 20 SOL)
        match update.sol_reserve.parse::<f64>() {
            Ok(reserve) => reserve >= 20.0,
            Err(_) => {
                warn!(
                    "Failed to parse SOL reserve '{}' for token {}",
                    update.sol_reserve, update.token
                );
                false
            }
        }
    }
}
