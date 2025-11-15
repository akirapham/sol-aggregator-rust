use crate::ws_server::{broadcast_price_update, WsMessage};
use dashmap::DashMap;
use eth_dex_quote::{DexVersion, TokenPrice};
use ethers::types::Address;
use log::info;
use std::sync::Arc;
use tokio::sync::mpsc;

/// In-memory price storage using DashMap for concurrent access
#[derive(Debug, Clone)]
pub struct PriceStore {
    prices: Arc<DashMap<Address, TokenPrice>>,
    broadcaster: Option<mpsc::UnboundedSender<WsMessage>>,
}

impl PriceStore {
    /// Create a new price store
    pub fn new() -> Self {
        Self {
            prices: Arc::new(DashMap::new()),
            broadcaster: None,
        }
    }

    /// Create a price store with WebSocket broadcaster
    pub fn with_broadcaster(broadcaster: mpsc::UnboundedSender<WsMessage>) -> Self {
        Self {
            prices: Arc::new(DashMap::new()),
            broadcaster: Some(broadcaster),
        }
    }

    /// Store or update a token price
    pub fn update_price(&self, token_address: Address, price: TokenPrice) {
        // Check if this is a new price or an update with different USD price
        let should_broadcast = if let Some(old_price) = self.prices.get(&token_address) {
            // Only broadcast if USD price changed
            old_price.price_in_usd != price.price_in_usd && price.price_in_usd.is_some()
        } else {
            // New token price with USD value
            price.price_in_usd.is_some()
        };

        // Insert the new price
        self.prices.insert(token_address, price.clone());

        // Broadcast if conditions are met
        if should_broadcast {
            if let Some(ref broadcaster) = self.broadcaster {
                broadcast_price_update(broadcaster, token_address, price);
            }
        }
    }

    /// Get the price of a token
    pub fn get_price(&self, token_address: &Address) -> Option<TokenPrice> {
        self.prices
            .get(token_address)
            .map(|entry| entry.value().clone())
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
        tokens.iter().map(|token| self.get_price(token)).collect()
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
                DexVersion::UniswapV2 => v2_count += 1,
                DexVersion::UniswapV3 => v3_count += 1,
                DexVersion::UniswapV4 => v4_count += 1,
                DexVersion::SushiswapV2 => todo!(),
                DexVersion::SushiswapV3 => todo!(),
                DexVersion::PancakeswapV2 => todo!(),
                DexVersion::PancakeswapV3 => todo!(),
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
