use crate::types::{QuoteError, Result, SwapQuote};
use crate::v2::{UniswapV2Quoter, V2Quoter};
use crate::v3::{UniswapV3Quoter, V3Quoter};
use ethers::types::{Address, U256};
use std::sync::Arc;

pub struct UniversalQuoter<P: ethers::providers::Middleware + 'static> {
    provider: Arc<P>,
    v2_quoter: Option<Arc<UniswapV2Quoter<P>>>,
    v3_quoter: Option<Arc<UniswapV3Quoter<P>>>,
}

impl<P: ethers::providers::Middleware + 'static> UniversalQuoter<P> {
    pub fn new(provider: Arc<P>) -> Self {
        Self {
            provider,
            v2_quoter: None,
            v3_quoter: None,
        }
    }

    pub fn with_v2(mut self, factory: Address) -> Self {
        self.v2_quoter = Some(Arc::new(UniswapV2Quoter::new(
            self.provider.clone(),
            factory,
        )));
        self
    }

    pub fn with_v3(mut self, quoter_v3: Address) -> Self {
        self.v3_quoter = Some(Arc::new(UniswapV3Quoter::new(
            self.provider.clone(),
            quoter_v3,
        )));
        self
    }

    pub async fn get_best_quote(
        &self,
        token_in: Address,
        token_out: Address,
        amount_in: U256,
        fee_tier: Option<u32>,
    ) -> Result<SwapQuote> {
        let mut best_quote: Option<SwapQuote> = None;

        // Try V2
        if let Some(v2) = &self.v2_quoter {
            if let Ok(quote) = v2.get_quote(token_in, token_out, amount_in).await {
                best_quote = Some(quote);
            }
        }

        // Try V3
        if let Some(v3) = &self.v3_quoter {
            if let Some(fee) = fee_tier {
                if let Ok(quote) = v3.get_quote(token_in, token_out, amount_in, fee).await {
                    match best_quote {
                        Some(ref current_best) => {
                            if quote.amount_out > current_best.amount_out {
                                best_quote = Some(quote);
                            }
                        }
                        None => {
                            best_quote = Some(quote);
                        }
                    }
                }
            }
        }

        best_quote.ok_or(QuoteError::NoLiquidity)
    }

    pub async fn get_v2_quote(
        &self,
        token_in: Address,
        token_out: Address,
        amount_in: U256,
    ) -> Result<SwapQuote> {
        let v2 = self.v2_quoter.as_ref().ok_or(QuoteError::ContractError(
            "V2 quoter not initialized".to_string(),
        ))?;

        v2.get_quote(token_in, token_out, amount_in).await
    }

    pub async fn get_v3_quote(
        &self,
        token_in: Address,
        token_out: Address,
        amount_in: U256,
        fee_tier: u32,
    ) -> Result<SwapQuote> {
        let v3 = self.v3_quoter.as_ref().ok_or(QuoteError::ContractError(
            "V3 quoter not initialized".to_string(),
        ))?;

        v3.get_quote(token_in, token_out, amount_in, fee_tier).await
    }
}
