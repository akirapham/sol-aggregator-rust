use async_trait::async_trait;
use solana_sdk::pubkey::Pubkey;

use crate::error::Result;
use crate::types::{SwapParams, SwapRoute, Token};

/// Common trait for all DEX implementations
#[async_trait]
pub trait DexInterface {
    async fn get_quote(&self, params: &SwapParams) -> Result<Option<SwapRoute>>;
}

#[async_trait]
pub trait TokenProviderInterface {
    async fn get_token_info(&self, mint: &Pubkey) -> Result<Option<Token>>;
}
