use crate::types::{DexType, QuoteError, Result, SwapQuote};
use async_trait::async_trait;
use ethers::prelude::*;
use std::sync::Arc;

// Generate contract bindings for Uniswap V4 Quoter
abigen!(
    UniswapV4QuoterContract,
    r#"[
        {
            "name": "quoteExactInputSingle",
            "type": "function",
            "stateMutability": "nonpayable",
            "inputs": [
                {
                    "name": "params",
                    "type": "tuple",
                    "components": [
                        {
                            "name": "poolKey",
                            "type": "tuple",
                            "components": [
                                {"name": "currency0", "type": "address"},
                                {"name": "currency1", "type": "address"},
                                {"name": "fee", "type": "uint24"},
                                {"name": "tickSpacing", "type": "int24"},
                                {"name": "hooks", "type": "address"}
                            ]
                        },
                        {"name": "zeroForOne", "type": "bool"},
                        {"name": "exactAmount", "type": "uint128"},
                        {"name": "hookData", "type": "bytes"}
                    ]
                }
            ],
            "outputs": [
                {"name": "amountOut", "type": "uint256"},
                {"name": "gasEstimate", "type": "uint256"}
            ]
        }
    ]"#
);

#[async_trait]
pub trait V4Quoter: Send + Sync {
    async fn get_quote(
        &self,
        token_in: Address,
        token_out: Address,
        amount_in: U256,
        fee_tier: u32,
        tick_spacing: i32,
        pool_id: Option<String>,
        hooks: Address,
    ) -> Result<SwapQuote>;
}

pub struct UniswapV4Quoter<P: ethers::providers::Middleware> {
    provider: Arc<P>,
    quote_router: Address,
}

impl<P: ethers::providers::Middleware + 'static> UniswapV4Quoter<P> {
    pub fn new(provider: Arc<P>, quote_router: Address) -> Self {
        Self {
            provider,
            quote_router,
        }
    }

    /// Call Uniswap V4 Quote Router to get amount out
    /// V4 requires full pool key specification including hooks
    pub async fn get_quote_from_contract(
        &self,
        token_in: Address,
        token_out: Address,
        amount_in: U256,
        fee: u32,
        tick_spacing: i32,
        pool_id: Option<String>,
        hooks: Address,
    ) -> Result<U256> {
        // Create contract instance using abigen-generated contract binding
        let quoter_contract =
            UniswapV4QuoterContract::new(self.quote_router, self.provider.clone());

        // Determine swap direction (zeroForOne)
        let zero_for_one = token_in < token_out;

        // Create pool key - currencies must be ordered
        let (currency0, currency1) = if zero_for_one {
            (token_in, token_out)
        } else {
            (token_out, token_in)
        };

        // exactAmount is an unsigned 128-bit integer; ensure amount_in fits
        if amount_in.bits() > 128 {
            return Err(QuoteError::RpcError(format!(
                "amount_in too large for uint128: {} bits",
                amount_in.bits()
            )));
        }

        // Prepare params matching QuoteExactSingleParams -> (PoolKey, bool zeroForOne, uint128 exactAmount, bytes hookData)
        let pool_key = (currency0, currency1, fee, tick_spacing, hooks);
        let exact_amount = amount_in.as_u128(); // Convert U256 to u128
        let hook_data = Bytes::new(); // Empty hook data

        let (amount_out, _gas_estimate) = quoter_contract
            .quote_exact_input_single((pool_key, zero_for_one, exact_amount, hook_data))
            .call()
            .await
            .map_err(|e| {
                log::error!("V4 Quoter call failed: {:?}", e);
                QuoteError::RpcError(format!(
                    "V4 Quoter call failed with token_in = {:?}, token_out = {:?}, fee = {}, amount_in = {:?}, quoter = {:?}, hooks = {:?}, tick spacing = {}, pool = {:?}: error = {:?}",
                    token_in, token_out, fee, amount_in, self.quote_router, hooks, tick_spacing, pool_id, e
                ))
            })?;

        if amount_out.is_zero() {
            return Err(QuoteError::NoLiquidity);
        }

        Ok(amount_out)
    }
}

#[async_trait]
impl<P: ethers::providers::Middleware + 'static> V4Quoter for UniswapV4Quoter<P> {
    async fn get_quote(
        &self,
        token_in: Address,
        token_out: Address,
        amount_in: U256,
        fee_tier: u32,
        tick_spacing: i32,
        pool_id: Option<String>,
        hooks: Address,
    ) -> Result<SwapQuote> {
        let amount_out = self
            .get_quote_from_contract(
                token_in,
                token_out,
                amount_in,
                fee_tier,
                tick_spacing,
                pool_id,
                hooks,
            )
            .await?;

        Ok(SwapQuote {
            amount_out,
            route: vec![token_in, token_out],
            dex: DexType::UniswapV4,
        })
    }
}
