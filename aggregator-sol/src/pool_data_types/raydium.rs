use std::sync::Arc;
use crate::{
    constants::is_base_token,
    pool_data_types::{GetAmmConfig, PoolUpdateEventType, traits::BuildSwapInstruction, common::constants},
    utils::tokens_equal,
};
use serde::{Deserialize, Serialize};
use solana_sdk::{pubkey::Pubkey, instruction::{Instruction, AccountMeta}};
use solana_streamer_sdk::streaming::event_parser::protocols::raydium_amm_v4::parser::RAYDIUM_AMM_V4_PROGRAM_ID;
use crate::types::SwapParams;
use async_trait::async_trait;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaydiumAmmV4PoolState {
    pub slot: u64,
    pub transaction_index: Option<u64>,
    pub address: Pubkey, // bonding curve address
    pub base_mint: Pubkey,
    pub quote_mint: Pubkey,
    pub amm_authority: Pubkey,
    pub amm_open_orders: Pubkey,
    pub amm_target_orders: Pubkey,
    pub pool_coin_token_account: Pubkey,
    pub pool_pc_token_account: Pubkey,
    pub serum_program: Pubkey,
    pub serum_market: Pubkey,
    pub serum_bids: Pubkey,
    pub serum_asks: Pubkey,
    pub serum_event_queue: Pubkey,
    pub serum_coin_vault_account: Pubkey,
    pub serum_pc_vault_account: Pubkey,
    pub serum_vault_signer: Pubkey,
    pub last_updated: u64,
    pub base_reserve: u64,
    pub quote_reserve: u64,
    pub liquidity_usd: f64,
    pub is_state_keys_initialized: bool,
}

#[derive(Debug, Clone)]
pub struct RaydiumAmmV4PoolUpdate {
    pub slot: u64,
    pub transaction_index: Option<u64>,
    pub address: Pubkey,
    pub base_mint: Pubkey,
    pub quote_mint: Pubkey,
    pub amm_authority: Pubkey,
    pub amm_open_orders: Pubkey,
    pub amm_target_orders: Pubkey,
    pub pool_coin_token_account: Pubkey,
    pub pool_pc_token_account: Pubkey,
    pub serum_program: Option<Pubkey>,
    pub serum_market: Option<Pubkey>,
    pub serum_bids: Option<Pubkey>,
    pub serum_asks: Option<Pubkey>,
    pub serum_event_queue: Option<Pubkey>,
    pub serum_coin_vault_account: Option<Pubkey>,
    pub serum_pc_vault_account: Option<Pubkey>,
    pub serum_vault_signer: Option<Pubkey>,
    pub last_updated: u64,
    pub base_reserve: u64,
    pub quote_reserve: u64,
    pub is_account_state_update: bool,
    pub pool_update_event_type: PoolUpdateEventType,
    pub additional_event_type: i32, // for tick array index tracking, 0 for others
}

#[allow(dead_code)]
impl RaydiumAmmV4PoolState {
    pub fn get_program_id() -> Pubkey {
        Pubkey::new_from_array(*RAYDIUM_AMM_V4_PROGRAM_ID.as_array())
    }

    /// Calculate output amount for PumpFun bonding curve
    pub fn calculate_output_amount(
        &self,
        input_token: &Pubkey,
        input_amount: u64,
        _: Arc<dyn GetAmmConfig>,
    ) -> u64 {
        let (base_token, _) = (self.base_mint, self.quote_mint);
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

        output_amount * 9975 / 10000 // Apply 0.25% fee
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
            let quote_price = if quote_token_str == "So11111111111111111111111111111111111111112" {
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
            // Neither token is a base token, assume relative pricing
            let base_price =
                (self.quote_reserve as f64 / self.base_reserve as f64) * decimal_scale * 1.0;
            (base_price, 1.0)
        }
    }
}

