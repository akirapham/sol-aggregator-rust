use anchor_client::solana_sdk::commitment_config::CommitmentLevel;
use rust_decimal::Decimal;
use std::env;

use crate::error::{DexAggregatorError, Result};
use crate::types::{
    AggregatorConfig, GasConfig, MevProtectionConfig, MevRisk, SmartRoutingConfig, SplitConfig,
};

/// Configuration loader that reads from environment variables
pub struct ConfigLoader;

impl ConfigLoader {
    /// Load configuration from environment variables
    pub fn load() -> Result<AggregatorConfig> {
        // Load .env file if it exists
        dotenv::dotenv().ok();

        Ok(AggregatorConfig {
            rpc_url: Self::get_string("RPC_URL", "https://api.mainnet-beta.solana.com")?,
            yellowstone_grpc_url: Self::get_string(
                "YELLOWSTONE_GRPC_URL",
                "https://solana-yellowstone-grpc.publicnode.com:443",
            )?,
            backup_grpc_url: match env::var("BACKUP_GRPC_URL") {
                Ok(url) if !url.trim().is_empty() => Some(url),
                _ => Some("https://solana-yellowstone-grpc.publicnode.com:443".to_string()),
            },
            commitment: Self::get_commitment_level()?,
            max_slippage: Self::get_decimal("MAX_SLIPPAGE", Decimal::new(5, 2))?,
            max_routes: Self::get_usize("MAX_ROUTES", 5)?,
            smart_routing: Self::load_smart_routing_config()?,
            gas_config: Self::load_gas_config()?,
            mev_protection: Self::load_mev_protection_config()?,
            split_config: Self::load_split_config()?,
        })
    }

    /// Load smart routing configuration
    fn load_smart_routing_config() -> Result<SmartRoutingConfig> {
        Ok(SmartRoutingConfig {
            enable_multi_hop: Self::get_bool("ENABLE_MULTI_HOP", true)?,
            enable_split_trading: Self::get_bool("ENABLE_SPLIT_TRADING", true)?,
            enable_arbitrage_detection: Self::get_bool("ENABLE_ARBITRAGE_DETECTION", true)?,
            max_hops: Self::get_usize("MAX_HOPS", 3)?,
            min_liquidity_threshold: Self::get_u64("MIN_LIQUIDITY_THRESHOLD", 1000000)?,
            price_impact_threshold: Self::get_decimal(
                "PRICE_IMPACT_THRESHOLD",
                Decimal::new(5, 2),
            )?,
            enable_route_simulation: Self::get_bool("ENABLE_ROUTE_SIMULATION", true)?,
            enable_dynamic_slippage: Self::get_bool("ENABLE_DYNAMIC_SLIPPAGE", true)?,
        })
    }

    /// Load gas configuration
    fn load_gas_config() -> Result<GasConfig> {
        Ok(GasConfig {
            max_gas_price: Self::get_u64("MAX_GAS_PRICE", 5000)?,
            priority_fee: Self::get_u64("PRIORITY_FEE", 1000)?,
            gas_limit: Self::get_u64("GAS_LIMIT", 200000)?,
            optimize_for_speed: Self::get_bool("OPTIMIZE_FOR_SPEED", false)?,
        })
    }

    /// Load MEV protection configuration
    fn load_mev_protection_config() -> Result<MevProtectionConfig> {
        Ok(MevProtectionConfig {
            use_private_mempool: Self::get_bool("USE_PRIVATE_MEMPOOL", false)?,
            max_slippage_tolerance: Self::get_decimal(
                "MAX_SLIPPAGE_TOLERANCE",
                Decimal::new(1, 2),
            )?,
            min_liquidity_threshold: Self::get_u64("MIN_LIQUIDITY_THRESHOLD_MEV", 10000000)?,
            max_mev_risk_tolerance: Self::get_mev_risk("MAX_MEV_RISK_TOLERANCE", MevRisk::Medium)?,
            use_flashloan_protection: Self::get_bool("USE_FLASHLOAN_PROTECTION", false)?,
        })
    }

    /// Load split trading configuration
    fn load_split_config() -> Result<SplitConfig> {
        Ok(SplitConfig {
            max_splits: Self::get_usize("MAX_SPLITS", 3)?,
            min_split_amount: Self::get_u64("MIN_SPLIT_AMOUNT", 1000000)?,
            max_price_impact_per_split: Self::get_decimal(
                "MAX_PRICE_IMPACT_PER_SPLIT",
                Decimal::new(2, 2),
            )?,
            prefer_low_mev: Self::get_bool("PREFER_LOW_MEV", true)?,
        })
    }

    /// Get commitment level from environment
    fn get_commitment_level() -> Result<CommitmentLevel> {
        let level = Self::get_string("COMMITMENT_LEVEL", "processed")?.to_lowercase();
        match level.as_str() {
            "processed" => Ok(CommitmentLevel::Processed),
            "confirmed" => Ok(CommitmentLevel::Confirmed),
            "finalized" => Ok(CommitmentLevel::Finalized),
            _ => Err(DexAggregatorError::SerializationError(format!(
                "Invalid commitment level: {}",
                level
            ))),
        }
    }

    /// Get MEV risk from environment
    fn get_mev_risk(key: &str, default: MevRisk) -> Result<MevRisk> {
        let value = Self::get_string(key, "")?.to_lowercase();
        match value.as_str() {
            "" => Ok(default),
            "low" => Ok(MevRisk::Low),
            "medium" => Ok(MevRisk::Medium),
            "high" => Ok(MevRisk::High),
            "critical" => Ok(MevRisk::Critical),
            _ => Err(DexAggregatorError::SerializationError(format!(
                "Invalid MEV risk level: {}",
                value
            ))),
        }
    }

    /// Get string value from environment
    fn get_string(key: &str, default: &str) -> Result<String> {
        Ok(env::var(key).unwrap_or_else(|_| default.to_string()))
    }

    /// Get boolean value from environment
    fn get_bool(key: &str, default: bool) -> Result<bool> {
        match env::var(key) {
            Ok(value) => match value.to_lowercase().as_str() {
                "true" | "1" | "yes" | "on" => Ok(true),
                "false" | "0" | "no" | "off" => Ok(false),
                _ => Err(DexAggregatorError::SerializationError(format!(
                    "Invalid boolean value for {}: {}",
                    key, value
                ))),
            },
            Err(_) => Ok(default),
        }
    }

    /// Get usize value from environment
    fn get_usize(key: &str, default: usize) -> Result<usize> {
        match env::var(key) {
            Ok(value) => value.parse().map_err(|_| {
                DexAggregatorError::SerializationError(format!(
                    "Invalid usize value for {}: {}",
                    key, value
                ))
            }),
            Err(_) => Ok(default),
        }
    }

    /// Get u64 value from environment
    fn get_u64(key: &str, default: u64) -> Result<u64> {
        match env::var(key) {
            Ok(value) => value.parse().map_err(|_| {
                DexAggregatorError::SerializationError(format!(
                    "Invalid u64 value for {}: {}",
                    key, value
                ))
            }),
            Err(_) => Ok(default),
        }
    }

    /// Get decimal value from environment
    fn get_decimal(key: &str, default: Decimal) -> Result<Decimal> {
        match env::var(key) {
            Ok(value) => value.parse().map_err(|_| {
                DexAggregatorError::SerializationError(format!(
                    "Invalid decimal value for {}: {}",
                    key, value
                ))
            }),
            Err(_) => Ok(default),
        }
    }
}
