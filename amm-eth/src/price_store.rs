use crate::types::TokenPrice;
use dashmap::DashMap;
use ethers::types::Address;
use log::{info, debug};
use std::sync::Arc;

/// In-memory price storage using DashMap for concurrent access
#[derive(Debug, Clone)]
pub struct PriceStore {
    prices: Arc<DashMap<Address, TokenPrice>>,
}

impl PriceStore {
    /// Create a new price store
    pub fn new() -> Self {
        Self {
            prices: Arc::new(DashMap::new()),
        }
    }

    /// Store or update a token price
    pub fn update_price(&self, token_address: Address, price: TokenPrice) {
        debug!(
            "Updating price for token {:?}: {} ETH ({})",
            token_address, price.price_in_eth, price.dex_version
        );
        self.prices.insert(token_address, price);
    }

    /// Get the price of a token
    pub fn get_price(&self, token_address: &Address) -> Option<TokenPrice> {
        self.prices.get(token_address).map(|entry| entry.value().clone())
    }

    /// Get all stored prices
    pub fn get_all_prices(&self) -> Vec<TokenPrice> {
        self.prices
            .iter()
            .map(|entry| entry.value().clone())
            .collect()
    }

    /// Get the number of tokens tracked
    pub fn len(&self) -> usize {
        self.prices.len()
    }

    /// Check if the store is empty
    pub fn is_empty(&self) -> bool {
        self.prices.is_empty()
    }

    /// Clear all prices
    pub fn clear(&self) {
        self.prices.clear();
    }

    /// Get prices for multiple tokens
    pub fn get_prices(&self, tokens: &[Address]) -> Vec<Option<TokenPrice>> {
        tokens
            .iter()
            .map(|token| self.get_price(token))
            .collect()
    }

    /// Log statistics about stored prices
    pub fn log_stats(&self) {
        info!("Price store statistics:");
        info!("  Total tokens tracked: {}", self.len());

        let mut v2_count = 0;
        let mut v3_count = 0;
        let mut v4_count = 0;

        for entry in self.prices.iter() {
            match entry.value().dex_version {
                crate::types::DexVersion::UniswapV2 => v2_count += 1,
                crate::types::DexVersion::UniswapV3 => v3_count += 1,
                crate::types::DexVersion::UniswapV4 => v4_count += 1,
            }
        }

        info!("  Uniswap V2 prices: {}", v2_count);
        info!("  Uniswap V3 prices: {}", v3_count);
        info!("  Uniswap V4 prices: {}", v4_count);
    }
}

impl Default for PriceStore {
    fn default() -> Self {
        Self::new()
    }
}
