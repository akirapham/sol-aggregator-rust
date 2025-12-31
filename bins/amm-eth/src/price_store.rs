use crate::ws_server::{broadcast_price_update, WsMessage};
use dashmap::DashMap;
use eth_dex_quote::TokenPrice;
use ethers::types::Address;
use log::{debug, info};
use std::sync::Arc;
use tokio::sync::mpsc;

/// In-memory price storage using DashMap for concurrent access
/// Structure: token_address -> pool_address_string -> TokenPrice
/// This allows storing multiple prices for the same token from different pools
/// Pool addresses are stored as strings to support both Address (0x...) and bytes32 (V4 pools)
#[derive(Debug, Clone)]
pub struct PriceStore {
    /// prices[token_address][pool_address_string] = TokenPrice
    /// Inner key is String to support V2/V3 (Address format) and V4 (bytes32 hex format) pools
    prices: Arc<DashMap<Address, Arc<DashMap<String, TokenPrice>>>>,
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

    /// Store or update a token price from a specific pool
    pub fn update_price(&self, token_address: Address, price: TokenPrice) {
        // Use pool_address as String key (supports Address and bytes32 formats)
        let pool_key = price.pool_address.clone();

        // Get or create the per-pool prices map for this token
        let pools = self
            .prices
            .entry(token_address)
            .or_insert_with(|| Arc::new(DashMap::new()))
            .clone();

        // Check if this is a new price or an update with different USD price
        let should_broadcast = if let Some(old_price) = pools.get(&pool_key) {
            // Only broadcast if USD price changed
            old_price.price_in_usd != price.price_in_usd && price.price_in_usd.is_some()
        } else {
            // New pool price with USD value
            price.price_in_usd.is_some()
        };

        // Insert the new price for this pool
        pools.insert(pool_key, price.clone());

        // Broadcast if conditions are met
        if should_broadcast {
            if let Some(ref broadcaster) = self.broadcaster {
                broadcast_price_update(broadcaster, token_address, price);
            }
        }
    }

    /// Get the price of a token from a specific pool
    pub fn get_price(&self, token_address: &Address, pool_address: &str) -> Option<TokenPrice> {
        self.prices
            .get(token_address)
            .and_then(|pools| pools.get(pool_address).map(|entry| entry.value().clone()))
    }

    /// Get all prices for a token across all pools
    pub fn get_prices_for_token(&self, token_address: &Address) -> Vec<TokenPrice> {
        self.prices
            .get(token_address)
            .map(|pools| pools.iter().map(|entry| entry.value().clone()).collect())
            .unwrap_or_default()
    }

    /// Get all stored prices across all tokens and pools
    pub fn get_all_prices(&self) -> Vec<TokenPrice> {
        self.prices
            .iter()
            .flat_map(|token_entry| {
                token_entry
                    .value()
                    .iter()
                    .map(|pool_entry| pool_entry.value().clone())
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    /// Get the number of tokens tracked
    pub fn token_count(&self) -> usize {
        self.prices.len()
    }

    /// Get total number of prices stored across all tokens and pools
    pub fn total_price_count(&self) -> usize {
        self.prices.iter().map(|entry| entry.value().len()).sum()
    }

    /// Check if the store is empty
    pub fn is_empty(&self) -> bool {
        self.prices.is_empty()
    }

    /// Clear all prices
    pub fn clear(&self) {
        self.prices.clear();
    }

    /// Log statistics about stored prices
    pub fn log_stats(&self) {
        let token_count = self.prices.len();
        let total_prices = self.total_price_count();
        info!(
            "[PriceStore] {} tokens with {} total prices across pools",
            token_count, total_prices
        );

        for entry in self.prices.iter() {
            let token = entry.key();
            let pools = entry.value();
            debug!("  Token {}: {} pools", token, pools.len());
        }
    }
}

impl Default for PriceStore {
    fn default() -> Self {
        Self::new()
    }
}
