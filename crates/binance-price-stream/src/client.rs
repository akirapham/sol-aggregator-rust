use crate::error::{BinanceError, Result};
use crate::types::*;
use dashmap::DashMap;
use futures_util::{SinkExt, StreamExt};
use log::{debug, error, info, warn};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};

/// Binance WebSocket client for real-time price streaming
#[derive(Clone)]
pub struct BinancePriceStream {
    config: BinanceConfig,
    price_cache: Arc<DashMap<String, PriceUpdate>>,
    symbols: Vec<String>,
}

impl BinancePriceStream {
    /// Create a new Binance price stream client
    pub fn new(config: BinanceConfig, symbols: Vec<String>) -> Self {
        Self {
            config,
            price_cache: Arc::new(DashMap::new()),
            symbols,
        }
    }

    /// Start the WebSocket client with automatic reconnection
    pub async fn start(&self) -> Result<mpsc::UnboundedReceiver<PriceUpdate>> {
        let (tx, rx) = mpsc::unbounded_channel();

        let config = self.config.clone();
        let symbols = self.symbols.clone();
        let price_cache = self.price_cache.clone();

        tokio::spawn(async move {
            loop {
                info!("Connecting to Binance WebSocket...");

                match Self::connect_and_stream(&config, &symbols, &price_cache, &tx).await {
                    Ok(_) => {
                        info!("WebSocket connection ended normally");
                    }
                    Err(e) => {
                        error!("WebSocket error: {}", e);
                    }
                }

                warn!("Reconnecting in {} seconds...", config.reconnect_delay_secs);
                tokio::time::sleep(Duration::from_secs(config.reconnect_delay_secs)).await;
            }
        });

        Ok(rx)
    }

    /// Connect to WebSocket and handle streaming
    async fn connect_and_stream(
        config: &BinanceConfig,
        symbols: &[String],
        price_cache: &Arc<DashMap<String, PriceUpdate>>,
        tx: &mpsc::UnboundedSender<PriceUpdate>,
    ) -> Result<()> {
        info!("Connecting to {}", config.websocket_url);

        let (ws_stream, _) = connect_async(&config.websocket_url)
            .await
            .map_err(|e| BinanceError::ConnectionError(e.to_string()))?;

        info!("Connected to Binance WebSocket");

        let (mut write, mut read) = ws_stream.split();

        // Subscribe to symbols
        let subscribe_msg = SubscribeMessage::new(symbols.to_vec(), &config.stream_type);
        let subscribe_json = serde_json::to_string(&subscribe_msg)?;

        info!("Subscribing to symbols: {:?}", symbols);
        write
            .send(Message::Text(subscribe_json.into()))
            .await
            .map_err(|e| BinanceError::ConnectionError(e.to_string()))?;

        // Create ping interval
        let mut ping_interval =
            tokio::time::interval(Duration::from_secs(config.ping_interval_secs));
        ping_interval.tick().await; // Skip first immediate tick

        // Main message loop
        loop {
            tokio::select! {
                // Handle incoming messages
                msg = read.next() => {
                    match msg {
                        Some(Ok(Message::Text(text))) => {
                            if let Err(e) = Self::handle_message(
                                &text,
                                &config.stream_type,
                                price_cache,
                                tx,
                            ) {
                                error!("Error handling message: {}", e);
                            }
                        }
                        Some(Ok(Message::Ping(data))) => {
                            debug!("Received ping, sending pong");
                            if let Err(e) = write.send(Message::Pong(data)).await {
                                error!("Failed to send pong: {}", e);
                                break;
                            }
                        }
                        Some(Ok(Message::Pong(_))) => {
                            debug!("Received pong");
                        }
                        Some(Ok(Message::Close(_))) => {
                            warn!("WebSocket closed by server");
                            break;
                        }
                        Some(Err(e)) => {
                            error!("WebSocket error: {}", e);
                            break;
                        }
                        None => {
                            warn!("WebSocket stream ended");
                            break;
                        }
                        _ => {}
                    }
                }
                // Send periodic ping
                _ = ping_interval.tick() => {
                    debug!("Sending ping");
                    if let Err(e) = write.send(Message::Ping(vec![].into())).await {
                        error!("Failed to send ping: {}", e);
                        break;
                    }
                }
            }
        }

        Ok(())
    }

    /// Handle incoming WebSocket message
    fn handle_message(
        text: &str,
        stream_type: &StreamType,
        price_cache: &Arc<DashMap<String, PriceUpdate>>,
        tx: &mpsc::UnboundedSender<PriceUpdate>,
    ) -> Result<()> {
        // Check if it's a subscription response
        if text.contains("\"result\":null") || text.contains("\"method\":\"SUBSCRIBE\"") {
            debug!("Subscription confirmed");
            return Ok(());
        }

        let price_update = match stream_type {
            StreamType::Trade => {
                let trade: TradeMessage = serde_json::from_str(text)?;
                PriceUpdate {
                    symbol: trade.symbol.clone(),
                    price: trade.price.parse().unwrap_or(0.0),
                    timestamp: trade.event_time,
                }
            }
            StreamType::Ticker => {
                let ticker: TickerMessage = serde_json::from_str(text)?;
                PriceUpdate {
                    symbol: ticker.symbol.clone(),
                    price: ticker.current_price.parse().unwrap_or(0.0),
                    timestamp: ticker.event_time,
                }
            }
            StreamType::MiniTicker => {
                let mini_ticker: MiniTickerMessage = serde_json::from_str(text)?;
                PriceUpdate {
                    symbol: mini_ticker.symbol.clone(),
                    price: mini_ticker.close_price.parse().unwrap_or(0.0),
                    timestamp: mini_ticker.event_time,
                }
            }
            StreamType::BookTicker => {
                let book_ticker: BookTickerMessage = serde_json::from_str(text)?;
                // Use mid-price (average of best bid and ask)
                let bid = book_ticker.best_bid_price.parse().unwrap_or(0.0);
                let ask = book_ticker.best_ask_price.parse().unwrap_or(0.0);
                let mid_price = (bid + ask) / 2.0;

                PriceUpdate {
                    symbol: book_ticker.symbol.clone(),
                    price: mid_price,
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_millis() as u64,
                }
            }
        };

        debug!(
            "Price update: {} = ${:.2}",
            price_update.symbol, price_update.price
        );

        // Update cache
        price_cache.insert(price_update.symbol.clone(), price_update.clone());

        // Send to channel
        if let Err(e) = tx.send(price_update) {
            error!("Failed to send price update: {}", e);
        }

        Ok(())
    }

    /// Get the current price for a symbol from cache
    pub fn get_price(&self, symbol: &str) -> Option<PriceUpdate> {
        self.price_cache
            .get(&symbol.to_uppercase())
            .map(|entry| entry.value().clone())
    }

    /// Get all cached prices
    pub fn get_all_prices(&self) -> Vec<PriceUpdate> {
        self.price_cache
            .iter()
            .map(|entry| entry.value().clone())
            .collect()
    }

    /// Get the number of symbols being tracked
    pub fn symbols_count(&self) -> usize {
        self.price_cache.len()
    }
}
