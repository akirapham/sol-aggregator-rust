use anchor_lang::prelude::*;

/// Main aggregator program state
#[account]
pub struct AggregatorState {
    pub admin: Pubkey,
    pub fee_rate: u64, // Fee rate in basis points
    pub total_fees_collected: u64,
    pub total_swaps_executed: u64,
    pub total_volume: u64,
    pub is_paused: bool,
    pub config: AggregatorConfig,
    pub bump: u8,
}

impl Space for AggregatorState {
    const INIT_SPACE: usize = 8 + // discriminator
        32 + // admin
        8 + // fee_rate
        8 + // total_fees_collected
        8 + // total_swaps_executed
        8 + // total_volume
        1 + // is_paused
        AggregatorConfig::INIT_SPACE + // config
        1; // bump
}

/// Aggregator configuration
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq)]
pub struct AggregatorConfig {
    pub max_slippage: u64, // in basis points
    pub max_routes: u8,
    pub min_liquidity_threshold: u64,
    pub price_impact_threshold: u64, // in basis points
    pub mev_protection: MevProtectionConfig,
    pub supported_dexs: Vec<DexType>,
}

impl Space for AggregatorConfig {
    const INIT_SPACE: usize = 8 + // max_slippage
        1 + // max_routes
        8 + // min_liquidity_threshold
        8 + // price_impact_threshold
        MevProtectionConfig::INIT_SPACE +
        4 + // supported_dexs length
        32; // supported_dexs (max 8 DEXs)
}

/// MEV protection configuration
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq)]
pub struct MevProtectionConfig {
    pub max_slippage_tolerance: u64, // in basis points
    pub min_liquidity_threshold: u64,
    pub max_mev_risk_tolerance: MevRisk,
    pub use_private_mempool: bool,
}

impl Space for MevProtectionConfig {
    const INIT_SPACE: usize = 8 + // max_slippage_tolerance
        8 + // min_liquidity_threshold
        1 + // max_mev_risk_tolerance
        1; // use_private_mempool
}

/// MEV risk levels
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq)]
pub enum MevRisk {
    Low,
    Medium,
    High,
    Critical,
}

/// DEX types supported by the aggregator
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq, Hash)]
pub enum DexType {
    PumpFun,
    PumpFunSwap,
    Raydium,
    RaydiumCpmm,
    Orca,
    Jupiter,
    // Add more DEXs here as needed
}

/// Swap parameters from the aggregator
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq)]
pub struct SwapParams {
    pub input_token: Pubkey,
    pub output_token: Pubkey,
    pub input_amount: u64,
    pub min_output_amount: u64,
    pub slippage_tolerance: u64, // in basis points
    pub user_wallet: Pubkey,
    pub priority: ExecutionPriority,
    pub deadline: i64, // Unix timestamp
}

/// Execution priority levels
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq)]
pub enum ExecutionPriority {
    Low,
    Medium,
    High,
    Critical,
}

/// Swap route from the aggregator
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq)]
pub struct SwapRoute {
    pub dex: DexType,
    pub input_token: Pubkey,
    pub output_token: Pubkey,
    pub input_amount: u64,
    pub expected_output_amount: u64,
    pub price_impact: u64, // in basis points
    pub fee: u64,
    pub route_path: Vec<Pubkey>, // For multi-hop swaps
    pub gas_cost: u64,
    pub execution_time_ms: u64,
    pub mev_risk: MevRisk,
    pub liquidity_depth: u64,
    pub dex_program_id: Pubkey, // The actual DEX program ID
    pub dex_instruction_data: Vec<u8>, // Pre-built instruction data
}

/// Split route for split swaps
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq)]
pub struct SplitRoute {
    pub route: SwapRoute,
    pub split_percentage: u64, // in basis points (e.g., 5000 = 50%)
    pub split_amount: u64,
}

/// Swap execution result
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq)]
pub struct SwapResult {
    pub success: bool,
    pub actual_output_amount: u64,
    pub fee_paid: u64,
    pub gas_used: u64,
    pub execution_time_ms: u64,
    pub price_impact_actual: u64,
    pub error_code: Option<u32>,
    pub dex_used: DexType,
}

/// Fee collection account
#[account]
pub struct FeeCollection {
    pub total_fees: u64,
    pub last_collected: i64,
    pub bump: u8,
}

impl Space for FeeCollection {
    const INIT_SPACE: usize = 8 + // discriminator
        8 + // total_fees
        8 + // last_collected
        1; // bump
}

/// User fee tracking
#[account]
pub struct UserFeeTracking {
    pub user: Pubkey,
    pub total_fees_paid: u64,
    pub total_swaps: u64,
    pub last_payment: i64,
    pub bump: u8,
}

impl Space for UserFeeTracking {
    const INIT_SPACE: usize = 8 + // discriminator
        32 + // user
        8 + // total_fees_paid
        8 + // total_swaps
        8 + // last_payment
        1; // bump
}

/// Swap execution log for analytics
#[account]
pub struct SwapLog {
    pub user: Pubkey,
    pub input_token: Pubkey,
    pub output_token: Pubkey,
    pub input_amount: u64,
    pub output_amount: u64,
    pub dex_used: DexType,
    pub fee_paid: u64,
    pub timestamp: i64,
    pub success: bool,
    pub bump: u8,
}

impl Space for SwapLog {
    const INIT_SPACE: usize = 8 + // discriminator
        32 + // user
        32 + // input_token
        32 + // output_token
        8 + // input_amount
        8 + // output_amount
        1 + // dex_used
        8 + // fee_paid
        8 + // timestamp
        1 + // success
        1; // bump
}