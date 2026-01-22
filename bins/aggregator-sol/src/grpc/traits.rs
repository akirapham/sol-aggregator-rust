use crate::types::{ChainStateUpdate, PoolUpdateEvent};
use async_trait::async_trait;
use tokio::sync::mpsc;

/// Trait for gRPC service to allow mocking in tests
#[async_trait]
pub trait GrpcServiceTrait: Send + Sync {
    /// Subscribe to pool updates from gRPC stream
    async fn subscribe_pool_updates(
        &self,
        pool_update_sender: mpsc::UnboundedSender<Vec<PoolUpdateEvent>>,
        chain_state_sender: mpsc::UnboundedSender<ChainStateUpdate>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;

    /// Stop the service
    async fn stop(&self);
}
