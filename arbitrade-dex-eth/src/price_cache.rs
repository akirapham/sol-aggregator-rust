use dashmap::DashMap;
use eth_dex_quote::TokenPriceUpdate;
use ethers::types::Address;
use std::collections::HashMap;
use std::sync::Arc;

/// Thread-safe in-memory price cache for all pools
/// Indexed by token address, then by pool address
/// This cache stores TokenPriceUpdate directly from WebSocket messages
#[derive(Debug)]
pub struct PriceCache {
    /// Map: token_address (lowercase string) -> HashMap<pool_address, TokenPriceUpdate>
    prices: Arc<DashMap<String, HashMap<String, TokenPriceUpdate>>>,
}

impl PriceCache {
    pub fn new() -> Self {
        PriceCache {
            prices: Arc::new(DashMap::new()),
        }
    }

    /// Add or update a pool price from a TokenPriceUpdate
    pub fn update_price(&self, price_update: TokenPriceUpdate) {
        let token_addr = price_update.token_address.to_lowercase();
        let pool_addr = price_update.pool_address.to_lowercase();

        log::debug!(
            "Storing price: token={}, pool={}, price_usd={:?}",
            token_addr,
            pool_addr,
            price_update.price_in_usd
        );

        self.prices
            .entry(token_addr.clone())
            .or_insert_with(HashMap::new)
            .insert(pool_addr, price_update);

        // read price again
        let prices_len = self.prices.get(&token_addr).unwrap().value().len();

        log::debug!(
            "Price stored. Cache now has {} for token {}",
            prices_len,
            token_addr
        );
    }

    /// Get the best (lowest) price for a token across all pools
    pub fn get_best_buy_price(&self, token_address: &Address) -> Option<TokenPriceUpdate> {
        let token_addr = format!("{:?}", token_address).to_lowercase();
        self.prices.get(&token_addr).and_then(|pools| {
            pools
                .values()
                .min_by(|a, b| a.price_in_eth.partial_cmp(&b.price_in_eth).unwrap())
                .cloned()
        })
    }

    /// Get the worst (highest) price for a token across all pools
    pub fn get_best_sell_price(&self, token_address: &Address) -> Option<TokenPriceUpdate> {
        let token_addr = format!("{:?}", token_address).to_lowercase();
        self.prices.get(&token_addr).and_then(|pools| {
            pools
                .values()
                .max_by(|a, b| a.price_in_eth.partial_cmp(&b.price_in_eth).unwrap())
                .cloned()
        })
    }

    /// Get all prices for a token
    pub fn get_all_prices(&self, token_address: &Address) -> Vec<TokenPriceUpdate> {
        let token_addr = format!("{:?}", token_address).to_lowercase();
        log::debug!(
            "Looking up token: {} (searching for: {})",
            token_address,
            token_addr
        );

        // Debug: print all keys in cache for comparison
        let all_keys: Vec<String> = self
            .prices
            .iter()
            .map(|ref_multi| ref_multi.key().clone())
            .collect();
        log::debug!("Cache keys: {:?}", all_keys);
        log::debug!(
            "Looking for key: '{}', keys contain it: {}",
            token_addr,
            all_keys.contains(&token_addr)
        );

        self.prices
            .get(&token_addr)
            .map(|pools| pools.values().cloned().collect())
            .unwrap_or_default()
    }

    /// Get total number of unique tokens being tracked
    pub fn token_count(&self) -> usize {
        self.prices.len()
    }

    /// Get total number of pools across all tokens
    pub fn pool_count(&self) -> usize {
        self.prices.iter().map(|entry| entry.value().len()).sum()
    }

    /// Get all tokens being tracked
    pub fn get_all_tokens(&self) -> Vec<Address> {
        self.prices
            .iter()
            .filter_map(|entry| {
                // Get the first price from the HashMap to extract token_address
                entry
                    .value()
                    .values()
                    .next()
                    .and_then(|p| p.token_address.parse::<Address>().ok())
            })
            .collect()
    }

    /// Clear old prices (older than specified seconds)
    pub fn prune_old_prices(&self, max_age_seconds: u64) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let min_timestamp = now.saturating_sub(max_age_seconds);

        for mut entry in self.prices.iter_mut() {
            entry
                .value_mut()
                .retain(|_, price| price.last_updated >= min_timestamp);

            // Remove token entry if no prices left
            if entry.value().is_empty() {
                drop(entry);
            }
        }
    }

    /// Get statistics about the cache
    pub fn get_stats(&self) -> CacheStats {
        let mut token_counts = vec![];
        let mut total_pools = 0;

        for entry in self.prices.iter() {
            let count = entry.value().len();
            total_pools += count;
            token_counts.push((entry.key().clone(), count));
        }

        CacheStats {
            unique_tokens: self.prices.len(),
            total_pools,
            tokens_with_multiple_pools: token_counts.iter().filter(|(_, c)| *c > 1).count(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CacheStats {
    pub unique_tokens: usize,
    pub total_pools: usize,
    pub tokens_with_multiple_pools: usize,
}

impl Default for PriceCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_price_cache_operations() {
        let cache = PriceCache::new();
        let token_addr = Address::from_str("0x1234567890123456789012345678901234567890").unwrap();

        let price1 = TokenPriceUpdate {
            token_address: format!("{:?}", token_addr).to_lowercase(),
            price_in_eth: 1.5,
            price_in_usd: Some(3000.0),
            pool_address: format!(
                "{:?}",
                Address::from_str("0x0000000000000000000000000000000000000001").unwrap()
            ),
            dex_version: "UniswapV2".to_string(),
            decimals: 18,
            last_updated: 100,
            pool_token0: Address::zero(),
            pool_token1: token_addr,
            eth_chain: eth_dex_quote::EthChain::Mainnet,
            fee_tier: None,
            tick_spacing: None,
            eth_price_usd: 3000.0,
        };

        let price2 = TokenPriceUpdate {
            price_in_eth: 1.2, // Better buy price
            price_in_usd: Some(2400.0),
            pool_address: format!(
                "{:?}",
                Address::from_str("0x0000000000000000000000000000000000000002").unwrap()
            ),
            eth_price_usd: 2400.0,
            ..price1.clone()
        };

        cache.update_price(price1.clone());
        cache.update_price(price2.clone());

        let best_buy = cache.get_best_buy_price(&token_addr);
        assert_eq!(best_buy.map(|p| p.price_in_eth), Some(1.2));

        let best_sell = cache.get_best_sell_price(&token_addr);
        assert_eq!(best_sell.map(|p| p.price_in_eth), Some(1.5));
    }
}
