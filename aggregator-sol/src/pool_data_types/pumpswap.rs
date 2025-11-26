use std::sync::Arc;

use crate::{
    constants::is_base_token,
    pool_data_types::{GetAmmConfig, PoolUpdateEventType, pumpf::{constants, functions::*}, common},
    utils::tokens_equal,
};
use serde::{Deserialize, Serialize};

use solana_sdk::pubkey::Pubkey;
use solana_streamer_sdk::streaming::event_parser::protocols::pumpswap::parser::PUMPSWAP_PROGRAM_ID;
use crate::pool_data_types::traits::BuildSwapInstruction;
use crate::types::SwapParams;
use solana_sdk::instruction::{AccountMeta, Instruction};
use anyhow::Result;
use async_trait::async_trait;
use sol_trade_sdk::utils::calc::pumpswap::{buy_quote_input_internal, sell_base_input_internal};

pub const BUY_DISCRIMINATOR: [u8; 8] = [102, 6, 61, 18, 1, 218, 235, 234];
pub const SELL_DISCRIMINATOR: [u8; 8] = [51, 230, 133, 164, 1, 127, 131, 173];

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PumpSwapPoolState {
    pub slot: u64,
    pub transaction_index: Option<u64>,
    pub address: Pubkey,
    pub index: u16,
    pub creator: Option<Pubkey>,
    pub base_mint: Pubkey,
    pub quote_mint: Pubkey,
    pub pool_base_token_account: Pubkey,
    pub pool_quote_token_account: Pubkey,
    pub last_updated: u64,
    pub base_reserve: u64,
    pub quote_reserve: u64,
    pub liquidity_usd: f64,
    pub is_state_keys_initialized: bool,
    pub is_base_token_2022: bool, // Whether base_mint uses Token-2022 program
    pub is_quote_token_2022: bool, // Whether quote_mint uses Token-2022 program
    pub coin_creator_vault_ata: Pubkey,
    pub coin_creator_vault_authority: Pubkey,
    pub base_token_program: Pubkey,
    pub quote_token_program: Pubkey,
    pub coin_creator: Pubkey,
    pub protocol_fee_recipient: Pubkey,
}

#[derive(Debug, Clone)]
pub struct PumpSwapPoolUpdate {
    pub slot: u64,
    pub transaction_index: Option<u64>,
    pub address: Pubkey,
    pub index: Option<u16>,
    pub creator: Option<Pubkey>,
    pub base_mint: Pubkey,
    pub quote_mint: Pubkey,
    pub pool_base_token_account: Pubkey,
    pub pool_quote_token_account: Pubkey,
    pub last_updated: u64,
    pub base_reserve: u64,
    pub quote_reserve: u64,
    pub coin_creator_vault_ata: Pubkey,
    pub coin_creator_vault_authority: Pubkey,
    pub base_token_program: Pubkey,
    pub quote_token_program: Pubkey,
    pub coin_creator: Pubkey,
    pub protocol_fee_recipient: Pubkey,
    pub is_account_state_update: bool,
    pub pool_update_event_type: PoolUpdateEventType,
    pub additional_event_type: i32, // for tick array index tracking, 0 for others
}

#[allow(dead_code)]
impl PumpSwapPoolState {
    pub fn get_program_id() -> Pubkey {
        Pubkey::new_from_array(*PUMPSWAP_PROGRAM_ID.as_array())
    }

    /// Calculate output amount for PumpFun bonding curve
    pub fn calculate_output_amount(
        &self,
        input_token: &Pubkey,
        input_amount: u64,
        _: Arc<dyn GetAmmConfig>,
    ) -> u64 {
        let (base_token, _quote_token) = (self.base_mint, self.quote_mint);
        let input_is_base = tokens_equal(input_token, &base_token);
        let (input_reserve, output_reserve) = if input_is_base {
            (self.base_reserve, self.quote_reserve)
        } else {
            (self.quote_reserve, self.base_reserve)
        };
        let new_input_reserve = input_reserve as u128 + input_amount as u128;
        let new_output_reserve =
            (input_reserve as u128 * output_reserve as u128 / new_input_reserve) as u64;
        let output_amount = output_reserve - new_output_reserve;

        output_amount * 997 / 1000 // Apply 0.3% fee
    }

