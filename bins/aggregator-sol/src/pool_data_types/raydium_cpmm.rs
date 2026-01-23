use crate::{
    pool_data_types::{common::functions, GetAmmConfig, PoolUpdateEventType},
    utils::tokens_equal,
};
use borsh::BorshDeserialize;
use serde::{Deserialize, Serialize};

use solana_sdk::pubkey::Pubkey;
use solana_streamer_sdk::streaming::event_parser::protocols::raydium_cpmm::parser::RAYDIUM_CPMM_PROGRAM_ID;

use crate::pool_data_types::traits::BuildSwapInstruction;
use crate::types::SwapParams;
use async_trait::async_trait;
use borsh::BorshSerialize;
// use solana_compute_budget_interface::ComputeBudgetInstruction;
// use solana_program::instruction::{AccountMeta, Instruction};
use spl_associated_token_account;
use std::str::FromStr;

#[derive(BorshSerialize)]
struct SwapBaseInputArgs {
    amount_in: u64,
    minimum_amount_out: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaydiumCpmmPoolState {
    pub slot: u64,
    pub transaction_index: Option<u64>,
    pub status: u8,
    pub address: Pubkey,
    pub token0: Pubkey,
    pub token1: Pubkey,
    pub token0_vault: Pubkey,
    pub token1_vault: Pubkey,
    pub token0_reserve: u64,
    pub token1_reserve: u64,
    pub amm_config: Pubkey,
    pub observation_state: Pubkey,
    pub last_updated: u64,
    pub liquidity_usd: f64,
    pub is_state_keys_initialized: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, BorshDeserialize)]
pub struct RaydiumCpmmAmmConfig {
    pub bump: u8,
    pub disable_create_pool: bool,
    pub index: u16,
    pub trade_fee_rate: u64,
    pub protocol_fee_rate: u64,
    pub fund_fee_rate: u64,
    pub create_pool_fee: u64,
    pub protocol_owner: Pubkey,
    pub fund_owner: Pubkey,
    pub padding: [u64; 16],
}

#[derive(Debug, Clone)]
pub struct RaydiumCpmmPoolUpdate {
    pub slot: u64,
    pub transaction_index: Option<u64>,
    pub status: Option<u8>,
    pub address: Pubkey,
    pub token0: Pubkey,
    pub token1: Pubkey,
    pub token0_vault: Pubkey,
    pub token1_vault: Pubkey,
    pub token0_reserve: u64,
    pub token1_reserve: u64,
    pub amm_config: Pubkey,
    pub observation_state: Pubkey,
    pub last_updated: u64,
    pub is_account_state_update: bool,
    pub pool_update_event_type: PoolUpdateEventType,
    pub additional_event_type: i32, // for tick array index tracking, 0 for others
}

#[allow(dead_code)]
impl RaydiumCpmmPoolState {
    pub fn get_program_id() -> Pubkey {
        Pubkey::new_from_array(*RAYDIUM_CPMM_PROGRAM_ID.as_array())
    }

    /// Calculate output amount for PumpFun bonding curve
    pub fn calculate_output_amount(
        &self,
        input_token: &Pubkey,
        input_amount: u64,
        _: &dyn GetAmmConfig,
    ) -> u64 {
        let (base_token, _) = (self.token0, self.token1);
        let input_is_base = tokens_equal(input_token, &base_token);
        let (input_reserve, output_reserve) = if input_is_base {
            (self.token0_reserve, self.token1_reserve)
        } else {
            (self.token1_reserve, self.token0_reserve)
        };
        let new_input_reserve = input_reserve as u128 + input_amount as u128;
        let new_output_reserve =
            (input_reserve as u128 * output_reserve as u128 / new_input_reserve) as u64;
        let output_amount = output_reserve - new_output_reserve;

        output_amount * 9975 / 10000 // Apply 0.25% fee
    }

    pub fn calculate_token_prices(
        &self,
        sol_price: f64,
        base_decimals: u8,
        quote_decimals: u8,
    ) -> (f64, f64) {
        functions::calculate_amm_token_prices(
            &self.token0,
            &self.token1,
            self.token0_reserve,
            self.token1_reserve,
            sol_price,
            base_decimals,
            quote_decimals,
        )
    }
}

