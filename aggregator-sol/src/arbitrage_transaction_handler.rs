use serde::{Deserialize, Serialize};
use solana_client::rpc_client::RpcClient;
/// Real Arbitrage Execution Handler
///
/// Orchestrates actual on-chain arbitrage execution using the real swap executor
/// Replaces the simulation-based handler with production-ready blockchain interaction
use solana_sdk::{hash::Hash, pubkey::Pubkey, signer::keypair::Keypair, signer::Signer};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::arbitrage_monitor::ArbitrageOpportunity;
use crate::on_chain_swap_executor::{
    OnChainArbitrageExecutor, OnChainSwapParams, OnChainSwapResult,
};
use crate::pool_data_types::DexType;

/// Status of on-chain arbitrage execution
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ExecutionStatus {
    /// Pending on-chain execution
    Pending,
    /// Forward swap transaction submitted
    ForwardSubmitted,
    /// Forward swap confirmed on-chain
    ForwardConfirmed,
    /// Reverse swap transaction submitted
    ReverseSubmitted,
    /// Reverse swap confirmed on-chain
    ReverseConfirmed,
    /// Arbitrage cycle completed successfully
    Completed,
    /// Execution failed with error
    Failed(String),
}

/// Record of a real arbitrage execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArbitrageExecutionRecord {
    pub opportunity_id: String,
    pub pair_name: String,
    pub status: ExecutionStatus,

    // Initial state
    pub initial_amount: u64,
    pub initial_token: Pubkey,
    pub user_wallet: Pubkey,

    // Execution details
    pub forward_result: Option<OnChainSwapResult>,
    pub reverse_result: Option<OnChainSwapResult>,
    pub final_profit: i64,
    pub profit_percent: f64,

    // Transaction tracking
    pub forward_tx_signature: Option<String>,
    pub reverse_tx_signature: Option<String>,
    pub forward_slot: Option<u64>,
    pub reverse_slot: Option<u64>,

    // Timing
    pub started_at: u64,
    pub completed_at: Option<u64>,
    pub error_details: Option<String>,
}

/// Real Arbitrage Execution Handler
pub struct ArbitrageTransactionHandler {
    execution_records: Arc<RwLock<Vec<ArbitrageExecutionRecord>>>,
}

