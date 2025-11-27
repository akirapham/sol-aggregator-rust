use std::sync::Arc;

use crate::{
    constants::is_base_token,
    pool_data_types::{GetAmmConfig, PoolUpdateEventType, common},
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
use solana_compute_budget_interface::ComputeBudgetInstruction;
use solana_program::instruction::{AccountMeta, Instruction};
use spl_associated_token_account;

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
        _: Arc<dyn GetAmmConfig>,
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
        if self.token1_reserve == 0 || self.token0_reserve == 0 {
            return (0.0, 0.0);
        }

        let token0_str = self.token0.to_string();
        let token1_str = self.token1.to_string();

        let is_token0_a_base_token = is_base_token(&token0_str);
        let is_token1_a_base_token = is_base_token(&token1_str);

        let decimal_scale = 10_f64.powi(base_decimals as i32 - quote_decimals as i32);

        // If token1 is a base token (like USDC, SOL), use its price
        if is_token1_a_base_token {
            let token1_price = if token1_str == "So11111111111111111111111111111111111111112" {
                sol_price // SOL
            } else {
                1.0 // Assume USDC/USDT are ~$1
            };

            let token0_price = (self.token1_reserve as f64 / self.token0_reserve as f64)
                * decimal_scale
                * token1_price;
            (token0_price, token1_price)
        } else if is_token0_a_base_token {
            // If token0 is a base token, use its price
            let token0_price = if token0_str == "So11111111111111111111111111111111111111112" {
                sol_price // SOL
            } else {
                1.0 // Assume USDC/USDT are ~$1
            };

            let token1_price = (self.token0_reserve as f64 / self.token1_reserve as f64)
                * (1.0 / decimal_scale)
                * token0_price;
            (token0_price, token1_price)
        } else {
            // Neither token is a base token, assume relative pricing
            let token0_price =
                (self.token1_reserve as f64 / self.token0_reserve as f64) * decimal_scale * 1.0;
            (token0_price, 1.0)
        }
    }
}

