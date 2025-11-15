use crate::types::{DexPriceMessage, DexSubscriptionMessage, PoolPrice};
use anyhow::{anyhow, Result};
use ethers::types::Address;
use futures::sink::SinkExt;
use futures::stream::StreamExt;
use log::{info, warn};
use std::str::FromStr;
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};

/// WebSocket client for connecting to amm-eth price feed
/// Returns a receiver that yields PoolPrice updates
#[derive(Clone)]
pub struct DexWsClient {
    websocket_url: String,
}

impl DexWsClient {
    pub fn new(websocket_url: String) -> Self {
        DexWsClient { websocket_url }
    }

    /// Connect to amm-eth WebSocket and return a receiver for price updates
    pub async fn start(&self) -> Result<mpsc::Receiver<PoolPrice>> {
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
    pub async fn start_with_reconnect(&self) -> Result<mpsc::Receiver<PoolPrice>> {
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

    async fn listen_loop(url: &str, tx: mpsc::Sender<PoolPrice>) -> Result<()> {
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

        // Listen for price updates
        while let Some(msg) = read.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    // Try to parse as generic JSON first to check message type
                    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
                        // Only process price_update messages, ignore welcome/heartbeat
                        if let Some(msg_type) = value.get("type").and_then(|t| t.as_str()) {
                            if msg_type == "price_update" {
                                // Now parse as DexPriceMessage
                                match serde_json::from_str::<DexPriceMessage>(&text) {
                                    Ok(price_msg) => {
                                        // Convert to PoolPrice
                                        let pool_price = PoolPrice {
                                            token_address: Address::from_str(
                                                &price_msg.data.token_address,
                                            )
                                            .unwrap_or_else(|_| Address::zero()),
                                            price_in_eth: price_msg.data.price_in_eth,
                                            price_in_usd: Some(price_msg.data.price_in_usd),
                                            pool_address: Address::from_str(
                                                &price_msg.data.pool_address,
                                            )
                                            .unwrap_or_else(|_| Address::zero()),
                                            dex_version: price_msg.data.dex_version,
                                            decimals: price_msg.data.decimals,
                                            last_updated: price_msg.data.last_updated,
                                            liquidity_eth: None,
                                            liquidity_usd: None,
                                        };

                                        let _ = tx.send(pool_price).await;
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
                Ok(Message::Close(_)) => {
                    info!("WebSocket closed by server");
                    break;
                }
                Ok(Message::Ping(data)) => {
                    let _ = write.send(Message::Pong(data)).await;
                }
                Ok(_) => {}
                Err(e) => {
                    warn!("WebSocket error: {}", e);
                    break;
                }
            }
        }

        Err(anyhow!("WebSocket connection closed"))
    }
}
