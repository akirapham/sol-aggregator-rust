use crate::pool_data_types::PoolState;
use crate::types::Token;
use anyhow::Result;
use async_trait::async_trait;
use solana_sdk::pubkey::Pubkey;

// Re-export traits from their proper crates
pub use crate::grpc::GrpcServiceTrait;
pub use binance_price_stream::PriceServiceTrait;

/// Trait for database operations to allow mocking in tests
#[async_trait]
pub trait DatabaseTrait: Send + Sync {
    /// Load pools from database
    async fn load_pools(&self) -> Result<Vec<PoolState>>;

    /// Save pools to database
    async fn save_pools(&self, pools: &[PoolState]) -> Result<()>;

    /// Load tokens from database
    async fn load_tokens(&self) -> Result<Vec<Token>>;

    /// Save tokens to database
    async fn save_tokens(&self, tokens: &[Token]) -> Result<()>;

    /// Load arbitrage tokens from database
    async fn load_arbitrage_tokens(&self) -> Result<Vec<Pubkey>>;

    /// Save arbitrage tokens to database
    async fn save_arbitrage_tokens(&self, tokens: &[Pubkey]) -> Result<()>;

    /// Add arbitrage token to database
    async fn add_arbitrage_token(&self, token: &Pubkey) -> Result<()>;

    /// Remove arbitrage token from database
    async fn remove_arbitrage_token(&self, token: &Pubkey) -> Result<()>;
}
