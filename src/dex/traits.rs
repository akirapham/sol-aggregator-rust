use async_trait::async_trait;
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;

use crate::error::Result;
use crate::pool_data_types::DexType;
use crate::pool_manager::PoolStateManager;
use crate::types::{PriceInfo, SwapParams, SwapRoute, Token};
use crate::DexAggregatorError;

/// Common trait for all DEX implementations
#[async_trait]
pub trait DexInterface {
    async fn get_quote(&self, params: &SwapParams) -> Result<Option<SwapRoute>>;
}

#[async_trait]
pub trait TokenProviderInterface {
    async fn get_token_info(&self, mint: &Pubkey) -> Result<Option<Token>>;
}
