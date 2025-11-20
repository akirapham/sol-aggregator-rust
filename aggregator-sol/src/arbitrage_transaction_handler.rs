use log;
use serde::{Deserialize, Serialize};
use solana_address::Address;
use solana_client::nonblocking::rpc_client::RpcClient;
use std::str::FromStr;
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
    compute_budget::ComputeBudgetInstruction,
    instruction::Instruction,
    message::{v0::Message, VersionedMessage},
    pubkey::Pubkey,
    signature::Signature,
    signer::{keypair::Keypair, Signer},
    transaction::Transaction,
    transaction::VersionedTransaction,
};

use spl_associated_token_account;

use orca_whirlpools_sdk::{swap_instructions, SwapInstructions, SwapQuote, SwapType};

use raydium_clmm_client::{
    config::{load_cfg, ClientConfig},
    instructions::amm_instructions::swap_instr,
    instructions::utils,
    raydium_amm_v3::states,
};

use anchor_lang::prelude::AccountMeta;
use arrayref::array_ref;

use crate::aggregator::SwapRoute;
use crate::pool_data_types::raydium_clmm::RaydiumClmmPoolState;
use spl_token_2022::{extension::StateWithExtensions, state::Account};
use std::collections::VecDeque;
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
        let forward_result =
            Self::execute_swap_route(&arbitrage_execution.forward_route, payer, rpc_client).await;

        match forward_result {
            Ok(result) => {
                log::info!("✅ Forward swap successful");
                log::info!("  Signature: {:?}", result.transaction_signature);
                record.forward_result = Some(result.clone());
                record.forward_tx_signature =
                    result.transaction_signature.map(|sig| sig.to_string());
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
        let reverse_result =
            Self::execute_swap_route(&arbitrage_execution.reverse_route, payer, rpc_client).await;

        match reverse_result {
            Ok(result) => {
                log::info!("✅ Reverse swap successful");
                log::info!("  Signature: {:?}", result.transaction_signature);
                record.reverse_result = Some(result.clone());
                record.reverse_tx_signature =
                    result.transaction_signature.map(|sig| sig.to_string());
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
        log::info!(
            "🔄 Executing swap route with {} pools",
            swap_route.paths.len()
        );
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
                OrcaWhirlpoolSwapExecutor::execute_swap(params, payer, rpc_client).await
            }
            DexType::RaydiumClmm => {
                RaydiumClmmSwapExecutor::execute_swap(params, payer, rpc_client).await
            }
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
            params.input_token_mint, // The token you're swapping from
            SwapType::ExactIn,       // You're specifying the INPUT amount
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
        let blockhash = rpc_client
            .get_latest_blockhash()
            .await
            .map_err(|e| format!("Failed to get latest blockhash: {}", e))?;
        // Sine blockhash is not guaranteed to be unique, we need to add a random memo to the tx
        // so that we can fire two seemingly identical transactions in a row.
        let instructions = [swap_instr.instructions, vec![]].concat();
        let message = VersionedMessage::V0(
            Message::try_compile(&payer.pubkey(), &instructions, &[], blockhash)
                .map_err(|e| format!("Failed to compile message: {}", e))?,
        );
        let transaction = VersionedTransaction::try_new(message, &[payer])
            .map_err(|e| format!("Failed to create transaction: {}", e))?;
        let signature = rpc_client
            .send_and_confirm_transaction(&transaction)
            .await
            .map_err(|e| format!("Failed to send transaction: {}", e))?;

        // Get the output amount from transaction logs or token account
        let output_amount =
            extract_output_amount(rpc_client, &params.output_token_mint, &payer.pubkey()).await?;

        let mut est_output_amount = 0;
        match &swap_instr.quote {
            SwapQuote::ExactIn(q) => {
                est_output_amount = q.token_est_out;
            }
            SwapQuote::ExactOut(_) => {
                est_output_amount = 0;
            }
        }
        log::info!("💰 Output Amount: {}", est_output_amount);

        Ok(OnChainSwapResult {
            success: true,
            error_message: None,
            transaction_signature: Some(signature),
            dex_type: params.dex_type,
            pool_address: params.pool_address,
            input_token_mint: params.input_token_mint,
            output_token_mint: params.output_token_mint,
            input_amount: params.input_amount,
            output_amount: output_amount,
        })
    }
}

pub struct RaydiumClmmSwapExecutor;

impl RaydiumClmmSwapExecutor {
    async fn load_cur_and_next_five_tick_array(
        rpc_client: &RpcClient,
        pool_config: &ClientConfig,
        pool_state: &states::PoolState,
        tickarray_bitmap_extension: &states::TickArrayBitmapExtension,
        zero_for_one: bool,
    ) -> VecDeque<states::TickArrayState> {
        let (_, mut current_valid_tick_array_start_index) = pool_state
            .get_first_initialized_tick_array(&Some(*tickarray_bitmap_extension), zero_for_one)
            .unwrap();
        let mut tick_array_keys = Vec::new();
        tick_array_keys.push(
            Pubkey::find_program_address(
                &[
                    states::TICK_ARRAY_SEED.as_bytes(),
                    pool_config.pool_id_account.unwrap().to_bytes().as_ref(),
                    &current_valid_tick_array_start_index.to_be_bytes(),
                ],
                &pool_config.raydium_v3_program,
            )
            .0,
        );
        let mut max_array_size = 5;
        while max_array_size != 0 {
            let next_tick_array_index = pool_state
                .next_initialized_tick_array_start_index(
                    &Some(*tickarray_bitmap_extension),
                    current_valid_tick_array_start_index,
                    zero_for_one,
                )
                .unwrap();
            if next_tick_array_index.is_none() {
                break;
            }
            current_valid_tick_array_start_index = next_tick_array_index.unwrap();
            tick_array_keys.push(
                Pubkey::find_program_address(
                    &[
                        states::TICK_ARRAY_SEED.as_bytes(),
                        pool_config.pool_id_account.unwrap().to_bytes().as_ref(),
                        &current_valid_tick_array_start_index.to_be_bytes(),
                    ],
                    &pool_config.raydium_v3_program,
                )
                .0,
            );
            max_array_size -= 1;
        }
        let tick_array_rsps = rpc_client
            .get_multiple_accounts(&tick_array_keys)
            .await
            .unwrap();
        let mut tick_arrays = VecDeque::new();
        for tick_array in tick_array_rsps {
            let tick_array_state =
                utils::deserialize_anchor_account::<states::TickArrayState>(&tick_array.unwrap())
                    .unwrap();
            tick_arrays.push_back(tick_array_state);
        }
        tick_arrays
    }

    /// Build Raydium CLMM swap instruction
    async fn build_swap_instruction(
        params: &OnChainSwapParams,
        payer: &Keypair,
        rpc_client: &RpcClient,
    ) -> Result<(Vec<Instruction>, u64), String> {
        log::info!("🔄 Building Raydium CLMM swap instruction");
        log::info!("  Pool: {}", params.pool_address);

        let client_config = "raydium_clmm_client_config.ini";
        let pool_config = load_cfg(&client_config.to_string()).unwrap();

        // Load multiple accounts
        let load_accounts = vec![
            params.input_token_mint,
            params.output_token_mint,
            pool_config.amm_config_key,
            pool_config
                .pool_id_account
                .ok_or("Pool ID account not found")?,
            pool_config
                .tickarray_bitmap_extension
                .ok_or("Tick array bitmap extension not found")?,
        ];

        // Get multiple accounts and unwrap the Result
        let rsps = rpc_client
            .get_multiple_accounts(&load_accounts)
            .await
            .map_err(|e| format!("Failed to get multiple accounts: {}", e))?;

        if rsps.len() < 5 {
            return Err("Not enough accounts returned from RPC".to_string());
        }

        // Now array_ref works on the unwrapped Vec
        let [user_input_account, user_output_account, amm_config_account, pool_account, tickarray_bitmap_extension_account] =
            array_ref![rsps, 0, 5];

        let user_input_state =
            StateWithExtensions::<Account>::unpack(&user_input_account.as_ref().unwrap().data)
                .unwrap();
        let user_output_state =
            StateWithExtensions::<Account>::unpack(&user_output_account.as_ref().unwrap().data)
                .unwrap();
        let amm_config_state = utils::deserialize_anchor_account::<states::AmmConfig>(
            amm_config_account.as_ref().unwrap(),
        )
        .map_err(|e| format!("Failed to deserialize AMM config: {}", e))?;
        let pool_state =
            utils::deserialize_anchor_account::<states::PoolState>(pool_account.as_ref().unwrap())
                .map_err(|e| format!("Failed to deserialize pool state: {}", e))?;
        let tickarray_bitmap_extension =
            utils::deserialize_anchor_account::<states::TickArrayBitmapExtension>(
                tickarray_bitmap_extension_account.as_ref().unwrap(),
            )
            .map_err(|e| format!("Failed to deserialize tick array bitmap: {}", e))?;

        let zero_for_one = user_input_state.base.mint == pool_state.token_mint_0
            && user_output_state.base.mint == pool_state.token_mint_1;

        const MIN_SQRT_PRICE_X64: u128 = 4295048016;
        const MAX_SQRT_PRICE_X64: u128 = 79226673521066979257578248091;
        let sqrt_price_limit_x64 = if zero_for_one {
            MIN_SQRT_PRICE_X64 + 1
        } else {
            MAX_SQRT_PRICE_X64 - 1
        };

        let mut tick_arrays = Self::load_cur_and_next_five_tick_array(
            &rpc_client,
            &pool_config,
            &pool_state,
            &tickarray_bitmap_extension,
            zero_for_one,
        )
        .await;

        let (mut other_amount_threshold, mut tick_array_indexs) =
            utils::get_out_put_amount_and_remaining_accounts(
                params.input_amount,
                Some(sqrt_price_limit_x64),
                zero_for_one,
                true,
                &amm_config_state,
                &pool_state,
                &tickarray_bitmap_extension,
                &mut tick_arrays,
            )
            .unwrap();
        other_amount_threshold = utils::amount_with_slippage(
            other_amount_threshold,
            params.slippage_tolerance_bps as f64 / 100.0,
            false,
        );

        let current_or_next_tick_array_key = Pubkey::find_program_address(
            &[
                states::TICK_ARRAY_SEED.as_bytes(),
                pool_config.pool_id_account.unwrap().to_bytes().as_ref(),
                &tick_array_indexs.pop_front().unwrap().to_be_bytes(),
            ],
            &pool_config.raydium_v3_program,
        )
        .0;

        let mut remaining_accounts = Vec::new();
        remaining_accounts.push(AccountMeta::new_readonly(
            pool_config.tickarray_bitmap_extension.unwrap(),
            false,
        ));
        let mut accounts = tick_array_indexs
            .into_iter()
            .map(|index| {
                AccountMeta::new(
                    Pubkey::find_program_address(
                        &[
                            states::TICK_ARRAY_SEED.as_bytes(),
                            pool_config.pool_id_account.unwrap().to_bytes().as_ref(),
                            &index.to_be_bytes(),
                        ],
                        &RaydiumClmmPoolState::get_program_id(),
                    )
                    .0,
                    false,
                )
            })
            .collect();
        remaining_accounts.append(&mut accounts);

        let mut instructions = Vec::new();
        let request_inits_instr = ComputeBudgetInstruction::set_compute_unit_limit(1400_000u32);
        instructions.push(request_inits_instr);
        let swap_instr = swap_instr(
            &pool_config,
            pool_state.amm_config,
            pool_config.pool_id_account.unwrap(),
            if zero_for_one {
                pool_state.token_vault_0
            } else {
                pool_state.token_vault_1
            },
            if zero_for_one {
                pool_state.token_vault_1
            } else {
                pool_state.token_vault_0
            },
            pool_state.observation_key,
            params.input_token_mint,
            params.output_token_mint,
            current_or_next_tick_array_key,
            remaining_accounts,
            params.input_amount,
            other_amount_threshold,
            Option::<u128>::Some(sqrt_price_limit_x64),
            true,
        )
        .unwrap();
        instructions.extend(swap_instr);
        Ok((instructions, other_amount_threshold))
    }

    /// Execute Raydium CLMM swap
    pub async fn execute_swap(
        params: &OnChainSwapParams,
        payer: &Keypair,
        rpc_client: &RpcClient,
    ) -> Result<OnChainSwapResult, String> {
        log::info!("🔄 Executing Raydium swap");
        log::info!("  Pool: {}", params.pool_address);
        log::info!("  Input Amount: {}", params.input_amount);
        log::info!("  Input Mint: {}", params.input_token_mint);

        // Build swap instruction using official Whirlpool SDK format
        let (swap_instr, mint_output_amount) =
            Self::build_swap_instruction(params, payer, rpc_client).await?;

        let signers = vec![&payer];
        let blockhash = rpc_client
            .get_latest_blockhash()
            .await
            .map_err(|e| format!("Failed to get latest blockhash: {}", e))?;
        let transaction = Transaction::new_signed_with_payer(
            &swap_instr,
            Some(&payer.pubkey()),
            &signers,
            blockhash,
        );
        let signature = rpc_client
            .send_and_confirm_transaction(&transaction)
            .await
            .map_err(|e| format!("Failed to send transaction: {}", e))?;

        // Get the output amount from transaction logs or token account
        let output_amount =
            extract_output_amount(rpc_client, &params.output_token_mint, &payer.pubkey()).await?;

        Ok(OnChainSwapResult {
            success: true,
            error_message: None,
            transaction_signature: Some(signature),
            dex_type: params.dex_type,
            pool_address: params.pool_address,
            input_token_mint: params.input_token_mint,
            output_token_mint: params.output_token_mint,
            input_amount: params.input_amount,
            output_amount: output_amount,
        })
    }
}

/// Extract output amount from transaction logs or token account
async fn extract_output_amount(
    rpc_client: &RpcClient,
    output_token_mint: &Pubkey,
    payer: &Pubkey,
) -> Result<u64, String> {
    // Convert Pubkey to Address for spl_associated_token_account
    let payer_address = Address::from_str(&payer.to_string())
        .map_err(|_| "Failed to convert payer to Address".to_string())?;
    let mint_address = Address::from_str(&output_token_mint.to_string())
        .map_err(|_| "Failed to convert mint to Address".to_string())?;

    let output_address =
        spl_associated_token_account::get_associated_token_address(&payer_address, &mint_address);
    // Convert Address back to Pubkey by parsing its string representation
    let output_ata = Pubkey::from_str(&output_address.to_string())
        .map_err(|_| "Failed to convert output address to Pubkey".to_string())?;
    match rpc_client.get_token_account_balance(&output_ata).await {
        Ok(balance) => {
            log::info!("📊 Token account balance: {}", balance.amount);
            Ok(balance
                .amount
                .parse::<u64>()
                .map_err(|e| format!("Failed to parse balance: {}", e))?)
        }
        Err(e) => Err(format!("Failed to get transaction: {}", e)),
    }
}