    pub fn calculate_token_prices(
        &self,
        sol_price: f64,
        base_decimals: u8,
        quote_decimals: u8,
    ) -> (f64, f64) {
        if self.quote_reserve == 0 || self.base_reserve == 0 {
            return (0.0, 0.0);
        }

        let base_token_str = self.base_mint.to_string();
        let quote_token_str = self.quote_mint.to_string();

        let is_base_a_base_token = is_base_token(&base_token_str);
        let is_quote_a_base_token = is_base_token(&quote_token_str);

        let decimal_scale = 10_f64.powi(base_decimals as i32 - quote_decimals as i32);

        // If quote is a base token (like USDC, SOL), use its price
        if is_quote_a_base_token {
            let quote_price = if is_quote_a_base_token
                && quote_token_str == "So11111111111111111111111111111111111111112"
            {
                sol_price // SOL
            } else {
                1.0 // Assume USDC/USDT are ~$1
            };

            let base_price = (self.quote_reserve as f64 / self.base_reserve as f64)
                * decimal_scale
                * quote_price;
            (base_price, quote_price)
        } else if is_base_a_base_token {
            // If base is a base token, use its price
            let base_price = if base_token_str == "So11111111111111111111111111111111111111112" {
                sol_price // SOL
            } else {
                1.0 // Assume USDC/USDT are ~$1
            };

            let quote_price = (self.base_reserve as f64 / self.quote_reserve as f64)
                * (1.0 / decimal_scale)
                * base_price;
            (base_price, quote_price)
        } else {
            // Neither token is a base token, assume quote_reserve is the pricing reference
            let base_price =
                (self.quote_reserve as f64 / self.base_reserve as f64) * decimal_scale * 1.0;
            (base_price, 1.0)
        }
    }
}

