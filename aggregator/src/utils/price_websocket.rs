use axum::body::Bytes;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::{interval, timeout, Instant};
use tokio_tungstenite::{connect_async, tungstenite::Message};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinanceTickerData {
    #[serde(rename = "s")]
    pub symbol: String,
    #[serde(rename = "c")]
    pub close_price: String,
    #[serde(rename = "E")]
    pub event_time: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinanceTickerEvent {
    #[serde(rename = "e")]
    pub event_type: String,
    #[serde(flatten)]
    pub data: BinanceTickerData,
}

#[derive(Debug, Clone)]
pub struct PriceData {
    pub symbol: String,
    pub price: f64,
    pub last_updated: Instant,
}

pub struct BinancePriceService {
    current_prices: Arc<RwLock<std::collections::HashMap<String, PriceData>>>,
    reconnect_attempts: Arc<RwLock<u32>>,
    max_reconnect_attempts: u32,
    reconnect_delay: Duration,
}

impl BinancePriceService {
    pub fn new() -> Self {
        Self {
            current_prices: Arc::new(RwLock::new(std::collections::HashMap::new())),
            reconnect_attempts: Arc::new(RwLock::new(0)),
            max_reconnect_attempts: 10,
            reconnect_delay: Duration::from_secs(5),
        }
    }

    pub async fn start(&self) {
        log::info!("Starting Binance price feed service...");

        let current_prices = Arc::clone(&self.current_prices);
        let reconnect_attempts = Arc::clone(&self.reconnect_attempts);
        let max_attempts = self.max_reconnect_attempts;
        let delay = self.reconnect_delay;

        tokio::spawn(async move {
            loop {
                match Self::connect_and_subscribe(
                    Arc::clone(&current_prices),
                    Arc::clone(&reconnect_attempts),
                )
                .await
                {
                    Ok(_) => {
                        log::info!("Binance WebSocket connection ended normally");
                        // Reset reconnect attempts on successful connection
                        *reconnect_attempts.write().await = 0;
                    }
                    Err(e) => {
                        let attempts = {
                            let mut attempts = reconnect_attempts.write().await;
                            *attempts += 1;
                            *attempts
                        };

                        log::error!("Binance WebSocket error (attempt {}): {:?}", attempts, e);

                        if attempts >= max_attempts {
                            log::error!(
                                "Max reconnection attempts ({}) reached. Stopping price feed.",
                                max_attempts
                            );
                            break;
                        }

                        log::info!("Reconnecting in {:?}...", delay);
                        tokio::time::sleep(delay).await;
                    }
                }
            }
        });
    }

    async fn connect_and_subscribe(
        current_prices: Arc<RwLock<std::collections::HashMap<String, PriceData>>>,
        reconnect_attempts: Arc<RwLock<u32>>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Use string directly instead of Url::parse
        let url = "wss://stream.binance.com:9443/ws/solusdt@ticker";

        log::info!("Connecting to Binance WebSocket: {}", url);
        let (ws_stream, _) = connect_async(url).await?; // Pass string directly
        log::info!("Connected to Binance WebSocket successfully");

        let (write, mut read) = ws_stream.split();

        // Start ping task
        let write_clone = Arc::new(tokio::sync::Mutex::new(write));
        let write_for_ping = Arc::clone(&write_clone);

        tokio::spawn(async move {
            let mut ping_interval = interval(Duration::from_secs(20)); // Ping every 20 seconds
            loop {
                ping_interval.tick().await;

                let mut writer = write_for_ping.lock().await;
                if let Err(e) = writer.send(Message::Ping(Bytes::new())).await {
                    // Use vec![] instead of Bytes::new()
                    log::error!("Failed to send ping: {:?}", e);
                    break;
                }
                log::debug!("Sent ping to Binance WebSocket");
            }
        });

        // Handle incoming messages
        while let Some(msg) = read.next().await {
            match msg? {
                Message::Text(text) => {
                    if let Err(e) = Self::handle_ticker_message(&text, &current_prices).await {
                        log::error!("Error handling ticker message: {:?}", e);
                    }
                }
                Message::Pong(_) => {
                    log::debug!("Received pong from Binance WebSocket");
                }
                Message::Ping(data) => {
                    log::debug!("Received ping from Binance WebSocket, sending pong");
                    let mut writer = write_clone.lock().await;
                    writer.send(Message::Pong(data)).await?;
                }
                Message::Close(_) => {
                    log::warn!("Binance WebSocket connection closed by server");
                    break;
                }
                Message::Binary(_) => {
                    log::debug!("Received binary message (ignoring)");
                }
                Message::Frame(_) => {
                    log::debug!("Received frame message (ignoring)");
                }
            }
        }

        Ok(())
    }

    async fn handle_ticker_message(
        text: &str,
        current_prices: &Arc<RwLock<std::collections::HashMap<String, PriceData>>>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let ticker_event: BinanceTickerEvent = serde_json::from_str(text)?;

        if ticker_event.event_type == "24hrTicker" {
            let price = ticker_event.data.close_price.parse::<f64>()?;

            let price_data = PriceData {
                symbol: ticker_event.data.symbol.clone(),
                price,
                last_updated: Instant::now(),
            };

            {
                let mut prices = current_prices.write().await;
                prices.insert(ticker_event.data.symbol.clone(), price_data.clone());
            }

            log::debug!("Updated {} price: ${:.4}", ticker_event.data.symbol, price);
        }

        Ok(())
    }

    pub async fn get_sol_price(&self) -> Option<f64> {
        let prices = self.current_prices.read().await;
        prices.get("SOLUSDT").map(|data| data.price)
    }

    pub async fn get_price(&self, symbol: &str) -> Option<PriceData> {
        let prices = self.current_prices.read().await;
        prices.get(symbol).cloned()
    }

    pub async fn get_all_prices(&self) -> std::collections::HashMap<String, PriceData> {
        let prices = self.current_prices.read().await;
        prices.clone()
    }
}

impl Default for BinancePriceService {
    fn default() -> Self {
        Self::new()
    }
}
