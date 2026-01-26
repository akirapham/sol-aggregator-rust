// Helius sender for fast transaction submission
// Uses Helius Sender API for ultra-low latency with Jito tips
// Docs: https://www.helius.dev/docs/sending-transactions/sender

use crate::tx_execution::transaction_builder::{PriorityLevel, TransactionBuilder};
use crate::tx_execution::KeypairManager;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_config::RpcSimulateTransactionConfig;
use solana_sdk::{
    instruction::Instruction, native_token::LAMPORTS_PER_SOL, pubkey::Pubkey, signature::Signature,
    transaction::Transaction,
};
// System program ID: 11111111111111111111111111111111
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

/// Helius Sender endpoint - Frankfurt (closest to Finland)
const HELIUS_SENDER_ENDPOINT: &str = "http://fra-sender.helius-rpc.com/fast";

/// Regional endpoints for backend/server applications (lower latency)
#[allow(dead_code)]
const HELIUS_SENDER_REGIONS: [&str; 7] = [
    "http://slc-sender.helius-rpc.com/fast", // Salt Lake City
    "http://ewr-sender.helius-rpc.com/fast", // Newark
    "http://lon-sender.helius-rpc.com/fast", // London
    "http://fra-sender.helius-rpc.com/fast", // Frankfurt
    "http://ams-sender.helius-rpc.com/fast", // Amsterdam
    "http://sg-sender.helius-rpc.com/fast",  // Singapore
    "http://tyo-sender.helius-rpc.com/fast", // Tokyo
];

/// Jito tip accounts for mainnet-beta
/// Transaction must include a tip transfer to one of these accounts
const JITO_TIP_ACCOUNTS: [&str; 8] = [
    "96gYZGLnJYVFmbjzopPSU6QiEV5fGqZNyN9nmNhvrZU5",
    "HFqU5x63VTqvQss8hp11i4bVmkekGTo46t2g26d5so1t",
    "Cw8CFyM9FkoMi7K7Crf6HNQqf4uEMzpKw6QNghXLvLkY",
    "ADaUMid9yfUytqMBgopwjb2DTLSokTSzL1zt6iGPaS49",
    "DfXygSm4jCyNCybVYYK6DwvWqjKee8pbDmJGcLWNDXjh",
    "ADuUkR4vqLUMWXxW9gh6D6L8pMSawimctcNZ5pGwDcEt",
    "DttWaMuVvTiduZRnguLF7jNxTgiMBZ1hyAumKUiL2KRL",
    "3AVi9Tg9Uo68tJfuvoKvqKNWKkC5wPdSSdeBnizKZ6jT",
];

/// Default tip amount: 0.001 SOL = 1,000,000 lamports
pub const DEFAULT_TIP_LAMPORTS: u64 = LAMPORTS_PER_SOL / 1000; // 0.001 SOL

