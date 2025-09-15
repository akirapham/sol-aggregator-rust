use async_trait::async_trait;
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;

use crate::error::Result;
use crate::pool_manager::PoolStateManager;
use crate::types::{PriceInfo, SwapParams, SwapRoute, Token};
use crate::{DexAggregatorError, PoolState};

/// Common trait for all DEX implementations
#[async_trait]
pub trait DexInterface {
    /// Get the DEX type
    fn get_dex_type(&self) -> crate::types::DexType;

    /// Get available pools for a token pair
    async fn get_pools(&self, token_a: &Pubkey, token_b: &Pubkey) -> Result<Vec<PoolState>>;

    /// Get the best route for a swap
    async fn get_best_route(&self, params: &SwapParams) -> Result<Option<SwapRoute>>;

    /// Get price information for a token pair
    async fn get_price(
        &self,
        input_token: &Pubkey,
        output_token: &Pubkey,
        amount: u64,
    ) -> Result<PriceInfo>;

    /// Get token information
    async fn get_token_info(&self, token_address: &Pubkey) -> Result<Token>;

    /// Check if the DEX supports a token pair
    async fn supports_token_pair(&self, token_a: &Pubkey, token_b: &Pubkey) -> Result<bool>;

    /// Get all supported tokens
    async fn get_supported_tokens(&self) -> Result<Vec<Token>>;

    /// Estimate gas/fee for a swap
    async fn estimate_swap_fee(&self, params: &SwapParams) -> Result<u64>;

    /// Check if a pool exists for the given tokens
    async fn pool_exists(&self, token_a: &Pubkey, token_b: &Pubkey) -> Result<bool>;

    fn set_pool_manager(&mut self, pool_manager: Arc<PoolStateManager>);

    /// Parse pool account data specific to this DEX
    fn get_pool_state(&self, pool_address: &Pubkey) -> Result<PoolState>;

    /// Get program IDs that this DEX uses
    fn get_program_ids(&self) -> Vec<Pubkey>;

    /// Get account filters for fetching pools
    fn get_pool_filters(&self) -> Vec<solana_client::rpc_filter::RpcFilterType>;

    /// Handle real-time account updates
    async fn handle_account_update(&self, pool_address: &Pubkey, account_data: &[u8])
        -> Result<()>;
}
