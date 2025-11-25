// use std::sync::Arc;
// use tokio::sync::RwLock;
// use crate::aggregator::{SwapRoute, SwapStepInternal};
// use crate::pool_data_types::traits::BuildSwapInstruction;
// use crate::pool_data_types::{DexType, GetAmmConfig};
// use solana_client::nonblocking::rpc_client::RpcClient;
// use solana_sdk::{
//     instruction::Instruction,
//     pubkey::Pubkey,
//     signature::Signature,
//     signer::{keypair::Keypair, Signer},
//     transaction::Transaction,
// };

// #[derive(Debug, Clone)]
// pub struct InputSwapParams {
//     pub dex_type: DexType,
//     pub input_token_mint: Pubkey,
//     pub output_token_mint: Pubkey,
//     pub input_amount: u64,
//     pub pool_address: Pubkey,
//     pub slippage_tolerance_bps: u16,
//     pub user_wallet: Pubkey,
// }

// /// Result of on-chain swap execution
// #[derive(Debug, Clone)]
// pub struct OnChainSwapResult {
//     pub success: bool,
//     pub error_message: Option<String>,
//     pub transaction_signature: Option<Signature>,
//     pub input_token_mint: Pubkey,
//     pub output_token_mint: Pubkey,
//     pub input_amount: u64,
//     pub output_amount: u64,
// }

// /// Status of on-chain arbitrage execution
// #[derive(Debug, Clone, PartialEq)]
// pub enum ExecutionStatus {
//     /// Pending on-chain execution
//     Pending,
//     /// Forward swap transaction submitted
//     ForwardSubmitted,
//     /// Forward swap confirmed on-chain
//     ForwardConfirmed,
//     /// Reverse swap transaction submitted
//     ReverseSubmitted,
//     /// Reverse swap confirmed on-chain
//     ReverseConfirmed,
//     /// Arbitrage cycle completed successfully
//     Completed,
//     /// Execution failed with error
//     Failed(String),
// }

// #[derive(Debug, Clone)]
// pub struct ArbitrageExecution {
//     pub forward_route: SwapRoute,
//     pub reverse_route: SwapRoute,
//     pub pair_name: String,
//     pub slippage_tolerance_bps: u16,
//     pub token_a: Pubkey,
//     pub token_b: Pubkey,
//     pub input_amount: u64,
//     pub detected_at: u64,
// }

// /// Record of a real arbitrage execution
// #[derive(Debug, Clone)]
// pub struct ArbitrageExecutionRecord {
//     pub pair_name: String,
//     pub detected_at: u64,
//     pub status: ExecutionStatus,

//     // Initial state
//     pub initial_amount: u64,
//     pub initial_token: Pubkey,
//     pub user_wallet: Pubkey,

//     // Execution details
//     pub forward_result: Option<OnChainSwapResult>,
//     pub reverse_result: Option<OnChainSwapResult>,
//     pub final_profit: i64,
//     pub profit_percent: f64,

//     // Transaction tracking
//     pub forward_tx_signature: Option<String>,
//     pub reverse_tx_signature: Option<String>,

//     // Timing
//     pub started_at: u64,
//     pub completed_at: Option<u64>,
//     pub error_details: Option<String>,
// }

// /// Real Arbitrage Execution Handler
// pub struct ArbitrageTransactionHandler {
//     execution_records: Arc<RwLock<Vec<ArbitrageExecutionRecord>>>,
// }

// impl ArbitrageTransactionHandler {
//     /// Create new handler
//     pub fn new() -> Self {
//         Self {
//             execution_records: Arc::new(RwLock::new(Vec::new())),
//         }
//     }

//     /// Execute a real arbitrage opportunity on blockchain
//     pub async fn execute_arbitrade_transaction(
//         arbitrage_execution: &ArbitrageExecution,
//         payer: &Keypair,
//         rpc_client: &RpcClient,
//         amm_config_fetcher: Arc<dyn GetAmmConfig>,
//     ) -> Result<ArbitrageExecutionRecord, String> {
//         log::info!("🎯 Executing the arbitrage opportunity");
//         log::info!("  Pair: {}", arbitrage_execution.pair_name);
//         log::info!("  Amount: {}", arbitrage_execution.input_amount);

//         let now = std::time::SystemTime::now()
//             .duration_since(std::time::UNIX_EPOCH)
//             .unwrap()
//             .as_secs();

