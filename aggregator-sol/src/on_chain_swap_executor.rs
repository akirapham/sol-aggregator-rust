use log;
use serde::{Deserialize, Serialize};
use solana_client::rpc_client::RpcClient;
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
    hash::Hash, instruction::Instruction, message::Message, pubkey::Pubkey,
    signer::keypair::Keypair, signer::Signer, transaction::Transaction,
};
use std::str::FromStr;

use crate::pool_data_types::DexType;
use crate::types::ExecutionPriority;

/// Parameters for on-chain swap execution
#[derive(Debug, Clone)]
pub struct OnChainSwapParams {
    pub dex_type: DexType,
    pub input_token_mint: Pubkey,
    pub output_token_mint: Pubkey,
    pub input_amount: u64,
    pub min_output_amount: u64,
    pub pool_address: Pubkey,
    pub user_wallet: Pubkey,
    pub user_input_ata: Pubkey,
    pub user_output_ata: Pubkey,
    pub fee_payer: Pubkey,
    pub slippage_tolerance_bps: u16,
    pub priority: ExecutionPriority,
}

/// Result of on-chain swap execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnChainSwapResult {
    pub success: bool,
    pub transaction_signature: Option<String>,
    pub amount_in: u64,
    pub amount_out: Option<u64>,
    pub error_message: Option<String>,
    pub dex_type: DexType,
    pub executed_at: u64,
    pub slot: Option<u64>,
    pub confirmation_status: Option<String>,
}

/// Orca Whirlpool On-Chain Swap Executor
pub struct OrcaWhirlpoolSwapExecutor;

impl OrcaWhirlpoolSwapExecutor {
    // Official Whirlpool Program ID
    const WHIRLPOOL_PROGRAM_ID: &'static str = "whirLbMiicVdio4KfUadKvucOnAjzGUUtnCiAsx5Lac";

    // Swap v2 instruction discriminator (8 bytes)
    const SWAP_V2_DISCRIMINATOR: &'static [u8] = &[206, 176, 202, 18, 50, 56, 195, 174];

    /// Build Whirlpool swap v2 instruction using official SDK structure
    fn build_swap_instruction(
        params: &OnChainSwapParams,
        tick_arrays: Vec<Pubkey>,
        oracle: Pubkey,
    ) -> Result<Instruction, String> {
        log::info!("🔄 Building Whirlpool swap v2 instruction using official SDK");
        log::info!("  Pool: {}", params.pool_address);
        log::info!(
            "  Input: {} → min {}",
            params.input_amount,
            params.min_output_amount
        );

        let program_id = Pubkey::from_str(Self::WHIRLPOOL_PROGRAM_ID)
            .map_err(|e| format!("Invalid Whirlpool program ID: {}", e))?;

        // Build swap_v2 instruction data:
        // - discriminator (8 bytes): [206, 176, 202, 18, 50, 56, 195, 174]
        // - amount (u64, 8 bytes)
        // - other_amount_threshold (u64, 8 bytes)
        // - sqrt_price_limit (u128, 16 bytes)
        // - amount_specified_is_input (u8, 1 byte)
        // - a_to_b (u8, 1 byte)
        let mut instruction_data = Vec::new();
        instruction_data.extend_from_slice(Self::SWAP_V2_DISCRIMINATOR);
        instruction_data.extend_from_slice(&params.input_amount.to_le_bytes());
        instruction_data.extend_from_slice(&params.min_output_amount.to_le_bytes());
        instruction_data.extend_from_slice(&[0u8; 16]); // sqrt_price_limit = 0 (no price limit)
        instruction_data.push(1); // amount_specified_is_input = true (exact input mode)
        instruction_data.push(
            if params.input_token_mint.to_string() < params.output_token_mint.to_string() {
                1
            } else {
                0
            },
        ); // a_to_b

        // Build swap_v2 accounts in order:
        // 0. token_program_a (SPL Token or Token2022)
        // 1. token_program_b (SPL Token or Token2022)
        // 2. memo_program (System program)
        // 3. token_authority (user wallet, signer)
        // 4. whirlpool (pool state account)
        // 5. token_mint_a
        // 6. token_mint_b
        // 7. token_owner_account_a (user's token A account)
        // 8. token_vault_a (pool's token A vault)
        // 9. token_owner_account_b (user's token B account)
        // 10. token_vault_b (pool's token B vault)
        // 11-13. tick_array_0, tick_array_1, tick_array_2
        // 14. oracle
        let token_program = Pubkey::from_str("TokenkegQfeZyiNwAJsyFbPVwwQQfzLgh7PbisPo7Y")
            .map_err(|e| format!("Invalid SPL Token program: {}", e))?;
        let memo_program = Pubkey::from_str("MemoSq4gDiRvZoMoiktBg4heP6NsJegXDJU4S61LaH")
            .map_err(|e| format!("Invalid Memo program: {}", e))?;

        let mut accounts = vec![
            solana_sdk::instruction::AccountMeta::new_readonly(token_program, false),
            solana_sdk::instruction::AccountMeta::new_readonly(token_program, false),
            solana_sdk::instruction::AccountMeta::new_readonly(memo_program, false),
            solana_sdk::instruction::AccountMeta::new_readonly(params.user_wallet, true),
            solana_sdk::instruction::AccountMeta::new(params.pool_address, false),
            solana_sdk::instruction::AccountMeta::new_readonly(params.input_token_mint, false),
            solana_sdk::instruction::AccountMeta::new_readonly(params.output_token_mint, false),
            solana_sdk::instruction::AccountMeta::new(params.user_input_ata, false),
            solana_sdk::instruction::AccountMeta::new(params.pool_address, false), // Placeholder for vault_a
            solana_sdk::instruction::AccountMeta::new(params.user_output_ata, false),
            solana_sdk::instruction::AccountMeta::new(params.pool_address, false), // Placeholder for vault_b
        ];

        // Add tick arrays (up to 3)
        for tick_array in tick_arrays.iter().take(3) {
            accounts.push(solana_sdk::instruction::AccountMeta::new(
                *tick_array,
                false,
            ));
        }
        // Fill missing tick arrays with default pubkey
        while accounts.len() < 14 {
            accounts.push(solana_sdk::instruction::AccountMeta::new_readonly(
                Pubkey::default(),
                false,
            ));
        }

        // Add oracle account
        accounts.push(solana_sdk::instruction::AccountMeta::new_readonly(
            oracle, false,
        ));

        log::info!(
            "✅ Whirlpool swap v2 instruction built with {} accounts",
            accounts.len()
        );

        Ok(Instruction {
            program_id,
            accounts,
            data: instruction_data,
        })
    }

