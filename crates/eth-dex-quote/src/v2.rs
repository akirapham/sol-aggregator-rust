use crate::types::{DexType, QuoteError, Result, SwapQuote};
use async_trait::async_trait;
use ethers::types::transaction::eip2718::TypedTransaction;
use ethers::types::{Address, Bytes, TransactionRequest, U256};
use std::sync::Arc;

#[async_trait]
pub trait V2Quoter: Send + Sync {
    async fn get_quote(
        &self,
        token_in: Address,
        token_out: Address,
        amount_in: U256,
    ) -> Result<SwapQuote>;
}

pub struct UniswapV2Quoter<P: ethers::providers::Middleware> {
    provider: Arc<P>,
    factory: Address,
}

impl<P: ethers::providers::Middleware + 'static> UniswapV2Quoter<P> {
    pub fn new(provider: Arc<P>, factory: Address) -> Self {
        Self { provider, factory }
    }

    pub async fn get_reserves(
        &self,
        token_in: Address,
        token_out: Address,
    ) -> Result<(U256, U256)> {
        let pair = self.get_pair(token_in, token_out).await?;

        // Encode getReserves() function call
        // Function selector for getReserves() is 0x0902f1ac
        let call_data = Bytes::from(vec![0x09, 0x02, 0xf1, 0xac]);

        let tx: TypedTransaction = TransactionRequest::new().to(pair).data(call_data).into();

        let reserves = self
            .provider
            .call(&tx, None)
            .await
            .map_err(|e| QuoteError::RpcError(e.to_string()))?;

        // Decode reserves - getReserves returns (uint112 reserve0, uint112 reserve1, uint32 blockTimestampLast)
        // We get 96 bytes total (3 uint256 values padded to 32 bytes each)
        if reserves.len() < 96 {
            return Err(QuoteError::ContractError(
                "Invalid reserve response".to_string(),
            ));
        }

        let reserve0 = U256::from_big_endian(&reserves[0..32]);
        let reserve1 = U256::from_big_endian(&reserves[32..64]);

        Ok((reserve0, reserve1))
    }

    async fn get_pair(&self, token_in: Address, token_out: Address) -> Result<Address> {
        let is_reverse = token_in > token_out;
        let (token_a, token_b) = if is_reverse {
            (token_out, token_in)
        } else {
            (token_in, token_out)
        };

        let init_code =
            hex::decode("96e8ac4277198ff8b6f785478aa9a39f403cb768dd02cbee326c3e265cbd3627")
                .map_err(|_| QuoteError::ContractError("Invalid init code".to_string()))?;

        let salt = ethers::utils::keccak256(ethers::abi::encode(&[
            ethers::abi::Token::Address(token_a),
            ethers::abi::Token::Address(token_b),
        ]));

        let pair = ethers::utils::get_create2_address(
            self.factory,
            salt,
            ethers::utils::keccak256(&init_code),
        );

        Ok(pair)
    }

    pub fn compute_amount_out(
        amount_in: U256,
        reserve_in: U256,
        reserve_out: U256,
    ) -> Result<U256> {
        if reserve_in.is_zero() || reserve_out.is_zero() {
            return Err(QuoteError::NoLiquidity);
        }

        let amount_in_with_fee = amount_in * U256::from(997);
        let numerator = amount_in_with_fee * reserve_out;
        let denominator = reserve_in * U256::from(1000) + amount_in_with_fee;

        Ok(numerator / denominator)
    }
}

#[async_trait]
impl<P: ethers::providers::Middleware + 'static> V2Quoter for UniswapV2Quoter<P> {
    async fn get_quote(
        &self,
        token_in: Address,
        token_out: Address,
        amount_in: U256,
    ) -> Result<SwapQuote> {
        let (reserve_in, reserve_out) = self.get_reserves(token_in, token_out).await?;

        let amount_out = Self::compute_amount_out(amount_in, reserve_in, reserve_out)?;

        Ok(SwapQuote {
            amount_out,
            route: vec![token_in, token_out],
            dex: DexType::UniswapV2,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_amount_out() {
        let amount_in = U256::from(1_000_000_000_000_000_000u64); // 1 token
        let reserve_in = U256::from(1_000_000_000_000_000_000u64);
        let reserve_out = U256::from(1_000_000_000_000_000_000u64);

        let result = UniswapV2Quoter::<ethers::providers::Provider<ethers::providers::Http>>::compute_amount_out(
            amount_in,
            reserve_in,
            reserve_out,
        );

        assert!(result.is_ok());
        let amount_out = result.unwrap();
        assert!(amount_out > U256::zero());
    }
}
