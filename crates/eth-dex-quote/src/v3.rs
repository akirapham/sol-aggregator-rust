use crate::types::{DexType, QuoteError, Result, SwapQuote};
use async_trait::async_trait;
use ethers::types::transaction::eip2718::TypedTransaction;
use ethers::types::{Address, Bytes, TransactionRequest, U256};
use std::sync::Arc;

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
        // Encode call to quoter: quoteExactInputSingle(address tokenIn, address tokenOut, uint24 fee, uint256 amountIn, uint160 sqrtPriceLimitX96)
        // Function selector for quoteExactInputSingle is 0x414bf389
        let selector = [0x41, 0x4b, 0xf3, 0x89];

        let mut call_data = Vec::new();
        call_data.extend_from_slice(&selector);

        // token_in (address - padded to 32 bytes)
        let mut token_in_bytes = [0u8; 32];
        token_in_bytes[12..].copy_from_slice(&token_in[..]);
        call_data.extend_from_slice(&token_in_bytes);

        // token_out (address - padded to 32 bytes)
        let mut token_out_bytes = [0u8; 32];
        token_out_bytes[12..].copy_from_slice(&token_out[..]);
        call_data.extend_from_slice(&token_out_bytes);

        // fee (uint24 - padded to 32 bytes)
        let mut fee_bytes = [0u8; 32];
        let fee_be = fee.to_be_bytes();
        fee_bytes[32 - 3..].copy_from_slice(&fee_be[1..]);
        call_data.extend_from_slice(&fee_bytes);

        // amount_in (uint256)
        let amount_in_bytes: [u8; 32] = amount_in.into();
        call_data.extend_from_slice(&amount_in_bytes);

        // sqrtPriceLimitX96 = 0 (uint160 - padded to 32 bytes)
        call_data.extend_from_slice(&[0u8; 32]);

        let tx: TypedTransaction = TransactionRequest::new()
            .to(self.quoter_v3)
            .data(Bytes::from(call_data))
            .into();

        let result = self
            .provider
            .call(&tx, None)
            .await
            .map_err(|e| QuoteError::RpcError(e.to_string()))?;

        if result.len() < 32 {
            return Err(QuoteError::ContractError(
                "Invalid response length".to_string(),
            ));
        }

        let amount_out = U256::from_big_endian(&result[0..32]);
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