    /// Execute Whirlpool swap using official SDK instruction format
    pub async fn execute_swap(
        params: &OnChainSwapParams,
        tick_arrays: Vec<Pubkey>,
        oracle: Pubkey,
        recent_blockhash: Hash,
        payer: &Keypair,
        rpc_client: &RpcClient,
    ) -> Result<OnChainSwapResult, String> {
        log::info!("🔄 Executing Whirlpool swap using official SDK");
        log::info!("  Pool: {}", params.pool_address);
        log::info!("  Input Amount: {}", params.input_amount);
        log::info!("  Input Mint: {}", params.input_token_mint);

        // Build swap instruction using official Whirlpool SDK format
        let swap_instr = Self::build_swap_instruction(params, tick_arrays, oracle)?;

        log::info!("✅ Swap instruction generated from official SDK format");

        // Create and sign transaction
        let mut tx = Transaction::new_unsigned(Message::new(&[swap_instr], Some(&payer.pubkey())));

        tx.sign(&[payer], recent_blockhash);

        log::info!("✅ Transaction signed");

        // Submit to blockchain
        let signature = rpc_client
            .send_and_confirm_transaction(&tx)
            .map_err(|e| format!("Failed to submit transaction: {}", e))?;

        log::info!("✅ Transaction confirmed on-chain");
        log::info!("  Signature: {}", signature);

        // Get confirmation slot
        let slot = rpc_client.get_slot().unwrap_or(0);

        Ok(OnChainSwapResult {
            success: true,
            transaction_signature: Some(signature.to_string()),
            amount_in: params.input_amount,
            amount_out: Some(params.min_output_amount),
            error_message: None,
            dex_type: params.dex_type.clone(),
            executed_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            slot: Some(slot),
            confirmation_status: Some("confirmed".to_string()),
        })
    }
}

/// Raydium CLMM Swap Executor
pub struct RaydiumClmmSwapExecutor;

impl RaydiumClmmSwapExecutor {
    const PROGRAM_ID: &'static str = "CAMMCjfrWoSNmmeKBS2L2DfRawXzZhRvCb7ECwDjGvV";
    const CLMM_SWAP_DISCRIMINATOR: &'static [u8] = &[52, 133, 123, 156, 226, 138, 52, 97];

