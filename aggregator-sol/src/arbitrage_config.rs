use rocksdb::DB;
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use std::collections::HashSet;
use std::fs;
use std::str::FromStr;
use std::sync::Arc;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ArbitrageConfig {
    pub settings: ArbitrageSettings,
    pub monitored_tokens: Vec<MonitoredToken>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ArbitrageSettings {
    /// Minimum profit threshold in basis points (e.g., 50 = 0.5%)
    pub min_profit_bps: u64,

    /// Base token to start arbitrage with (e.g., USDC address)
    pub base_token: String,

    /// Amount of base token to use for arbitrage (in token's base units)
    pub base_amount: u64,

    /// Slippage tolerance in basis points
    pub slippage_bps: u16,

    /// Maximum concurrent arbitrage checks
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent_checks: usize,
}

fn default_max_concurrent() -> usize {
    10
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Hash)]
pub struct MonitoredToken {
    /// Token symbol for logging
    pub symbol: String,

    /// Token address
    pub address: String,

    /// Whether this token is enabled for monitoring
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

impl MonitoredToken {
    pub fn get_pubkey(&self) -> Result<Pubkey, String> {
        Pubkey::from_str(&self.address).map_err(|e| format!("Invalid token address: {}", e))
    }
}

impl ArbitrageConfig {
    /// Load configuration from TOML file
    pub fn from_file(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let contents = fs::read_to_string(path)?;
        let config: ArbitrageConfig = toml::from_str(&contents)?;
        Ok(config)
    }

    /// Get base token pubkey
    pub fn get_base_token(&self) -> Result<Pubkey, String> {
        Pubkey::from_str(&self.settings.base_token)
            .map_err(|e| format!("Invalid base_token: {}", e))
    }

    /// Get all enabled monitored tokens
    pub fn get_enabled_tokens(&self) -> Vec<&MonitoredToken> {
        self.monitored_tokens.iter().filter(|t| t.enabled).collect()
    }

    /// Get set of all monitored token pubkeys (excluding base token to avoid self-trading)
    pub fn get_monitored_token_pubkeys(&self) -> HashSet<Pubkey> {
        let base_token = self.get_base_token().ok();
        let mut tokens = HashSet::new();

        for token in self.get_enabled_tokens() {
            if let Ok(pubkey) = token.get_pubkey() {
                // Don't include base token in monitored list (we always trade FROM base token)
                if Some(pubkey) != base_token {
                    tokens.insert(pubkey);
                }
            }
        }
        tokens
    }

    /// Add a token to the monitored list
    pub fn add_token(&mut self, symbol: String, address: String) -> Result<(), String> {
        // Validate address
        Pubkey::from_str(&address).map_err(|e| format!("Invalid token address: {}", e))?;

        // Check if already exists
        if self.monitored_tokens.iter().any(|t| t.address == address) {
            return Err("Token already exists in monitored list".to_string());
        }

        self.monitored_tokens.push(MonitoredToken {
            symbol,
            address,
            enabled: true,
        });

        Ok(())
    }

    /// Remove a token from the monitored list
    pub fn remove_token(&mut self, address: &str) -> Result<MonitoredToken, String> {
        let pos = self
            .monitored_tokens
            .iter()
            .position(|t| t.address == address)
            .ok_or_else(|| "Token not found in monitored list".to_string())?;

        Ok(self.monitored_tokens.remove(pos))
    }

    #[allow(unused)]
    /// Enable or disable a token
    pub fn set_token_enabled(&mut self, address: &str, enabled: bool) -> Result<(), String> {
        let token = self
            .monitored_tokens
            .iter_mut()
            .find(|t| t.address == address)
            .ok_or_else(|| "Token not found in monitored list".to_string())?;

        token.enabled = enabled;
        Ok(())
    }

    /// Save monitored tokens to RocksDB
    pub fn save_tokens_to_db(db: &Arc<DB>, tokens: &[MonitoredToken]) -> Result<(), String> {
        const ARBITRAGE_TOKENS_KEY: &[u8] = b"arbitrage_monitored_tokens";

        let json = serde_json::to_string(tokens)
            .map_err(|e| format!("Failed to serialize tokens: {}", e))?;

        db.put(ARBITRAGE_TOKENS_KEY, json.as_bytes())
            .map_err(|e| format!("Failed to save tokens to DB: {}", e))?;

        Ok(())
    }

    /// Load monitored tokens from RocksDB
    pub fn load_tokens_from_db(db: &Arc<DB>) -> Result<Vec<MonitoredToken>, String> {
        const ARBITRAGE_TOKENS_KEY: &[u8] = b"arbitrage_monitored_tokens";

        match db.get(ARBITRAGE_TOKENS_KEY) {
            Ok(Some(bytes)) => {
                let json = String::from_utf8(bytes.to_vec())
                    .map_err(|e| format!("Invalid UTF-8 in DB: {}", e))?;

                serde_json::from_str(&json)
                    .map_err(|e| format!("Failed to deserialize tokens: {}", e))
            }
            Ok(None) => Ok(Vec::new()), // No tokens saved yet
            Err(e) => Err(format!("Failed to load tokens from DB: {}", e)),
        }
    }

    /// Merge tokens from TOML config and RocksDB
    /// DB tokens take precedence (allow runtime modifications to persist)
    pub fn merge_with_db_tokens(mut self, db_tokens: Vec<MonitoredToken>) -> Self {
        if db_tokens.is_empty() {
            return self;
        }

        // Create a map of DB tokens by address
        let db_map: std::collections::HashMap<String, MonitoredToken> = db_tokens
            .into_iter()
            .map(|t| (t.address.clone(), t))
            .collect();

        // Update existing tokens with DB values or keep TOML values
        for token in &mut self.monitored_tokens {
            if let Some(db_token) = db_map.get(&token.address) {
                *token = db_token.clone();
            }
        }

        // Add new tokens from DB that aren't in TOML
        for (address, db_token) in db_map {
            if !self.monitored_tokens.iter().any(|t| t.address == address) {
                self.monitored_tokens.push(db_token);
            }
        }

        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_config() {
        let config_str = r#"
[settings]
min_profit_bps = 50
base_token = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
base_amount = 1000000000
slippage_bps = 50

[[monitored_tokens]]
symbol = "SOL"
address = "So11111111111111111111111111111111111111112"
enabled = true

[[monitored_tokens]]
symbol = "USDT"
address = "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB"
enabled = true
"#;

        let config: ArbitrageConfig = toml::from_str(config_str).unwrap();
        assert_eq!(config.settings.min_profit_bps, 50);
        assert_eq!(config.monitored_tokens.len(), 2);
        assert_eq!(config.monitored_tokens[0].symbol, "SOL");
    }
}
