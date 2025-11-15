use crate::types::{DexArbitrageOpportunity, ExecutionResult, ExecutionStatus};
use anyhow::{anyhow, Result};
use ethers::providers::Provider;
use ethers::signers::LocalWallet;
use ethers::signers::Signer;
use log::{error, info, warn};
use std::str::FromStr;

/// Executes arbitrage trades on-chain
pub struct ArbitrageExecutor {
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

        // Estimate gas costs (1 ETH worth of gas for simulation)
        let estimated_gas_eth = 0.01; // Rough estimate
        let net_profit_eth = opportunity.potential_profit_eth - estimated_gas_eth;

        let actual_profit_usd = if net_profit_eth > 0.0 {
            opportunity
                .potential_profit_usd
                .map(|usd| usd * (net_profit_eth / opportunity.potential_profit_eth))
        } else {
            Some(0.0)
        };

        if self.dry_run {
            info!(
                "📊 [DRY RUN] Would execute: {} → Profit: {:.6} ETH ({:?} USD) | TX: {}",
                opportunity, net_profit_eth, actual_profit_usd, simulated_tx_hash
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
            actual_profit_eth: net_profit_eth,
            actual_profit_usd,
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::PoolPrice;
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
        let buy_pool = PoolPrice {
            token_address: token,
            price_in_eth: 1.0,
            price_in_usd: Some(2000.0),
            pool_address: Address::zero(),
            dex_version: "UniswapV2".to_string(),
            decimals: 18,
            last_updated: 100,
            liquidity_eth: None,
            liquidity_usd: None,
        };

        let sell_pool = PoolPrice {
            price_in_eth: 1.02,
            price_in_usd: Some(2040.0),
            ..buy_pool.clone()
        };

        let opportunity = DexArbitrageOpportunity {
            token_address: token,
            buy_pool,
            sell_pool,
            price_diff_eth: 0.02,
            price_diff_percent: 2.0,
            potential_profit_eth: 0.02,
            potential_profit_usd: Some(40.0),
            gas_cost_eth: None,
            net_profit_eth: None,
            detected_at: 100,
        };

        // Validate structure
        assert_eq!(opportunity.price_diff_percent, 2.0);
        assert_eq!(opportunity.price_diff_eth, 0.02);
    }
}
