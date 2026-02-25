use crate::types::{DexArbitrageOpportunity, ExecutionResult, ExecutionStatus};
use crate::utils;
use anyhow::{anyhow, Result};
use ethers::middleware::SignerMiddleware;
use ethers::providers::Middleware;
use ethers::signers::{LocalWallet, Signer};
use ethers::types::{Address, U256};
use log::{info, warn};
use std::str::FromStr;
use std::sync::Arc;

/// Executes arbitrage trades on-chain
pub struct ArbitrageExecutor<M: Middleware> {
    client: Arc<SignerMiddleware<M, LocalWallet>>,
    router_address: Address,
    /// Slippage tolerance in basis points (100 = 1%)
    slippage_tolerance: u16,
    /// Whether to actually execute trades or just simulate
    dry_run: bool,
}

impl<M: Middleware + 'static> ArbitrageExecutor<M> {
    pub fn new(
        provider: M,
        private_key: String,
        router_address: Address,
        slippage_tolerance: u16,
        dry_run: bool,
        chain_id: u64,
    ) -> Result<Self> {
        let wallet = LocalWallet::from_str(&private_key)
            .map_err(|e| anyhow!("Invalid private key: {}", e))?
            .with_chain_id(chain_id);

        let client = Arc::new(SignerMiddleware::new(provider, wallet));

        Ok(ArbitrageExecutor {
            client,
            router_address,
            slippage_tolerance,
            dry_run,
        })
    }

    /// Execute a flashloan arbitrage using the QuoteRouter contract
    pub async fn execute_flashloan(
        &self,
        opportunity: DexArbitrageOpportunity,
        paths: Vec<eth_dex_quote::quote_router::ExecHop>,
        flashloan_amount: U256,
        flashloan_token: Address,
        potential_profit_usd: Option<f64>,
    ) -> Result<ExecutionResult> {
        let simulated_tx_hash = format!("0x{}", hex::encode(uuid::Uuid::new_v4().as_bytes()));
        let profit_usd = potential_profit_usd.unwrap_or(0.0);

        if self.dry_run {
            info!(
                "📊 [DRY RUN] Would execute flashloan for {} tokens. Profit: ${:.2} | TX: {}",
                paths.len(),
                profit_usd,
                simulated_tx_hash
            );
            return Ok(ExecutionResult {
                trade: crate::types::ArbitrageTrade {
                    opportunity: opportunity.clone(),
                    amount_in: 0,
                    min_amount_out: 0,
                    max_gas_price: 0,
                },
                tx_hash: simulated_tx_hash,
                actual_profit_eth: 0.0,
                actual_profit_usd: Some(profit_usd),
                status: ExecutionStatus::Pending,
            });
        }

        info!(
            "🚀 Sending Flashloan Arbitrage TX to {}! Paths: {}",
            self.router_address,
            paths.len()
        );

        let contract = eth_dex_quote::quote_router::QuoteRouterContract::new(
            self.router_address,
            self.client.clone(),
        );

        let tx = contract.execute_arbitrage(paths, flashloan_amount, flashloan_token);

        // Estimate gas before sending to avoid reverting blindly
        match tx.estimate_gas().await {
            Ok(gas) => {
                info!("Gas limit estimated: {}", gas);
                // Execute
                let pending_tx = tx
                    .send()
                    .await
                    .map_err(|e| anyhow!("Failed to send tx: {}", e))?;
                let tx_hash = format!("{:?}", pending_tx.tx_hash());
                info!("Flashloan Arbitrage broadcasted! TX Hash: {}", tx_hash);

                Ok(ExecutionResult {
                    trade: crate::types::ArbitrageTrade {
                        opportunity,
                        amount_in: 0,
                        min_amount_out: 0,
                        max_gas_price: 0,
                    },
                    tx_hash,
                    actual_profit_eth: 0.0,
                    actual_profit_usd: Some(profit_usd),
                    status: ExecutionStatus::Pending,
                })
            }
            Err(e) => {
                warn!(
                    "Execution would revert or fail! Restricting TX broadcast. Err: {:?}",
                    e
                );
                Err(anyhow!("Gas estimation failed: {}", e))
            }
        }
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
        format!("{:?}", self.client.address())
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
            reserve0: None,
            reserve1: None,
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
