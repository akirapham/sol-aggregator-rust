use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Configuration for a single DEX on a specific chain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DexConfig {
    /// Router contract address (optional — some DEXes like Camelot Algebra have no router)
    #[serde(default)]
    pub router: Option<String>,
    /// Factory contract address (for V2 style DEXes)
    #[serde(default)]
    pub factory: Option<String>,
    /// Quoter contract address (for V3 style DEXes)
    #[serde(default)]
    pub quoter: Option<String>,
    /// Vault address (for V4 style DEXes)
    #[serde(default)]
    pub vault: Option<String>,
    /// Position manager address (for V4 style DEXes)
    #[serde(default)]
    pub position_manager: Option<String>,
    /// Fee tiers supported by this DEX (e.g., [100, 500, 3000, 10000])
    #[serde(default)]
    pub fee_tiers: Vec<u32>,
    /// Fee basis points for swaps (e.g., 30 for 0.3%)
    #[serde(default)]
    pub fee_bps: u32,
}

/// Configuration for all DEXes on a specific chain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainConfig {
    /// Chain ID (1 for Ethereum, 137 for Polygon, etc.)
    pub chain_id: u64,
    /// Chain name (ethereum, polygon, arbitrum, etc.)
    pub chain_name: String,
    /// RPC endpoint
    pub rpc_url: String,
    /// Base tokens that can be borrowed for flashloan arbitrage
    #[serde(default)]
    pub base_tokens: Vec<(String, bool)>, // (token address, is_stable)
    /// Quote Router contract deployment address
    #[serde(default)]
    pub quote_router: Option<String>,
    /// DEXes configured for this chain, keyed by DEX name
    pub dexes: HashMap<String, DexConfig>,
}

/// Complete configuration for all chains
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DexConfiguration {
    /// Configurations for each chain, keyed by chain name
    pub chains: HashMap<String, ChainConfig>,
}

impl DexConfiguration {
    /// Load configuration from a TOML file
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn std::error::Error>> {
        let content = fs::read_to_string(path)?;
        let config: DexConfiguration = toml::from_str(&content)?;
        Ok(config)
    }

    /// Load configuration from the default eth_dex_config.toml file in the eth-dex-quote crate
    pub fn load() -> Result<Self, Box<dyn std::error::Error>> {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let config_path = format!("{}/eth_dex_config.toml", manifest_dir);
        Self::from_file(&config_path)
    }

    /// Get configuration for a specific chain
    pub fn get_chain(&self, chain_name: &str) -> Option<&ChainConfig> {
        self.chains.get(chain_name)
    }

    /// Get DEX configuration on a specific chain
    pub fn get_dex_on_chain(&self, chain_name: &str, dex_name: &str) -> Option<&DexConfig> {
        self.chains
            .get(chain_name)
            .and_then(|chain| chain.dexes.get(dex_name))
    }

