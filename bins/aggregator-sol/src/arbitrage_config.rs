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

/// Represents a triangle arbitrage path: SOL → Token1 → Token2 → SOL
#[derive(Debug, Clone)]
pub struct TrianglePath {
    pub name: String,
    /// Leg 1: base_token → intermediate_token_1
    pub leg1_from: Pubkey,
    pub leg1_to: Pubkey,
    /// Leg 2: intermediate_token_1 → intermediate_token_2
    pub leg2_from: Pubkey,
    pub leg2_to: Pubkey,
    /// Leg 3: intermediate_token_2 → base_token
    pub leg3_from: Pubkey,
    pub leg3_to: Pubkey,
}

impl TrianglePath {
    pub fn new(name: &str, tokens: [&str; 4]) -> Result<Self, String> {
        // tokens: [base, token1, token2, base] - forms a cycle
        let base = Pubkey::from_str(tokens[0]).map_err(|e| e.to_string())?;
        let token1 = Pubkey::from_str(tokens[1]).map_err(|e| e.to_string())?;
        let token2 = Pubkey::from_str(tokens[2]).map_err(|e| e.to_string())?;
        let base2 = Pubkey::from_str(tokens[3]).map_err(|e| e.to_string())?;

        if base != base2 {
            return Err("Triangle must start and end with same token".to_string());
        }

        Ok(Self {
            name: name.to_string(),
            leg1_from: base,
            leg1_to: token1,
            leg2_from: token1,
            leg2_to: token2,
            leg3_from: token2,
            leg3_to: base,
        })
    }

    /// Check if this triangle involves a specific token
    pub fn involves_token(&self, token: &Pubkey) -> bool {
        self.leg1_from == *token || self.leg1_to == *token || self.leg2_to == *token
    }

    /// Check if this triangle has a leg involving the given token pair
    /// Returns true if any leg of the triangle swaps between token_a and token_b
    pub fn involves_pair(&self, token_a: &Pubkey, token_b: &Pubkey) -> bool {
        // Check all 3 legs for the pair (order doesn't matter)
        let leg1_matches = (self.leg1_from == *token_a && self.leg1_to == *token_b)
            || (self.leg1_from == *token_b && self.leg1_to == *token_a);
        let leg2_matches = (self.leg2_from == *token_a && self.leg2_to == *token_b)
            || (self.leg2_from == *token_b && self.leg2_to == *token_a);
        let leg3_matches = (self.leg3_from == *token_a && self.leg3_to == *token_b)
            || (self.leg3_from == *token_b && self.leg3_to == *token_a);

        leg1_matches || leg2_matches || leg3_matches
    }

    /// Get all token pairs (edges) in this triangle
    /// Returns normalized pairs (smaller pubkey first) for consistent hashing
    pub fn get_pairs(&self) -> Vec<(Pubkey, Pubkey)> {
        vec![
            Self::normalize_pair(self.leg1_from, self.leg1_to),
            Self::normalize_pair(self.leg2_from, self.leg2_to),
            Self::normalize_pair(self.leg3_from, self.leg3_to),
        ]
    }

    /// Normalize a pair so smaller pubkey is first (for consistent HashMap keys)
    fn normalize_pair(a: Pubkey, b: Pubkey) -> (Pubkey, Pubkey) {
        if a < b {
            (a, b)
        } else {
            (b, a)
        }
    }
}

use std::collections::HashMap;

/// Index for fast O(1) lookup of triangle paths by token pair
/// Maps (token_a, token_b) -> Vec<path_index> for efficient filtering
#[derive(Debug, Clone)]
pub struct TrianglePathIndex {
    /// All triangle paths
    pub paths: Vec<TrianglePath>,
    /// Map from normalized (token_a, token_b) pair to indices of paths involving that pair
    pair_to_paths: HashMap<(Pubkey, Pubkey), Vec<usize>>,
}

impl TrianglePathIndex {
    /// Build index from a list of triangle paths
    pub fn new(paths: Vec<TrianglePath>) -> Self {
        let mut pair_to_paths: HashMap<(Pubkey, Pubkey), Vec<usize>> = HashMap::new();

        for (idx, path) in paths.iter().enumerate() {
            // Index each leg's pair
            for pair in path.get_pairs() {
                pair_to_paths.entry(pair).or_default().push(idx);
            }
        }

        Self {
            paths,
            pair_to_paths,
        }
    }

    /// Get all triangle paths that involve the given token pair
    /// This is O(1) lookup + cloning relevant paths
    pub fn get_paths_for_pair(&self, token_a: &Pubkey, token_b: &Pubkey) -> Vec<&TrianglePath> {
        let normalized = TrianglePath::normalize_pair(*token_a, *token_b);

        self.pair_to_paths
            .get(&normalized)
            .map(|indices| indices.iter().map(|&i| &self.paths[i]).collect())
            .unwrap_or_default()
    }

