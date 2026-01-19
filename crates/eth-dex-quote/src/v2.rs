use crate::types::{QuoteError, Result};
use async_trait::async_trait;
use ethers::prelude::*;
use std::sync::Arc;

abigen!(
    UniswapV2Router,
    r#"[
        function getAmountsOut(uint amountIn, address[] calldata path) external view returns (uint[] memory amounts)
    ]"#
);

#[async_trait]
pub trait V2Quoter: Send + Sync {
    async fn get_quote(&self, amount_in: U256, path: Vec<Address>) -> Result<U256>;
}

pub struct UniswapV2Quoter<P: ethers::providers::Middleware> {
    provider: Arc<P>,
    router: Option<Address>,
}

impl<P: ethers::providers::Middleware + 'static> UniswapV2Quoter<P> {
    pub fn new(provider: Arc<P>) -> Self {
        Self {
            provider,
            router: None,
        }
    }

    pub fn with_router(mut self, router: Address) -> Self {
        self.router = Some(router);
        self
    }

    /// Get quote via router contract using getAmountsOut
    /// This is the preferred method as it accounts for slippage and actual execution path
    pub async fn get_quote_from_router(&self, amount_in: U256, path: Vec<Address>) -> Result<U256> {
        let router_addr = self.router.ok_or(QuoteError::ContractError(
            "Router not configured".to_string(),
        ))?;

        // Create contract instance using abigen
        let router = UniswapV2Router::new(router_addr, self.provider.clone());
        // Call getAmountsOut - abigen handles all ABI encoding
        let amounts = router
            .get_amounts_out(amount_in, path.clone())
            .call()
            .await
            .map_err(|e| QuoteError::RpcError(format!("Router call failed with amount_in = {}, path = {:?}, router = {:?} : error = {}", amount_in, path, router_addr, e)))?;

        // Return the last amount (final output)
        amounts.last().copied().ok_or(QuoteError::ContractError(
            "No amounts returned from router".to_string(),
        ))
    }
}

#[async_trait]
impl<P: ethers::providers::Middleware + 'static> V2Quoter for UniswapV2Quoter<P> {
    async fn get_quote(&self, amount_in: U256, path: Vec<Address>) -> Result<U256> {
        self.get_quote_from_router(amount_in, path).await
    }
}
