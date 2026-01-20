use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;

use crate::{
    error::Result,
    pool_data_types::{
        BonkPoolUpdate, DbcPoolUpdate, DexType, MeteoraDammV2PoolUpdate, MeteoraDlmmPoolUpdate,
        PoolUpdateEventType, PumpSwapPoolUpdate, PumpfunPoolUpdate, RaydiumAmmV4PoolUpdate,
        RaydiumClmmPoolUpdate, RaydiumCpmmPoolUpdate, WhirlpoolPoolUpdate,
    },
};

/// Represents a token with its metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Token {
    pub address: Pubkey,
    pub decimals: u8,
    pub is_token_2022: bool,
    pub symbol: Option<String>,
    pub name: Option<String>,
    pub logo_uri: Option<String>,
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
            DexType::MeteoraDbc => write!(f, "Meteora DBC"),
            DexType::MeteoraDammV2 => write!(f, "Meteora DammV2"),
            DexType::MeteoraDlmm => write!(f, "Meteora DLMM"),
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
    // DEX enable/disable flags
    pub enable_pumpfun: bool,
    pub enable_pumpfun_swap: bool,
    pub enable_bonk: bool,
    pub enable_raydium_cpmm: bool,
    pub enable_raydium_clmm: bool,
    pub enable_raydium_amm_v4: bool,
    pub enable_orca_whirlpools: bool,
    pub enable_meteora_dbc: bool,
    pub enable_meteora_dammv2: bool,
    pub enable_meteora_dlmm: bool,
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

#[allow(unused)]
#[derive(Debug, Clone)]
pub enum PoolUpdateEvent {
    Pumpfun(PumpfunPoolUpdate),
    PumpSwap(PumpSwapPoolUpdate),
    Raydium(RaydiumAmmV4PoolUpdate),
    RaydiumCpmm(RaydiumCpmmPoolUpdate),
    Bonk(BonkPoolUpdate),
    RaydiumClmm(Box<RaydiumClmmPoolUpdate>),
    MeteoraDbc(DbcPoolUpdate),
    MeteoraDammV2(MeteoraDammV2PoolUpdate),
    MeteoraDlmm(MeteoraDlmmPoolUpdate),
    Whirlpool(Box<WhirlpoolPoolUpdate>),
}

impl PoolUpdateEvent {
    pub fn address(&self) -> Pubkey {
        match self {
            PoolUpdateEvent::Pumpfun(update) => update.address,
            PoolUpdateEvent::PumpSwap(update) => update.address,
            PoolUpdateEvent::Raydium(update) => update.address,
            PoolUpdateEvent::RaydiumCpmm(update) => update.address,
            PoolUpdateEvent::Bonk(update) => update.address,
            PoolUpdateEvent::RaydiumClmm(update) => update.address,
            PoolUpdateEvent::MeteoraDbc(update) => update.address,
            PoolUpdateEvent::Whirlpool(update) => update.address,
            PoolUpdateEvent::MeteoraDammV2(update) => update.address,
            PoolUpdateEvent::MeteoraDlmm(update) => update.address,
        }
    }

    pub fn is_account_state_update(&self) -> bool {
        match self {
            PoolUpdateEvent::Pumpfun(update) => update.is_account_state_update,
            PoolUpdateEvent::PumpSwap(update) => update.is_account_state_update,
            PoolUpdateEvent::Raydium(update) => update.is_account_state_update,
            PoolUpdateEvent::RaydiumCpmm(update) => update.is_account_state_update,
            PoolUpdateEvent::Bonk(update) => update.is_account_state_update,
            PoolUpdateEvent::RaydiumClmm(update) => update.is_account_state_update,
            PoolUpdateEvent::MeteoraDbc(update) => update.is_account_state_update,
            PoolUpdateEvent::Whirlpool(update) => update.is_account_state_update,
            PoolUpdateEvent::MeteoraDammV2(update) => update.is_account_state_update,
            PoolUpdateEvent::MeteoraDlmm(update) => update.is_account_state_update,
        }
    }

    pub fn get_pool_update_event_type(&self) -> PoolUpdateEventType {
        match self {
            PoolUpdateEvent::Pumpfun(update) => update.pool_update_event_type,
            PoolUpdateEvent::PumpSwap(update) => update.pool_update_event_type,
            PoolUpdateEvent::Raydium(update) => update.pool_update_event_type,
            PoolUpdateEvent::RaydiumCpmm(update) => update.pool_update_event_type,
            PoolUpdateEvent::Bonk(update) => update.pool_update_event_type,
            PoolUpdateEvent::RaydiumClmm(update) => update.pool_update_event_type,
            PoolUpdateEvent::MeteoraDbc(update) => update.pool_update_event_type,
            PoolUpdateEvent::Whirlpool(update) => update.pool_update_event_type,
            PoolUpdateEvent::MeteoraDammV2(update) => update.pool_update_event_type,
            PoolUpdateEvent::MeteoraDlmm(update) => update.pool_update_event_type,
        }
    }

    pub fn recv_us(&self) -> u64 {
        match self {
            PoolUpdateEvent::Pumpfun(update) => update.last_updated,
            PoolUpdateEvent::PumpSwap(update) => update.last_updated,
            PoolUpdateEvent::Raydium(update) => update.last_updated,
            PoolUpdateEvent::RaydiumCpmm(update) => update.last_updated,
            PoolUpdateEvent::Bonk(update) => update.last_updated,
            PoolUpdateEvent::RaydiumClmm(update) => update.last_updated,
            PoolUpdateEvent::MeteoraDbc(update) => update.last_updated,
            PoolUpdateEvent::Whirlpool(update) => update.last_updated,
            PoolUpdateEvent::MeteoraDammV2(update) => update.last_updated,
            PoolUpdateEvent::MeteoraDlmm(update) => update.last_updated,
        }
    }

    pub fn get_additional_event_type(&self) -> i32 {
        match self {
            PoolUpdateEvent::Pumpfun(update) => update.additional_event_type,
            PoolUpdateEvent::PumpSwap(update) => update.additional_event_type,
            PoolUpdateEvent::Raydium(update) => update.additional_event_type,
            PoolUpdateEvent::RaydiumCpmm(update) => update.additional_event_type,
            PoolUpdateEvent::Bonk(update) => update.additional_event_type,
            PoolUpdateEvent::RaydiumClmm(update) => update.additional_event_type,
            PoolUpdateEvent::MeteoraDbc(update) => update.additional_event_type,
            PoolUpdateEvent::Whirlpool(update) => update.additional_event_type,
            PoolUpdateEvent::MeteoraDammV2(update) => update.additional_event_type,
            PoolUpdateEvent::MeteoraDlmm(update) => update.additional_event_type,
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
            // Enable all DEXes by default
            enable_pumpfun: true,
            enable_pumpfun_swap: true,
            enable_bonk: true,
            enable_raydium_cpmm: true,
            enable_raydium_clmm: true,
            enable_raydium_amm_v4: true,
            enable_orca_whirlpools: true,
            enable_meteora_dbc: true,
            enable_meteora_dammv2: true,
            enable_meteora_dlmm: true,
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
