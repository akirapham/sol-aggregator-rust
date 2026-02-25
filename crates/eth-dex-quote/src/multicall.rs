use ethers::abi::{self, Token};
use ethers::prelude::*;
use ethers::types::Bytes;
use std::sync::Arc;

/// Multicall3 contract address — deployed at the same address on all EVM chains
/// See: https://www.multicall3.com/
pub const MULTICALL3_ADDRESS: &str = "0xcA11bde05977b3631167028862bE2a173976CA11";

/// A single quote request to be batched
#[derive(Debug, Clone)]
pub struct BatchQuoteRequest {
    /// Target contract address (quoter or router)
    pub target: Address,
    /// Pre-encoded calldata for the quote call
    pub calldata: Bytes,
}

/// Result of a single batched quote
#[derive(Debug, Clone)]
pub struct BatchQuoteResult {
    /// Whether this individual call succeeded
    pub success: bool,
    /// Decoded amount_out if successful
    pub amount_out: Option<U256>,
}

/// Batches multiple quote calls into a single Multicall3 RPC round-trip
pub struct QuoteBatcher<P: ethers::providers::Middleware> {
    provider: Arc<P>,
    multicall_address: Address,
}

impl<P: ethers::providers::Middleware + 'static> QuoteBatcher<P> {
    pub fn new(provider: Arc<P>) -> Self {
        Self {
            provider,
            multicall_address: MULTICALL3_ADDRESS.parse().unwrap(),
        }
    }

    pub fn with_address(mut self, address: Address) -> Self {
        self.multicall_address = address;
        self
    }

    /// Encode a V3 quoteExactInputSingle call
    pub fn encode_v3_quote(
        quoter_address: Address,
        token_in: Address,
        token_out: Address,
        fee: u32,
        amount_in: U256,
    ) -> BatchQuoteRequest {
        let selector = &ethers::utils::keccak256(
            "quoteExactInputSingle(address,address,uint24,uint256,uint160)",
        )[..4];

        let encoded_args = abi::encode(&[
            Token::Address(token_in),
            Token::Address(token_out),
            Token::Uint(U256::from(fee)),
            Token::Uint(amount_in),
            Token::Uint(U256::zero()), // sqrtPriceLimitX96 = 0
        ]);

        let mut calldata = Vec::with_capacity(4 + encoded_args.len());
        calldata.extend_from_slice(selector);
        calldata.extend_from_slice(&encoded_args);

        BatchQuoteRequest {
            target: quoter_address,
            calldata: Bytes::from(calldata),
        }
    }

    /// Encode a V2 getAmountsOut call
    pub fn encode_v2_quote(
        router_address: Address,
        amount_in: U256,
        path: Vec<Address>,
    ) -> BatchQuoteRequest {
        let selector = &ethers::utils::keccak256("getAmountsOut(uint256,address[])")[..4];

        let path_tokens: Vec<Token> = path.into_iter().map(Token::Address).collect();

        let encoded_args = abi::encode(&[Token::Uint(amount_in), Token::Array(path_tokens)]);

        let mut calldata = Vec::with_capacity(4 + encoded_args.len());
        calldata.extend_from_slice(selector);
        calldata.extend_from_slice(&encoded_args);

        BatchQuoteRequest {
            target: router_address,
            calldata: Bytes::from(calldata),
        }
    }

    /// Execute multiple quote calls in a single RPC round-trip.
    ///
    /// Uses Multicall3.tryAggregate(false, calls) so individual call failures
    /// don't revert the batch. Returns a Vec of results for each input request.
    pub async fn batch_quotes(
        &self,
        requests: Vec<BatchQuoteRequest>,
    ) -> anyhow::Result<Vec<BatchQuoteResult>> {
        if requests.is_empty() {
            return Ok(vec![]);
        }

        // Build the Call[] tuple array for tryAggregate
        // Each Call is (address target, bytes callData)
        let calls: Vec<Token> = requests
            .iter()
            .map(|req| {
                Token::Tuple(vec![
                    Token::Address(req.target),
                    Token::Bytes(req.calldata.to_vec()),
                ])
            })
            .collect();

        // Encode tryAggregate(bool requireSuccess, Call[] calls)
        let selector = &ethers::utils::keccak256("tryAggregate(bool,(address,bytes)[])")[..4];

        let encoded_args = abi::encode(&[
            Token::Bool(false), // don't require all to succeed
            Token::Array(calls),
        ]);

        let mut calldata = Vec::with_capacity(4 + encoded_args.len());
        calldata.extend_from_slice(selector);
        calldata.extend_from_slice(&encoded_args);

        // Execute as eth_call
        let tx = TransactionRequest::new()
            .to(self.multicall_address)
            .data(Bytes::from(calldata));

        let result_bytes = self
            .provider
            .call(&tx.into(), None)
            .await
            .map_err(|e| anyhow::anyhow!("Multicall3 eth_call failed: {:?}", e))?;

        // Decode the return: Result[] where Result = (bool success, bytes returnData)
        let decoded = abi::decode(
            &[abi::ParamType::Array(Box::new(abi::ParamType::Tuple(
                vec![abi::ParamType::Bool, abi::ParamType::Bytes],
            )))],
            &result_bytes,
        )
        .map_err(|e| anyhow::anyhow!("Failed to decode Multicall3 response: {:?}", e))?;

        let results_array = match &decoded[0] {
            Token::Array(arr) => arr,
            _ => return Err(anyhow::anyhow!("Unexpected Multicall3 response format")),
        };

        let batch_results: Vec<BatchQuoteResult> = results_array
            .iter()
            .map(|token| {
                if let Token::Tuple(fields) = token {
                    let success = matches!(&fields[0], Token::Bool(true));
                    let return_data = match &fields[1] {
                        Token::Bytes(data) => data.clone(),
                        _ => vec![],
                    };

                    let amount_out = if success && return_data.len() >= 32 {
                        Some(U256::from_big_endian(&return_data[..32]))
                    } else {
                        None
                    };

                    BatchQuoteResult {
                        success,
                        amount_out,
                    }
                } else {
                    BatchQuoteResult {
                        success: false,
                        amount_out: None,
                    }
                }
            })
            .collect();

        Ok(batch_results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_v3_quote() {
        let quoter: Address = "0xb27308f9f90d607463bb33ea1bebb41c27ce5ab6"
            .parse()
            .unwrap();
        let usdt: Address = "0xdac17f958d2ee523a2206206994597c13d831ec7"
            .parse()
            .unwrap();
        let usdc: Address = "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48"
            .parse()
            .unwrap();

        let req =
            QuoteBatcher::<ethers::providers::Provider<ethers::providers::Http>>::encode_v3_quote(
                quoter,
                usdt,
                usdc,
                500,
                U256::from(1_000_000u64),
            );

        assert_eq!(req.target, quoter);
        // 4 bytes selector + 5 * 32 bytes args = 164 bytes
        assert_eq!(req.calldata.len(), 164);
    }

    #[test]
    fn test_encode_v2_quote() {
        let router: Address = "0x7a250d5630b4cf539739df2c5dacb4c659f2488d"
            .parse()
            .unwrap();
        let usdt: Address = "0xdac17f958d2ee523a2206206994597c13d831ec7"
            .parse()
            .unwrap();
        let usdc: Address = "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48"
            .parse()
            .unwrap();

        let req =
            QuoteBatcher::<ethers::providers::Provider<ethers::providers::Http>>::encode_v2_quote(
                router,
                U256::from(1_000_000u64),
                vec![usdt, usdc],
            );

        assert_eq!(req.target, router);
        assert!(!req.calldata.is_empty());
    }

    /// Integration test: batch V3 quotes on Ethereum mainnet
    #[tokio::test]
    async fn test_batch_v3_quotes_real() {
        let rpc_url =
            std::env::var("ETH_RPC_URL").unwrap_or_else(|_| "https://eth.merkle.io".to_string());
        let provider = Arc::new(
            ethers::providers::Provider::<ethers::providers::Http>::try_from(rpc_url.as_str())
                .unwrap(),
        );

        let batcher = QuoteBatcher::new(provider);

        let quoter: Address = "0xb27308f9f90d607463bb33ea1bebb41c27ce5ab6"
            .parse()
            .unwrap();
        let usdt: Address = "0xdac17f958d2ee523a2206206994597c13d831ec7"
            .parse()
            .unwrap();
        let usdc: Address = "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48"
            .parse()
            .unwrap();

        // Batch 2 quotes at different fee tiers
        let requests = vec![
            QuoteBatcher::<ethers::providers::Provider<ethers::providers::Http>>::encode_v3_quote(
                quoter,
                usdt,
                usdc,
                100,
                U256::from(100_000_000u64), // 100 USDT
            ),
            QuoteBatcher::<ethers::providers::Provider<ethers::providers::Http>>::encode_v3_quote(
                quoter,
                usdt,
                usdc,
                500,
                U256::from(100_000_000u64), // 100 USDT
            ),
        ];

        let results = batcher.batch_quotes(requests).await.unwrap();

        assert_eq!(results.len(), 2);
        for (i, result) in results.iter().enumerate() {
            println!(
                "Quote {}: success={}, amount_out={:?}",
                i, result.success, result.amount_out
            );
        }
        assert!(
            results.iter().any(|r| r.success && r.amount_out.is_some()),
            "At least one quote should succeed"
        );
    }
}
