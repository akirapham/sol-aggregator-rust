use crate::types::{DexType, QuoteError, Result, SwapQuote};
use async_trait::async_trait;
use ethers::prelude::*;
use std::sync::Arc;

// Generate contract bindings for Uniswap V3 Quoter
abigen!(
    UniswapV3QuoterContract,
    r#"[
        function quoteExactInputSingle(address tokenIn, address tokenOut, uint24 fee, uint256 amountIn, uint160 sqrtPriceLimitX96) external view returns (uint256 amountOut)
    ]"#
);

#[async_trait]
pub trait V3Quoter: Send + Sync {
    async fn get_quote(
        &self,
        token_in: Address,
        token_out: Address,
        amount_in: U256,
        fee_tier: u32,
    ) -> Result<SwapQuote>;
}

pub struct UniswapV3Quoter<P: ethers::providers::Middleware> {
    provider: Arc<P>,
    quoter_v3: Address,
}

impl<P: ethers::providers::Middleware + 'static> UniswapV3Quoter<P> {
    pub fn new(provider: Arc<P>, quoter_v3: Address) -> Self {
        Self {
            provider,
            quoter_v3,
        }
    }

    /// Call Uniswap V3 Quoter contract to get amount out
    pub async fn get_quote_from_contract(
        &self,
        token_in: Address,
        token_out: Address,
        fee: u32,
        amount_in: U256,
    ) -> Result<U256> {
        // Create contract instance using abigen-generated contract binding
        let quoter_contract = UniswapV3QuoterContract::new(self.quoter_v3, self.provider.clone());

        // Call quoteExactInputSingle using the generated binding
        // .call() performs an eth_call (staticcall) under the hood
        let amount_out: U256 = quoter_contract
            .quote_exact_input_single(token_in, token_out, fee, amount_in, U256::zero())
            .call()
            .await
            .map_err(|e| {
                QuoteError::RpcError(format!("Quoter call failed with token_in = {:?}, token_out = {:?}, fee = {}, amount_in = {:?}, quoter = {:?}: error = {:?}", token_in, token_out, fee, amount_in, self.quoter_v3, e))
            })?;

        Ok(amount_out)
    }
}

#[async_trait]
impl<P: ethers::providers::Middleware + 'static> V3Quoter for UniswapV3Quoter<P> {
    async fn get_quote(
        &self,
        token_in: Address,
        token_out: Address,
        amount_in: U256,
        fee_tier: u32,
    ) -> Result<SwapQuote> {
        let amount_out = self
            .get_quote_from_contract(token_in, token_out, fee_tier, amount_in)
            .await?;

        Ok(SwapQuote {
            amount_out,
            route: vec![token_in, token_out],
            dex: DexType::UniswapV3,
        })
    }
}