//         let mut record = ArbitrageExecutionRecord {
//             pair_name: arbitrage_execution.pair_name.clone(),
//             detected_at: arbitrage_execution.detected_at,
//             status: ExecutionStatus::Pending,
//             initial_amount: arbitrage_execution.input_amount,
//             initial_token: arbitrage_execution.token_a,
//             user_wallet: payer.pubkey(),
//             forward_result: None,
//             reverse_result: None,
//             final_profit: 0,
//             profit_percent: 0.0,
//             forward_tx_signature: None,
//             reverse_tx_signature: None,
//             started_at: now,
//             completed_at: None,
//             error_details: None,
//         };

//         // Execute forward swap using unified atomic execution
//         log::info!("🔄 Executing forward swap...");
//         let forward_result =
//             Self::execute_swap_route(&arbitrage_execution.forward_route, payer, rpc_client, amm_config_fetcher.clone()).await;

//         match forward_result {
//             Ok(result) => {
//                 log::info!("✅ Forward swap successful");
//                 log::info!("  Signature: {:?}", result.transaction_signature);
//                 record.forward_result = Some(result.clone());
//                 record.forward_tx_signature =
//                     result.transaction_signature.map(|sig| sig.to_string());
//                 record.status = ExecutionStatus::ForwardSubmitted;
//             }
//             Err(e) => {
//                 log::error!("❌ Forward swap failed: {}", e);
//                 record.status = ExecutionStatus::Failed(e.clone());
//                 record.error_details = Some(e.clone());
//                 return Err(format!("Forward swap failed: {}", e));
//             }
//         }

//         // Wait for forward confirmation (in production, would poll on-chain)
//         log::info!("⏳ Waiting for forward swap confirmation...");
//         record.status = ExecutionStatus::ForwardConfirmed;

//         let forward_output = record
//             .forward_result
//             .as_ref()
//             .ok_or("Forward result missing")?
//             .output_amount;

//         log::info!("💱 Forward output: {}", forward_output);

//         // Execute reverse swap using unified atomic execution
//         log::info!("🔄 Executing reverse swap...");
//         let reverse_result =
//             Self::execute_swap_route(&arbitrage_execution.reverse_route, payer, rpc_client, amm_config_fetcher.clone()).await;

//         match reverse_result {
//             Ok(result) => {
//                 log::info!("✅ Reverse swap successful");
//                 log::info!("  Signature: {:?}", result.transaction_signature);
//                 record.reverse_result = Some(result.clone());
//                 record.reverse_tx_signature =
//                     result.transaction_signature.map(|sig| sig.to_string());
//                 record.status = ExecutionStatus::ReverseSubmitted;
//             }
//             Err(e) => {
//                 log::error!("❌ Reverse swap failed: {}", e);
//                 record.status = ExecutionStatus::Failed(e.clone());
//                 record.error_details = Some(e.clone());
//                 return Err(format!("Reverse swap failed: {}", e));
//             }
//         }

//         // Wait for reverse confirmation
//         log::info!("⏳ Waiting for reverse swap confirmation...");
//         record.status = ExecutionStatus::ReverseConfirmed;

//         let final_amount = record
//             .reverse_result
//             .as_ref()
//             .ok_or("Reverse result missing")?
//             .output_amount;

//         log::info!("💰 Final amount: {}", final_amount);

//         // Calculate profit
//         let profit = final_amount as i64 - record.initial_amount as i64;
//         let profit_percent = (profit as f64 / record.initial_amount as f64) * 100.0;

//         record.final_profit = profit;
//         record.profit_percent = profit_percent;
//         record.status = ExecutionStatus::Completed;
//         record.completed_at = Some(
//             std::time::SystemTime::now()
//                 .duration_since(std::time::UNIX_EPOCH)
//                 .unwrap()
//                 .as_secs(),
//         );

//         log::info!("🎉 Arbitrage cycle completed!");
//         log::info!("  Profit: {} ({:.4}%)", profit, profit_percent);

//         Ok(record)
//     }

//     /// Build swap instructions for a single step using the pool state (works for all DEX types)
//     async fn build_step_instructions(
//         step: &SwapStepInternal,
//         slippage_tolerance_bps: u16,
//         payer: &Keypair,
//         amm_config_fetcher: Arc<dyn GetAmmConfig>,
//     ) -> Result<(Vec<Instruction>, u64), String> {
//         let params = InputSwapParams {
//             dex_type: step.dex,
//             input_token_mint: step.input_token,
//             output_token_mint: step.output_token,
//             input_amount: step.input_amount,
//             pool_address: step.pool_address,
//             slippage_tolerance_bps,
//             user_wallet: payer.pubkey(),
//         };

