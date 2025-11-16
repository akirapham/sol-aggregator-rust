use eth_dex_quote::TokenPriceUpdate;
use ethers::types::Address;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Pool price information from a specific DEX pool
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PoolPrice {
    /// Token contract address
    pub token_address: Address,
    /// Price in ETH
    pub price_in_eth: f64,
    /// Price in USD (if available)
    pub price_in_usd: Option<f64>,
    /// Pool contract address
    pub pool_address: Address,
    /// DEX version (Uniswap V2, V3, V4, etc.)
    pub dex_version: String,
    /// Token decimals
    pub decimals: u8,
    /// Timestamp of last update (Unix seconds)
    pub last_updated: u64,
    /// Pool liquidity depth (for slippage estimation)
    pub liquidity_eth: Option<f64>,
    /// Liquidity in USD
    pub liquidity_usd: Option<f64>,
}

impl fmt::Display for PoolPrice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} ({}@{}) - ${:.6}/ETH",
            self.token_address, self.dex_version, self.pool_address, self.price_in_eth
        )
    }
}

/// DEX Arbitrage opportunity across multiple pools
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DexArbitrageOpportunity {
    pub token_address: Address,
    /// Buy pool (lowest price)
    pub buy_pool: TokenPriceUpdate,
    /// Sell pool (highest price)
    pub sell_pool: TokenPriceUpdate,
    /// Price difference percentage
    pub price_diff_percent: f64,
    /// Potential profit in USD
    pub potential_profit_usd: Option<f64>,
    /// Timestamp when opportunity was detected
    pub detected_at: u64,
}

impl fmt::Display for DexArbitrageOpportunity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ARB {} - Buy@${:.6} ({}@{}), Sell@${:.6} ({}@{}) = {:.2}% profit",
            self.token_address,
            self.buy_pool.price_in_usd.unwrap_or(0.0),
            self.buy_pool.dex_version,
            self.buy_pool.pool_address,
            self.sell_pool.price_in_usd.unwrap_or(0.0),
            self.sell_pool.dex_version,
            self.sell_pool.pool_address,
            self.price_diff_percent,
        )
    }
}

/// WebSocket message from amm-eth
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DexPriceMessage {
    #[serde(rename = "type")]
    pub r#type: String,
    pub data: TokenPriceUpdate,
}

/// Subscription message format for amm-eth
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DexSubscriptionMessage {
    pub topics: String,
}

/// Arbitrage trade execution parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArbitrageTrade {
    pub opportunity: DexArbitrageOpportunity,
    /// Amount of token to buy (in base units)
    pub amount_in: u128,
    /// Minimum amount to receive after slippage
    pub min_amount_out: u128,
    /// Max gas price willing to pay
    pub max_gas_price: u128,
}

/// Execution result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    pub trade: ArbitrageTrade,
    pub tx_hash: String,
    pub actual_profit_eth: f64,
    pub actual_profit_usd: Option<f64>,
    pub status: ExecutionStatus,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ExecutionStatus {
    #[serde(rename = "pending")]
    Pending,
    #[serde(rename = "confirmed")]
    Confirmed,
    #[serde(rename = "failed")]
    Failed,
    #[serde(rename = "reverted")]
    Reverted,
}

impl fmt::Display for ExecutionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExecutionStatus::Pending => write!(f, "pending"),
            ExecutionStatus::Confirmed => write!(f, "confirmed"),
            ExecutionStatus::Failed => write!(f, "failed"),
            ExecutionStatus::Reverted => write!(f, "reverted"),
        }
    }
}
