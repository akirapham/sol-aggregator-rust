use crate::price_cache::PriceCache;
use crate::types::{DexArbitrageOpportunity, PoolPrice};
use ethers::types::Address;
use log::{debug, info};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Detects arbitrage opportunities across DEX pools
pub struct ArbitrageDetector {
    price_cache: Arc<PriceCache>,
    /// Minimum profit percentage to consider an opportunity
    min_profit_percent: f64,
    /// Minimum price difference in ETH
    min_price_diff_eth: f64,
}

impl ArbitrageDetector {
    pub fn new(
        price_cache: Arc<PriceCache>,
        min_profit_percent: f64,
        min_price_diff_eth: f64,
    ) -> Self {
        ArbitrageDetector {
            price_cache,
            min_profit_percent,
            min_price_diff_eth,
        }
    }

    /// Check for arbitrage opportunities for a specific token
    pub fn find_opportunities(&self, token_address: &Address) -> Vec<DexArbitrageOpportunity> {
        let all_prices = self.price_cache.get_all_prices(token_address);
        if all_prices.len() < 2 {
            debug!(
                "Token {} has {} pool(s), need at least 2 for arbitrage",
                token_address,
                all_prices.len()
            );
            return vec![];
        }

        let mut opportunities = vec![];
        // Compare all pairs of pools
        for (i, buy_pool) in all_prices.iter().enumerate() {
            info!(
                "Checking token {}: buy from pool {} at {:.6} ETH",
                token_address, buy_pool.pool_address, buy_pool.price_in_eth
            );
            for sell_pool in all_prices.iter().skip(i + 1) {
                info!(
                    "Checking token {}: sell to pool {} at {:.6} ETH",
                    token_address, sell_pool.pool_address, sell_pool.price_in_eth
                );
                // Try both directions
                if let Some(opp) = self.check_pair(buy_pool.clone(), sell_pool.clone()) {
                    opportunities.push(opp);
                }
                if let Some(opp) = self.check_pair(sell_pool.clone(), buy_pool.clone()) {
                    opportunities.push(opp);
                }
            }
        }

        // Sort by profit percentage
        opportunities.sort_by(|a, b| {
            b.price_diff_percent
                .partial_cmp(&a.price_diff_percent)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Log top opportunities
        if !opportunities.is_empty() {
            info!(
                "Found {} arbitrage opportunity(ies) for token {}",
                opportunities.len(),
                token_address
            );
            for (idx, opp) in opportunities.iter().take(3).enumerate() {
                info!(
                    "  #{}: {} - {:.2}% profit ({:.6} ETH)",
                    idx + 1,
                    opp,
                    opp.price_diff_percent,
                    opp.potential_profit_eth
                );
            }
        }

        opportunities
    }

    /// Check if a buy/sell pool pair creates an arbitrage opportunity
    fn check_pair(
        &self,
        buy_pool: PoolPrice,
        sell_pool: PoolPrice,
    ) -> Option<DexArbitrageOpportunity> {
        // Check if it's profitable
        if sell_pool.price_in_eth <= buy_pool.price_in_eth {
            return None;
        }

        let price_diff_eth = sell_pool.price_in_eth - buy_pool.price_in_eth;
        let price_diff_percent = (price_diff_eth / buy_pool.price_in_eth) * 100.0;

        // Filter by minimum thresholds
        if price_diff_percent < self.min_profit_percent || price_diff_eth < self.min_price_diff_eth
        {
            return None;
        }

        let detected_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let potential_profit_eth = price_diff_eth;
        let potential_profit_usd = sell_pool
            .price_in_usd
            .and_then(|sell_usd| buy_pool.price_in_usd.map(|buy_usd| sell_usd - buy_usd));

        Some(DexArbitrageOpportunity {
            token_address: buy_pool.token_address,
            buy_pool,
            sell_pool,
            price_diff_eth,
            price_diff_percent,
            potential_profit_eth,
            potential_profit_usd,
            gas_cost_eth: None,
            net_profit_eth: None,
            detected_at,
        })
    }

    /// Find all tokens with arbitrage opportunities
    pub fn find_all_opportunities(&self) -> Vec<DexArbitrageOpportunity> {
        let tokens = self.price_cache.get_all_tokens();
        let mut all_opportunities = vec![];

        for token in tokens {
            all_opportunities.extend(self.find_opportunities(&token));
        }

        // Sort by total profit
        all_opportunities.sort_by(|a, b| {
            b.potential_profit_eth
                .partial_cmp(&a.potential_profit_eth)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        all_opportunities
    }

    /// Set minimum profit threshold
    pub fn set_min_profit_percent(&mut self, min_profit_percent: f64) {
        self.min_profit_percent = min_profit_percent;
    }

    /// Set minimum price difference threshold
    pub fn set_min_price_diff_eth(&mut self, min_price_diff_eth: f64) {
        self.min_price_diff_eth = min_price_diff_eth;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_arbitrage_detection() {
        let cache = Arc::new(PriceCache::new());
        let detector = ArbitrageDetector::new(cache.clone(), 1.0, 0.001);

        let token = Address::from_str("0x1234567890123456789012345678901234567890").unwrap();

        // Create two pools with different prices
        let buy_pool = PoolPrice {
            token_address: token,
            price_in_eth: 1.0,
            price_in_usd: Some(2000.0),
            pool_address: Address::from_str("0x0000000000000000000000000000000000000001").unwrap(),
            dex_version: "UniswapV2".to_string(),
            decimals: 18,
            last_updated: 100,
            liquidity_eth: None,
            liquidity_usd: None,
        };

        let sell_pool = PoolPrice {
            price_in_eth: 1.02, // 2% more expensive
            price_in_usd: Some(2040.0),
            pool_address: Address::from_str("0x0000000000000000000000000000000000000002").unwrap(),
            ..buy_pool.clone()
        };

        cache.update_price(buy_pool);
        cache.update_price(sell_pool);

        let opportunities = detector.find_opportunities(&token);
        assert_eq!(opportunities.len(), 1);
        assert!(
            opportunities[0].price_diff_percent > 1.9 && opportunities[0].price_diff_percent < 2.1
        );
    }
}

use std::str::FromStr;