/// Helius sender for optimized transaction submission
#[allow(dead_code)]
pub struct HeliusSender {
    rpc_client: Arc<RpcClient>,
    http_client: Client,
    sender_endpoint: String,
    api_key: String,
    keypair_manager: KeypairManager,
    tip_lamports: u64,
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

/// JSON-RPC request for sendTransaction
#[derive(Debug, Serialize)]
struct SendTransactionRequest {
    jsonrpc: &'static str,
    id: String,
    method: &'static str,
    params: (String, SendTransactionOptions),
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SendTransactionOptions {
    encoding: &'static str,
    skip_preflight: bool,
    max_retries: u8,
}

/// JSON-RPC response
#[derive(Debug, Deserialize)]
struct JsonRpcResponse<T> {
    result: Option<T>,
    error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcError {
    code: i64,
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
    /// Create a new Helius sender with default tip (0.001 SOL)
    pub fn new(rpc_url: &str, api_key: &str, keypair_manager: KeypairManager) -> Self {
        Self::with_tip(rpc_url, api_key, keypair_manager, DEFAULT_TIP_LAMPORTS)
    }

    /// Create a new Helius sender with custom tip amount
    pub fn with_tip(
        rpc_url: &str,
        api_key: &str,
        keypair_manager: KeypairManager,
        tip_lamports: u64,
    ) -> Self {
        let rpc_client = Arc::new(RpcClient::new(rpc_url.to_string()));

        // Build HTTP client with optimal settings for low latency
        let http_client = Client::builder()
            .timeout(Duration::from_secs(30))
            .pool_max_idle_per_host(10)
            .tcp_keepalive(Some(Duration::from_secs(60)))
            .build()
            .expect("Failed to build HTTP client");

        // Use Frankfurt endpoint (closest to Finland)
        let sender_endpoint = if api_key.is_empty() {
            HELIUS_SENDER_ENDPOINT.to_string()
        } else {
            format!("{}?api-key={}", HELIUS_SENDER_ENDPOINT, api_key)
        };

        Self {
            rpc_client,
            http_client,
            sender_endpoint,
            api_key: api_key.to_string(),
            keypair_manager,
            tip_lamports,
        }
    }

    /// Create from environment variables
    /// Requires: HELIUS_RPC_URL or SOLANA_RPC_URL
    /// Optional: HELIUS_API_KEY, HELIUS_TIP_LAMPORTS
    pub fn from_env() -> Result<Self, String> {
        let rpc_url = std::env::var("HELIUS_RPC_URL")
            .or_else(|_| std::env::var("SOLANA_RPC_URL"))
            .map_err(|_| "HELIUS_RPC_URL or SOLANA_RPC_URL must be set")?;

        let api_key = std::env::var("HELIUS_API_KEY").unwrap_or_default();

        let tip_lamports: u64 = std::env::var("HELIUS_TIP_LAMPORTS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_TIP_LAMPORTS);

        let keypair_manager = KeypairManager::from_env()?;

        log::info!(
            "HeliusSender initialized: tip={} lamports ({:.4} SOL)",
            tip_lamports,
            tip_lamports as f64 / LAMPORTS_PER_SOL as f64
        );

        Ok(Self::with_tip(
            &rpc_url,
            &api_key,
            keypair_manager,
            tip_lamports,
        ))
    }

    /// Get RPC client reference (for simulation and other RPC calls)
    pub fn rpc_client(&self) -> &RpcClient {
        &self.rpc_client
    }

    /// Get payer pubkey
    pub fn payer_pubkey(&self) -> Pubkey {
        self.keypair_manager.pubkey()
    }

    /// Get a random Jito tip account
    fn get_random_tip_account() -> Pubkey {
        use rand::Rng;
        let mut rng = rand::rng();
        let idx = rng.random_range(0..JITO_TIP_ACCOUNTS.len());
        let tip_account_str = JITO_TIP_ACCOUNTS[idx];
        Pubkey::from_str(tip_account_str).expect("Invalid tip account pubkey")
    }

    /// Create a tip instruction (SOL transfer to Jito tip account)
    pub fn create_tip_instruction(&self, payer: &Pubkey) -> Instruction {
        use solana_sdk::instruction::AccountMeta;

        let tip_account = Self::get_random_tip_account();
        log::debug!("Tip: {} lamports to {}", self.tip_lamports, tip_account);

        // System program transfer instruction (instruction index 2)
        // Layout: 4 bytes instruction index + 8 bytes lamports
        let mut data = vec![2, 0, 0, 0]; // Transfer instruction = 2
        data.extend_from_slice(&self.tip_lamports.to_le_bytes());

        // System Program ID is all zeros (11111111111111111111111111111111 in base58)
        let system_program_id = Pubkey::default();

        Instruction {
            program_id: system_program_id,
            accounts: vec![
                AccountMeta::new(*payer, true),       // from (writable, signer)
                AccountMeta::new(tip_account, false), // to (writable, not signer)
            ],
            data,
        }
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

    /// Get priority fee estimate (static for now, can be enhanced with Helius Priority Fee API)
    pub async fn get_priority_fee_estimate(
        &self,
        priority_level: PriorityLevel,
    ) -> Result<u64, String> {
        // Use static values based on priority level
        // TODO: Implement Helius Priority Fee API for dynamic estimation
        Ok(TransactionBuilder::calculate_priority_fee(priority_level))
    }

    /// Send transaction via Helius Sender API
    /// This is the core method that uses the fast sender endpoint with Jito routing
    async fn send_via_helius_sender(&self, tx: &Transaction) -> Result<Signature, String> {
        // Serialize and encode transaction to base64
        let tx_bytes = bincode::serde::encode_to_vec(tx, bincode::config::standard())
            .map_err(|e| format!("Failed to serialize transaction: {}", e))?;
        let tx_base64 = BASE64.encode(&tx_bytes);

        // Build JSON-RPC request
        let request = SendTransactionRequest {
            jsonrpc: "2.0",
            id: uuid::Uuid::new_v4().to_string(),
            method: "sendTransaction",
            params: (
                tx_base64,
                SendTransactionOptions {
                    encoding: "base64",
                    skip_preflight: true, // REQUIRED by Helius Sender
                    max_retries: 0,       // Let Helius handle retries
                },
            ),
        };

        // Send to Helius Sender
        let response = self
            .http_client
            .post(&self.sender_endpoint)
            .json(&request)
            .send()
            .await
            .map_err(|e| format!("Failed to send to Helius Sender: {}", e))?;

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| format!("Failed to read response: {}", e))?;

        if !status.is_success() {
            return Err(format!("Helius Sender error ({}): {}", status, body));
        }

        // Parse response
        let rpc_response: JsonRpcResponse<String> = serde_json::from_str(&body)
            .map_err(|e| format!("Failed to parse response: {} - body: {}", e, body))?;

        if let Some(error) = rpc_response.error {
            return Err(format!(
                "Helius Sender RPC error ({}): {}",
                error.code, error.message
            ));
        }

        let signature_str = rpc_response.result.ok_or("No signature in response")?;

        let signature =
            Signature::from_str(&signature_str).map_err(|e| format!("Invalid signature: {}", e))?;

        log::info!("🚀 Transaction sent via Helius Sender: {}", signature);
        Ok(signature)
    }

    /// Build and send a smart transaction with optimized fees and Jito tip
    /// This includes:
    /// 1. Tip instruction (0.001 SOL to Jito)
    /// 2. Compute budget instructions (CU limit + priority fee)
    /// 3. User instructions
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

        // Create tip instruction
        let tip_ix = self.create_tip_instruction(&payer);

        // Combine tip + user instructions for simulation
        let mut all_instructions = vec![tip_ix.clone()];
        all_instructions.extend(instructions.clone());

        // Build initial transaction for simulation (without compute budget)
        let mut initial_tx = Transaction::new_with_payer(&all_instructions, Some(&payer));
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
            "Priority fee: {} microlamports/CU ({:?}), Tip: {} lamports",
            priority_fee,
            priority_level,
            self.tip_lamports
        );

        // Build optimized transaction with tip included
        let final_instructions = {
            let mut ixs = vec![tip_ix];
            ixs.extend(instructions);
            ixs
        };

        let mut tx = TransactionBuilder::build_optimized_transaction(
            final_instructions,
            &payer,
            blockhash,
            compute_units,
            priority_fee,
        );

        // Sign transaction
        self.keypair_manager.sign_transaction(&mut tx)?;

        // Send via Helius Sender API
        let signature = self.send_via_helius_sender(&tx).await?;

        Ok(ExecutionResult {
            signature,
            slot: 0, // Slot is not returned by sender, would need confirmation call
            compute_units_consumed: Some(compute_units as u64),
        })
    }

