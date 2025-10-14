pub mod bitget;
pub mod bybit;
pub mod gate;
pub mod kucoin;
pub mod mexc;
use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterAddressType {
    Ethereum,
    Solana,
}

/// Represents the trading and deposit status of a token on an exchange
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenStatus {
    pub symbol: String,                   // Trading pair symbol (e.g., "BTCUSDT")
    pub base_asset: String,               // Base currency (e.g., "BTC")
    pub contract_address: Option<String>, // Token contract address
    pub is_trading: bool,                 // Whether trading is enabled
    pub is_deposit_enabled: bool,         // Whether deposits are enabled on the correct network
    pub network_verified: bool, // Whether network matches filter (ERC20 for Ethereum, Solana chain for Solana)
    pub last_updated: u64,      // Unix timestamp of last update
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenPrice {
    pub symbol: String,
    pub price: f64,
}

/// Represents a balance entry in the portfolio
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Balance {
    pub asset: String, // Asset symbol (e.g., "BTC", "USDT", "LINK")
    pub free: f64,     // Available balance
    pub locked: f64,   // Locked balance (in orders, etc.)
    pub total: f64,    // Total balance (free + locked)
}

/// Account balances grouped by type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountBalances {
    pub balances: Vec<Balance>, // All non-zero balances in this account
    pub total_usdt_value: f64,  // Total value in USDT for this account
}

/// Portfolio summary across all assets
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Portfolio {
    pub exchange: String,         // Exchange name
    pub trading: AccountBalances, // Trading account balances (UNIFIED for Bybit, trade for KuCoin, spot for MEXC)
    pub funding: AccountBalances, // Funding account balances (FUND for Bybit, main for KuCoin, same as trading for MEXC)
    pub total_usdt_value: f64,    // Total portfolio value in USDT (trading + funding)
    pub timestamp: u64,           // Unix timestamp when portfolio was fetched
}

#[async_trait]
#[allow(dead_code)]
pub trait PriceProvider {
    async fn get_price(&self, symbol: &str) -> Option<TokenPrice>;
    async fn get_all_prices(&self) -> Vec<TokenPrice>;
    async fn get_prices(&self, mints: &Vec<String>) -> Vec<Option<TokenPrice>>;
    async fn start(&self) -> Result<()>;
    fn get_price_provider_name(&self) -> &'static str;

    /// Check if a token is safe for arbitrage:
    /// - Trading must be enabled
    /// - Deposits must be enabled on the correct network (ERC20 for Ethereum, Solana chain for Solana)
    /// Returns true only if ALL conditions are met
    async fn is_token_safe_for_arbitrage(
        &self,
        symbol: &str,
        contract_address: Option<&str>,
    ) -> bool;

    /// Get the status of a token (trading, deposit, network verification)
    async fn get_token_status(
        &self,
        symbol: &str,
        contract_address: Option<&str>,
    ) -> Option<TokenStatus>;

    /// Refresh the token status cache and return list of safe trading pair symbols
    /// (should be called periodically, e.g., every 12 hours)
    /// Returns: Vec of trading pair symbols that passed all safety checks
    async fn refresh_token_status(&self) -> Result<Vec<String>>;

    /// Get deposit address for a token on the exchange
    /// - `symbol`: Base asset symbol (e.g., "LINK")
    /// - `address_type`: Either Ethereum (ERC20) or Solana (SPL)
    /// Returns: Deposit address for the specified token and network
    async fn get_deposit_address(
        &self,
        symbol: &str,
        address_type: FilterAddressType,
    ) -> Result<String>;

    /// Sell tokens for USDT on the exchange
    /// - `symbol`: Trading pair symbol (e.g., "LINKUSDT", "LINK_USDT", etc.)
    /// - `amount`: Amount of tokens to sell
    /// Returns: (order_id, executed_quantity, usdt_received)
    async fn sell_token_for_usdt(&self, symbol: &str, amount: f64) -> Result<(String, f64, f64)>;

    /// Withdraw USDT to an external wallet
    /// - `address`: Destination wallet address
    /// - `amount`: Amount of USDT to withdraw
    /// - `address_type`: Either Ethereum (ERC20) or Solana (SPL)
    /// Returns: withdrawal_id
    async fn withdraw_usdt(
        &self,
        address: &str,
        amount: f64,
        address_type: FilterAddressType,
    ) -> Result<String>;

    /// Get account portfolio/balances on the exchange
    /// Returns: Portfolio with all non-zero balances and total USDT value
    async fn get_portfolio(&self) -> Result<Portfolio>;

    /// Transfer all assets to trading account (UNIFIED/SPOT for trading)
    /// This prepares assets for selling/trading
    /// For exchanges without separate accounts (like MEXC), this is a no-op
    /// - `coin`: Optional specific coin to transfer, None = all coins
    /// Returns: Number of transfers executed
    async fn transfer_all_to_trading(&self, coin: Option<&str>) -> Result<u32>;

    /// Transfer all assets to funding account (for withdrawal)
    /// This prepares assets for withdrawal
    /// For exchanges without separate accounts (like MEXC), this is a no-op
    /// - `coin`: Optional specific coin to transfer, None = all coins
    /// Returns: Number of transfers executed
    async fn transfer_all_to_funding(&self, coin: Option<&str>) -> Result<u32>;

    async fn get_token_symbol_for_contract_address(&self, contract_address: &str)
        -> Option<String>;

    /// Get the quantity precision (decimal places) for a trading pair
    /// This is needed to round quantities correctly before placing orders
    /// - `symbol`: Base asset symbol (e.g., "LINK")
    /// Returns: Number of decimal places allowed for quantity (e.g., 2, 4, 6)
    async fn get_quantity_precision(&self, symbol: &str) -> Result<u32>;
}
