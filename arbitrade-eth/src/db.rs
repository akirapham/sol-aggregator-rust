use anyhow::{Context, Result};
use rocksdb::{IteratorMode, Options, DB};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArbitrageOpportunity {
    pub timestamp: i64,
    pub token_address: String,
    pub token_symbol: String,
    pub dex_price: f64,
    pub cex_name: String,
    pub cex_price: f64,
    pub cex_symbol: String,
    pub price_diff_percent: f64,
    pub liquidity_usdt: f64,
    pub profit_usdt: f64,
    pub profit_percent: f64,
    // These fields were added later, so make them optional for backward compatibility
    #[serde(default)]
    pub arb_amount_usdt: f64,
    #[serde(default)]
    pub tokens_from_dex: f64,
    #[serde(default)]
    pub gas_fee_usd: f64,
}

pub struct ArbitrageDb {
    db: DB,
}

impl ArbitrageDb {
    /// Open or create a RocksDB database at the specified path
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.create_missing_column_families(true);

        let db = DB::open(&opts, path).context("Failed to open RocksDB")?;

        log::info!("Opened ArbitrageDb at {:?}", db.path());

        Ok(Self { db })
    }

    /// Add an address to the blacklist
    /// Key format: blacklist_address
    pub fn add_to_blacklist(&self, address: &str) -> Result<()> {
        let key = format!("blacklist_{}", address.to_lowercase());
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let value = timestamp.to_string();

        self.db
            .put(key.as_bytes(), value.as_bytes())
            .context("Failed to add address to blacklist")?;

        log::info!("Added address {} to blacklist", address);
        Ok(())
    }

    /// Remove an address from the blacklist
    pub fn remove_from_blacklist(&self, address: &str) -> Result<()> {
        let key = format!("blacklist_{}", address.to_lowercase());

        self.db
            .delete(key.as_bytes())
            .context("Failed to remove address from blacklist")?;

        log::info!("Removed address {} from blacklist", address);
        Ok(())
    }

    /// Check if an address is blacklisted
    pub fn is_blacklisted(&self, address: &str) -> Result<bool> {
        let key = format!("blacklist_{}", address.to_lowercase());

        Ok(self.db.get(key.as_bytes())?.is_some())
    }

    /// Get all blacklisted addresses
    pub fn get_blacklist(&self) -> Result<Vec<String>> {
        let mut addresses = Vec::new();
        let iter = self.db.iterator(IteratorMode::Start);

        for item in iter {
            let (key, _) = item.context("Failed to read from RocksDB")?;
            let key_str = String::from_utf8_lossy(&key);

            if let Some(address) = key_str.strip_prefix("blacklist_") {
                addresses.push(address.to_string());
            }
        }

        Ok(addresses)
    }

    /// Save an arbitrage opportunity to the database
    /// Key format: timestamp_token_address
    pub fn save_opportunity(&self, opp: &ArbitrageOpportunity) -> Result<()> {
        let key = format!("{}_{}", opp.timestamp, opp.token_address);
        let value = serde_json::to_vec(opp).context("Failed to serialize opportunity")?;

        self.db
            .put(key.as_bytes(), value)
            .context("Failed to save opportunity to RocksDB")?;

        Ok(())
    }

    /// Get all opportunities, optionally filtered by token address
    pub fn get_opportunities(
        &self,
        token_address: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Vec<ArbitrageOpportunity>> {
        let mut opportunities = Vec::new();
        let iter = self.db.iterator(IteratorMode::End);

        for item in iter {
            let (key, value) = item.context("Failed to read from RocksDB")?;

            // Skip blacklist entries
            let key_str = String::from_utf8_lossy(&key);
            if key_str.starts_with("blacklist_") {
                continue;
            }

            // Parse the key to check token address filter
            if let Some(filter_addr) = token_address {
                if !key_str.contains(filter_addr) {
                    continue;
                }
            }

            // Try to deserialize, skip if it fails (corrupted data)
            match serde_json::from_slice::<ArbitrageOpportunity>(&value) {
                Ok(opp) => {
                    opportunities.push(opp);

                    if let Some(max) = limit {
                        if opportunities.len() >= max {
                            break;
                        }
                    }
                }
                Err(e) => {
                    log::warn!(
                        "Failed to deserialize opportunity with key {}: {}",
                        key_str,
                        e
                    );
                    continue;
                }
            }
        }

        Ok(opportunities)
    }

    /// Get opportunities within a time range
    pub fn get_opportunities_by_time_range(
        &self,
        start_timestamp: i64,
        end_timestamp: i64,
        limit: Option<usize>,
    ) -> Result<Vec<ArbitrageOpportunity>> {
        let mut opportunities = Vec::new();
        let iter = self.db.iterator(IteratorMode::End);

        for item in iter {
            let (key, value) = item.context("Failed to read from RocksDB")?;

            // Skip blacklist entries
            let key_str = String::from_utf8_lossy(&key);
            if key_str.starts_with("blacklist_") {
                continue;
            }

            // Try to deserialize, skip if it fails (corrupted data)
            match serde_json::from_slice::<ArbitrageOpportunity>(&value) {
                Ok(opp) => {
                    if opp.timestamp >= start_timestamp && opp.timestamp <= end_timestamp {
                        opportunities.push(opp);

                        if let Some(max) = limit {
                            if opportunities.len() >= max {
                                break;
                            }
                        }
                    }
                }
                Err(e) => {
                    log::warn!(
                        "Failed to deserialize opportunity with key {}: {}",
                        key_str,
                        e
                    );
                    continue;
                }
            }
        }

        Ok(opportunities)
    }

    /// Get the most profitable opportunities
    pub fn get_top_opportunities(&self, limit: usize) -> Result<Vec<ArbitrageOpportunity>> {
        let mut opportunities = self.get_opportunities(None, None)?;

        // Sort by profit in descending order
        opportunities.sort_by(|a, b| {
            b.profit_usdt
                .partial_cmp(&a.profit_usdt)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        opportunities.truncate(limit);

        Ok(opportunities)
    }

    /// Get statistics about stored opportunities
    pub fn get_stats(&self) -> Result<DbStats> {
        let iter = self.db.iterator(IteratorMode::Start);
        let mut total_count = 0;
        let mut total_profit = 0.0;
        let mut max_profit: f64 = 0.0;
        let mut profitable_count = 0;
        let mut unprofitable_count = 0;
        let mut unique_tokens = std::collections::HashSet::new();
        let mut exchange_data: std::collections::HashMap<String, Vec<f64>> =
            std::collections::HashMap::new();

        for item in iter {
            let (key, value) = item.context("Failed to read from RocksDB")?;

            // Skip blacklist entries
            let key_str = String::from_utf8_lossy(&key);
            if key_str.starts_with("blacklist_") {
                continue;
            }

            // Try to deserialize, skip if it fails (corrupted data)
            match serde_json::from_slice::<ArbitrageOpportunity>(&value) {
                Ok(opp) => {
                    total_count += 1;
                    total_profit += opp.profit_usdt;
                    max_profit = max_profit.max(opp.profit_usdt);
                    unique_tokens.insert(opp.token_address.clone());

                    // Count profitable vs unprofitable
                    if opp.profit_usdt > 0.0 {
                        profitable_count += 1;
                    } else {
                        unprofitable_count += 1;
                    }

                    // Collect per-exchange data
                    exchange_data
                        .entry(opp.cex_name.clone())
                        .or_insert_with(Vec::new)
                        .push(opp.profit_usdt);
                }
                Err(e) => {
                    log::warn!(
                        "Failed to deserialize opportunity with key {}: {}",
                        key_str,
                        e
                    );
                    continue;
                }
            }
        }

        // Calculate per-exchange statistics
        let mut exchange_stats: Vec<ExchangeStats> = exchange_data
            .into_iter()
            .map(|(name, profits)| {
                let total = profits.len();
                let profitable = profits.iter().filter(|&&p| p > 0.0).count();
                let unprofitable = profits.iter().filter(|&&p| p <= 0.0).count();
                let total_profit: f64 = profits.iter().sum();
                let max_profit = profits.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
                let min_profit = profits.iter().fold(f64::INFINITY, |a, &b| a.min(b));

                ExchangeStats {
                    exchange_name: name,
                    total_opportunities: total,
                    profitable_count: profitable,
                    unprofitable_count: unprofitable,
                    win_rate: if total > 0 {
                        (profitable as f64 / total as f64) * 100.0
                    } else {
                        0.0
                    },
                    total_profit,
                    average_profit: if total > 0 {
                        total_profit / total as f64
                    } else {
                        0.0
                    },
                    max_profit,
                    min_profit,
                }
            })
            .collect();

        // Sort by total profit descending
        exchange_stats.sort_by(|a, b| {
            b.total_profit
                .partial_cmp(&a.total_profit)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        Ok(DbStats {
            total_opportunities: total_count,
            unique_tokens: unique_tokens.len(),
            total_profit_usdt: total_profit,
            average_profit_usdt: if total_count > 0 {
                total_profit / total_count as f64
            } else {
                0.0
            },
            max_profit_usdt: max_profit,
            profitable_count,
            unprofitable_count,
            win_rate: if total_count > 0 {
                (profitable_count as f64 / total_count as f64) * 100.0
            } else {
                0.0
            },
            exchange_stats,
        })
    }

    /// Delete old opportunities (cleanup)
    pub fn delete_old_opportunities(&self, before_timestamp: i64) -> Result<usize> {
        let mut count = 0;
        let iter = self.db.iterator(IteratorMode::Start);
        let mut keys_to_delete = Vec::new();

        for item in iter {
            let (key, value) = item.context("Failed to read from RocksDB")?;

            // Skip blacklist entries
            let key_str = String::from_utf8_lossy(&key);
            if key_str.starts_with("blacklist_") {
                continue;
            }

            // Try to deserialize, skip if it fails (corrupted data)
            match serde_json::from_slice::<ArbitrageOpportunity>(&value) {
                Ok(opp) => {
                    if opp.timestamp < before_timestamp {
                        keys_to_delete.push(key.to_vec());
                    }
                }
                Err(e) => {
                    log::warn!(
                        "Failed to deserialize opportunity with key {}: {}",
                        key_str,
                        e
                    );
                    continue;
                }
            }
        }

        for key in keys_to_delete {
            self.db
                .delete(key)
                .context("Failed to delete from RocksDB")?;
            count += 1;
        }

        log::info!("Deleted {} old opportunities from database", count);

        Ok(count)
    }

    /// Delete all opportunities for a specific token address
    pub fn delete_opportunities_by_token(&self, token_address: &str) -> Result<usize> {
        let mut count = 0;
        let iter = self.db.iterator(IteratorMode::Start);
        let mut keys_to_delete = Vec::new();
        let token_address_lower = token_address.to_lowercase();

        for item in iter {
            let (key, value) = item.context("Failed to read from RocksDB")?;

            // Skip blacklist entries
            let key_str = String::from_utf8_lossy(&key);
            if key_str.starts_with("blacklist_") {
                continue;
            }

            let opp: ArbitrageOpportunity =
                serde_json::from_slice(&value).context("Failed to deserialize opportunity")?;

            if opp.token_address.to_lowercase() == token_address_lower {
                keys_to_delete.push(key.to_vec());
            }
        }

        for key in keys_to_delete {
            self.db
                .delete(key)
                .context("Failed to delete from RocksDB")?;
            count += 1;
        }

        log::info!(
            "Deleted {} opportunities for token {} from database",
            count,
            token_address
        );

        Ok(count)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DbStats {
    pub total_opportunities: usize,
    pub unique_tokens: usize,
    pub total_profit_usdt: f64,
    pub average_profit_usdt: f64,
    pub max_profit_usdt: f64,
    pub profitable_count: usize,
    pub unprofitable_count: usize,
    pub win_rate: f64,
    pub exchange_stats: Vec<ExchangeStats>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ExchangeStats {
    pub exchange_name: String,
    pub total_opportunities: usize,
    pub profitable_count: usize,
    pub unprofitable_count: usize,
    pub win_rate: f64,
    pub total_profit: f64,
    pub average_profit: f64,
    pub max_profit: f64,
    pub min_profit: f64,
}