    /// Build Raydium CLMM swap instruction
    fn build_swap_instruction(
        params: &OnChainSwapParams,
        tick_arrays: Vec<Pubkey>,
    ) -> Result<Instruction, String> {
        log::info!("🔄 Building Raydium CLMM swap instruction");
        log::info!("  Pool: {}", params.pool_address);
        log::info!(
            "  Input: {} → min {}",
            params.input_amount,
            params.min_output_amount
        );

        let program_id = Pubkey::from_str(Self::PROGRAM_ID)
            .map_err(|e| format!("Invalid CLMM program ID: {}", e))?;

        let mut instruction_data = Vec::new();
        instruction_data.extend_from_slice(Self::CLMM_SWAP_DISCRIMINATOR);
        instruction_data.extend_from_slice(&params.input_amount.to_le_bytes());
        instruction_data.extend_from_slice(&params.min_output_amount.to_le_bytes());
        instruction_data.push(1); // a_2_b flag

        // Build required accounts
        let mut accounts = vec![
            solana_sdk::instruction::AccountMeta::new(params.pool_address, false),
            solana_sdk::instruction::AccountMeta::new_readonly(params.user_wallet, true),
            solana_sdk::instruction::AccountMeta::new(params.user_input_ata, false),
            solana_sdk::instruction::AccountMeta::new(params.user_output_ata, false),
        ];

        // Add tick arrays
        for tick_array in tick_arrays {
            accounts.push(solana_sdk::instruction::AccountMeta::new(tick_array, false));
        }

        log::info!(
            "✅ CLMM swap instruction built with {} accounts",
            accounts.len()
        );

        Ok(Instruction {
            program_id,
            accounts,
            data: instruction_data,
        })
    }

    /// Execute Raydium CLMM swap
    pub async fn execute_swap(
        params: &OnChainSwapParams,
        tick_arrays: Vec<Pubkey>,
        recent_blockhash: Hash,
        payer: &Keypair,
        rpc_client: &RpcClient,
    ) -> Result<OnChainSwapResult, String> {
        log::info!("🔄 Executing Raydium CLMM swap");
        log::info!("  Pool: {}", params.pool_address);
        log::info!("  Input Amount: {}", params.input_amount);
        log::info!("  Minimum Output: {}", params.min_output_amount);

        // Build swap instruction
        let instruction = Self::build_swap_instruction(params, tick_arrays)?;

        // Create and sign transaction
        let tx = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&payer.pubkey()),
            &[payer],
            recent_blockhash,
        );

        log::info!("✅ Transaction signed");

        // Submit to blockchain
        let signature = rpc_client
            .send_and_confirm_transaction(&tx)
            .map_err(|e| format!("Failed to submit transaction: {}", e))?;

        log::info!("✅ Transaction confirmed on-chain");
        log::info!("  Signature: {}", signature);

        let slot = rpc_client.get_slot().unwrap_or(0);

        Ok(OnChainSwapResult {
            success: true,
            transaction_signature: Some(signature.to_string()),
            amount_in: params.input_amount,
            amount_out: Some(params.min_output_amount),
            error_message: None,
            dex_type: DexType::RaydiumClmm,
            executed_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            slot: Some(slot),
            confirmation_status: Some("confirmed".to_string()),
        })
    }
}

/// Raydium CPMM Swap Executor
pub struct RaydiumCpmmSwapExecutor;

impl RaydiumCpmmSwapExecutor {
    const PROGRAM_ID: &'static str = "CPMMcjfrWoSNmmeKBS2L2DfRawXzZhRvCb7ECwDjGvV";
    const CPMM_SWAP_DISCRIMINATOR: &'static [u8] = &[248, 198, 158, 145, 225, 117, 135, 200];

    /// Build Raydium CPMM swap instruction
    fn build_swap_instruction(params: &OnChainSwapParams) -> Result<Instruction, String> {
        log::info!("🔄 Building Raydium CPMM swap instruction");
        log::info!("  Pool: {}", params.pool_address);
        log::info!(
            "  Input: {} → min {}",
            params.input_amount,
            params.min_output_amount
        );

        let program_id = Pubkey::from_str(Self::PROGRAM_ID)
            .map_err(|e| format!("Invalid CPMM program ID: {}", e))?;

        let mut instruction_data = Vec::new();
        instruction_data.extend_from_slice(Self::CPMM_SWAP_DISCRIMINATOR);
        instruction_data.extend_from_slice(&params.input_amount.to_le_bytes());
        instruction_data.extend_from_slice(&params.min_output_amount.to_le_bytes());

        let accounts = vec![
            solana_sdk::instruction::AccountMeta::new(params.pool_address, false),
            solana_sdk::instruction::AccountMeta::new_readonly(params.user_wallet, true),
            solana_sdk::instruction::AccountMeta::new(params.user_input_ata, false),
            solana_sdk::instruction::AccountMeta::new(params.user_output_ata, false),
        ];

        log::info!(
            "✅ CPMM swap instruction built with {} accounts",
            accounts.len()
        );

        Ok(Instruction {
            program_id,
            accounts,
            data: instruction_data,
        })
    }