    /// Get stats about the index
    pub fn stats(&self) -> (usize, usize) {
        (self.paths.len(), self.pair_to_paths.len())
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

    /// Get all hardcoded triangle arbitrage paths
    /// These are pre-computed valid 3-hop cycles from the configured pools
    pub fn get_triangle_paths(&self) -> Vec<TrianglePath> {
        // Token addresses
        const SOL: &str = "So11111111111111111111111111111111111111112";
        const USDC: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
        const USDT: &str = "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB";
        const MSOL: &str = "mSoLzYCxHdYgdzU16g5QSh3i5K3z3KZK7ytfqcJm7So";
        const JITOSOL: &str = "J1toso1uCk3RLmjorhTtrVwY9HJ7X8V9yYac6Y7kGCPn";
        const RAY: &str = "4k3Dyjzvzp8eMZWUXbBCjEvwSkkk59S5iCNLY3QrkX6R";
        const JUP: &str = "JUPyiwrYJFskUPiHa7hkeR8VUtAeFoSYbKedZNsDvCN";
        const PUMP: &str = "pumpCmXqMfrsAkQ5r49WcJnRayYRqmXz6ae8H7H9Dfn";
        const BONK: &str = "DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263";
        const WIF: &str = "EKpQGSJtjMFqKZ9KQanSqYXRcF8fBopzLHYxdM65zcjm";
        const PYTH: &str = "HZ1JovNiVvGrGNiiYvEozEVgZ58xaU3RKwX8eACQBCt3";
        const RENDER: &str = "rndrizKT3MK1iimdxRdWabcF7Zg7AR5T4nud4EkHBof";
        const PENGU: &str = "2zMMhcVQEXDtdE6vsFS7S7D5oUodfJHE8vd1gnBouauv";

        let paths = vec![
            // =================================================================
            // LST Triangles (Highest Opportunity - staking yield arbitrage)
            // =================================================================
            TrianglePath::new("SOL→mSOL→USDC→SOL", [SOL, MSOL, USDC, SOL]),
            TrianglePath::new("SOL→jitoSOL→USDC→SOL", [SOL, JITOSOL, USDC, SOL]),
            TrianglePath::new("SOL→mSOL→jitoSOL→SOL", [SOL, MSOL, JITOSOL, SOL]),
            // Reverse LST Triangles
            TrianglePath::new("SOL→USDC→mSOL→SOL", [SOL, USDC, MSOL, SOL]),
            TrianglePath::new("SOL→USDC→jitoSOL→SOL", [SOL, USDC, JITOSOL, SOL]),
            TrianglePath::new("SOL→jitoSOL→mSOL→SOL", [SOL, JITOSOL, MSOL, SOL]),
            // =================================================================
            // DeFi Token Triangles (RAY, JUP - high volume DEX tokens)
            // =================================================================
            TrianglePath::new("SOL→RAY→USDC→SOL", [SOL, RAY, USDC, SOL]),
            TrianglePath::new("SOL→USDC→RAY→SOL", [SOL, USDC, RAY, SOL]),
            TrianglePath::new("SOL→RAY→USDT→SOL", [SOL, RAY, USDT, SOL]),
            TrianglePath::new("SOL→JUP→USDC→SOL", [SOL, JUP, USDC, SOL]),
            TrianglePath::new("SOL→USDC→JUP→SOL", [SOL, USDC, JUP, SOL]),
            TrianglePath::new("SOL→JUP→RAY→SOL", [SOL, JUP, RAY, SOL]),
            // =================================================================
            // PUMP Token Triangles ($16M+ liquidity main pool)
            // =================================================================
            TrianglePath::new("SOL→PUMP→USDC→SOL", [SOL, PUMP, USDC, SOL]),
            TrianglePath::new("SOL→USDC→PUMP→SOL", [SOL, USDC, PUMP, SOL]),
            // =================================================================
            // Meme Coin Triangles (BONK, WIF - high volatility)
            // =================================================================
            TrianglePath::new("SOL→BONK→USDC→SOL", [SOL, BONK, USDC, SOL]),
            TrianglePath::new("SOL→USDC→BONK→SOL", [SOL, USDC, BONK, SOL]),
            TrianglePath::new("SOL→WIF→USDC→SOL", [SOL, WIF, USDC, SOL]),
            TrianglePath::new("SOL→USDC→WIF→SOL", [SOL, USDC, WIF, SOL]),
            // =================================================================
            // Oracle/Infrastructure Triangles (PYTH, RENDER)
            // =================================================================
            TrianglePath::new("SOL→PYTH→USDC→SOL", [SOL, PYTH, USDC, SOL]),
            TrianglePath::new("SOL→USDC→PYTH→SOL", [SOL, USDC, PYTH, SOL]),
            TrianglePath::new("SOL→RENDER→USDC→SOL", [SOL, RENDER, USDC, SOL]),
            TrianglePath::new("SOL→USDC→RENDER→SOL", [SOL, USDC, RENDER, SOL]),
            // =================================================================
            // NFT/Gaming Triangles (PENGU - high volume meme)
            // =================================================================
            TrianglePath::new("SOL→PENGU→USDC→SOL", [SOL, PENGU, USDC, SOL]),
            TrianglePath::new("SOL→USDC→PENGU→SOL", [SOL, USDC, PENGU, SOL]),
            TrianglePath::new("SOL→PENGU→jitoSOL→SOL", [SOL, PENGU, JITOSOL, SOL]),
            // =================================================================
            // Cross-token Triangles (for price discrepancies between related tokens)
            // =================================================================
            TrianglePath::new("SOL→BONK→WIF→SOL", [SOL, BONK, WIF, SOL]),
            TrianglePath::new("SOL→mSOL→USDT→SOL", [SOL, MSOL, USDT, SOL]),
            TrianglePath::new("SOL→jitoSOL→USDT→SOL", [SOL, JITOSOL, USDT, SOL]),
        ];

        // Filter to only valid paths (parsing succeeded)
        paths.into_iter().filter_map(|r| r.ok()).collect()
    }

    /// Build an indexed lookup table for fast triangle path retrieval by token pair
    /// This creates a HashMap that maps each token pair to the triangle paths that use it
    pub fn build_triangle_index(&self) -> TrianglePathIndex {
        let paths = self.get_triangle_paths();
        TrianglePathIndex::new(paths)
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
