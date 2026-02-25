use crate::types::Result;
use ethers::prelude::*;
use std::sync::Arc;

// Generate contract bindings for QuoteRouter from the compiled Foundry artifact
abigen!(
    QuoteRouterContract,
    "../../contracts/out/QuoteRouter.sol/QuoteRouter.json"
);

pub struct QuoteRouterClient<P: ethers::providers::Middleware> {
    provider: Arc<P>,
    router_address: Address,
}

impl<P: ethers::providers::Middleware + 'static> QuoteRouterClient<P> {
    pub fn new(provider: Arc<P>, router_address: Address) -> Self {
        Self {
            provider,
            router_address,
        }
    }

    /// Single path quote (N hops)
    pub async fn quote_single_path(&self, hops: Vec<Hop>, amount_in: U256) -> Result<U256> {
        let contract = QuoteRouterContract::new(self.router_address, self.provider.clone());
        let amount_out = contract
            .quote_single_path(hops, amount_in)
            .call()
            .await
            .map_err(|e| crate::types::QuoteError::ContractError(e.to_string()))?;
        Ok(amount_out)
    }

    /// 2-Hop Arbitrage Quote (X -> A -> X)
    pub async fn quote_arbitrage_2_hop(
        &self,
        hop1: Hop,
        hop2: Hop,
        amount_in: U256,
    ) -> Result<(U256, I256)> {
        let contract = QuoteRouterContract::new(self.router_address, self.provider.clone());
        let (amount_out, profit) = contract
            .quote_arbitrage_2_hop(hop1, hop2, amount_in)
            .call()
            .await
            .map_err(|e| crate::types::QuoteError::ContractError(e.to_string()))?;
        Ok((amount_out, profit))
    }

    /// Batch Arbitrage Quotes (many 2-hop pairs)
    pub async fn quote_batch_arbitrage(&self, quotes: Vec<ArbQuote>) -> Result<Vec<ArbResult>> {
        let contract = QuoteRouterContract::new(self.router_address, self.provider.clone());
        let results = contract
            .quote_batch_arbitrage(quotes)
            .call()
            .await
            .map_err(|e| crate::types::QuoteError::ContractError(e.to_string()))?;
        Ok(results)
    }
}
