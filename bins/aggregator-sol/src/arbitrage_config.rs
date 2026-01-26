use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use sqlx::{Pool, Postgres};
use std::collections::HashSet;
use std::fs;
use std::str::FromStr;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ArbitrageConfig {
    pub settings: ArbitrageSettings,
    pub monitored_tokens: Vec<MonitoredToken>,
    #[serde(default)]
    pub monitored_pools: Vec<MonitoredPool>,
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

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Hash)]
pub struct MonitoredPool {
    /// Pool address
    pub address: String,

    /// DEX type (e.g., "MeteoraDlmm", "Raydium")
    pub dex: String,

    /// Token pair (e.g., "PUMP/SOL")
    pub pair: String,

    /// Whether this pool is enabled for monitoring
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

impl MonitoredPool {
    pub fn get_pubkey(&self) -> Result<Pubkey, String> {
        Pubkey::from_str(&self.address).map_err(|e| format!("Invalid pool address: {}", e))
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

    /// Get all enabled monitored pools
    pub fn get_enabled_pools(&self) -> Vec<&MonitoredPool> {
        self.monitored_pools.iter().filter(|p| p.enabled).collect()
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

    /// Save monitored tokens to Postgres
    /// We can use a dedicated table or a single JSONB row in a 'configs' table.
    /// Given the previous implementation used a key-value store, let's create a simplified table
    /// or reuse the `arbitrage_monitored_tokens` concept.
    /// For simplicity and to match the schema, let's assume we can store this as a JSON blob
    /// within a generic 'app_config' table or similar, OR just use the `tokens` table with a flag?
    /// The `tokens` table is for metadata. This is "which tokens are monitored".
    /// A simple key-value table `app_settings` (key, value) is best for this migration.
    /// Or we can just create a file, but ensuring DB persistence is the goal.
    /// Let's use `app_settings` table (we'll need to create it) or just use a specific query.
    /// Let's CREATE the table if not exists here (or in init.sql).
    pub async fn save_tokens_to_db(
        db: &Pool<Postgres>,
        tokens: &[MonitoredToken],
    ) -> Result<(), String> {
        let json = serde_json::to_value(tokens)
            .map_err(|e| format!("Failed to serialize tokens: {}", e))?;

        // Using a simple KV table pattern on the fly or assuming existence.
        // Let's assume we add `app_settings` to init.sql or create it here.
        // Creating here is safer for immediate migration correctness.
        sqlx::query("CREATE TABLE IF NOT EXISTS app_settings (key TEXT PRIMARY KEY, value JSONB)")
            .execute(db)
            .await
            .map_err(|e| format!("Failed to ensure settings table: {}", e))?;

        sqlx::query("INSERT INTO app_settings (key, value) VALUES ($1, $2) ON CONFLICT (key) DO UPDATE SET value = $2")
            .bind("arbitrage_monitored_tokens")
            .bind(json)
            .execute(db)
            .await
            .map_err(|e| format!("Failed to save tokens to DB: {}", e))?;

        Ok(())
    }

    /// Load monitored tokens from Postgres
    pub async fn load_tokens_from_db(db: &Pool<Postgres>) -> Result<Vec<MonitoredToken>, String> {
        // Ensure table exists first to avoid error on fresh start
        sqlx::query("CREATE TABLE IF NOT EXISTS app_settings (key TEXT PRIMARY KEY, value JSONB)")
            .execute(db)
            .await
            .map_err(|e| format!("Failed to ensure settings table: {}", e))?;

        let row = sqlx::query("SELECT value FROM app_settings WHERE key = $1")
            .bind("arbitrage_monitored_tokens")
            .fetch_optional(db)
            .await
            .map_err(|e| format!("Failed to load tokens from DB: {}", e))?;

        if let Some(row) = row {
            use sqlx::Row;
            let json: serde_json::Value = row.try_get("value").unwrap_or(serde_json::Value::Null);
            if json.is_null() {
                return Ok(Vec::new());
            }
            serde_json::from_value(json).map_err(|e| format!("Failed to deserialize tokens: {}", e))
        } else {
            Ok(Vec::new())
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

[[monitored_pools]]
address = "Hi9jUWFHAYu9zRDem4wDS17s1ckiVSve3wpNRdwSDH7o"
dex = "MeteoraDlmm"
pair = "PUMP/SOL"
enabled = true
"#;

        let config: ArbitrageConfig = toml::from_str(config_str).unwrap();
        assert_eq!(config.settings.min_profit_bps, 50);
        assert_eq!(config.monitored_tokens.len(), 2);
        assert_eq!(config.monitored_tokens[0].symbol, "SOL");
        assert_eq!(config.monitored_pools.len(), 1);
        assert_eq!(config.monitored_pools[0].pair, "PUMP/SOL");
    }
}