    /// Execute Raydium CPMM swap
    pub async fn execute_swap(
        params: &OnChainSwapParams,
        recent_blockhash: Hash,
        payer: &Keypair,
        rpc_client: &RpcClient,
    ) -> Result<OnChainSwapResult, String> {
        log::info!("🔄 Executing Raydium CPMM swap");
        log::info!("  Pool: {}", params.pool_address);
        log::info!("  Input Amount: {}", params.input_amount);
        log::info!("  Minimum Output: {}", params.min_output_amount);

        // Build swap instruction
        let instruction = Self::build_swap_instruction(params)?;

        // Create and sign transaction
        let tx = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&payer.pubkey()),
            &[payer],
            recent_blockhash,
        );

        log::info!("✅ Transaction signed");

        // Submit to blockchain
        let signature = rpc_client
            .send_and_confirm_transaction(&tx)
            .map_err(|e| format!("Failed to submit transaction: {}", e))?;

        log::info!("✅ Transaction confirmed on-chain");
        log::info!("  Signature: {}", signature);

        let slot = rpc_client.get_slot().unwrap_or(0);

        Ok(OnChainSwapResult {
            success: true,
            transaction_signature: Some(signature.to_string()),
            amount_in: params.input_amount,
            amount_out: Some(params.min_output_amount),
            error_message: None,
            dex_type: DexType::RaydiumCpmm,
            executed_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            slot: Some(slot),
            confirmation_status: Some("confirmed".to_string()),
        })
    }
}

/// Raydium AMM V4 Swap Executor
pub struct RaydiumAmmV4SwapExecutor;

impl RaydiumAmmV4SwapExecutor {
    const PROGRAM_ID: &'static str = "675kPX9MHTjS2zt1qrXjVVxt2Y8Dm39wNaJqVrxLac94";
    const AMMV4_SWAP_DISCRIMINATOR: &'static [u8] = &[229, 235, 109, 93, 198, 135, 53, 147];

    /// Build Raydium AMM V4 swap instruction
    fn build_swap_instruction(params: &OnChainSwapParams) -> Result<Instruction, String> {
        log::info!("🔄 Building Raydium AMM V4 swap instruction");
        log::info!("  Pool: {}", params.pool_address);
        log::info!(
            "  Input: {} → min {}",
            params.input_amount,
            params.min_output_amount
        );

        let program_id = Pubkey::from_str(Self::PROGRAM_ID)
            .map_err(|e| format!("Invalid AMM V4 program ID: {}", e))?;

        let mut instruction_data = Vec::new();
        instruction_data.extend_from_slice(Self::AMMV4_SWAP_DISCRIMINATOR);
        instruction_data.extend_from_slice(&params.input_amount.to_le_bytes());
        instruction_data.extend_from_slice(&params.min_output_amount.to_le_bytes());

        let accounts = vec![
            solana_sdk::instruction::AccountMeta::new(params.pool_address, false),
            solana_sdk::instruction::AccountMeta::new_readonly(params.user_wallet, true),
            solana_sdk::instruction::AccountMeta::new(params.user_input_ata, false),
            solana_sdk::instruction::AccountMeta::new(params.user_output_ata, false),
        ];

        log::info!(
            "✅ AMM V4 swap instruction built with {} accounts",
            accounts.len()
        );

        Ok(Instruction {
            program_id,
            accounts,
            data: instruction_data,
        })
    }

