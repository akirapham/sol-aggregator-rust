use anchor_client::solana_sdk::commitment_config::CommitmentLevel;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;

use crate::{
    error::Result,
    pool_data_types::{
        BonkPoolUpdate, DexType, PumpSwapPoolUpdate, PumpfunPoolUpdate, RaydiumAmmV4PoolUpdate, RaydiumClmmPoolUpdate, RaydiumCpmmPoolUpdate
    },
};

/// Represents a token with its metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Token {
    pub address: Pubkey,
    pub decimals: u8,
    // pub is_token_2022: bool,
}

/// Represents a swap route through a specific DEX
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwapRoute {
    pub dex: DexType,
    pub input_token: Token,
    pub output_token: Token,
    pub input_amount: u64,
    pub output_amount: u64,
    pub price_impact: Decimal,
    pub route_path: Vec<Pubkey>, // For multi-hop swaps
    pub mev_risk: MevRisk,       // MEV risk assessment
    pub liquidity_depth: u64,    // Available liquidity at this price level
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
    pub routes: Vec<SwapRoute>,
    pub total_input_amount: u64,
    pub total_output_amount: u64,
    pub total_price_impact: Decimal,
    pub execution_priority: ExecutionPriority,
    pub max_mev_risk: MevRisk,
    pub route_type: RouteType,
    pub split_ratio: Option<Vec<Decimal>>, // For split trading
}

/// Types of routing strategies
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RouteType {
    SingleHop, // Direct swap A→B
    MultiHop,  // Multi-hop swap A→B→C
    Split,     // Split across multiple DEXs
    Arbitrage, // Cross-DEX arbitrage opportunity
    Optimal,   // AI-optimized route
}

/// Multi-hop path for complex routing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiHopPath {
    pub hops: Vec<Hop>,
    pub total_input_amount: u64,
    pub total_output_amount: u64,
    pub total_fee: u64,
    pub total_gas_cost: u64,
    pub price_impact: Decimal,
    pub mev_risk: MevRisk,
}

/// Individual hop in a multi-hop path
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hop {
    pub dex: DexType,
    pub input_token: Token,
    pub output_token: Token,
    pub input_amount: u64,
    pub output_amount: u64,
    pub fee: u64,
    pub gas_cost: u64,
    pub pool_address: Pubkey,
}

/// Split trading configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SplitConfig {
    pub max_splits: usize,
    pub min_split_amount: u64,
    pub max_price_impact_per_split: Decimal,
    pub prefer_low_mev: bool,
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
    pub use_private_mempool: bool,
    pub max_slippage_tolerance: Decimal,
    pub min_liquidity_threshold: u64,
    pub max_mev_risk_tolerance: MevRisk,
    pub use_flashloan_protection: bool,
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
pub struct SwapParams {
    pub input_token: Token,
    pub output_token: Token,
    pub input_amount: u64,
    pub slippage_tolerance: Decimal, // e.g., 0.01 for 1%
    pub user_wallet: Pubkey,
    pub priority: ExecutionPriority,
}

/// Price information for a token pair
#[derive(Debug, Clone)]
pub struct PriceInfo {
    pub dex: DexType,
    pub input_token: Pubkey,
    pub output_token: Pubkey,
    pub price: Decimal,
    pub liquidity: u64,
    pub last_updated: u64, // Unix timestamp
}

/// Configuration for the aggregator
#[derive(Debug, Clone)]
pub struct AggregatorConfig {
    pub rpc_url: String,
    pub yellowstone_grpc_url: String,
    pub commitment: CommitmentLevel,
    pub max_slippage: Decimal,
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
}

impl Default for AggregatorConfig {
    fn default() -> Self {
        Self {
            rpc_url: "https://api.mainnet-beta.solana.com".to_string(),
            yellowstone_grpc_url: "http://grpc.solana.com:10000".to_string(),
            commitment: CommitmentLevel::Processed,
            max_slippage: Decimal::new(5, 2), // 5%
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
            use_private_mempool: false,
            max_slippage_tolerance: Decimal::new(1, 2), // 1%
            min_liquidity_threshold: 10000000,          // 10M lamports
            max_mev_risk_tolerance: MevRisk::Medium,
            use_flashloan_protection: false,
        }
    }
}

impl Default for SplitConfig {
    fn default() -> Self {
        Self {
            max_splits: 3,
            min_split_amount: 1000000,                      // 1M lamports
            max_price_impact_per_split: Decimal::new(2, 2), // 2%
            prefer_low_mev: true,
        }
    }
}

impl AggregatorConfig {
    /// Create configuration from environment variables
    pub fn from_env() -> Result<Self> {
        crate::config::ConfigLoader::load()
    }
}
