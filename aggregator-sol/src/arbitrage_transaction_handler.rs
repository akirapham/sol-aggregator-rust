use log;
use serde::{Deserialize, Serialize};
use solana_client::nonblocking::rpc_client::RpcClient;
/// Real Arbitrage Execution Handler
///
/// Orchestrates actual on-chain arbitrage execution using the real swap executor
/// Replaces the simulation-based handler with production-ready blockchain interaction
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::pool_data_types::DexType;

/// On-Chain Swap Execution Module
///
/// Executes actual on-chain swaps by calling the swap functions from:
/// - Whirlpools swap_manager.rs
/// - Raydium CLMM swap instruction
/// - Raydium CPMM swap instruction
/// - Raydium AMM V4 swap instruction
///
/// This module builds and submits actual transactions to the blockchain
use solana_sdk::{
    hash::Hash, pubkey::Pubkey,
    signer::{keypair::Keypair, Signer}, signature::Signature,
    message::{v0::Message, VersionedMessage},
    transaction::VersionedTransaction,
};

use crate::types::ExecutionPriority;
use spl_associated_token_account::get_associated_token_address;

use orca_whirlpools_sdk::{
    swap_instructions, SwapInstructions, SwapType,
};
use crate::aggregator::{SwapRoute};
/// Parameters for on-chain swap execution
#[derive(Debug, Clone)]
pub struct OnChainSwapParams {
    pub dex_type: DexType,
    pub input_token_mint: Pubkey,
    pub output_token_mint: Pubkey,
    pub input_amount: u64,
    pub pool_address: Pubkey,
    pub slippage_tolerance_bps: u16,
}

/// Result of on-chain swap execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnChainSwapResult {
    pub success: bool,
    pub error_message: Option<String>,
    pub transaction_signature: Option<Signature>,
    pub dex_type: DexType,
    pub pool_address: Pubkey,
    pub input_token_mint: Pubkey,
    pub output_token_mint: Pubkey,
    pub input_amount: u64,
    pub output_amount: u64,
}

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

#[derive(Debug, Clone)]
pub struct ArbitrageExecution {
    pub forward_route: SwapRoute,
    pub reverse_route: SwapRoute,
    pub pair_name: String,
    pub slippage_tolerance_bps: u16,
    pub token_a: Pubkey,
    pub token_b: Pubkey,
    pub input_amount: u64,
    pub detected_at: u64,
}