    /// Send a pre-built transaction via Helius Sender
    /// Note: Transaction should already include tip instruction
    pub async fn send_transaction(&self, tx: &Transaction) -> Result<Signature, String> {
        self.send_via_helius_sender(tx).await
    }

    /// Send transaction via standard RPC (fallback)
    pub async fn send_transaction_rpc(&self, tx: &Transaction) -> Result<Signature, String> {
        self.rpc_client
            .send_transaction(tx)
            .await
            .map_err(|e| format!("Failed to send transaction: {}", e))
    }

    /// Warm up connection to Helius Sender endpoint
    /// Call this at startup to reduce latency on first transaction
    pub async fn warm_connection(&self) -> Result<(), String> {
        let ping_endpoint = self.sender_endpoint.replace("/fast", "/ping");

        self.http_client
            .get(&ping_endpoint)
            .send()
            .await
            .map_err(|e| format!("Failed to warm connection: {}", e))?;

        log::debug!("🔥 Helius Sender connection warmed");
        Ok(())
    }

    /// Spawn a background task that keeps the connection warm
    /// Pings every 30 seconds as recommended by Helius docs
    /// https://www.helius.dev/docs/sending-transactions/sender#connection-warming
    pub fn spawn_connection_warmer(self: Arc<Self>) {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(30));

            log::info!("🔥 Starting Helius Sender connection warmer (every 30s)");

            loop {
                interval.tick().await;

                if let Err(e) = self.warm_connection().await {
                    log::warn!("Connection warming failed: {}", e);
                }
            }
        });
    }

    /// Get current tip amount in lamports
    pub fn tip_lamports(&self) -> u64 {
        self.tip_lamports
    }
}