#[async_trait]
impl BuildSwapInstruction for PumpSwapPoolState {
    async fn build_swap_instruction(
        &self,
        params: &SwapParams,
        _amm_config_fetcher: Arc<dyn GetAmmConfig>,
    ) -> Result<Vec<Instruction>, String> {
        let pool = self.address;
        let base_mint = self.base_mint;
        let quote_mint = self.quote_mint;
        let pool_base_token_account = self.pool_base_token_account;
        let pool_quote_token_account = self.pool_quote_token_account;
        
        // Default values for missing fields
        let is_mayhem_mode = false;
        let create_wsol_ata = true;
        let close_wsol_ata = true;
        let creator = self.coin_creator;
        let params_coin_creator_vault_authority = self.coin_creator_vault_authority;
        let params_coin_creator_vault_ata = self.coin_creator_vault_ata;
        let base_token_program = self.base_token_program;
        let quote_token_program = self.quote_token_program;    

        let is_wsol = (base_mint == constants::WSOL_TOKEN_ACCOUNT
            && quote_mint != constants::USDC_TOKEN_ACCOUNT)
            || (quote_mint == constants::WSOL_TOKEN_ACCOUNT
                && base_mint != constants::USDC_TOKEN_ACCOUNT);
        let is_usdc = (base_mint == constants::USDC_TOKEN_ACCOUNT
            && quote_mint != constants::WSOL_TOKEN_ACCOUNT)
            || (quote_mint == constants::USDC_TOKEN_ACCOUNT
                && base_mint != constants::WSOL_TOKEN_ACCOUNT);
        if !is_wsol && !is_usdc {
            return Err("Pool must contain WSOL or USDC".to_string());
        }

        // ========================================
        // Trade calculation and account address preparation
        // ========================================
        let quote_is_wsol_or_usdc = quote_mint == constants::WSOL_TOKEN_ACCOUNT
            || quote_mint == constants::USDC_TOKEN_ACCOUNT;

        let (mut token_amount, sol_amount) = if quote_is_wsol_or_usdc {
            let result = buy_quote_input_internal(
                params.input_amount,
                params.slippage_bps as u64,
                self.base_reserve,
                self.quote_reserve,
                &creator,
            )
            .unwrap();
            // base_amount_out, max_quote_amount_in
            (result.base, result.max_quote)
        } else {
            let result = sell_base_input_internal(
                params.input_amount,
                params.slippage_bps as u64,
                self.base_reserve,
                self.quote_reserve,
                &creator,
            )
            .unwrap();
            // min_quote_amount_out, base_amount_in
            (result.min_quote, params.input_amount)
        };

        // Convert Address types to anchor_lang Pubkey for compatibility
        let user_wallet_old = anchor_lang::prelude::Pubkey::new_from_array(params.user_wallet.to_bytes());
        let base_mint_old = anchor_lang::prelude::Pubkey::new_from_array(params.input_token.address.to_bytes());
        let quote_mint_old = anchor_lang::prelude::Pubkey::new_from_array(params.output_token.address.to_bytes());
        let base_token_program_old = anchor_lang::prelude::Pubkey::new_from_array(base_token_program.to_bytes());
        let quote_token_program_old = anchor_lang::prelude::Pubkey::new_from_array(quote_token_program.to_bytes());

        // Get user's associated token accounts
        let user_base_token_account_old = spl_associated_token_account::get_associated_token_address_with_program_id(
            &user_wallet_old,
            &base_mint_old,
            &base_token_program_old,
        );
        let user_quote_token_account_old = spl_associated_token_account::get_associated_token_address_with_program_id(
            &user_wallet_old,
            &quote_mint_old,
            &quote_token_program_old,
        );
        // Convert back to Address for AccountMeta
        let user_base_token_account = solana_sdk::pubkey::Pubkey::new_from_array(user_base_token_account_old.to_bytes());
        let user_quote_token_account = solana_sdk::pubkey::Pubkey::new_from_array(user_quote_token_account_old.to_bytes());
        let is_mayhem_mode = self.protocol_fee_recipient == constants::MAYHEM_FEE_RECIPIENT_SWAP;
        let fee_recipient =
            if is_mayhem_mode { constants::MAYHEM_FEE_RECIPIENT } else { constants::FEE_RECIPIENT };
        let fee_recipient_meta = if is_mayhem_mode {
            constants::MAYHEM_FEE_RECIPIENT_META
        } else {
            constants::FEE_RECIPIENT_META
        };

        let fee_recipient_old = anchor_lang::prelude::Pubkey::new_from_array(fee_recipient.to_bytes());

        let fee_recipient_ata_old = spl_associated_token_account::get_associated_token_address(
            &fee_recipient_old,
            &quote_mint_old,
        );
        let fee_recipient_ata = solana_sdk::pubkey::Pubkey::new_from_array(fee_recipient_ata_old.to_bytes());

        // ========================================
        // Build instructions
        // ========================================
        let mut instructions = Vec::with_capacity(6);

        if create_wsol_ata {
            instructions
                .extend(sol_trade_sdk::trading::common::handle_wsol(&params.user_wallet, sol_amount));
        }

        // Create output token ATA instruction (idempotent - creates if doesn't exist)
        let create_output_ata_accounts = vec![
            AccountMeta::new(params.user_wallet, true), // funding
            AccountMeta::new(user_quote_token_account, false), // associated_token
            AccountMeta::new_readonly(params.user_wallet, false), // wallet
            AccountMeta::new_readonly(quote_mint, false), // mint
            common::constants::SYSTEM_PROGRAM_META, // system_program
            if is_mayhem_mode {
                common::constants::TOKEN_PROGRAM_2022_META
            } else {
                common::constants::TOKEN_PROGRAM_META
            }, // token_program
        ];
        
        let spl_associated_token_account_program_id = solana_sdk::pubkey::Pubkey::new_from_array(spl_associated_token_account::id().to_bytes());
        let create_output_ata_ix = Instruction {
            program_id: spl_associated_token_account_program_id,
            accounts: create_output_ata_accounts,
            data: vec![1], // Idempotent instruction discriminator
        };
        instructions.push(create_output_ata_ix);


        // Create buy instruction
        let mut accounts = Vec::with_capacity(23);
        accounts.extend([
            AccountMeta::new(pool, false),                          // pool_id
            AccountMeta::new(params.user_wallet, true),          // user (signer)
            constants::GLOBAL_ACCOUNT_META,                          // global (readonly)
            AccountMeta::new_readonly(base_mint, false),            // base_mint (readonly)
            AccountMeta::new_readonly(quote_mint, false),           // quote_mint (readonly)
            AccountMeta::new(user_base_token_account, false),       // user_base_token_account
            AccountMeta::new(user_quote_token_account, false),      // user_quote_token_account
            AccountMeta::new(pool_base_token_account, false),       // pool_base_token_account
            AccountMeta::new(pool_quote_token_account, false),      // pool_quote_token_account
            fee_recipient_meta,                                     // fee_recipient (readonly)
            AccountMeta::new(fee_recipient_ata, false),             // fee_recipient_ata
            AccountMeta::new_readonly(base_token_program, false),   // TOKEN_PROGRAM_ID (readonly)
            AccountMeta::new_readonly(quote_token_program, false), // TOKEN_PROGRAM_ID (readonly, duplicated as in JS)
            common::constants::SYSTEM_PROGRAM_META,                 // System Program (readonly)
            constants::ASSOCIATED_TOKEN_PROGRAM_META, // ASSOCIATED_TOKEN_PROGRAM_ID (readonly)
            constants::EVENT_AUTHORITY_META,          // event_authority (readonly)
            constants::AMM_PROGRAM_META,              // PUMP_AMM_PROGRAM_ID (readonly)
            AccountMeta::new(params_coin_creator_vault_ata, false), // coin_creator_vault_ata
            AccountMeta::new_readonly(params_coin_creator_vault_authority, false), // coin_creator_vault_authority (readonly)
        ]);
        if quote_is_wsol_or_usdc {
            accounts.push(constants::GLOBAL_VOLUME_ACCUMULATOR_META);
            accounts.push(AccountMeta::new(
                get_user_volume_accumulator_pda(&params.user_wallet).unwrap(),
                false,
            ));
        }
        accounts.push(constants::FEE_CONFIG_META);
        accounts.push(constants::FEE_PROGRAM_META);

        // Create instruction data
        let mut data = [0u8; 24];
        if quote_is_wsol_or_usdc {
            data[..8].copy_from_slice(&BUY_DISCRIMINATOR);
            // base_amount_out
            data[8..16].copy_from_slice(&token_amount.to_le_bytes());
            // max_quote_amount_in
            data[16..24].copy_from_slice(&sol_amount.to_le_bytes());
        } else {
            data[..8].copy_from_slice(&SELL_DISCRIMINATOR);
            // base_amount_in
            data[8..16].copy_from_slice(&sol_amount.to_le_bytes());
            // min_quote_amount_out
            data[16..24].copy_from_slice(&token_amount.to_le_bytes());
        }

        let buy_instruction = Instruction {
            program_id: accounts::AMM_PROGRAM,
            accounts: accounts.clone(),
            data: data.to_vec(),
        };

        instructions.push(buy_instruction);
        if close_wsol_ata {
            instructions.extend(sol_trade_sdk::trading::common::close_wsol(&params.user_wallet));
        }
        Ok(instructions)
    }
}