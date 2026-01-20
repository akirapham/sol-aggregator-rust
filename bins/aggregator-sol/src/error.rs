use thiserror::Error;

#[derive(Error, Debug)]
pub enum DexAggregatorError {
    #[error("RPC error: {0}")]
    RpcError(String),
    #[error("Database error: {0}")]
    DatabaseError(String),
    #[error("Config error: {0}")]
    ConfigError(String),
    #[error("Internal error: {0}")]
    InternalError(String),
    #[error("Not found: {0}")]
    NotFound(String),
    #[error("Serialization error: {0}")]
    SerializationError(String),
    #[error("Invalid token address: {0}")]
    InvalidTokenAddress(String),
}

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