#[async_trait]
impl BuildSwapInstruction for RaydiumAmmV4PoolState {
    async fn build_swap_instruction(
        &self,
        params: &SwapParams,
        _amm_config_fetcher: Arc<dyn GetAmmConfig>,
    ) -> Result<Vec<Instruction>, String> {
        // Determine if this is a buy (WSOL/USDC -> Token) or sell (Token -> WSOL/USDC)
        let is_wsol = self.base_mint == constants::WSOL_TOKEN_ACCOUNT
            || self.quote_mint == constants::WSOL_TOKEN_ACCOUNT;
        let is_usdc = self.base_mint == constants::USDC_TOKEN_ACCOUNT
            || self.quote_mint == constants::USDC_TOKEN_ACCOUNT;

        if !is_wsol && !is_usdc {
            return Err("Pool must contain WSOL or USDC".to_string());
        }

        // Determine swap direction and calculate amounts
        let is_buy = tokens_equal(&params.input_token.address, &constants::WSOL_TOKEN_ACCOUNT)
            || tokens_equal(&params.input_token.address, &constants::USDC_TOKEN_ACCOUNT);

        let output_amount = self.calculate_output_amount(
            &params.input_token.address,
            params.input_amount,
            _amm_config_fetcher.clone(),
        );

        // Apply slippage tolerance
        let minimum_amount_out = (output_amount as u64) * (10000 - params.slippage_bps as u64) / 10000;

        // Determine source and destination token accounts
        let (source_mint, dest_mint) = if is_buy {
            (
                if is_wsol { constants::WSOL_TOKEN_ACCOUNT } else { constants::USDC_TOKEN_ACCOUNT },
                params.output_token.address
            )
        } else {
            (
                params.input_token.address,
                if is_wsol { constants::WSOL_TOKEN_ACCOUNT } else { constants::USDC_TOKEN_ACCOUNT }
            )
        };

        // Convert to anchor_lang Pubkey for ATA derivation
        let user_wallet_anchor = anchor_lang::prelude::Pubkey::new_from_array(params.user_wallet.to_bytes());
        let source_mint_anchor = anchor_lang::prelude::Pubkey::new_from_array(source_mint.to_bytes());
        let dest_mint_anchor = anchor_lang::prelude::Pubkey::new_from_array(dest_mint.to_bytes());

        // Derive ATAs using anchor_lang types
        let user_source_token_account_anchor = spl_associated_token_account::get_associated_token_address(
            &user_wallet_anchor,
            &source_mint_anchor,
        );
        let user_destination_token_account_anchor = spl_associated_token_account::get_associated_token_address(
            &user_wallet_anchor,
            &dest_mint_anchor,
        );

        // Convert back to solana_sdk Pubkey for AccountMeta
        let user_source_token_account = solana_sdk::pubkey::Pubkey::new_from_array(user_source_token_account_anchor.to_bytes());
        let user_destination_token_account = solana_sdk::pubkey::Pubkey::new_from_array(user_destination_token_account_anchor.to_bytes());

        // Build instructions
        let mut instructions = Vec::with_capacity(6);

        // Handle WSOL wrapping/unwrapping
        if is_buy {
            // Buying token with SOL: wrap SOL to WSOL
            instructions.extend(sol_trade_sdk::trading::common::handle_wsol(
                &params.user_wallet,
                params.input_amount,
            ));
        } else {
            // Selling token for SOL: create WSOL ATA to receive
            instructions.extend(
                sol_trade_sdk::trading::common::wsol_manager::create_wsol_ata(&params.user_wallet),
            );
        }

        let spl_associated_token_account_program_id = solana_sdk::pubkey::Pubkey::new_from_array(
            spl_associated_token_account::id().to_bytes(),
        );

        // Create output ATA if needed (for buying tokens)
        
        let user_output_token_account = solana_sdk::pubkey::Pubkey::new_from_array(user_destination_token_account_anchor.to_bytes());
        let create_output_ata_accounts = vec![
            AccountMeta::new(params.user_wallet, true),                 // funding
            AccountMeta::new(user_output_token_account, false),         // associated_token
            AccountMeta::new_readonly(params.user_wallet, false),       // wallet
            AccountMeta::new_readonly(params.output_token.address, false), // mint
            constants::SYSTEM_PROGRAM_META,                     // system_program
            constants::TOKEN_PROGRAM_META,                      // token_program (SPL Token only for Raydium V4)
        ];
        let create_output_ata_ix = Instruction {
            program_id: spl_associated_token_account_program_id,
            accounts: create_output_ata_accounts,
            data: vec![1], // Idempotent instruction discriminator
        };
        instructions.push(create_output_ata_ix);
        
        // Build the swap instruction (SwapBaseIn - tag 9)
        // 17 accounts as per Raydium AMM V4 spec
        let accounts: Vec<AccountMeta> = vec![
            constants::TOKEN_PROGRAM_META,             // 0. Token Program
            AccountMeta::new(self.address, false),                         // 1. AMM
            AccountMeta::new(self.amm_authority, false),          // 2. AMM Authority
            AccountMeta::new(self.amm_open_orders, false),                 // 3. AMM Open Orders
            AccountMeta::new(self.pool_coin_token_account, false),         // 4. Pool Coin Token Account
            AccountMeta::new(self.pool_pc_token_account, false),           // 5. Pool PC Token Account
            AccountMeta::new(self.serum_program, false),          // 6. Serum Program
            AccountMeta::new(self.serum_market, false),                    // 7. Serum Market
            AccountMeta::new(self.serum_bids, false),                      // 8. Serum Bids
            AccountMeta::new(self.serum_asks, false),                      // 9. Serum Asks
            AccountMeta::new(self.serum_event_queue, false),               // 10. Serum Event Queue
            AccountMeta::new(self.serum_coin_vault_account, false),        // 11. Serum Coin Vault
            AccountMeta::new(self.serum_pc_vault_account, false),          // 12. Serum PC Vault
            AccountMeta::new(self.serum_vault_signer, false),     // 13. Serum Vault Signer
            AccountMeta::new(user_source_token_account, false),            // 14. User Source Token Account
            AccountMeta::new(user_destination_token_account, false),       // 15. User Destination Token Account
            AccountMeta::new(params.user_wallet, true),                    // 16. User wallet (signer)
        ];

        // Create instruction data: [discriminator(1 byte) | amount_in(8 bytes) | minimum_amount_out(8 bytes)]
        let mut data = vec![9u8]; // SwapBaseIn discriminator
        data.extend_from_slice(&params.input_amount.to_le_bytes());
        data.extend_from_slice(&minimum_amount_out.to_le_bytes());

        let swap_instruction = Instruction {
            program_id: Self::get_program_id(),
            accounts,
            data,
        };
        instructions.push(swap_instruction);

        // Close WSOL ATA after swap if selling tokens
        if !is_buy {
            instructions.extend(sol_trade_sdk::trading::common::close_wsol(&params.user_wallet));
        }

        Ok(instructions)
    }
}