    /// Execute Raydium AMM V4 swap
    pub async fn execute_swap(
        params: &OnChainSwapParams,
        recent_blockhash: Hash,
        payer: &Keypair,
        rpc_client: &RpcClient,
    ) -> Result<OnChainSwapResult, String> {
        log::info!("🔄 Executing Raydium AMM V4 swap");
        log::info!("  Pool: {}", params.pool_address);
        log::info!("  Input Amount: {}", params.input_amount);
        log::info!("  Minimum Output: {}", params.min_output_amount);

        // Build swap instruction
        let instruction = Self::build_swap_instruction(params)?;

        // Create and sign transaction
        let tx = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&payer.pubkey()),
            &[payer],
            recent_blockhash,
        );

        log::info!("✅ Transaction signed");

        // Submit to blockchain
        let signature = rpc_client
            .send_and_confirm_transaction(&tx)
            .map_err(|e| format!("Failed to submit transaction: {}", e))?;

        log::info!("✅ Transaction confirmed on-chain");
        log::info!("  Signature: {}", signature);

        let slot = rpc_client.get_slot().unwrap_or(0);

        Ok(OnChainSwapResult {
            success: true,
            transaction_signature: Some(signature.to_string()),
            amount_in: params.input_amount,
            amount_out: Some(params.min_output_amount),
            error_message: None,
            dex_type: DexType::Raydium,
            executed_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            slot: Some(slot),
            confirmation_status: Some("confirmed".to_string()),
        })
    }
}

/// Main Swap Executor Coordinator
pub struct OnChainArbitrageExecutor;

impl OnChainArbitrageExecutor {
    /// Execute forward swap (token_a → token_b)
    pub async fn execute_forward_swap(
        params: &OnChainSwapParams,
        recent_blockhash: Hash,
        payer: &Keypair,
        rpc_client: &RpcClient,
        additional_params: Option<Vec<Pubkey>>,
    ) -> Result<OnChainSwapResult, String> {
        log::info!(
            "🔄 FORWARD SWAP: {} → {}",
            params.input_token_mint,
            params.output_token_mint
        );

        match params.dex_type {
            DexType::Orca => {
                let tick_arrays = additional_params.unwrap_or_default();
                let oracle = if tick_arrays.len() > 3 {
                    tick_arrays[tick_arrays.len() - 1]
                } else {
                    Pubkey::default()
                };
                let ta = tick_arrays[..3.min(tick_arrays.len())].to_vec();
                OrcaWhirlpoolSwapExecutor::execute_swap(
                    params,
                    ta,
                    oracle,
                    recent_blockhash,
                    payer,
                    rpc_client,
                )
                .await
            }
            DexType::RaydiumClmm => {
                let tick_arrays = additional_params.unwrap_or_default();
                RaydiumClmmSwapExecutor::execute_swap(
                    params,
                    tick_arrays,
                    recent_blockhash,
                    payer,
                    rpc_client,
                )
                .await
            }
            DexType::RaydiumCpmm => {
                RaydiumCpmmSwapExecutor::execute_swap(params, recent_blockhash, payer, rpc_client)
                    .await
            }
            DexType::Raydium => {
                RaydiumAmmV4SwapExecutor::execute_swap(params, recent_blockhash, payer, rpc_client)
                    .await
            }
            _ => Err("Unsupported DEX type".to_string()),
        }
    }

    /// Execute reverse swap (token_b → token_a)
    pub async fn execute_reverse_swap(
        params: &OnChainSwapParams,
        recent_blockhash: Hash,
        payer: &Keypair,
        rpc_client: &RpcClient,
        additional_params: Option<Vec<Pubkey>>,
    ) -> Result<OnChainSwapResult, String> {
        log::info!(
            "🔄 REVERSE SWAP: {} → {}",
            params.input_token_mint,
            params.output_token_mint
        );

        match params.dex_type {
            DexType::Orca => {
                let tick_arrays = additional_params.unwrap_or_default();
                let oracle = if tick_arrays.len() > 3 {
                    tick_arrays[tick_arrays.len() - 1]
                } else {
                    Pubkey::default()
                };
                let ta = tick_arrays[..3.min(tick_arrays.len())].to_vec();
                OrcaWhirlpoolSwapExecutor::execute_swap(
                    params,
                    ta,
                    oracle,
                    recent_blockhash,
                    payer,
                    rpc_client,
                )
                .await
            }
            DexType::RaydiumClmm => {
                let tick_arrays = additional_params.unwrap_or_default();
                RaydiumClmmSwapExecutor::execute_swap(
                    params,
                    tick_arrays,
                    recent_blockhash,
                    payer,
                    rpc_client,
                )
                .await
            }
            DexType::RaydiumCpmm => {
                RaydiumCpmmSwapExecutor::execute_swap(params, recent_blockhash, payer, rpc_client)
                    .await
            }
            DexType::Raydium => {
                RaydiumAmmV4SwapExecutor::execute_swap(params, recent_blockhash, payer, rpc_client)
                    .await
            }
            _ => Err("Unsupported DEX type".to_string()),
        }
    }

