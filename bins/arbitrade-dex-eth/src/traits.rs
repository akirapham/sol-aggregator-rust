use crate::{DexArbitrageOpportunity, ExecutionResult};
use anyhow::Result;
use async_trait::async_trait;
use eth_dex_quote::TokenPriceUpdate;
use ethers::types::Address;
use ethers::types::U256;

pub trait PriceCacheTrait: Send + Sync {
    fn update_price(&self, price_update: TokenPriceUpdate);
}

pub trait ArbitrageDetectorTrait: Send + Sync {
    fn check_opportunities_for_token(
        &self,
        token_address: &Address,
    ) -> Vec<DexArbitrageOpportunity>;
}

#[async_trait]
pub trait ArbitrageExecutorTrait: Send + Sync {
    async fn execute_flashloan(
        &self,
        opportunity: DexArbitrageOpportunity,
        paths: Vec<eth_dex_quote::quote_router::ExecHop>,
        flashloan_amount: U256,
        flashloan_token: Address,
        potential_profit_usd: Option<f64>,
    ) -> Result<ExecutionResult>;
}
