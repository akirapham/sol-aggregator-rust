use crate::pool_data_types::PoolState;
use crate::pool_manager::traits::{DatabaseTrait, GrpcServiceTrait, PriceServiceTrait};
use anyhow::Result;
use async_trait::async_trait;
use solana_sdk::pubkey::Pubkey;
use std::sync::{Arc, RwLock};

/// Mock gRPC service for testing
pub struct MockGrpcService;

use crate::types::{ChainStateUpdate, PoolUpdateEvent};
use tokio::sync::mpsc;

#[async_trait]
impl GrpcServiceTrait for MockGrpcService {
    async fn subscribe_pool_updates(
        &self,
        _pool_update_sender: mpsc::UnboundedSender<Vec<PoolUpdateEvent>>,
        _chain_state_sender: mpsc::UnboundedSender<ChainStateUpdate>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // No-op for testing
        Ok(())
    }

    async fn stop(&self) {
        // No-op
    }
}

use crate::types::Token;
use std::collections::HashMap;

/// Mock database for testing with in-memory storage
pub struct MockDatabase {
    pools: Arc<RwLock<Vec<PoolState>>>,
    tokens: Arc<RwLock<Vec<Token>>>,
    arbitrage_tokens: Arc<RwLock<Vec<Pubkey>>>,
}

impl MockDatabase {
    pub fn new() -> Self {
        Self {
            pools: Arc::new(RwLock::new(Vec::new())),
            tokens: Arc::new(RwLock::new(Vec::new())),
            arbitrage_tokens: Arc::new(RwLock::new(Vec::new())),
        }
    }
}

#[async_trait]
impl DatabaseTrait for MockDatabase {
    async fn load_pools(&self) -> Result<Vec<PoolState>> {
        Ok(self.pools.read().unwrap().clone())
    }

    async fn save_pools(&self, pools: &[PoolState]) -> Result<()> {
        *self.pools.write().unwrap() = pools.to_vec();
        Ok(())
    }

    async fn load_tokens(&self) -> Result<Vec<Token>> {
        Ok(self.tokens.read().unwrap().clone())
    }

    async fn save_tokens(&self, tokens: &[Token]) -> Result<()> {
        *self.tokens.write().unwrap() = tokens.to_vec();
        Ok(())
    }

    async fn load_arbitrage_tokens(&self) -> Result<Vec<Pubkey>> {
        Ok(self.arbitrage_tokens.read().unwrap().clone())
    }

    async fn save_arbitrage_tokens(&self, tokens: &[Pubkey]) -> Result<()> {
        *self.arbitrage_tokens.write().unwrap() = tokens.to_vec();
        Ok(())
    }

    async fn add_arbitrage_token(&self, token: &Pubkey) -> Result<()> {
        self.arbitrage_tokens.write().unwrap().push(*token);
        Ok(())
    }

    async fn remove_arbitrage_token(&self, token: &Pubkey) -> Result<()> {
        self.arbitrage_tokens
            .write()
            .unwrap()
            .retain(|t| t != token);
        Ok(())
    }
}

/// Mock price service for testing
pub struct MockPriceService {
    sol_price: f64,
}

impl MockPriceService {
    pub fn new(sol_price: f64) -> Self {
        Self { sol_price }
    }
}

impl PriceServiceTrait for MockPriceService {
    fn get_sol_price(&self) -> f64 {
        self.sol_price
    }
}
