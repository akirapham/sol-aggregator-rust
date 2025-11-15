use thiserror::Error;

#[derive(Error, Debug)]
pub enum BinanceError {
    #[error("WebSocket connection error: {0}")]
    ConnectionError(String),

    #[error("WebSocket error: {0}")]
    WebSocketError(#[from] tokio_tungstenite::tungstenite::Error),

    #[error("JSON parsing error: {0}")]
    JsonError(#[from] serde_json::Error),

    #[error("Invalid symbol: {0}")]
    InvalidSymbol(String),

    #[error("Subscription error: {0}")]
    SubscriptionError(String),

    #[error("Stream closed")]
    StreamClosed,

    #[error("Other error: {0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, BinanceError>;