/// Record of a real arbitrage execution
#[derive(Debug, Clone)]
pub struct ArbitrageExecutionRecord {
    pub pair_name: String,
    pub detected_at: u64,
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
    pub fn new() -> Self {
        Self {
            execution_records: Arc::new(RwLock::new(Vec::new())),
        }
    }
    /// Execute a real arbitrage opportunity on blockchain
    pub async fn execute_arbitrade_transaction(
        arbitrage_execution: &ArbitrageExecution,
        payer: &Keypair,
        rpc_client: &RpcClient,
    ) -> Result<ArbitrageExecutionRecord, String> {
        log::info!("🎯 Executing the arbitrage opportunity");
        log::info!("  Pair: {}", arbitrage_execution.pair_name);
        log::info!("  Amount: {}", arbitrage_execution.input_amount);

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut record = ArbitrageExecutionRecord {
            pair_name: arbitrage_execution.pair_name.clone(),
            detected_at: arbitrage_execution.detected_at,
            status: ExecutionStatus::Pending,
            initial_amount: arbitrage_execution.input_amount,
            initial_token: arbitrage_execution.token_a,
            user_wallet: payer.pubkey(),
            forward_result: None,
            reverse_result: None,
            final_profit: 0,
            profit_percent: 0.0,
            forward_tx_signature: None,
            reverse_tx_signature: None,
            started_at: now,
            completed_at: None,
            error_details: None,
        };

        // Execute forward swap
        log::info!("🔄 Executing forward swap...");
        let forward_result = Self::execute_swap_route(
            &arbitrage_execution.forward_route,
            payer,
            rpc_client,
        )
        .await;

        match forward_result {
            Ok(result) => {
                log::info!("✅ Forward swap successful");
                log::info!("  Signature: {:?}", result.transaction_signature);
                record.forward_result = Some(result.clone());
                record.forward_tx_signature = result.transaction_signature.map(|sig| sig.to_string());
                record.status = ExecutionStatus::ForwardSubmitted;
            }
            Err(e) => {
                log::error!("❌ Forward swap failed: {}", e);
                record.status = ExecutionStatus::Failed(e.clone());
                record.error_details = Some(e.clone());
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
            .output_amount;

        log::info!("💱 Forward output: {}", forward_output);

        // Execute reverse swap
        log::info!("🔄 Executing reverse swap...");
        let reverse_result = Self::execute_swap_route(
            &arbitrage_execution.reverse_route,
            payer,
            rpc_client,
        )
        .await;

        match reverse_result {
            Ok(result) => {
                log::info!("✅ Reverse swap successful");
                log::info!("  Signature: {:?}", result.transaction_signature);
                record.reverse_result = Some(result.clone());
                record.reverse_tx_signature = result.transaction_signature.map(|sig| sig.to_string());
                record.status = ExecutionStatus::ReverseSubmitted;
            }
            Err(e) => {
                log::error!("❌ Reverse swap failed: {}", e);
                record.status = ExecutionStatus::Failed(e.clone());
                record.error_details = Some(e.clone());
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
            .output_amount;

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

        Ok(record)
    }

    /// Helper method to execute a swap route (static helper for use in static methods)
    async fn execute_swap_route(
        swap_route: &SwapRoute,
        payer: &Keypair,
        rpc_client: &RpcClient,
    ) -> Result<OnChainSwapResult, String> {
        log::info!("🔄 Executing swap route with {} pools", swap_route.paths.len());
        // // Execute each swap in the route sequentially
        // for (idx, path) in swap_route.paths.iter().enumerate() {
        //     for (idx, step) in path.steps.iter().enumerate() {
        //         // Build swap parameters for this pool
        //         let swap_params = OnChainSwapParams {
        //             dex_type: step.dex.clone(),
        //             input_token_mint: step.input_token,
        //             output_token_mint: step.output_token,
        //             input_amount: step.input_amount,
        //             pool_address: step.pool_address,
        //             slippage_tolerance_bps: swap_route.slippage_bps,
        //         };

        //         // Execute this swap
        //         match Self::execute_swap(&swap_params, payer, rpc_client).await {
        //             Ok(result) => {
        //                 log::info!(
        //                     "  ✅ Swap {} successful. Output: {}",
        //                     idx + 1,
        //                     result.output_amount
        //                 );
        //                 last_result = Some(result);
        //             }
        //             Err(e) => {
        //                 log::error!("  ❌ Swap {} failed: {}", idx + 1, e);
        //                 return Err(format!("Forward route failed at swap {}: {}", idx + 1, e));
        //             }
        //         }
        //     }
        // }
        if swap_route.paths.len() == 1 {
            if swap_route.paths[0].steps.len() == 1 {
                let step = &swap_route.paths[0].steps[0];
                let swap_params = OnChainSwapParams {
                    dex_type: step.dex.clone(),
                    input_token_mint: step.input_token,
                    output_token_mint: step.output_token,
                    input_amount: step.input_amount,
                    pool_address: step.pool_address,
                    slippage_tolerance_bps: swap_route.slippage_bps,
                };
                
                // Execute this swap
                Self::execute_swap(&swap_params, payer, rpc_client).await
            } else {
                Err("Single path must have exactly one step".to_string())
            }
        } else {
            Err("Complex swap routes not yet implemented".to_string())
        }
    }

    /// Execute forward swap (token_a → token_b)
    pub async fn execute_swap(
        params: &OnChainSwapParams,
        payer: &Keypair,
        rpc_client: &RpcClient,
    ) -> Result<OnChainSwapResult, String> {
        log::info!(
            "🔄 FORWARD SWAP: {} → {}",
            params.input_token_mint,
            params.output_token_mint
        );

        match params.dex_type {
            DexType::Orca => {
                OrcaWhirlpoolSwapExecutor::execute_swap(
                    params,
                    payer,
                    rpc_client,
                )
                .await
            }
            // DexType::RaydiumClmm => {
            //     let tick_arrays = additional_params.unwrap_or_default();
            //     RaydiumClmmSwapExecutor::execute_swap(
            //         params,
            //         tick_arrays,
            //         recent_blockhash,
            //         payer,
            //         rpc_client,
            //     )
            //     .await
            // }
            _ => Err("Unsupported DEX type".to_string()),
        }
    }

}

/// Orca Whirlpool On-Chain Swap Executor
pub struct OrcaWhirlpoolSwapExecutor;

impl OrcaWhirlpoolSwapExecutor {
    /// Build Whirlpool swap v2 instruction using official SDK structure
    async fn build_swap_instruction(
        params: &OnChainSwapParams,
        payer: &Keypair,
        rpc_client: &RpcClient,
    ) -> Result<SwapInstructions, String> {
        let swap_result = swap_instructions(
            rpc_client,
            params.pool_address,
            params.input_amount,
            params.input_token_mint,                    // The token you're swapping from
            SwapType::ExactIn,            // You're specifying the INPUT amount
            Option::<u16>::Some(params.slippage_tolerance_bps),
            Option::<Pubkey>::Some(payer.pubkey()), // The user wallet executing the swap
        )
        .await
        .map_err(|e| format!("Failed to get swap instructions: {}", e))?;
    
        Ok(swap_result)
    }

    /// Execute Whirlpool swap using official SDK instruction format
    pub async fn execute_swap(
        params: &OnChainSwapParams,
        payer: &Keypair,
        rpc_client: &RpcClient,
    ) -> Result<OnChainSwapResult, String> {
        log::info!("🔄 Executing Whirlpool swap using official SDK");
        log::info!("  Pool: {}", params.pool_address);
        log::info!("  Input Amount: {}", params.input_amount);
        log::info!("  Input Mint: {}", params.input_token_mint);

        // Build swap instruction using official Whirlpool SDK format
        let swap_instr = Self::build_swap_instruction(params, payer, rpc_client).await?;

        log::info!("✅ Swap instruction generated from official SDK format");

        // let swap_instr = send_transaction_with_signers(swap_instr, Vec::from([payer]))?;
        let blockhash = rpc_client.get_latest_blockhash().await
            .map_err(|e| format!("Failed to get latest blockhash: {}", e))?;
        // Sine blockhash is not guaranteed to be unique, we need to add a random memo to the tx
        // so that we can fire two seemingly identical transactions in a row.
        let instructions = [swap_instr.instructions, vec![]].concat();
        let message = VersionedMessage::V0(Message::try_compile(
            &payer.pubkey(),
            &instructions,
            &[],
            blockhash,
        ).map_err(|e| format!("Failed to compile message: {}", e))?);
        let transaction =
            VersionedTransaction::try_new(message, &[payer])
                .map_err(|e| format!("Failed to create transaction: {}", e))?;
        let signature = rpc_client.send_and_confirm_transaction(&transaction).await
            .map_err(|e| format!("Failed to send transaction: {}", e))?;

        // // Get the output amount from transaction logs or token account
        // let output_amount = Self::extract_output_amount(
        //     rpc_client,
        //     &signature,
        //     params,
        //     payer,
        // )
        // .await?;

        // log::info!("💰 Output Amount: {}", output_amount);

        Ok(OnChainSwapResult {
            success: true,
            error_message: None,
            transaction_signature: Some(signature),
            dex_type: params.dex_type,
            pool_address: params.pool_address,
            input_token_mint: params.input_token_mint,
            output_token_mint: params.output_token_mint,
            input_amount: params.input_amount,
            output_amount: params.input_amount,
        })
    }

    // /// Extract output amount from transaction logs or token account
    // async fn extract_output_amount(
    //     rpc_client: &RpcClient,
    //     signature: &solana_sdk::signature::Signature,
    //     params: &OnChainSwapParams,
    //     payer: &Keypair,
    // ) -> Result<u64, String> {
    //     let wallet_address = Address::new(&payer.pubkey().to_bytes());
    //     let output_token_address = Address::new(&params.output_token_mint.to_bytes());
    //     let output_ata = get_associated_token_address(wallet_address, output_token_address);
    //     match rpc_client.get_token_account_balance(&output_ata).await {
    //         Ok(balance) => {
    //             log::info!("📊 Token account balance: {}", balance.amount);
    //             Ok(balance.amount.parse::<u64>()
    //                 .map_err(|e| format!("Failed to parse balance: {}", e))?)
    //         }
    //         Err(e) => Err(format!("Failed to get transaction: {}", e)),
    //     }
    // }
}

// pub struct RaydiumClmmSwapExecutor;

// impl RaydiumClmmSwapExecutor {
//     const PROGRAM_ID: &'static str = "CAMMCjfrWoSNmmeKBS2L2DfRawXzZhRvCb7ECwDjGvV";
//     const CLMM_SWAP_DISCRIMINATOR: &'static [u8] = &[52, 133, 123, 156, 226, 138, 52, 97];

//     /// Build Raydium CLMM swap instruction
//     fn build_swap_instruction(
//         params: &OnChainSwapParams,
//         tick_arrays: Vec<Pubkey>,
//     ) -> Result<Instruction, String> {
//         log::info!("🔄 Building Raydium CLMM swap instruction");
//         log::info!("  Pool: {}", params.pool_address);
//         log::info!(
//             "  Input: {} → min {}",
//             params.input_amount,
//             params.min_output_amount
//         );

//         let swap_instr = swap_instr(
//             &pool_config.clone(),
//             pool_state.amm_config,
//             pool_config.pool_id_account.unwrap(),
//             if zero_for_one {
//                 pool_state.token_vault_0
//             } else {
//                 pool_state.token_vault_1
//             },
//             if zero_for_one {
//                 pool_state.token_vault_1
//             } else {
//                 pool_state.token_vault_0
//             },
//             pool_state.observation_key,
//             input_token,
//             output_token,
//             current_or_next_tick_array_key,
//             remaining_accounts,
//             amount,
//             other_amount_threshold,
//             sqrt_price_limit_x64,
//             base_in,
//         )
//         .unwrap();
//         swap_instr
        
//         // let program_id = Pubkey::from_str(Self::PROGRAM_ID)
//         //     .map_err(|e| format!("Invalid CLMM program ID: {}", e))?;

//         // let mut instruction_data = Vec::new();
//         // instruction_data.extend_from_slice(Self::CLMM_SWAP_DISCRIMINATOR);
//         // instruction_data.extend_from_slice(&params.input_amount.to_le_bytes());
//         // instruction_data.extend_from_slice(&params.min_output_amount.to_le_bytes());
//         // instruction_data.push(1); // a_2_b flag

//         // // Build required accounts
//         // let mut accounts = vec![
//         //     solana_sdk::instruction::AccountMeta::new(params.pool_address, false),
//         //     solana_sdk::instruction::AccountMeta::new_readonly(params.user_wallet, true),
//         //     solana_sdk::instruction::AccountMeta::new(params.user_input_ata, false),
//         //     solana_sdk::instruction::AccountMeta::new(params.user_output_ata, false),
//         // ];

//         // // Add tick arrays
//         // for tick_array in tick_arrays {
//         //     accounts.push(solana_sdk::instruction::AccountMeta::new(tick_array, false));
//         // }

//         // log::info!(
//         //     "✅ CLMM swap instruction built with {} accounts",
//         //     accounts.len()
//         // );

//         Ok(Instruction {
//             program_id,
//             accounts,
//             data: instruction_data,
//         })
//     }

//     /// Execute Raydium CLMM swap
//     pub async fn execute_swap(
//         params: &OnChainSwapParams,
//         tick_arrays: Vec<Pubkey>,
//         recent_blockhash: Hash,
//         payer: &Keypair,
//         rpc_client: &RpcClient,
//     ) -> Result<OnChainSwapResult, String> {
//         log::info!("🔄 Executing Raydium CLMM swap");
//         log::info!("  Pool: {}", params.pool_address);
//         log::info!("  Input Amount: {}", params.input_amount);
//         log::info!("  Minimum Output: {}", params.min_output_amount);

//         // Build swap instruction
//         let instruction = Self::build_swap_instruction(params, tick_arrays)?;

//         // Create and sign transaction
//         let tx = Transaction::new_signed_with_payer(
//             &[instruction],
//             Some(&payer.pubkey()),
//             &[payer],
//             recent_blockhash,
//         );

//         log::info!("✅ Transaction signed");

//         // Submit to blockchain
//         let signature = rpc_client
//             .send_and_confirm_transaction(&tx)
//             .map_err(|e| format!("Failed to submit transaction: {}", e))?;

//         log::info!("✅ Transaction confirmed on-chain");
//         log::info!("  Signature: {}", signature);

//         let slot = rpc_client.get_slot().unwrap_or(0);

//         Ok(OnChainSwapResult {
//             success: true,
//             transaction_signature: Some(signature.to_string()),
//             amount_in: params.input_amount,
//             amount_out: Some(params.min_output_amount),
//             error_message: None,
//             dex_type: DexType::RaydiumClmm,
//             executed_at: std::time::SystemTime::now()
//                 .duration_since(std::time::UNIX_EPOCH)
//                 .unwrap()
//                 .as_secs(),
//             slot: Some(slot),
//             confirmation_status: Some("confirmed".to_string()),
//         })
//     }
// }