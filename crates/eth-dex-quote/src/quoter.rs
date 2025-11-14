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

    pub fn with_v2(mut self) -> Self {
        self.v2_quoter = Some(Arc::new(UniswapV2Quoter::new(self.provider.clone())));
        self
    }

    pub fn with_v3(mut self, quoter_v3: Address) -> Self {
        self.v3_quoter = Some(Arc::new(UniswapV3Quoter::new(
            self.provider.clone(),
            quoter_v3,
        )));
        self
    }

    pub async fn get_v2_quote(&self, amount_in: U256, path: Vec<Address>) -> Result<U256> {
        let v2 = self.v2_quoter.as_ref().ok_or(QuoteError::ContractError(
            "V2 quoter not initialized".to_string(),
        ))?;

        v2.get_quote(amount_in, path).await
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