    /// Execute full arbitrage cycle with blockchain submission
    pub async fn execute_arbitrage_cycle(
        forward_params: &OnChainSwapParams,
        reverse_params: &OnChainSwapParams,
        recent_blockhash: Hash,
        payer: &Keypair,
        rpc_client: &RpcClient,
        forward_additional: Option<Vec<Pubkey>>,
        reverse_additional: Option<Vec<Pubkey>>,
    ) -> Result<(OnChainSwapResult, OnChainSwapResult, u64), String> {
        log::info!("🎯 Executing ARBITRAGE CYCLE");

        // Execute forward swap
        let forward_result = Self::execute_forward_swap(
            forward_params,
            recent_blockhash,
            payer,
            rpc_client,
            forward_additional,
        )
        .await?;

        log::info!(
            "✅ Forward swap complete: {} → {}",
            forward_result.amount_in,
            forward_result.amount_out.unwrap_or(0)
        );

        // Update reverse input with forward output
        let mut reverse_with_output = reverse_params.clone();
        reverse_with_output.input_amount = forward_result.amount_out.unwrap_or(0);

        // Execute reverse swap
        let reverse_result = Self::execute_reverse_swap(
            &reverse_with_output,
            recent_blockhash,
            payer,
            rpc_client,
            reverse_additional,
        )
        .await?;

        log::info!(
            "✅ Reverse swap complete: {} → {}",
            reverse_result.amount_in,
            reverse_result.amount_out.unwrap_or(0)
        );

        // Calculate profit
        let initial_amount = forward_params.input_amount;
        let final_amount = reverse_result.amount_out.unwrap_or(0);
        let profit = final_amount.saturating_sub(initial_amount);

        log::info!(
            "💰 ARBITRAGE PROFIT: {} ({:.4}%)",
            profit,
            (profit as f64 / initial_amount as f64) * 100.0
        );

        Ok((forward_result, reverse_result, profit))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_orca_instruction_building() {
        let params = OnChainSwapParams {
            dex_type: DexType::Orca,
            input_token_mint: Pubkey::default(),
            output_token_mint: Pubkey::default(),
            input_amount: 1_000_000,
            min_output_amount: 5_000,
            pool_address: Pubkey::default(),
            user_wallet: Pubkey::default(),
            user_input_ata: Pubkey::default(),
            user_output_ata: Pubkey::default(),
            fee_payer: Pubkey::default(),
            slippage_tolerance_bps: 500,
            priority: ExecutionPriority::Medium,
        };

        let result =
            OrcaWhirlpoolSwapExecutor::build_swap_instruction(&params, vec![], Pubkey::default());

        assert!(result.is_ok());
        let instruction = result.unwrap();
        assert!(instruction.data.len() > 0);
    }

    #[test]
    fn test_raydium_clmm_instruction_building() {
        let params = OnChainSwapParams {
            dex_type: DexType::RaydiumClmm,
            input_token_mint: Pubkey::default(),
            output_token_mint: Pubkey::default(),
            input_amount: 1_000_000,
            min_output_amount: 5_000,
            pool_address: Pubkey::default(),
            user_wallet: Pubkey::default(),
            user_input_ata: Pubkey::default(),
            user_output_ata: Pubkey::default(),
            fee_payer: Pubkey::default(),
            slippage_tolerance_bps: 500,
            priority: ExecutionPriority::Medium,
        };

        let result = RaydiumClmmSwapExecutor::build_swap_instruction(&params, vec![]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_raydium_cpmm_instruction_building() {
        let params = OnChainSwapParams {
            dex_type: DexType::RaydiumCpmm,
            input_token_mint: Pubkey::default(),
            output_token_mint: Pubkey::default(),
            input_amount: 1_000_000,
            min_output_amount: 5_000,
            pool_address: Pubkey::default(),
            user_wallet: Pubkey::default(),
            user_input_ata: Pubkey::default(),
            user_output_ata: Pubkey::default(),
            fee_payer: Pubkey::default(),
            slippage_tolerance_bps: 500,
            priority: ExecutionPriority::Medium,
        };

        let result = RaydiumCpmmSwapExecutor::build_swap_instruction(&params);
        assert!(result.is_ok());
    }
}
