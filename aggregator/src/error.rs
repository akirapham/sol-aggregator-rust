use thiserror::Error;

#[derive(Error, Debug)]
pub enum DexAggregatorError {
    #[error("DEX operation failed: {0}")]
    DexError(String),

    #[error("Invalid token address: {0}")]
    InvalidTokenAddress(String),

    #[error("Insufficient liquidity for swap")]
    InsufficientLiquidity,

    #[error("Price calculation failed: {0}")]
    PriceCalculationError(String),

    #[error("Route not found for token pair")]
    RouteNotFound,

    #[error("Pool not found: {0}")]
    PoolNotFound(String),

    #[error("Token not found: {0}")]
    TokenNotFound(String),

    #[error("Configuration error: {0}")]
    ConfigurationError(String),

    #[error("Parse error: {0}")]
    ParseError(String),

    #[error("RPC error: {0}")]
    RpcError(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Network error: {0}")]
    NetworkError(#[from] reqwest::Error),

    #[error("Solana client error: {0}")]
    SolanaError(#[from] solana_client::client_error::ClientError),

    #[error("Anchor error: {0}")]
    AnchorError(#[from] anchor_client::anchor_lang::error::Error),
}

pub type Result<T> = std::result::Result<T, DexAggregatorError>;