//         // Polymorphic call - works for ANY DEX through PoolState trait!
//         step.pool_state.build_swap_instruction(&params, amm_config_fetcher).await
//     }

//     /// Execute a complete swap route atomically (works for all DEX types)
//     async fn execute_swap_route(
//         swap_route: &SwapRoute,
//         payer: &Keypair,
//         rpc_client: &RpcClient,
//         amm_config_fetcher: Arc<dyn GetAmmConfig>,
//     ) -> Result<OnChainSwapResult, String> {
//         log::info!(
//             "🔄 Executing swap route with {} paths",
//             swap_route.paths.len()
//         );

//         // Collect all instructions from all paths and steps
//         let mut all_instructions = Vec::new();
//         let mut total_expected_output = 0u64;

//         for path in &swap_route.paths {
//             for step in &path.steps {
//                 let (instructions, expected_output) =
//                     Self::build_step_instructions(step, swap_route.slippage_bps, payer, amm_config_fetcher.clone()).await?;

//                 all_instructions.extend(instructions);
//                 total_expected_output += expected_output;

//                 log::debug!(
//                     "  ├─ {:?}: {} -> {} (expected: {})",
//                     step.dex,
//                     step.input_token,
//                     step.output_token,
//                     expected_output
//                 );
//             }
//         }

//         // Build and send ONE atomic transaction for all swaps
//         let blockhash = rpc_client
//             .get_latest_blockhash()
//             .await
//             .map_err(|e| format!("Failed to get blockhash: {}", e))?;

//         let transaction = Transaction::new_signed_with_payer(
//             &all_instructions,
//             Some(&payer.pubkey()),
//             &[payer],
//             blockhash,
//         );
        
//         let sim_result = rpc_client.simulate_transaction(&transaction).await
//             .map_err(|e| format!("Simulation failed: {}", e))?;
        
//         log::info!("✅ Simulate Transaction: pre_balances {:?} post_balances {:?}", 
//             sim_result.value.pre_balances,
//             sim_result.value.post_balances
//         );
        
//         // Check for simulation errors
//         if let Some(err) = sim_result.value.err {
//             log::error!("Simulation error: {:?}", err);
//             if let Some(logs) = sim_result.value.logs {
//                 for log in logs {
//                     log::error!("  {}", log);
//                 }
//             }
//             return Err(format!("Transaction simulation failed: {:?}", err));
//         }
        
//         let signature = transaction.signatures[0];
//         // let signature = rpc_client
//         //     .send_and_confirm_transaction(&transaction)
//         //     .await
//         //     .map_err(|e| format!("Transaction failed: {}", e))?;
//         // log::info!("✅ Transaction confirmed: {}", signature);

//         // Return result with all required fields
//         Ok(OnChainSwapResult {
//             success: true,
//             error_message: None,
//             transaction_signature: Some(signature),
//             input_token_mint: swap_route.input_token,
//             output_token_mint: swap_route.output_token,
//             input_amount: swap_route.input_amount,
//             output_amount: total_expected_output,
//         })
//     }
// }

// // /// Extract output amount from transaction logs or token account
// // async fn extract_output_amount(
// //     rpc_client: &RpcClient,
// //     output_token_mint: &Pubkey,
// //     payer: &Pubkey,
// // ) -> Result<u64, String> {
// //     // // Convert Pubkey to Address for spl_associated_token_account
// //     // let payer_address = Address::from_str(&payer.to_string())
// //     //     .map_err(|_| "Failed to convert payer to Address".to_string())?;
// //     // let mint_address = Address::from_str(&output_token_mint.to_string())
// //     //     .map_err(|_| "Failed to convert mint to Address".to_string())?;

// //     let output_address =
// //         spl_associated_token_account::get_associated_token_address(&payer, &output_token_mint);
// //     // Convert Address back to Pubkey by parsing its string representation
// //     let output_ata = Pubkey::from_str(&output_address.to_string())
// //         .map_err(|_| "Failed to convert output address to Pubkey".to_string())?;
// //     match rpc_client.get_token_account_balance(&output_ata).await {
// //         Ok(balance) => {
// //             log::info!("📊 Token account balance: {}", balance.amount);
// //             Ok(balance
// //                 .amount
// //                 .parse::<u64>()
// //                 .map_err(|e| format!("Failed to parse balance: {}", e))?)
// //         }
// //         Err(e) => Err(format!("Failed to get transaction: {}", e)),
// //     }
// // }
