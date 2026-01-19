use crate::types::{DexPriceMessage, DexSubscriptionMessage};
use anyhow::{anyhow, Result};
use bytes::Bytes;
use eth_dex_quote::TokenPriceUpdate;
use futures::sink::SinkExt;
use futures::stream::StreamExt;
use log::{info, warn};
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};

/// WebSocket client for connecting to amm-eth price feed
/// Returns a receiver that yields TokenPriceUpdate messages
#[derive(Clone)]
pub struct DexWsClient {
    websocket_url: String,
}

impl DexWsClient {
    pub fn new(websocket_url: String) -> Self {
        DexWsClient { websocket_url }
    }

    /// Connect to amm-eth WebSocket and return a receiver for price updates
    pub async fn start(&self) -> Result<mpsc::Receiver<TokenPriceUpdate>> {
        let (tx, rx) = mpsc::channel(1000);

        let url = self.websocket_url.clone();

        tokio::spawn(async move {
            if let Err(e) = Self::listen_loop(&url, tx).await {
                warn!("WebSocket listener error: {}", e);
            }
        });

        Ok(rx)
    }

    /// Listen to WebSocket with auto-reconnect
    pub async fn start_with_reconnect(&self) -> Result<mpsc::Receiver<TokenPriceUpdate>> {
        let (tx, rx) = mpsc::channel(1000);

        let url = self.websocket_url.clone();

        tokio::spawn(async move {
            let mut backoff = 1;
            const MAX_BACKOFF: u64 = 60;

            loop {
                match Self::listen_loop(&url, tx.clone()).await {
                    Ok(_) => {
                        info!("WebSocket connection ended gracefully");
                        return;
                    }
                    Err(e) => {
                        warn!("WebSocket error: {}. Reconnecting in {}s...", e, backoff);
                        tokio::time::sleep(tokio::time::Duration::from_secs(backoff)).await;
                        backoff = (backoff * 2).min(MAX_BACKOFF);
                    }
                }
            }
        });

        Ok(rx)
    }

    async fn listen_loop(url: &str, tx: mpsc::Sender<TokenPriceUpdate>) -> Result<()> {
        info!("Connecting to DEX WebSocket: {}", url);

        let (ws_stream, _) = connect_async(url)
            .await
            .map_err(|e| anyhow!("Failed to connect to WebSocket: {}", e))?;

        info!("✅ Connected to DEX WebSocket");

        let (mut write, mut read) = ws_stream.split();

        // Subscribe to all price updates
        let subscription = DexSubscriptionMessage {
            topics: "prices".to_string(),
        };
        let subscription_msg = serde_json::to_string(&subscription)
            .map_err(|e| anyhow!("Failed to serialize subscription: {}", e))?;

        write
            .send(Message::Text(subscription_msg.into()))
            .await
            .map_err(|e| anyhow!("Failed to send subscription: {}", e))?;

        info!("📡 Subscribed to price updates");

        // Use a timeout-based select! to send ping heartbeat every 15 seconds
        let mut heartbeat_interval = tokio::time::interval(tokio::time::Duration::from_secs(15));

        // Listen for price updates
        loop {
            tokio::select! {
                // Send ping heartbeat every 15 seconds
                _ = heartbeat_interval.tick() => {
                    if let Err(e) = write.send(Message::Ping(Bytes::new())).await {
                        warn!("Failed to send ping heartbeat: {}", e);
                        break;
                    }
                    log::debug!("Sent ping heartbeat");
                }

                // Receive messages from websocket
                msg = read.next() => {
                    match msg {
                        Some(Ok(Message::Text(text))) => {
                            // Try to parse as generic JSON first to check message type
                            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
                                // Only process price_update messages, ignore welcome/heartbeat
                                if let Some(msg_type) = value.get("type").and_then(|t| t.as_str()) {
                                    if msg_type == "price_update" {
                                        // Now parse as DexPriceMessage
                                        match serde_json::from_str::<DexPriceMessage>(&text) {
                                            Ok(price_msg) => {
                                                log::debug!("Parsed price_update: {:?}", price_msg.data);

                                                // Use try_send so the websocket loop doesn't await if receiver is slow.
                                                // If the channel is full or closed, log a warning and continue.
                                                match tx.try_send(price_msg.data) {
                                                    Ok(_) => {}
                                                    Err(e) => {
                                                        warn!(
                                                            "Failed to forward price_update to channel: {}. Channel may be full or closed",
                                                            e
                                                        );
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                warn!(
                                                    "Failed to parse price_update message: {} - Raw: {}",
                                                    e, text
                                                );
                                            }
                                        }
                                    }
                                    // Silently ignore other message types (welcome, heartbeat, etc.)
                                }
                            }
                        }
                        Some(Ok(Message::Close(_))) => {
                            info!("WebSocket closed by server");
                            break;
                        }
                        Some(Ok(Message::Ping(data))) => {
                            let _ = write.send(Message::Pong(data)).await;
                        }
                        Some(Ok(_)) => {}
                        Some(Err(e)) => {
                            warn!("WebSocket error: {}", e);
                            break;
                        }
                        None => {
                            info!("WebSocket stream ended");
                            break;
                        }
                    }
                }
            }
        }

        info!("WebSocket listener loop ended");
        Err(anyhow!("WebSocket connection closed"))
    }
}