#[async_trait]
impl BuildSwapInstruction for RaydiumCpmmPoolState {
    async fn build_swap_instruction(
        &self,
        params: &SwapParams,
        amm_config_fetcher: Arc<dyn GetAmmConfig>,
    ) -> std::result::Result<Vec<Instruction>, String> {
        // 1. Determine direction
        // In Raydium CPMM, we need to know if we are swapping Token 0 -> Token 1 or Token 1 -> Token 0
        // to correctly assign input/output accounts.
        let is_input_token0 = params.input_token.address == self.token0;
        if !is_input_token0 && params.input_token.address != self.token1 {
            return Err("Input token does not match pool mints".to_string());
        }

        // 2. Calculate output amount
        let amount_out = self.calculate_output_amount(
            &params.input_token.address,
            params.input_amount,
            amm_config_fetcher.clone(),
        );

        // 3. Calculate minimum output amount (slippage)
        let slippage_factor = 10000 - params.slippage_bps as u64;
        let minimum_amount_out = (amount_out as u128 * slippage_factor as u128 / 10000) as u64;

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
                if params.input_token.is_token_2022 {
                    common::constants::TOKEN_PROGRAM_2022
                } else {
                    common::constants::TOKEN_PROGRAM
                },
                if params.output_token.is_token_2022 {
                    common::constants::TOKEN_PROGRAM_2022
                } else {
                    common::constants::TOKEN_PROGRAM
                },
            )
        } else {
            (
                self.token1_vault,
                self.token0_vault,
                self.token1,
                self.token0,
                if params.input_token.is_token_2022 {
                    common::constants::TOKEN_PROGRAM_2022
                } else {
                    common::constants::TOKEN_PROGRAM
                },
                if params.output_token.is_token_2022 {
                    common::constants::TOKEN_PROGRAM_2022
                } else {
                    common::constants::TOKEN_PROGRAM
                },
            )
        };

        // User ATAs
        let user_wallet_old =
            anchor_lang::prelude::Pubkey::new_from_array(params.user_wallet.to_bytes());
        let input_mint_old =
            anchor_lang::prelude::Pubkey::new_from_array(params.input_token.address.to_bytes());
        let output_mint_old =
            anchor_lang::prelude::Pubkey::new_from_array(params.output_token.address.to_bytes());

        let user_input_token_old =
            spl_associated_token_account::get_associated_token_address_with_program_id(
                &user_wallet_old,
                &input_mint_old,
                &anchor_lang::prelude::Pubkey::new_from_array(input_token_program.to_bytes()),
            );
        let user_output_token_old =
            spl_associated_token_account::get_associated_token_address_with_program_id(
                &user_wallet_old,
                &output_mint_old,
                &anchor_lang::prelude::Pubkey::new_from_array(output_token_program.to_bytes()),
            );

        let user_input_token =
            solana_sdk::pubkey::Pubkey::new_from_array(user_input_token_old.to_bytes());
        let user_output_token =
            solana_sdk::pubkey::Pubkey::new_from_array(user_output_token_old.to_bytes());

        // 5. Construct Instruction Data
        // global:swap_base_input discriminator: [143, 190, 90, 218, 196, 30, 51, 222]
        let discriminator: [u8; 8] = [143, 190, 90, 218, 196, 30, 51, 222];
        let args = SwapBaseInputArgs {
            amount_in: params.input_amount,
            minimum_amount_out,
        };
        let mut data = Vec::with_capacity(8 + 16);
        data.extend_from_slice(&discriminator);
        args.serialize(&mut data).map_err(|e| e.to_string())?;

        // 6. Construct Account Metas
        // Order:
        // 0. payer (signer, writable)
        // 1. authority (readonly) - derived from program
        // 2. amm_config (readonly)
        // 3. pool_state (writable)
        // 4. input_token_account (writable)
        // 5. output_token_account (writable)
        // 6. input_vault (writable)
        // 7. output_vault (writable)
        // 8. input_token_program (readonly)
        // 9. output_token_program (readonly)
        // 10. input_token_mint (readonly)
        // 11. output_token_mint (readonly)
        // 12. observation_state (writable)

        // Derive Authority
        // AUTH_SEED = "vault_and_lp_mint_auth_seed"
        let (authority, _) = Pubkey::find_program_address(
            &[b"vault_and_lp_mint_auth_seed"],
            &Self::get_program_id(),
        );

        let accounts = vec![
            AccountMeta::new(params.user_wallet, true),        // payer
            AccountMeta::new_readonly(authority, false),       // authority
            AccountMeta::new_readonly(self.amm_config, false), // amm_config
            AccountMeta::new(self.address, false),             // pool_state
            AccountMeta::new(user_input_token, false),         // input_token_account
            AccountMeta::new(user_output_token, false),        // output_token_account
            AccountMeta::new(input_vault, false),              // input_vault
            AccountMeta::new(output_vault, false),             // output_vault
            AccountMeta::new_readonly(input_token_program, false), // input_token_program
            AccountMeta::new_readonly(output_token_program, false), // output_token_program
            AccountMeta::new_readonly(input_mint, false),      // input_token_mint
            AccountMeta::new_readonly(output_mint, false),     // output_token_mint
            AccountMeta::new(self.observation_state, false),   // observation_state
        ];

        let swap_instruction = Instruction {
            program_id: Self::get_program_id(),
            accounts,
            data,
        };

        // 7. Assemble Instructions
        let mut instructions = Vec::new();

        // Compute Budget
        instructions.push(ComputeBudgetInstruction::set_compute_unit_limit(1_400_000));

        // Create Input ATA (Idempotent)
        let spl_associated_token_account_program_id = solana_sdk::pubkey::Pubkey::new_from_array(
            spl_associated_token_account::id().to_bytes(),
        );

        // Create Output ATA (Idempotent)
        let create_output_ata_accounts = vec![
            AccountMeta::new(params.user_wallet, true),
            AccountMeta::new(user_output_token, false),
            AccountMeta::new_readonly(params.user_wallet, false),
            AccountMeta::new_readonly(output_mint, false),
            common::constants::SYSTEM_PROGRAM_META, // system_program
            AccountMeta::new_readonly(output_token_program, false),                 // token_program
        ];
        instructions.push(Instruction {
            program_id: spl_associated_token_account_program_id,
            accounts: create_output_ata_accounts,
            data: vec![1], // Idempotent
        });

        // Swap Instruction
        instructions.push(swap_instruction);

        Ok(instructions)
    }
}

