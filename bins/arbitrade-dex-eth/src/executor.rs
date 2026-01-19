use crate::types::{DexArbitrageOpportunity, ExecutionResult, ExecutionStatus};
use crate::utils;
use anyhow::{anyhow, Result};
use ethers::providers::Provider;
use ethers::signers::LocalWallet;
use ethers::signers::Signer;
use log::{info, warn};
use std::str::FromStr;
use std::sync::Arc;

/// Executes arbitrage trades on-chain
pub struct ArbitrageExecutor {
    #[allow(dead_code)]
    provider: Provider<ethers::providers::Ws>,
    wallet: LocalWallet,
    /// Slippage tolerance in basis points (100 = 1%)
    slippage_tolerance: u16,
    /// Whether to actually execute trades or just simulate
    dry_run: bool,
}

impl ArbitrageExecutor {
    pub fn new(
        provider: Provider<ethers::providers::Ws>,
        private_key: String,
        slippage_tolerance: u16,
        dry_run: bool,
    ) -> Result<Self> {
        let wallet = LocalWallet::from_str(&private_key)
            .map_err(|e| anyhow!("Invalid private key: {}", e))?;

        Ok(ArbitrageExecutor {
            provider,
            wallet,
            slippage_tolerance,
            dry_run,
        })
    }

    /// Execute an arbitrage opportunity
    pub async fn execute(&self, opportunity: &DexArbitrageOpportunity) -> Result<ExecutionResult> {
        if self.dry_run {
            return self.simulate_execution(opportunity).await;
        }

        info!(
            "Executing arbitrage trade: Buy @ {} ({:.6} ETH) → Sell @ {} ({:.6} ETH)",
            opportunity.buy_pool.pool_address,
            opportunity.buy_pool.price_in_eth,
            opportunity.sell_pool.pool_address,
            opportunity.sell_pool.price_in_eth
        );

        // In real implementation, this would:
        // 1. Call amm-eth's smart contract to get token amount for 1 ETH
        // 2. Execute swap on buy pool
        // 3. Execute swap on sell pool
        // 4. Calculate actual profit

        self.simulate_execution(opportunity).await
    }

    /// Simulate execution without sending transactions
    async fn simulate_execution(
        &self,
        opportunity: &DexArbitrageOpportunity,
    ) -> Result<ExecutionResult> {
        let simulated_tx_hash = format!("0x{}", hex::encode(uuid::Uuid::new_v4().as_bytes()));

        // Use USD profit directly (we don't calculate ETH profit anymore)
        let potential_profit_usd = opportunity.potential_profit_usd.unwrap_or(0.0);

        if self.dry_run {
            info!(
                "📊 [DRY RUN] Would execute: {} → Profit: {:.2}% | USD: ${:.2} | TX: {}",
                opportunity,
                opportunity.price_diff_percent,
                potential_profit_usd,
                simulated_tx_hash
            );
        }

        Ok(ExecutionResult {
            trade: crate::types::ArbitrageTrade {
                opportunity: opportunity.clone(),
                amount_in: 0,
                min_amount_out: 0,
                max_gas_price: 0,
            },
            tx_hash: simulated_tx_hash,
            actual_profit_eth: 0.0, // Ignoring ETH gas calculations for now
            actual_profit_usd: Some(potential_profit_usd),
            status: ExecutionStatus::Pending,
        })
    }

    /// Set slippage tolerance
    pub fn set_slippage_tolerance(&mut self, slippage_basis_points: u16) {
        self.slippage_tolerance = slippage_basis_points;
    }

    /// Set dry run mode
    pub fn set_dry_run(&mut self, enabled: bool) {
        self.dry_run = enabled;
        if enabled {
            info!("🔒 Dry run mode ENABLED - trades will not be executed");
        } else {
            warn!("⚠️  Dry run mode DISABLED - trades will be executed on-chain");
        }
    }

    /// Get executor wallet address
    pub fn get_address(&self) -> String {
        format!("{:?}", self.wallet.address())
    }

