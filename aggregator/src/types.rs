use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;

use crate::{
    error::Result,
    pool_data_types::{
        BonkPoolUpdate, DexType, PumpSwapPoolUpdate, PumpfunPoolUpdate, RaydiumAmmV4PoolUpdate,
        RaydiumClmmPoolUpdate, RaydiumCpmmPoolUpdate,
    },
};

/// Represents a token with its metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Token {
    pub address: Pubkey,
    pub decimals: u8,
    pub is_token_2022: bool,
}

/// Represents a swap route through a specific DEX
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwapStep {
    pub dex: DexType,
    pub input_token: String,
    pub output_token: String,
    pub pool_address: String,
    pub input_amount: u64,
    pub output_amount: u64,
    pub percent: u64,
}

/// MEV risk assessment levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum MevRisk {
    Low,      // Low MEV risk (private mempool, high liquidity)
    Medium,   // Medium MEV risk (standard mempool, moderate liquidity)
    High,     // High MEV risk (public mempool, low liquidity)
    Critical, // Critical MEV risk (very low liquidity, high value)
}

/// Represents the best route found by the aggregator
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BestRoute {
    pub swap_plan: Vec<SwapStep>,
    pub input_amount: u64,
    pub output_amount: u64,
    pub price_impact: f64,
    pub execution_priority: ExecutionPriority,
    pub max_mev_risk: MevRisk,
}

/// Split trading configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SplitConfig {
    pub max_splits: usize,
    pub min_split_value: f64,
}

/// Gas optimization settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GasConfig {
    pub max_gas_price: u64,       // Max gas price in lamports
    pub priority_fee: u64,        // Priority fee in lamports
    pub gas_limit: u64,           // Gas limit for transactions
    pub optimize_for_speed: bool, // Optimize for speed vs cost
}

/// MEV protection settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MevProtectionConfig {
    pub min_liquidity_threshold: u64,
    pub max_mev_risk_tolerance: MevRisk,
}

impl std::fmt::Display for DexType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DexType::PumpFun => write!(f, "PumpFun"),
            DexType::PumpFunSwap => write!(f, "PumpFun Swap"),
            DexType::Raydium => write!(f, "Raydium"),
            DexType::RaydiumCpmm => write!(f, "Raydium CPMM"),
            DexType::Orca => write!(f, "Orca"),
            DexType::Bonk => write!(f, "Bonk"),
            DexType::RaydiumClmm => write!(f, "Raydium CLMM"),
        }
    }
}

/// Execution priority for different routes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionPriority {
    High,   // Fastest execution, higher fees
    Medium, // Balanced execution
    Low,    // Slowest execution, lower fees
}

/// Swap parameters for executing a trade
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct SwapParams {
    pub input_token: Token,
    pub output_token: Token,
    pub input_amount: u64,
    pub slippage_bps: u16,
    pub user_wallet: Pubkey,
    pub priority: ExecutionPriority,
}

/// Configuration for the aggregator
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct AggregatorConfig {
    pub rpc_url: String,
    pub yellowstone_grpc_url: String,
    pub backup_grpc_url: Option<String>,
    pub max_routes: usize,
    pub smart_routing: SmartRoutingConfig,
    pub gas_config: GasConfig,
    pub mev_protection: MevProtectionConfig,
    pub split_config: SplitConfig,
}

/// Smart routing configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmartRoutingConfig {
    pub enable_multi_hop: bool,
    pub enable_split_trading: bool,
    pub enable_arbitrage_detection: bool,
    pub max_hops: usize,
    pub min_liquidity_threshold: u64,
    pub price_impact_threshold: Decimal,
    pub enable_route_simulation: bool,
    pub enable_dynamic_slippage: bool,
}

#[derive(Debug, Clone)]
pub enum PoolUpdateEvent {
    PumpfunPoolUpdate(PumpfunPoolUpdate),
    PumpSwapPoolUpdate(PumpSwapPoolUpdate),
    RaydiumPoolUpdate(RaydiumAmmV4PoolUpdate),
    RaydiumCpmmPoolUpdate(RaydiumCpmmPoolUpdate),
    BonkPoolUpdate(BonkPoolUpdate),
    RaydiumClmmPoolUpdate(RaydiumClmmPoolUpdate),
}

impl PoolUpdateEvent {
    pub fn address(&self) -> Pubkey {
        match self {
            PoolUpdateEvent::PumpfunPoolUpdate(update) => update.address,
            PoolUpdateEvent::PumpSwapPoolUpdate(update) => update.address,
            PoolUpdateEvent::RaydiumPoolUpdate(update) => update.address,
            PoolUpdateEvent::RaydiumCpmmPoolUpdate(update) => update.address,
            PoolUpdateEvent::BonkPoolUpdate(update) => update.address,
            PoolUpdateEvent::RaydiumClmmPoolUpdate(update) => update.address,
        }
    }

    pub fn is_account_state_update(&self) -> bool {
        match self {
            PoolUpdateEvent::PumpfunPoolUpdate(update) => update.is_account_state_update,
            PoolUpdateEvent::PumpSwapPoolUpdate(update) => update.is_account_state_update,
            PoolUpdateEvent::RaydiumPoolUpdate(update) => update.is_account_state_update,
            PoolUpdateEvent::RaydiumCpmmPoolUpdate(update) => update.is_account_state_update,
            PoolUpdateEvent::BonkPoolUpdate(update) => update.is_account_state_update,
            PoolUpdateEvent::RaydiumClmmPoolUpdate(update) => update.is_account_state_update,
        }
    }
}

impl Default for AggregatorConfig {
    fn default() -> Self {
        Self {
            rpc_url: "https://api.mainnet-beta.solana.com".to_string(),
            yellowstone_grpc_url: "https://solana-yellowstone-grpc.publicnode.com:443".to_string(),
            backup_grpc_url: Some("https://solana-yellowstone-grpc.publicnode.com:443".to_string()),
            max_routes: 5,
            smart_routing: SmartRoutingConfig::default(),
            gas_config: GasConfig::default(),
            mev_protection: MevProtectionConfig::default(),
            split_config: SplitConfig::default(),
        }
    }
}

impl Default for SmartRoutingConfig {
    fn default() -> Self {
        Self {
            enable_multi_hop: true,
            enable_split_trading: true,
            enable_arbitrage_detection: true,
            max_hops: 3,
            min_liquidity_threshold: 1000000, // 1M lamports
            price_impact_threshold: Decimal::new(5, 2), // 5%
            enable_route_simulation: true,
            enable_dynamic_slippage: true,
        }
    }
}

impl Default for GasConfig {
    fn default() -> Self {
        Self {
            max_gas_price: 5000, // 5000 lamports
            priority_fee: 1000,  // 1000 lamports
            gas_limit: 200000,   // 200k compute units
            optimize_for_speed: false,
        }
    }
}

impl Default for MevProtectionConfig {
    fn default() -> Self {
        Self {
            min_liquidity_threshold: 4000, // 4k$
            max_mev_risk_tolerance: MevRisk::Medium,
        }
    }
}

impl Default for SplitConfig {
    fn default() -> Self {
        Self {
            max_splits: 3,
            min_split_value: 1000.0, // 1000$
        }
    }
}

impl AggregatorConfig {
    /// Create configuration from environment variables
    pub fn from_env() -> Result<Self> {
        crate::config::ConfigLoader::load()
    }
}

#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct ChainStateUpdate {
    pub slot: u64,
    pub block_time: i64,
    pub block_hash: String,
}
