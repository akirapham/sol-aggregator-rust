use crate::types::PoolPrice;
use dashmap::DashMap;
use ethers::types::Address;
use log::info;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

/// Thread-safe in-memory price cache for all pools
/// Indexed by token address, then by pool address
#[derive(Debug)]
pub struct PriceCache {
    /// Map: token_address -> HashMap<pool_address, PoolPrice>
    prices: Arc<DashMap<String, HashMap<String, PoolPrice>>>,
}

impl PriceCache {
    pub fn new() -> Self {
        PriceCache {
            prices: Arc::new(DashMap::new()),
        }
    }

    /// Add or update a pool price
    pub fn update_price(&self, pool_price: PoolPrice) {
        let token_addr = pool_price.token_address.to_string().to_lowercase();
        let pool_addr = pool_price.pool_address.to_string().to_lowercase();

        self.prices
            .entry(token_addr)
            .or_insert_with(HashMap::new)
            .insert(pool_addr, pool_price);
    }

    /// Get the best (lowest) price for a token across all pools
    pub fn get_best_buy_price(&self, token_address: &Address) -> Option<PoolPrice> {
        let token_addr = token_address.to_string().to_lowercase();
        self.prices.get(&token_addr).and_then(|pools| {
            pools
                .values()
                .min_by(|a, b| a.price_in_eth.partial_cmp(&b.price_in_eth).unwrap())
                .cloned()
        })
    }

    /// Get the worst (highest) price for a token across all pools
    pub fn get_best_sell_price(&self, token_address: &Address) -> Option<PoolPrice> {
        let token_addr = token_address.to_string().to_lowercase();
        self.prices.get(&token_addr).and_then(|pools| {
            pools
                .values()
                .max_by(|a, b| a.price_in_eth.partial_cmp(&b.price_in_eth).unwrap())
                .cloned()
        })
    }

    /// Get all prices for a token
    pub fn get_all_prices(&self, token_address: &Address) -> Vec<PoolPrice> {
        let token_addr = token_address.to_string().to_lowercase();
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
                entry.value().values().next().map(|p| p.token_address)
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

    #[test]
    fn test_price_cache_operations() {
        let cache = PriceCache::new();
        let token_addr = Address::from_str("0x1234567890123456789012345678901234567890").unwrap();

        let price1 = PoolPrice {
            token_address: token_addr,
            price_in_eth: 1.5,
            price_in_usd: Some(3000.0),
            pool_address: Address::from_str("0x0000000000000000000000000000000000000001").unwrap(),
            dex_version: "UniswapV2".to_string(),
            decimals: 18,
            last_updated: 100,
            liquidity_eth: None,
            liquidity_usd: None,
        };

        let price2 = PoolPrice {
            price_in_eth: 1.2, // Better buy price
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
