pub mod pumpfun;

use crate::pool_data_types::PoolState;
use async_trait::async_trait;
use solana_sdk::pubkey::Pubkey;

#[async_trait]
pub trait PoolDiscovery: Send + Sync {
    /// Discover pools for a specific token
    async fn discover_for_token(&self, token: &Pubkey) -> anyhow::Result<Vec<PoolState>>;

    /// Discover top/trending pools
    async fn discover_top_pools(&self, limit: usize) -> anyhow::Result<Vec<PoolState>>;
}
