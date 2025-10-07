use serde::{Deserialize, Serialize};
use std::fmt;

/// Price update from Binance WebSocket
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceUpdate {
    /// Trading symbol (e.g., "ETHUSDT", "BTCUSDT")
    pub symbol: String,
    /// Current price
    pub price: f64,
    /// Timestamp in milliseconds
    pub timestamp: u64,
}

/// Binance WebSocket stream types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StreamType {
    /// Trade stream - real-time trades
    Trade,
    /// Ticker stream - 24hr ticker statistics
    Ticker,
    /// Mini ticker stream - simplified ticker
    MiniTicker,
    /// Book ticker stream - best bid/ask prices
    BookTicker,
}

impl fmt::Display for StreamType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StreamType::Trade => write!(f, "trade"),
            StreamType::Ticker => write!(f, "ticker"),
            StreamType::MiniTicker => write!(f, "miniTicker"),
            StreamType::BookTicker => write!(f, "bookTicker"),
        }
    }
}

/// Configuration for Binance WebSocket client
#[derive(Debug, Clone)]
pub struct BinanceConfig {
    /// WebSocket URL (default: wss://stream.binance.com:9443/ws)
    pub websocket_url: String,
    /// Stream type to subscribe to
    pub stream_type: StreamType,
    /// Reconnection delay in seconds
    pub reconnect_delay_secs: u64,
    /// Ping interval in seconds
    pub ping_interval_secs: u64,
}

impl Default for BinanceConfig {
    fn default() -> Self {
        Self {
            websocket_url: "wss://stream.binance.com:9443/ws".to_string(),
            stream_type: StreamType::BookTicker,
            reconnect_delay_secs: 5,
            ping_interval_secs: 30,
        }
    }
}

impl BinanceConfig {
    /// Create config for specific stream type
    pub fn with_stream_type(stream_type: StreamType) -> Self {
        Self {
            stream_type,
            ..Default::default()
        }
    }
}

/// Binance trade message
#[derive(Debug, Clone, Deserialize)]
pub struct TradeMessage {
    #[serde(rename = "e")]
    pub event_type: String,
    #[serde(rename = "E")]
    pub event_time: u64,
    #[serde(rename = "s")]
    pub symbol: String,
    #[serde(rename = "p")]
    pub price: String,
    #[serde(rename = "q")]
    pub quantity: String,
}

/// Binance ticker message
#[derive(Debug, Clone, Deserialize)]
pub struct TickerMessage {
    #[serde(rename = "e")]
    pub event_type: String,
    #[serde(rename = "E")]
    pub event_time: u64,
    #[serde(rename = "s")]
    pub symbol: String,
    #[serde(rename = "c")]
    pub current_price: String,
}

/// Binance mini ticker message
#[derive(Debug, Clone, Deserialize)]
pub struct MiniTickerMessage {
    #[serde(rename = "e")]
    pub event_type: String,
    #[serde(rename = "E")]
    pub event_time: u64,
    #[serde(rename = "s")]
    pub symbol: String,
    #[serde(rename = "c")]
    pub close_price: String,
}

/// Binance book ticker message (best bid/ask)
#[derive(Debug, Clone, Deserialize)]
pub struct BookTickerMessage {
    #[serde(rename = "u")]
    pub update_id: u64,
    #[serde(rename = "s")]
    pub symbol: String,
    #[serde(rename = "b")]
    pub best_bid_price: String,
    #[serde(rename = "B")]
    pub best_bid_qty: String,
    #[serde(rename = "a")]
    pub best_ask_price: String,
    #[serde(rename = "A")]
    pub best_ask_qty: String,
}

/// Subscription request message
#[derive(Debug, Clone, Serialize)]
pub struct SubscribeMessage {
    pub method: String,
    pub params: Vec<String>,
    pub id: u64,
}

impl SubscribeMessage {
    /// Create a subscribe message for given symbols and stream type
    pub fn new(symbols: Vec<String>, stream_type: &StreamType) -> Self {
        let params = symbols
            .iter()
            .map(|symbol| format!("{}@{}", symbol.to_lowercase(), stream_type))
            .collect();

        Self {
            method: "SUBSCRIBE".to_string(),
            params,
            id: 1,
        }
    }
}
