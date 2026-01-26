// Helius sender for fast transaction submission
// Uses Helius RPC for priority fee estimation and smart sending

use crate::tx_execution::transaction_builder::{PriorityLevel, TransactionBuilder};
use crate::tx_execution::KeypairManager;
use serde::{Deserialize, Serialize};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_config::RpcSimulateTransactionConfig;
use solana_sdk::{
    instruction::Instruction, pubkey::Pubkey, signature::Signature, transaction::Transaction,
};
use std::sync::Arc;

/// Helius sender for optimized transaction submission
pub struct HeliusSender {
    rpc_client: Arc<RpcClient>,
    api_key: String,
    keypair_manager: KeypairManager,
}

/// Priority fee estimate from Helius API
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct PriorityFeeEstimate {
    pub priority_fee_estimate: Option<f64>,
    pub priority_fee_levels: Option<PriorityFeeLevels>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct PriorityFeeLevels {
    pub min: f64,
    pub low: f64,
    pub medium: f64,
    pub high: f64,
    pub very_high: f64,
    pub unsafe_max: f64,
}

/// Request payload for priority fee estimation
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct PriorityFeeRequest {
    jsonrpc: String,
    id: String,
    method: String,
    params: Vec<PriorityFeeParams>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct PriorityFeeParams {
    transaction: String,
    options: PriorityFeeOptions,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct PriorityFeeOptions {
    priority_level: String,
}

/// Response from Helius API
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct HeliusResponse<T> {
    result: Option<T>,
    error: Option<HeliusError>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct HeliusError {
    message: String,
}

/// Transaction execution result
#[derive(Debug)]
pub struct ExecutionResult {
    pub signature: Signature,
    pub slot: u64,
    pub compute_units_consumed: Option<u64>,
}

impl HeliusSender {
    /// Create a new Helius sender
    pub fn new(rpc_url: &str, api_key: &str, keypair_manager: KeypairManager) -> Self {
        let rpc_client = Arc::new(RpcClient::new(rpc_url.to_string()));

        Self {
            rpc_client,
            api_key: api_key.to_string(),
            keypair_manager,
        }
    }

    /// Create from environment variables
    pub fn from_env() -> Result<Self, String> {
        let rpc_url = std::env::var("HELIUS_RPC_URL")
            .or_else(|_| std::env::var("SOLANA_RPC_URL"))
            .map_err(|_| "HELIUS_RPC_URL or SOLANA_RPC_URL must be set")?;

        let api_key = std::env::var("HELIUS_API_KEY").unwrap_or_default(); // API key is optional for some operations

        let keypair_manager = KeypairManager::from_env()?;

        Ok(Self::new(&rpc_url, &api_key, keypair_manager))
    }

    /// Get RPC client reference
    pub fn rpc_client(&self) -> &RpcClient {
        &self.rpc_client
    }

    /// Get payer pubkey
    pub fn payer_pubkey(&self) -> Pubkey {
        self.keypair_manager.pubkey()
    }

    /// Simulate transaction and get compute units consumed
    pub async fn simulate_and_get_cu(&self, tx: &Transaction) -> Result<u64, String> {
        let config = RpcSimulateTransactionConfig {
            sig_verify: false,
            replace_recent_blockhash: true,
            ..Default::default()
        };

        let result = self
            .rpc_client
            .simulate_transaction_with_config(tx, config)
            .await
            .map_err(|e| format!("Simulation failed: {}", e))?;

        if let Some(err) = result.value.err {
            return Err(format!("Simulation error: {:?}", err));
        }

        Ok(result.value.units_consumed.unwrap_or(200_000))
    }

    /// Get priority fee estimate from Helius API
    pub async fn get_priority_fee_estimate(
        &self,
        priority_level: PriorityLevel,
    ) -> Result<u64, String> {
        // Fallback to static values if no API key
        if self.api_key.is_empty() {
            return Ok(TransactionBuilder::calculate_priority_fee(priority_level));
        }

        // TODO: Implement actual Helius API call
        // For now, use static values based on priority level
        Ok(TransactionBuilder::calculate_priority_fee(priority_level))
    }

    /// Build and send a smart transaction with optimized fees
    pub async fn send_smart_transaction(
        &self,
        instructions: Vec<Instruction>,
        priority_level: PriorityLevel,
    ) -> Result<ExecutionResult, String> {
        let payer = self.payer_pubkey();

        // Get recent blockhash
        let blockhash = self
            .rpc_client
            .get_latest_blockhash()
            .await
            .map_err(|e| format!("Failed to get blockhash: {}", e))?;

        // Build initial transaction for simulation (without compute budget)
        let mut initial_tx = Transaction::new_with_payer(&instructions, Some(&payer));
        initial_tx.message.recent_blockhash = blockhash;

        // Simulate to get compute units
        let simulated_cu = self.simulate_and_get_cu(&initial_tx).await?;
        let compute_units = TransactionBuilder::estimate_compute_units_with_buffer(simulated_cu);

        log::info!(
            "Simulated CU: {}, Using: {} (with buffer)",
            simulated_cu,
            compute_units
        );

        // Get priority fee
        let priority_fee = self.get_priority_fee_estimate(priority_level).await?;

        log::info!(
            "Priority fee: {} microlamports/CU ({:?})",
            priority_fee,
            priority_level
        );

        // Build optimized transaction
        let mut tx = TransactionBuilder::build_optimized_transaction(
            instructions,
            &payer,
            blockhash,
            compute_units,
            priority_fee,
        );

        // Sign transaction
        self.keypair_manager.sign_transaction(&mut tx)?;

        // Send transaction
        let signature = self
            .rpc_client
            .send_transaction(&tx)
            .await
            .map_err(|e| format!("Failed to send transaction: {}", e))?;

        log::info!("Transaction sent: {}", signature);

        // Confirm transaction
        let confirmation = self
            .rpc_client
            .confirm_transaction(&signature)
            .await
            .map_err(|e| format!("Failed to confirm transaction: {}", e))?;

        if !confirmation {
            return Err("Transaction was not confirmed".to_string());
        }

        Ok(ExecutionResult {
            signature,
            slot: 0, // Would need additional call to get slot
            compute_units_consumed: Some(compute_units as u64),
        })
    }

    /// Send a pre-built transaction (already signed or for simulation)
    pub async fn send_transaction(&self, tx: &Transaction) -> Result<Signature, String> {
        self.rpc_client
            .send_transaction(tx)
            .await
            .map_err(|e| format!("Failed to send transaction: {}", e))
    }
}
