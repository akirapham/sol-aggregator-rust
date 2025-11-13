use crate::types::{QuoteError, Result};
use async_trait::async_trait;
use ethers::types::transaction::eip2718::TypedTransaction;
use ethers::types::{Address, Bytes, TransactionRequest, U256};
use std::sync::Arc;

#[async_trait]
pub trait V2Quoter: Send + Sync {
    async fn get_quote(
        &self,
        pair_address: Address,
        token_in: Address,
        token_out: Address,
        amount_in: U256,
    ) -> Result<U256>;
}

pub struct UniswapV2Quoter<P: ethers::providers::Middleware> {
    provider: Arc<P>,
}

impl<P: ethers::providers::Middleware + 'static> UniswapV2Quoter<P> {
    pub fn new(provider: Arc<P>) -> Self {
        Self { provider }
    }

    pub async fn get_reserves_from_pair(&self, pair_address: Address) -> Result<(U256, U256)> {
        // Encode getReserves() function call
        // Function selector for getReserves() is 0x0902f1ac
        let call_data = Bytes::from(vec![0x09, 0x02, 0xf1, 0xac]);

        let tx: TypedTransaction = TransactionRequest::new()
            .to(pair_address)
            .data(call_data)
            .into();

        let reserves = self
            .provider
            .call(&tx, None)
            .await
            .map_err(|e| QuoteError::RpcError(e.to_string()))?;

        // Decode reserves - getReserves returns (uint112 reserve0, uint112 reserve1, uint32 blockTimestampLast)
        // Response should be at least 96 bytes (3 x 32-byte words)
        // But sometimes it might be shorter depending on encoding
        if reserves.len() < 32 {
            return Err(QuoteError::ContractError(format!(
                "Invalid reserve response length: {}",
                reserves.len()
            )));
        }

        // Parse first reserve (reserve0) - take first 32 bytes
        let reserve0 = if reserves.len() >= 32 {
            U256::from_big_endian(&reserves[0..32])
        } else {
            return Err(QuoteError::ContractError(
                "Could not parse reserve0".to_string(),
            ));
        };

        // Parse second reserve (reserve1) - take next 32 bytes
        let reserve1 = if reserves.len() >= 64 {
            U256::from_big_endian(&reserves[32..64])
        } else {
            return Err(QuoteError::ContractError(
                "Could not parse reserve1".to_string(),
            ));
        };

        Ok((reserve0, reserve1))
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
        pair_address: Address,
        token_in: Address,
        token_out: Address,
        amount_in: U256,
    ) -> Result<U256> {
        let (reserve0, reserve1) = self.get_reserves_from_pair(pair_address).await?;

        // For simplicity, assume reserves are in standard order (token0, token1)
        // In production, you'd query pair.token0() to determine the actual order
        let (reserve_in, reserve_out) = if token_in < token_out {
            (reserve0, reserve1)
        } else {
            (reserve1, reserve0)
        };

        let amount_out = Self::compute_amount_out(amount_in, reserve_in, reserve_out)?;

        Ok(amount_out)
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
