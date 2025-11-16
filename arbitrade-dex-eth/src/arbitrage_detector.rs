use crate::price_cache::PriceCache;
use crate::types::DexArbitrageOpportunity;
use eth_dex_quote::TokenPriceUpdate;
use ethers::types::Address;
use log::{debug, info};
use std::str::FromStr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct ArbitrageDetector {
    price_cache: Arc<PriceCache>,
    /// Minimum profit percentage to consider an opportunity
    min_profit_percent: f64,
}

impl ArbitrageDetector {
    pub fn new(
        price_cache: Arc<PriceCache>,
        min_profit_percent: f64,
        _min_price_diff_eth: f64, // Deprecated: we use min_profit_percent only
    ) -> Self {
        ArbitrageDetector {
            price_cache,
            min_profit_percent,
        }
    }

    /// Check for arbitrage opportunities when a new price update arrives
    /// This is called reactively from the WebSocket listener
    pub fn check_opportunities_for_token(
        &self,
        token_address: &Address,
    ) -> Vec<DexArbitrageOpportunity> {
        let all_prices = self.price_cache.get_all_prices(token_address);
        if all_prices.is_empty() {
            debug!(
                "Token {} has no prices in cache",
                format!("{:?}", token_address).to_lowercase()
            );
            return vec![];
        }

        if all_prices.len() < 2 {
            debug!(
                "Token {} has {} pool(s), need at least 2 for arbitrage",
                format!("{:?}", token_address).to_lowercase(),
                all_prices.len()
            );
            return vec![];
        }

        let token_address_str = format!("{:?}", token_address).to_lowercase();

        info!(
            "Checking arbitrage for token {} with {} pools",
            token_address_str,
            all_prices.len()
        );

        let mut opportunities = vec![];

        // Compare all pairs of pools
        for (i, buy_pool) in all_prices.iter().enumerate() {
            for sell_pool in all_prices.iter().skip(i + 1) {
                debug!(
                    "  Comparing pools: buy={} (${:.6}), sell={} (${:.6})",
                    buy_pool.pool_address,
                    buy_pool.price_in_eth,
                    sell_pool.pool_address,
                    sell_pool.price_in_eth
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
                "💰 Found {} arbitrage opportunity(ies) for token {}",
                opportunities.len(),
                token_address_str
            );
            for (idx, opp) in opportunities.iter().take(5).enumerate() {
                if let Some(usd_profit) = opp.potential_profit_usd {
                    info!(
                        "  #{}: Buy@${:.6} ({}) / Sell@${:.6} ({}) = {:.2}% profit (${:.2})",
                        idx + 1,
                        opp.buy_pool.price_in_usd.unwrap_or(0.0),
                        opp.buy_pool.dex_version,
                        opp.sell_pool.price_in_usd.unwrap_or(0.0),
                        opp.sell_pool.dex_version,
                        opp.price_diff_percent,
                        usd_profit
                    );
                }
            }
        }

        opportunities
    }

    /// Check if a buy/sell pool pair creates an arbitrage opportunity
    /// We trade USDT pairs, so we compare prices in USD
    fn check_pair(
        &self,
        buy_pool: TokenPriceUpdate,
        sell_pool: TokenPriceUpdate,
    ) -> Option<DexArbitrageOpportunity> {
        // Both prices must be in USD
        let buy_price_usd = buy_pool.price_in_usd?;
        let sell_price_usd = sell_pool.price_in_usd?;

        // Check if it's profitable: sell price must be higher than buy price
        if sell_price_usd <= buy_price_usd {
            return None;
        }

        let price_diff_usd = sell_price_usd - buy_price_usd;
        let price_diff_percent = (price_diff_usd / buy_price_usd) * 100.0;

        // Filter by minimum thresholds
        if price_diff_percent < self.min_profit_percent {
            return None;
        }

        let detected_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Parse token address
        let token_address = Address::from_str(&buy_pool.token_address).unwrap_or(Address::zero());

        Some(DexArbitrageOpportunity {
            token_address,
            buy_pool,
            sell_pool,
            price_diff_percent,
            potential_profit_usd: Some(price_diff_usd),
            detected_at,
        })
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
        let buy_pool = TokenPriceUpdate {
            token_address: format!("{:?}", token).to_lowercase(),
            price_in_eth: 1.0,
            price_in_usd: Some(2000.0),
            pool_address: format!(
                "{:?}",
                Address::from_str("0x0000000000000000000000000000000000000001").unwrap()
            ),
            dex_version: "UniswapV2".to_string(),
            decimals: 18,
            last_updated: 100,
            pool_token0: Address::zero(),
            pool_token1: token,
            eth_chain: eth_dex_quote::EthChain::Mainnet,
            fee_tier: None,
            tick_spacing: None,
            eth_price_usd: 2000.0,
        };

        let sell_pool = TokenPriceUpdate {
            price_in_eth: 1.02, // 2% more expensive
            price_in_usd: Some(2040.0),
            pool_address: format!(
                "{:?}",
                Address::from_str("0x0000000000000000000000000000000000000002").unwrap()
            ),
            eth_price_usd: 2040.0,
            ..buy_pool.clone()
        };

        cache.update_price(buy_pool);
        cache.update_price(sell_pool);

        let opportunities = detector.check_opportunities_for_token(&token);
        assert_eq!(opportunities.len(), 1);
        assert!(
            opportunities[0].price_diff_percent > 1.9 && opportunities[0].price_diff_percent < 2.1
        );
    }
}
