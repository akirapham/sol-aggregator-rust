use crate::types::{DexType, QuoteError, Result, SwapQuote};
use async_trait::async_trait;
use ethers::prelude::*;
use ethers::types::transaction::eip2718::TypedTransaction;
use ethers::utils::keccak256;
use std::sync::Arc;

#[async_trait]
pub trait V4Quoter: Send + Sync {
    async fn get_quote(
        &self,
        token_in: Address,
        token_out: Address,
        amount_in: U256,
        fee_tier: u32,
        tick_spacing: i32,
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
        hooks: Address,
    ) -> Result<U256> {
        // Encode call to quoteExactInputSingle(QuoteExactSingleParams memory params)
        // Function selector for quoteExactInputSingle
        let selector = keccak256(
            "quoteExactInputSingle((address,address,uint24,int24,address,bool,int256,uint160))",
        );
        let selector = [selector[0], selector[1], selector[2], selector[3]];

        let mut call_data = Vec::new();
        call_data.extend_from_slice(&selector);

        // Determine swap direction (zeroForOne)
        let zero_for_one = token_in < token_out;

        // Create pool key
        let (currency0, currency1) = if zero_for_one {
            (token_in, token_out)
        } else {
            (token_out, token_in)
        };

        // Encode the tuple params:
        // (currency0: address, currency1: address, fee: uint24, tickSpacing: int24, hooks: address, zeroForOne: bool, amountSpecified: int256, sqrtPriceLimitX96: uint160)

        // currency0
        let mut currency0_bytes = [0u8; 32];
        currency0_bytes[12..].copy_from_slice(&currency0[..]);
        call_data.extend_from_slice(&currency0_bytes);

        // currency1
        let mut currency1_bytes = [0u8; 32];
        currency1_bytes[12..].copy_from_slice(&currency1[..]);
        call_data.extend_from_slice(&currency1_bytes);

        // fee (uint24) - padded to 32 bytes
        let mut fee_bytes = [0u8; 32];
        let fee_be = fee.to_be_bytes();
        fee_bytes[32 - 3..].copy_from_slice(&fee_be[1..]);
        call_data.extend_from_slice(&fee_bytes);

        // tickSpacing (int24) - padded to 32 bytes
        let mut tick_spacing_bytes = [0u8; 32];
        let ts_be = (tick_spacing as i32).to_be_bytes();
        tick_spacing_bytes[32 - 3..].copy_from_slice(&ts_be[1..]);
        call_data.extend_from_slice(&tick_spacing_bytes);

        // hooks (address)
        let mut hooks_bytes = [0u8; 32];
        hooks_bytes[12..].copy_from_slice(&hooks[..]);
        call_data.extend_from_slice(&hooks_bytes);

        // zeroForOne (bool)
        let mut zero_for_one_bytes = [0u8; 32];
        if zero_for_one {
            zero_for_one_bytes[31] = 1;
        }
        call_data.extend_from_slice(&zero_for_one_bytes);

        // amountSpecified (int256) - as positive value
        let amount_specified_bytes: [u8; 32] = amount_in.into();
        call_data.extend_from_slice(&amount_specified_bytes);

        // sqrtPriceLimitX96 (uint160) = 0
        call_data.extend_from_slice(&[0u8; 32]);

        let tx: TypedTransaction = TransactionRequest::new()
            .to(self.quote_router)
            .data(Bytes::from(call_data))
            .into();

        let result = self
            .provider
            .call(&tx, None)
            .await
            .map_err(|e| QuoteError::RpcError(e.to_string()))?;

        // Result is encoded as [amountOut (32 bytes), gasEstimate (32 bytes)]
        if result.len() < 64 {
            return Err(QuoteError::ContractError(
                "Invalid V4 quoter response length".to_string(),
            ));
        }

        let amount_out = U256::from_big_endian(&result[0..32]);

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
        hooks: Address,
    ) -> Result<SwapQuote> {
        let amount_out = self
            .get_quote_from_contract(
                token_in,
                token_out,
                amount_in,
                fee_tier,
                tick_spacing,
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
