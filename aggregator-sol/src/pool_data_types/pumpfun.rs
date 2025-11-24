use std::sync::Arc;

use crate::arbitrage_transaction_handler::InputSwapParams;
use crate::pool_data_types::traits::BuildSwapInstruction;
use crate::{
    pool_data_types::{GetAmmConfig, PoolUpdateEventType, pumpf::{constants, functions::*}, common},
    utils::{get_sol_mint, tokens_equal},
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use solana_sdk::instruction::AccountMeta;
use solana_sdk::instruction::Instruction;
use solana_sdk::pubkey::Pubkey;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PumpfunPoolState {
    pub slot: u64,
    pub transaction_index: Option<u64>,
    pub address: Pubkey, // bonding curve address
    pub mint: Pubkey,
    pub last_updated: u64,
    pub liquidity_usd: f64,
    pub is_state_keys_initialized: bool,
    pub virtual_token_reserves: u64,
    pub virtual_sol_reserves: u64,
    pub real_token_reserves: u64,
    pub real_sol_reserves: u64,
    pub token_total_supply: u64,
    pub complete: bool,
    pub creator: Pubkey,
    pub is_mayhem_mode: bool,
    pub is_base_token_2022: bool, // Whether token_mint0 uses Token-2022 program
    pub is_quote_token_2022: bool, // Whether token_mint1 uses Token-2022 program
}

#[derive(Debug, Clone)]
pub struct PumpfunPoolUpdate {
    pub slot: u64,
    pub transaction_index: Option<u64>,
    pub address: Pubkey,
    pub mint: Pubkey,
    pub virtual_token_reserves: u64,
    pub virtual_sol_reserves: u64,
    pub real_token_reserves: u64,
    pub real_sol_reserves: u64,
    pub token_total_supply: u64,
    pub complete: bool,
    pub creator: Pubkey,
    pub is_mayhem_mode: bool,
    pub last_updated: u64,
    pub is_account_state_update: bool,
    pub pool_update_event_type: PoolUpdateEventType,
    pub additional_event_type: i32, // for tick array index tracking, 0 for others
}

#[allow(dead_code)]
impl PumpfunPoolState {
    pub fn get_program_id() -> Pubkey {
        constants::PUMPFUN
    }

    /// Calculate output amount for PumpFun bonding curve
    pub fn calculate_output_amount(
        &self,
        input_token: &Pubkey,
        input_amount: u64,
        _: Arc<dyn GetAmmConfig>,
    ) -> u64 {
        let is_buy = tokens_equal(input_token, &get_sol_mint());
        if is_buy {
            get_buy_token_amount_from_sol_amount(
                self.virtual_token_reserves as u128,
                self.virtual_sol_reserves as u128,
                self.real_token_reserves as u128,
                self.creator,
                input_amount,
            )
        } else {
            get_sell_sol_amount_from_token_amount(
                self.virtual_token_reserves as u128,
                self.virtual_sol_reserves as u128,
                self.creator,
                input_amount,
            )
        }
    }

    pub fn calculate_token_prices(
        &self,
        sol_price: f64,
        base_decimals: u8,
        quote_decimals: u8,
    ) -> (f64, f64) {
        // For Pumpfun: mint price in USD, sol price in USD
        // Price ratio needs to account for decimal scaling:
        // token_price_usd = (sol_reserve / token_reserve) * (10^base_decimals / 10^quote_decimals) * sol_price_usd

        if self.real_token_reserves == 0 {
            return (0.0, sol_price);
        }

        let decimal_scale = 10_f64.powi(base_decimals as i32 - quote_decimals as i32);
        let token_price =
            (self.real_sol_reserves as f64 / self.real_token_reserves as f64) * decimal_scale * sol_price;

        (token_price, sol_price)
    }
}

#[async_trait]
impl BuildSwapInstruction for PumpfunPoolState {
    /// Build PumpFun swap instruction
    /// Returns (instructions, other_amount_threshold) where:
    /// - For BUY: other_amount_threshold is the minimum tokens expected (output)
    /// - For SELL: other_amount_threshold is the minimum SOL expected (output)
    async fn build_swap_instruction(
        &self,
        params: &InputSwapParams,
        _amm_config_fetcher: Arc<dyn GetAmmConfig>,
    ) -> Result<(Vec<Instruction>, u64), String> {
        // Determine if this is a buy (SOL -> Token) or sell (Token -> SOL)
        let is_buy = tokens_equal(&params.input_token_mint, &get_sol_mint());
        
        // Convert creator Address to old Pubkey type
        let creator_old = anchor_lang::prelude::Pubkey::new_from_array(self.creator.to_bytes());
        let creator_vault_pda_old = sol_trade_sdk::instruction::utils::pumpfun::get_creator_vault_pda(&creator_old)
            .ok_or("Failed to derive creator vault")?;
        
        // Convert back to Address
        let creator_vault_pda = Pubkey::new_from_array(creator_vault_pda_old.to_bytes());
        let is_mayhem_mode = self.is_mayhem_mode;
        let token_program = if is_mayhem_mode {
            common::constants::TOKEN_PROGRAM_2022
        } else {
            common::constants::TOKEN_PROGRAM
        };
        let token_program_meta = if is_mayhem_mode {
            common::constants::TOKEN_PROGRAM_2022_META
        } else {
            common::constants::TOKEN_PROGRAM_META
        };
        let fee_recipient_meta = if is_mayhem_mode {
            constants::MAYHEM_FEE_RECIPIENT_META
        } else {
            constants::FEE_RECIPIENT_META
        };
        if is_buy {
            // ========================================
            // BUY: SOL -> Token
            // ========================================
            // Calculate expected token output
            let buy_token_amount = get_buy_token_amount_from_sol_amount(
                self.virtual_token_reserves as u128,
                self.virtual_sol_reserves as u128,
                self.real_token_reserves as u128,
                self.creator,
                params.input_amount,
            );
            // Calculate max SOL cost with slippage
            let max_sol_cost = params.input_amount
                + (params.input_amount * params.slippage_tolerance_bps as u64) / 10000;

            // Calculate minimum token output with slippage
            let min_token_output = buy_token_amount
                - (buy_token_amount * params.slippage_tolerance_bps as u64) / 10000;

            let bonding_curve_addr = if self.address == Pubkey::default() {
                get_bonding_curve_pda(&params.output_token_mint)
                    .ok_or(format!("Failed to get bonding curve PDA"))?
            } else {
                self.address
            };
            
            // Convert Address types to anchor_lang Pubkey for compatibility
            let user_wallet_old = anchor_lang::prelude::Pubkey::new_from_array(params.user_wallet.to_bytes());
            let output_mint_old = anchor_lang::prelude::Pubkey::new_from_array(params.output_token_mint.to_bytes());
            let bonding_curve_old = anchor_lang::prelude::Pubkey::new_from_array(bonding_curve_addr.to_bytes());
            let token_program_old = anchor_lang::prelude::Pubkey::new_from_array(token_program.to_bytes());
            
            // Get associated token accounts
            let associated_bonding_curve_old = spl_associated_token_account::get_associated_token_address_with_program_id(
                &bonding_curve_old,
                &output_mint_old,
                &token_program_old,
            );
            let user_token_account_old = spl_associated_token_account::get_associated_token_address_with_program_id(
                &user_wallet_old,
                &output_mint_old,
                &token_program_old,
            );
            
            // Convert back to Address for AccountMeta
            let associated_bonding_curve = solana_sdk::pubkey::Pubkey::new_from_array(associated_bonding_curve_old.to_bytes());
            let user_token_account = solana_sdk::pubkey::Pubkey::new_from_array(user_token_account_old.to_bytes());
            
            let mut instructions = Vec::with_capacity(2);

            // Manually build create ATA instruction to match Instruction types
            let create_ata_accounts = vec![
                AccountMeta::new(params.user_wallet, true), // funding
                AccountMeta::new(user_token_account, false), // associated_token
                AccountMeta::new_readonly(params.user_wallet, false), // wallet
                AccountMeta::new_readonly(params.output_token_mint, false), // mint
                constants::SYSTEM_PROGRAM_META, // system_program
                token_program_meta.clone(), // token_program
            ];
            
            let spl_associated_token_account_program_id = solana_sdk::pubkey::Pubkey::new_from_array(spl_associated_token_account::id().to_bytes());
            let create_ata_ix = Instruction {
                program_id: spl_associated_token_account_program_id,
                accounts: create_ata_accounts,
                data: vec![1], // Idempotent instruction discriminator
            };
            instructions.push(create_ata_ix);

            let user_volume_accumulator = get_user_volume_accumulator_pda(&params.user_wallet)
                .ok_or("Failed to get user volume accumulator".to_string())?;

            // Build instruction data (8 byte discriminator + 8 byte amount + 8 byte max cost)
            let mut buy_data = [0u8; 24];
            buy_data[..8].copy_from_slice(&[102, 6, 61, 18, 1, 218, 235, 234]); // Buy method ID
            buy_data[8..16].copy_from_slice(&buy_token_amount.to_le_bytes());
            buy_data[16..24].copy_from_slice(&max_sol_cost.to_le_bytes());
            // Build accounts array
            let buy_accounts: Vec<AccountMeta> = vec![
                constants::GLOBAL_ACCOUNT_META,
                fee_recipient_meta,
                AccountMeta::new_readonly(params.output_token_mint, false),
                AccountMeta::new(bonding_curve_addr, false),
                AccountMeta::new(associated_bonding_curve, false),
                AccountMeta::new(user_token_account, false),
                AccountMeta::new(params.user_wallet, true),
                constants::SYSTEM_PROGRAM_META,
                token_program_meta,
                AccountMeta::new(creator_vault_pda, false),
                constants::EVENT_AUTHORITY_META,
                constants::PUMPFUN_META,
                constants::GLOBAL_VOLUME_ACCUMULATOR_META,
                AccountMeta::new(user_volume_accumulator, false),
                constants::FEE_CONFIG_META,
                constants::FEE_PROGRAM_META,
            ];

            instructions.push(Instruction::new_with_bytes(Self::get_program_id(), &buy_data, buy_accounts));
            Ok((instructions, min_token_output))
        } else {    
            // ========================================
            // SELL: Token -> SOL
            // ========================================
            // Calculate expected SOL output
            let sol_amount = get_sell_sol_amount_from_token_amount(
                self.virtual_token_reserves as u128,
                self.virtual_sol_reserves as u128,
                self.creator,
                params.input_amount,
            );
            // Calculate minimum SOL output with slippage
            let min_sol_output = if sol_amount <= params.slippage_tolerance_bps as u64 {    
                1
            } else {
                sol_amount - (sol_amount * params.slippage_tolerance_bps as u64) / 10000
            };
            // Get bonding curve PDA
            let bonding_curve_addr = if self.address == Pubkey::default() {
                get_bonding_curve_pda(&params.input_token_mint)
                    .ok_or(format!("Failed to get bonding curve PDA"))?
            } else {
                self.address
            };
            
            // Convert Address types to anchor_lang Pubkey for compatibility
            let user_wallet_old = anchor_lang::prelude::Pubkey::new_from_array(params.user_wallet.to_bytes());
            let input_mint_old = anchor_lang::prelude::Pubkey::new_from_array(params.input_token_mint.to_bytes());
            let bonding_curve_old = anchor_lang::prelude::Pubkey::new_from_array(bonding_curve_addr.to_bytes());
            let token_program_old = anchor_lang::prelude::Pubkey::new_from_array(token_program.to_bytes());
            
            // Get associated token accounts
            let associated_bonding_curve_old = spl_associated_token_account::get_associated_token_address_with_program_id(
                &bonding_curve_old,
                &input_mint_old,
                &token_program_old,
            );
            let user_token_account_old = spl_associated_token_account::get_associated_token_address_with_program_id(
                &user_wallet_old,
                &input_mint_old,
                &token_program_old,
            );
            
            // Convert back to Address for AccountMeta
            let associated_bonding_curve = solana_sdk::pubkey::Pubkey::new_from_array(associated_bonding_curve_old.to_bytes());
            let user_token_account = solana_sdk::pubkey::Pubkey::new_from_array(user_token_account_old.to_bytes());
            
            // ========================================
            // Build instructions
            // ======================================== 
            let mut instructions = Vec::with_capacity(2);
            
            // Manually build create ATA instruction to match Instruction types
            let create_ata_accounts = vec![
                AccountMeta::new(params.user_wallet, true), // funding
                AccountMeta::new(user_token_account, false), // associated_token
                AccountMeta::new_readonly(params.user_wallet, false), // wallet
                AccountMeta::new_readonly(params.input_token_mint, false), // mint
                constants::SYSTEM_PROGRAM_META, // system_program
                token_program_meta.clone(), // token_program
            ];
            
            let spl_associated_token_account_program_id = solana_sdk::pubkey::Pubkey::new_from_array(spl_associated_token_account::id().to_bytes());
            let create_ata_ix = Instruction {
                program_id: spl_associated_token_account_program_id,
                accounts: create_ata_accounts,
                data: vec![1], // Idempotent instruction discriminator
            };
            instructions.push(create_ata_ix);
            // Build instruction data (8 byte discriminator + 8 byte amount + 8 byte min output)
            let mut sell_data = [0u8; 24];
            sell_data[..8].copy_from_slice(&[51, 230, 133, 164, 1, 127, 131, 173]); // Sell method ID
            sell_data[8..16].copy_from_slice(&params.input_amount.to_le_bytes());
            sell_data[16..24].copy_from_slice(&min_sol_output.to_le_bytes());
            // Build accounts array (14 accounts for sell)
            let sell_accounts: Vec<AccountMeta> = vec![
                constants::GLOBAL_ACCOUNT_META,
                fee_recipient_meta,
                AccountMeta::new_readonly(params.input_token_mint, false),
                AccountMeta::new(bonding_curve_addr, false),
                AccountMeta::new(associated_bonding_curve, false),
                AccountMeta::new(user_token_account, false),
                AccountMeta::new(params.user_wallet, true),
                constants::SYSTEM_PROGRAM_META,
                AccountMeta::new(creator_vault_pda, false),
                token_program_meta,
                constants::EVENT_AUTHORITY_META,
                constants::PUMPFUN_META,
                constants::FEE_CONFIG_META,
                constants::FEE_PROGRAM_META,
            ];

            instructions.push(Instruction::new_with_bytes(Self::get_program_id(), &sell_data, sell_accounts));
            Ok((instructions, min_sol_output))
        }
    }
}