    /// Get all DEX names on a specific chain
    pub fn get_dex_names_on_chain(&self, chain_name: &str) -> Vec<String> {
        self.chains
            .get(chain_name)
            .map(|chain| chain.dexes.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Get all chain names
    pub fn get_chain_names(&self) -> Vec<String> {
        self.chains.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dex_config_serialization() {
        let config = DexConfig {
            router: Some("0x1111111254fb6c44bac0bed2854e76f90643097d".to_string()),
            factory: Some("0x5c69bee701ef814a2b6a3edd4b1652cb9cc5aa6f".to_string()),
            quoter: None,
            vault: None,
            position_manager: None,
            fee_tiers: vec![100, 500, 3000, 10000],
            fee_bps: 30,
        };

        let toml_str = toml::to_string(&config).unwrap();
        let parsed: DexConfig = toml::from_str(&toml_str).unwrap();

        assert_eq!(config.router, parsed.router);
        assert_eq!(config.fee_bps, parsed.fee_bps);
    }

    #[test]
    fn test_load_configuration_from_file() {
        let config = DexConfiguration::load();

        assert!(
            config.is_ok(),
            "Failed to load configuration: {:?}",
            config.err()
        );

        let config = config.unwrap();
        assert!(
            !config.chains.is_empty(),
            "Configuration should have at least one chain"
        );
    }

    #[test]
    fn test_get_chain_configuration() {
        let config = DexConfiguration::load().expect("Failed to load configuration");

        let ethereum = config.get_chain("ethereum");
        assert!(ethereum.is_some(), "Ethereum chain should be configured");

        let ethereum = ethereum.unwrap();
        assert_eq!(ethereum.chain_id, 1, "Ethereum chain ID should be 1");
        assert_eq!(ethereum.chain_name, "ethereum");
    }

    #[test]
    fn test_get_dex_on_chain() {
        let config = DexConfiguration::load().expect("Failed to load configuration");

        let uniswap_v2 = config.get_dex_on_chain("ethereum", "uniswap_v2");
        assert!(
            uniswap_v2.is_some(),
            "Uniswap V2 should be configured on Ethereum"
        );

        let uniswap_v2 = uniswap_v2.unwrap();
        assert!(
            uniswap_v2.router.is_some(),
            "Uniswap V2 router should have an address"
        );
        assert!(
            uniswap_v2.factory.is_some(),
            "Uniswap V2 should have a factory address"
        );
    }

    #[test]
    fn test_get_all_dex_names_on_chain() {
        let config = DexConfiguration::load().expect("Failed to load configuration");

        let dex_names = config.get_dex_names_on_chain("ethereum");
        assert!(
            !dex_names.is_empty(),
            "Ethereum should have at least one DEX configured"
        );
        assert!(
            dex_names.contains(&"uniswap_v2".to_string()),
            "Uniswap V2 should be in the list"
        );
    }

    #[test]
    fn test_get_all_chain_names() {
        let config = DexConfiguration::load().expect("Failed to load configuration");

        let chain_names = config.get_chain_names();
        assert!(
            !chain_names.is_empty(),
            "Configuration should have at least one chain"
        );
        assert!(
            chain_names.contains(&"ethereum".to_string()),
            "Ethereum should be in the list"
        );
    }

    #[test]
    fn test_uniswap_v3_configuration() {
        let config = DexConfiguration::load().expect("Failed to load configuration");

        let uniswap_v3 = config.get_dex_on_chain("ethereum", "uniswap_v3");
        assert!(
            uniswap_v3.is_some(),
            "Uniswap V3 should be configured on Ethereum"
        );

        let uniswap_v3 = uniswap_v3.unwrap();
        assert!(
            uniswap_v3.quoter.is_some(),
            "Uniswap V3 should have a quoter address"
        );
        assert!(
            !uniswap_v3.fee_tiers.is_empty(),
            "Uniswap V3 should have fee tiers configured"
        );
    }

    #[test]
    fn test_uniswap_v4_configuration() {
        let config = DexConfiguration::load().expect("Failed to load configuration");

        let uniswap_v4 = config.get_dex_on_chain("ethereum", "uniswap_v4");
        assert!(
            uniswap_v4.is_some(),
            "Uniswap V4 should be configured on Ethereum"
        );

        let uniswap_v4 = uniswap_v4.unwrap();
        assert!(
            uniswap_v4.router.is_some(),
            "Uniswap V4 should have a router/quote address"
        );
        assert!(
            !uniswap_v4.fee_tiers.is_empty(),
            "Uniswap V4 should have fee tiers configured"
        );
    }

    #[test]
    fn test_sushiswap_configuration() {
        let config = DexConfiguration::load().expect("Failed to load configuration");

        let sushiswap = config.get_dex_on_chain("ethereum", "sushiswap_v2");
        assert!(
            sushiswap.is_some(),
            "Sushiswap V2 should be configured on Ethereum"
        );

        let sushiswap = sushiswap.unwrap();
        assert!(
            sushiswap.factory.is_some(),
            "Sushiswap V2 should have a factory address"
        );
    }

    #[test]
    fn test_rpc_url_is_set() {
        let config = DexConfiguration::load().expect("Failed to load configuration");

        let ethereum = config.get_chain("ethereum").expect("Ethereum should exist");
        assert!(
            !ethereum.rpc_url.is_empty(),
            "Ethereum RPC URL should be configured"
        );
        assert!(
            ethereum.rpc_url.starts_with("http"),
            "RPC URL should be a valid HTTP(S) URL"
        );
    }

    #[test]
    fn test_chain_id_matches_name() {
        let config = DexConfiguration::load().expect("Failed to load configuration");

        let ethereum = config.get_chain("ethereum").expect("Ethereum should exist");
        assert_eq!(ethereum.chain_id, 1, "Ethereum chain ID should be 1");
        assert_eq!(ethereum.chain_name, "ethereum", "Chain name should match");
    }
}