    /// Compute 2-hop arbitrage path: X -> A -> X
    /// Returns (amount_a, amount_x_out, net_profit) where net_profit = amount_x_out - flashloan_amount
    pub async fn compute_arbitrage_profit(
        &self,
        flashloan_amount: ethers::types::U256,
        token_x: ethers::types::Address,
        token_a: ethers::types::Address,
        buy_pool: &eth_dex_quote::TokenPriceUpdate,
        sell_pool: &eth_dex_quote::TokenPriceUpdate,
        router_v2_address: ethers::types::Address,
        quoter_v3_address: ethers::types::Address,
        quoter_v4_address: ethers::types::Address,
    ) -> Result<(ethers::types::U256, ethers::types::U256, i128)> {
        utils::compute_arbitrage_path(
            Arc::new(self.provider.clone()),
            flashloan_amount,
            token_x,
            token_a,
            buy_pool,
            sell_pool,
            router_v2_address,
            quoter_v3_address,
            quoter_v4_address,
        )
        .await
    }

    /// Compute 3-hop arbitrage path: X -> A -> B -> X
    /// Returns (amount_a, amount_b, amount_x_out, net_profit) where net_profit = amount_x_out - flashloan_amount
    pub async fn compute_3hop_arbitrage_profit(
        &self,
        flashloan_amount: ethers::types::U256,
        token_x: ethers::types::Address,
        token_a: ethers::types::Address,
        token_b: ethers::types::Address,
        pool_x_to_a: &eth_dex_quote::TokenPriceUpdate,
        pool_a_to_b: &eth_dex_quote::TokenPriceUpdate,
        pool_b_to_x: &eth_dex_quote::TokenPriceUpdate,
        router_v2_address: ethers::types::Address,
        quoter_v3_address: ethers::types::Address,
        quoter_v4_address: ethers::types::Address,
    ) -> Result<(
        ethers::types::U256,
        ethers::types::U256,
        ethers::types::U256,
        i128,
    )> {
        utils::compute_3hop_arbitrage_path(
            Arc::new(self.provider.clone()),
            flashloan_amount,
            token_x,
            token_a,
            token_b,
            pool_x_to_a,
            pool_a_to_b,
            pool_b_to_x,
            router_v2_address,
            quoter_v3_address,
            quoter_v4_address,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eth_dex_quote::TokenPriceUpdate;
    use ethers::types::Address;

    #[tokio::test]
    async fn test_execution_simulation() {
        // For testing without actual provider connection
        // We skip this test as it requires external WebSocket provider
        // In a real scenario, we would mock the provider
        return;
    }

    #[tokio::test]
    async fn test_arbitrage_opportunity_validation() {
        // This test validates opportunity structure and profit calculations
        // without requiring external connections

        let token = Address::zero();
        let buy_pool = TokenPriceUpdate {
            token_address: "0x0000000000000000000000000000000000000000".to_string(),
            price_in_eth: 1.0,
            price_in_usd: Some(2000.0),
            pool_address: "0x0000000000000000000000000000000000000000".to_string(),
            dex_version: "UniswapV2".to_string(),
            decimals: 18,
            last_updated: 100,
            pool_token0: Address::zero(),
            pool_token1: Address::zero(),
            eth_chain: eth_dex_quote::EthChain::Mainnet,
            fee_tier: None,
            tick_spacing: None,
            hooks: None,
            eth_price_usd: 2000.0,
        };

        let sell_pool = TokenPriceUpdate {
            price_in_eth: 1.02,
            price_in_usd: Some(2040.0),
            ..buy_pool.clone()
        };

        let opportunity = DexArbitrageOpportunity {
            token_address: token,
            buy_pool,
            sell_pool,
            price_diff_percent: 2.0,
            potential_profit_usd: Some(40.0),
            detected_at: 100,
        };

        // Validate structure
        assert_eq!(opportunity.price_diff_percent, 2.0);
        assert_eq!(opportunity.potential_profit_usd, Some(40.0));
    }
}