#[async_trait]
impl BuildSwapInstruction for RaydiumCpmmPoolState {
    async fn build_swap_instruction(
        &self,
        params: &SwapParams,
        amm_config_fetcher: &dyn GetAmmConfig,
        _rpc_client: Option<&std::sync::Arc<solana_client::nonblocking::rpc_client::RpcClient>>,
    ) -> std::result::Result<Vec<solana_sdk::instruction::Instruction>, String> {
        let mut instructions = Vec::new();

        // Helper to convert anchor pubkey to sdk pubkey
        let to_sdk_pubkey =
            |pk: anchor_lang::prelude::Pubkey| -> Pubkey { Pubkey::new_from_array(pk.to_bytes()) };

        // 1. Determine direction
        let is_input_token0 = params.input_token.address == self.token0;
        if !is_input_token0 && params.input_token.address != self.token1 {
            return Err("Input token does not match pool mints".to_string());
        }

        // 2. Calculate output amount
        let amount_out = self.calculate_output_amount(
            &params.input_token.address,
            params.input_amount,
            amm_config_fetcher,
        );

        // 3. Calculate minimum output amount (slippage)
        let minimum_amount_out = functions::calculate_slippage(amount_out, params.slippage_bps)?;

        // 4. Prepare accounts
        let (
            input_vault,
            output_vault,
            input_mint,
            output_mint,
            input_token_program,
            output_token_program,
        ) = if is_input_token0 {
            (
                self.token0_vault,
                self.token1_vault,
                self.token0,
                self.token1,
                functions::get_token_program(params.input_token.is_token_2022),
                functions::get_token_program(params.output_token.is_token_2022),
            )
        } else {
            (
                self.token1_vault,
                self.token0_vault,
                self.token1,
                self.token0,
                functions::get_token_program(params.input_token.is_token_2022),
                functions::get_token_program(params.output_token.is_token_2022),
            )
        };

        // User ATAs
        // Convert SDK Pubkeys to Anchor Pubkeys for spl_associated_token_account
        let user_wallet_anchor = functions::to_pubkey(&params.user_wallet);
        let input_mint_anchor = functions::to_pubkey(&input_mint);
        let output_mint_anchor = functions::to_pubkey(&output_mint);
        let input_token_program_anchor = functions::to_pubkey(&input_token_program);
        let output_token_program_anchor = functions::to_pubkey(&output_token_program);

        let user_input_token_anchor =
            spl_associated_token_account::get_associated_token_address_with_program_id(
                &user_wallet_anchor,
                &input_mint_anchor,
                &input_token_program_anchor,
            );
        let user_output_token_anchor =
            spl_associated_token_account::get_associated_token_address_with_program_id(
                &user_wallet_anchor,
                &output_mint_anchor,
                &output_token_program_anchor,
            );

        let user_input_token = to_sdk_pubkey(user_input_token_anchor);
        let user_output_token = to_sdk_pubkey(user_output_token_anchor);

        // Compute Budget (Manual SDK construction)
        // Program: ComputeBudget111111111111111111111111111111
        // Instruction: SetComputeUnitLimit (2)
        let compute_budget_program =
            Pubkey::from_str("ComputeBudget111111111111111111111111111111")
                .map_err(|e| e.to_string())?;
        let mut cb_data = vec![2]; // SetComputeUnitLimit discriminator
        cb_data.extend_from_slice(&(1_400_000u32).to_le_bytes());

        instructions.push(solana_sdk::instruction::Instruction {
            program_id: compute_budget_program,
            accounts: vec![],
            data: cb_data,
        });

        // WSOL Logic
        let wsol_mint = Pubkey::from_str("So11111111111111111111111111111111111111112")
            .map_err(|e| e.to_string())?;

        if params.input_token.address == wsol_mint {
            let spl_token_id = Pubkey::from_str("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA")
                .map_err(|e| e.to_string())?;
            let associated_token_program =
                Pubkey::from_str("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL")
                    .map_err(|e| e.to_string())?;
            let system_program =
                Pubkey::from_str("11111111111111111111111111111111").map_err(|e| e.to_string())?;

            // 1. Create WSOL ATA (Idempotent)
            instructions.push(solana_sdk::instruction::Instruction {
                program_id: associated_token_program,
                accounts: vec![
                    solana_sdk::instruction::AccountMeta::new(params.user_wallet, true),
                    solana_sdk::instruction::AccountMeta::new(user_input_token, false),
                    solana_sdk::instruction::AccountMeta::new_readonly(params.user_wallet, false),
                    solana_sdk::instruction::AccountMeta::new_readonly(wsol_mint, false),
                    solana_sdk::instruction::AccountMeta::new_readonly(system_program, false),
                    solana_sdk::instruction::AccountMeta::new_readonly(spl_token_id, false),
                ],
                data: vec![1], // Idempotent
            });

            // 2. Transfer SOL
            let mut transfer_data = vec![2, 0, 0, 0];
            transfer_data.extend_from_slice(&params.input_amount.to_le_bytes());
            instructions.push(solana_sdk::instruction::Instruction {
                program_id: system_program,
                accounts: vec![
                    solana_sdk::instruction::AccountMeta::new(params.user_wallet, true),
                    solana_sdk::instruction::AccountMeta::new(user_input_token, false),
                ],
                data: transfer_data,
            });

            // 3. Sync Native
            instructions.push(solana_sdk::instruction::Instruction {
                program_id: spl_token_id,
                accounts: vec![solana_sdk::instruction::AccountMeta::new(
                    user_input_token,
                    false,
                )],
                data: vec![17],
            });
        }

        // Create Output ATA (Idempotent) - Manual build
        let spl_token_prog = Pubkey::new_from_array(output_token_program.to_bytes());
        let associated_token_program =
            Pubkey::from_str("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL")
                .map_err(|e| e.to_string())?;
        let system_program =
            Pubkey::from_str("11111111111111111111111111111111").map_err(|e| e.to_string())?;

        instructions.push(solana_sdk::instruction::Instruction {
            program_id: associated_token_program,
            accounts: vec![
                solana_sdk::instruction::AccountMeta::new(params.user_wallet, true),
                solana_sdk::instruction::AccountMeta::new(user_output_token, false),
                solana_sdk::instruction::AccountMeta::new_readonly(params.user_wallet, false),
                solana_sdk::instruction::AccountMeta::new_readonly(output_mint, false),
                solana_sdk::instruction::AccountMeta::new_readonly(system_program, false),
                solana_sdk::instruction::AccountMeta::new_readonly(spl_token_prog, false),
            ],
            data: vec![1], // Idempotent
        });

        // 5. Construct Swap Instruction Data
        let discriminator: [u8; 8] = [143, 190, 90, 218, 196, 30, 51, 222];
        let args = SwapBaseInputArgs {
            amount_in: params.input_amount,
            minimum_amount_out,
        };
        let mut data = Vec::with_capacity(8 + 16);
        data.extend_from_slice(&discriminator);
        args.serialize(&mut data).map_err(|e| e.to_string())?;

        let (authority, _) = Pubkey::find_program_address(
            &[b"vault_and_lp_mint_auth_seed"],
            &Self::get_program_id(),
        );

        let accounts = vec![
            solana_sdk::instruction::AccountMeta::new(params.user_wallet, true),
            solana_sdk::instruction::AccountMeta::new_readonly(authority, false),
            solana_sdk::instruction::AccountMeta::new_readonly(self.amm_config, false),
            solana_sdk::instruction::AccountMeta::new(self.address, false),
            solana_sdk::instruction::AccountMeta::new(user_input_token, false),
            solana_sdk::instruction::AccountMeta::new(user_output_token, false),
            solana_sdk::instruction::AccountMeta::new(input_vault, false),
            solana_sdk::instruction::AccountMeta::new(output_vault, false),
            solana_sdk::instruction::AccountMeta::new_readonly(input_token_program, false),
            solana_sdk::instruction::AccountMeta::new_readonly(output_token_program, false),
            solana_sdk::instruction::AccountMeta::new_readonly(input_mint, false),
            solana_sdk::instruction::AccountMeta::new_readonly(output_mint, false),
            solana_sdk::instruction::AccountMeta::new(self.observation_state, false),
        ];

        instructions.push(solana_sdk::instruction::Instruction {
            program_id: Self::get_program_id(),
            accounts,
            data,
        });

        // WSOL Unwrapping
        if params.output_token.address == wsol_mint {
            let spl_token_id = Pubkey::from_str("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA")
                .map_err(|e| e.to_string())?;
            instructions.push(solana_sdk::instruction::Instruction {
                program_id: spl_token_id,
                accounts: vec![
                    solana_sdk::instruction::AccountMeta::new(user_output_token, false),
                    solana_sdk::instruction::AccountMeta::new(params.user_wallet, false),
                    solana_sdk::instruction::AccountMeta::new(params.user_wallet, true),
                ],
                data: vec![9], // CloseAccount
            });
        }

        Ok(instructions)
    }
}