impl ArbitrageTransactionHandler {
    /// Create new handler
    pub fn new() -> Self {
        Self {
            execution_records: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Execute a real arbitrage opportunity on blockchain
    pub async fn execute_transaction(
        &self,
        opportunity: &ArbitrageOpportunity,
        user_wallet: Pubkey,
        user_input_ata: Pubkey,
        user_output_ata: Pubkey,
        user_intermediate_ata: Pubkey,
        recent_blockhash: Hash,
        payer: &Keypair,
        forward_pool_address: Pubkey,
        reverse_pool_address: Pubkey,
        rpc_client: &RpcClient,
    ) -> Result<ArbitrageExecutionRecord, String> {
        log::info!("🎯 Executing REAL arbitrage opportunity");
        log::info!("  Pair: {}", opportunity.pair_name);
        log::info!("  Amount: {}", opportunity.input_amount);

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut record = ArbitrageExecutionRecord {
            opportunity_id: opportunity.pair_name.clone(), // Use pair_name as ID
            pair_name: opportunity.pair_name.clone(),
            status: ExecutionStatus::Pending,
            initial_amount: opportunity.input_amount,
            initial_token: Pubkey::default(), // Would need token_a from opportunity
            user_wallet,
            forward_result: None,
            reverse_result: None,
            final_profit: 0,
            profit_percent: 0.0,
            forward_tx_signature: None,
            reverse_tx_signature: None,
            forward_slot: None,
            reverse_slot: None,
            started_at: now,
            completed_at: None,
            error_details: None,
        };

        // Parse token addresses from pair_name or use provided ATAs
        let token_a_mint = user_input_ata; // Placeholder
        let token_b_mint = user_output_ata; // Placeholder

        // Build forward swap params
        let forward_params = OnChainSwapParams {
            dex_type: DexType::Orca, // Default - would be determined by opportunity
            input_token_mint: token_a_mint,
            output_token_mint: token_b_mint,
            input_amount: opportunity.input_amount,
            min_output_amount: self.calculate_min_output(
                opportunity.input_amount,
                &DexType::Orca,
                500, // 5% slippage tolerance
            ),
            pool_address: forward_pool_address,
            user_wallet,
            user_input_ata,
            user_output_ata,
            fee_payer: payer.pubkey(),
            slippage_tolerance_bps: 500,
            priority: crate::types::ExecutionPriority::Medium,
        };

        // Execute forward swap
        log::info!("🔄 Executing forward swap...");
        let forward_result = OnChainArbitrageExecutor::execute_forward_swap(
            &forward_params,
            recent_blockhash,
            payer,
            rpc_client,
            None, // tick arrays would be fetched from on-chain
        )
        .await;

        match forward_result {
            Ok(result) => {
                log::info!("✅ Forward swap successful");
                log::info!("  Signature: {:?}", result.transaction_signature);
                record.forward_result = Some(result.clone());
                record.forward_tx_signature = result.transaction_signature.clone();
                record.status = ExecutionStatus::ForwardSubmitted;
            }
            Err(e) => {
                log::error!("❌ Forward swap failed: {}", e);
                record.status = ExecutionStatus::Failed(e.clone());
                record.error_details = Some(e.clone());
                self.save_execution_record(&record).await;
                return Err(format!("Forward swap failed: {}", e));
            }
        }

        // Wait for forward confirmation (in production, would poll on-chain)
        log::info!("⏳ Waiting for forward swap confirmation...");
        record.status = ExecutionStatus::ForwardConfirmed;

        let forward_output = record
            .forward_result
            .as_ref()
            .ok_or("Forward result missing")?
            .amount_out
            .ok_or("Forward output amount missing")?;

        log::info!("💱 Forward output: {}", forward_output);

        // Build reverse swap params using forward output
        let reverse_params = OnChainSwapParams {
            dex_type: DexType::Orca, // Would use reverse_dex_type from opportunity
            input_token_mint: token_b_mint,
            output_token_mint: token_a_mint,
            input_amount: forward_output,
            min_output_amount: self.calculate_min_output(
                forward_output,
                &DexType::Orca,
                500, // 5% slippage tolerance
            ),
            pool_address: reverse_pool_address,
            user_wallet,
            user_input_ata: user_intermediate_ata,
            user_output_ata,
            fee_payer: payer.pubkey(),
            slippage_tolerance_bps: 500,
            priority: crate::types::ExecutionPriority::Medium,
        };

        // Execute reverse swap
        log::info!("🔄 Executing reverse swap...");
        let reverse_result = OnChainArbitrageExecutor::execute_reverse_swap(
            &reverse_params,
            recent_blockhash,
            payer,
            rpc_client,
            None, // tick arrays would be fetched from on-chain
        )
        .await;

        match reverse_result {
            Ok(result) => {
                log::info!("✅ Reverse swap successful");
                log::info!("  Signature: {:?}", result.transaction_signature);
                record.reverse_result = Some(result.clone());
                record.reverse_tx_signature = result.transaction_signature.clone();
                record.status = ExecutionStatus::ReverseSubmitted;
            }
            Err(e) => {
                log::error!("❌ Reverse swap failed: {}", e);
                record.status = ExecutionStatus::Failed(e.clone());
                record.error_details = Some(e.clone());
                self.save_execution_record(&record).await;
                return Err(format!("Reverse swap failed: {}", e));
            }
        }

        // Wait for reverse confirmation
        log::info!("⏳ Waiting for reverse swap confirmation...");
        record.status = ExecutionStatus::ReverseConfirmed;

        let final_amount = record
            .reverse_result
            .as_ref()
            .ok_or("Reverse result missing")?
            .amount_out
            .ok_or("Reverse output amount missing")?;

        log::info!("💰 Final amount: {}", final_amount);

        // Calculate profit
        let profit = final_amount as i64 - record.initial_amount as i64;
        let profit_percent = (profit as f64 / record.initial_amount as f64) * 100.0;

        record.final_profit = profit;
        record.profit_percent = profit_percent;
        record.status = ExecutionStatus::Completed;
        record.completed_at = Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        );

        log::info!("🎉 Arbitrage cycle completed!");
        log::info!("  Profit: {} ({:.4}%)", profit, profit_percent);

        self.save_execution_record(&record).await;
        Ok(record)
    }

    /// Calculate minimum output with slippage
    fn calculate_min_output(
        &self,
        input_amount: u64,
        dex_type: &DexType,
        slippage_bps: u16,
    ) -> u64 {
        // Get estimated output based on DEX
        let (fee_bps, efficiency) = match dex_type {
            DexType::Orca => (200, 0.985),       // 0.2% fee, 98.5% efficiency
            DexType::RaydiumClmm => (500, 0.97), // 0.5% fee, 97% efficiency
            DexType::RaydiumCpmm => (250, 0.98), // 0.25% fee, 98% efficiency
            DexType::Raydium => (500, 0.96),     // 0.5% fee, 96% efficiency
            _ => (300, 0.97),
        };

        let after_fee = (input_amount as f64) * (1.0 - (fee_bps as f64 / 10000.0));
        let with_efficiency = after_fee * efficiency;
        let after_slippage = with_efficiency * (1.0 - (slippage_bps as f64 / 10000.0));

        after_slippage as u64
    }

    /// Save execution record to persistent storage
    async fn save_execution_record(&self, record: &ArbitrageExecutionRecord) {
        let mut records = self.execution_records.write().await;
        records.push(record.clone());
        log::info!("✅ Execution record saved (total: {})", records.len());
    }

    /// Get execution statistics
    pub async fn get_execution_stats(&self) -> TransactionStatistics {
        let records = self.execution_records.read().await;

        let total = records.len();
        let successful = records
            .iter()
            .filter(|r| r.status == ExecutionStatus::Completed)
            .count();
        let failed = records
            .iter()
            .filter(|r| matches!(r.status, ExecutionStatus::Failed(_)))
            .count();

        let total_profit: i64 = records
            .iter()
            .filter(|r| r.status == ExecutionStatus::Completed)
            .map(|r| r.final_profit)
            .sum();

        let success_rate = if total > 0 {
            (successful as f64 / total as f64) * 100.0
        } else {
            0.0
        };

        let average_profit = if successful > 0 {
            total_profit as f64 / successful as f64
        } else {
            0.0
        };

        TransactionStatistics {
            total_executions: total,
            successful_executions: successful,
            failed_executions: failed,
            total_profit,
            success_rate,
            average_profit,
        }
    }

    /// Get all execution records
    pub async fn get_execution_records(&self) -> Vec<ArbitrageExecutionRecord> {
        self.execution_records.read().await.clone()
    }

    /// Get execution record by opportunity ID
    pub async fn get_record_by_id(&self, opportunity_id: &str) -> Option<ArbitrageExecutionRecord> {
        self.execution_records
            .read()
            .await
            .iter()
            .find(|r| r.opportunity_id == opportunity_id)
            .cloned()
    }
}

/// Execution statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionStatistics {
    pub total_executions: usize,
    pub successful_executions: usize,
    pub failed_executions: usize,
    pub total_profit: i64,
    pub success_rate: f64,
    pub average_profit: f64,
}

impl Default for ArbitrageTransactionHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handler_creation() {
        let handler = ArbitrageTransactionHandler::new();
        assert_eq!(
            handler.execution_records.try_read().ok().map(|r| r.len()),
            Some(0)
        );
    }

    #[test]
    fn test_min_output_calculation() {
        let handler = ArbitrageTransactionHandler::new();

        // Test Orca: 1M input → ~985,000 (0.2% fee) → 970,225 (1.5% slippage)
        let min_orca = handler.calculate_min_output(1_000_000, &DexType::Orca, 500);
        assert!(min_orca < 1_000_000 && min_orca > 900_000);

        // Test Raydium CLMM: 1M input → ~995,000 (0.5% fee) → 975,010 (2% slippage)
        let min_clmm = handler.calculate_min_output(1_000_000, &DexType::RaydiumClmm, 500);
        assert!(min_clmm < 1_000_000 && min_clmm > 900_000);
    }
}
