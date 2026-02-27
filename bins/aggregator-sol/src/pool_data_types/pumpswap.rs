use crate::{
    pool_data_types::{
        common,
        common::functions,
        pumpf::{
            constants,
            functions::{self as pumpf_functions, *},
        },
        GetAmmConfig, PoolUpdateEventType,
    },
    utils::{get_sol_mint, tokens_equal},
};
use serde::{Deserialize, Serialize};

use crate::pool_data_types::traits::BuildSwapInstruction;
use crate::types::SwapParams;
use async_trait::async_trait;
use sol_trade_sdk::utils::calc::pumpswap::{buy_quote_input_internal, sell_base_input_internal};
use solana_sdk::instruction::{AccountMeta, Instruction};
use solana_sdk::pubkey::Pubkey;
use solana_streamer_sdk::streaming::event_parser::protocols::pumpswap::parser::PUMPSWAP_PROGRAM_ID;

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
    pub coin_creator: Pubkey,
    pub protocol_fee_recipient: Pubkey,
    pub is_cashback: bool,
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
    pub coin_creator: Pubkey,
    pub protocol_fee_recipient: Pubkey,
    pub is_account_state_update: bool,
    pub pool_update_event_type: PoolUpdateEventType,
    pub additional_event_type: i32, // for tick array index tracking, 0 for others
    pub is_cashback: Option<bool>,
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
        _: &dyn GetAmmConfig,
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
        functions::calculate_amm_token_prices(
            &self.base_mint,
            &self.quote_mint,
            self.base_reserve,
            self.quote_reserve,
            sol_price,
            base_decimals,
            quote_decimals,
        )
    }
}

use solana_client::nonblocking::rpc_client::RpcClient;
use std::sync::Arc;

#[async_trait]
impl BuildSwapInstruction for PumpSwapPoolState {
    async fn build_swap_instruction(
        &self,
        params: &SwapParams,
        _amm_config_fetcher: &dyn GetAmmConfig,
        _rpc_client: Option<&Arc<RpcClient>>,
    ) -> std::result::Result<Vec<Instruction>, String> {
        let pool = self.address;
        let base_mint = self.base_mint;
        let quote_mint = self.quote_mint;
        let pool_base_token_account = self.pool_base_token_account;
        let pool_quote_token_account = self.pool_quote_token_account;
        let creator = self.coin_creator;
        let coin_creator_vault_ata = pumpf_functions::coin_creator_vault_ata(creator, quote_mint);
        let coin_creator_vault_authority = pumpf_functions::coin_creator_vault_authority(creator);
        let base_token_program =
            functions::get_token_program(if params.input_token.address == base_mint {
                params.input_token.is_token_2022
            } else {
                params.output_token.is_token_2022
            });

        let quote_token_program =
            functions::get_token_program(if params.input_token.address == quote_mint {
                params.input_token.is_token_2022
            } else {
                params.output_token.is_token_2022
            });

        let is_wsol = (base_mint == common::constants::WSOL_TOKEN_ACCOUNT
            && quote_mint != common::constants::USDC_TOKEN_ACCOUNT)
            || (quote_mint == common::constants::WSOL_TOKEN_ACCOUNT
                && base_mint != common::constants::USDC_TOKEN_ACCOUNT);
        let is_usdc = (base_mint == common::constants::USDC_TOKEN_ACCOUNT
            && quote_mint != common::constants::WSOL_TOKEN_ACCOUNT)
            || (quote_mint == common::constants::USDC_TOKEN_ACCOUNT
                && base_mint != common::constants::WSOL_TOKEN_ACCOUNT);
        if !is_wsol && !is_usdc {
            return Err("Pool must contain WSOL or USDC".to_string());
        }

        // ========================================
        // Trade calculation and account address preparation
        // ========================================
        let quote_is_wsol_or_usdc = quote_mint == common::constants::WSOL_TOKEN_ACCOUNT
            || quote_mint == common::constants::USDC_TOKEN_ACCOUNT;

        let is_buy = tokens_equal(&params.input_token.address, &get_sol_mint());
        let (token_amount, sol_amount): (u64, u64);
        if is_buy {
            (token_amount, sol_amount) = if quote_is_wsol_or_usdc {
                let result = buy_quote_input_internal(
                    params.input_amount,
                    params.slippage_bps as u64,
                    self.base_reserve,
                    self.quote_reserve,
                    &creator,
                )
                .map_err(|e| format!("PumpSwap calculation failed: {}", e))?;
                (result.base, result.max_quote)
            } else {
                let result = sell_base_input_internal(
                    params.input_amount,
                    params.slippage_bps as u64,
                    self.base_reserve,
                    self.quote_reserve,
                    &creator,
                )
                .map_err(|e| format!("PumpSwap calculation failed: {}", e))?;
                (result.min_quote, params.input_amount)
            };
        } else {
            (token_amount, sol_amount) = if quote_is_wsol_or_usdc {
                let result = sell_base_input_internal(
                    params.input_amount,
                    params.slippage_bps as u64,
                    self.base_reserve,
                    self.quote_reserve,
                    &creator,
                )
                .map_err(|e| format!("PumpSwap calculation failed: {}", e))?;
                (params.input_amount, result.min_quote)
            } else {
                let result = buy_quote_input_internal(
                    params.input_amount,
                    params.slippage_bps as u64,
                    self.base_reserve,
                    self.quote_reserve,
                    &creator,
                )
                .map_err(|e| format!("PumpSwap calculation failed: {}", e))?;
                (result.max_quote, result.base)
            };
        }

        if token_amount == 0 || sol_amount == 0 {
            return Err("PumpSwap calculated amounts must be greater than zero".to_string());
        }

        // Determine if this is an actual BUY or SELL instruction
        let is_buy_instruction =
            (is_buy && quote_is_wsol_or_usdc) || (!is_buy && !quote_is_wsol_or_usdc);

        // Convert Address types to anchor_lang Pubkey for compatibility
        let user_wallet_anchor = functions::to_pubkey(&params.user_wallet);
        let base_mint_anchor = functions::to_pubkey(&base_mint);
        let quote_mint_anchor = functions::to_pubkey(&quote_mint);
        let base_token_program_anchor = functions::to_pubkey(&base_token_program);
        let quote_token_program_anchor = functions::to_pubkey(&quote_token_program);

        // Get user's associated token accounts
        let user_base_token_account_old =
            spl_associated_token_account::get_associated_token_address_with_program_id(
                &user_wallet_anchor,
                &base_mint_anchor,
                &base_token_program_anchor,
            );
        let user_quote_token_account_old =
            spl_associated_token_account::get_associated_token_address_with_program_id(
                &user_wallet_anchor,
                &quote_mint_anchor,
                &quote_token_program_anchor,
            );
        // Convert back to Address for AccountMeta
        let user_base_token_account = functions::to_address(&user_base_token_account_old);
        let user_quote_token_account = functions::to_address(&user_quote_token_account_old);

        // Determine fee recipient based on pool's protocol_fee_recipient
        let fee_recipient = if self.protocol_fee_recipient == constants::MAYHEM_FEE_RECIPIENT {
            constants::MAYHEM_FEE_RECIPIENT
        } else {
            constants::FEE_RECIPIENT
        };
        let fee_recipient_meta = if fee_recipient == constants::MAYHEM_FEE_RECIPIENT {
            constants::MAYHEM_FEE_RECIPIENT_META
        } else {
            constants::FEE_RECIPIENT_META
        };

        let fee_recipient_anchor = functions::to_pubkey(&fee_recipient);
        let fee_recipient_ata_anchor = spl_associated_token_account::get_associated_token_address(
            &fee_recipient_anchor,
            &quote_mint_anchor,
        );
        let fee_recipient_ata = functions::to_address(&fee_recipient_ata_anchor);

        // ========================================
        // Build instructions
        // ========================================
        let mut instructions = Vec::with_capacity(6);

        if is_buy {
            instructions.extend(sol_trade_sdk::trading::common::handle_wsol(
                &params.user_wallet,
                sol_amount,
            ));
        } else {
            instructions.extend(
                sol_trade_sdk::trading::common::wsol_manager::create_wsol_ata(&params.user_wallet),
            );
        }

        let spl_associated_token_account_program_id = solana_sdk::pubkey::Pubkey::new_from_array(
            spl_associated_token_account::id().to_bytes(),
        );

        // Create output token ATA instruction (idempotent - creates if doesn't exist)
        // Only if output is NOT WSOL
        if params.output_token.address != common::constants::WSOL_TOKEN_ACCOUNT {
            let user_output_token_account_anchor =
                spl_associated_token_account::get_associated_token_address_with_program_id(
                    &user_wallet_anchor,
                    &functions::to_pubkey(&params.output_token.address),
                    &functions::to_pubkey(&functions::get_token_program(
                        params.output_token.is_token_2022,
                    )),
                );
            let user_output_token_account =
                functions::to_address(&user_output_token_account_anchor);

            instructions.push(functions::create_ata_instruction(
                params.user_wallet,
                user_output_token_account,
                params.output_token.address,
                params.output_token.is_token_2022,
            ));
        }

        // Create buy instruction
        let mut accounts = Vec::with_capacity(23);
        accounts.extend([
            AccountMeta::new(pool, false),                         // pool_id
            AccountMeta::new(params.user_wallet, true),            // user (signer)
            constants::PUMPSWAP_GLOBAL_ACCOUNT_META,               // global (readonly)
            AccountMeta::new_readonly(base_mint, false),           // base_mint (readonly)
            AccountMeta::new_readonly(quote_mint, false),          // quote_mint (readonly)
            AccountMeta::new(user_base_token_account, false),      // user_base_token_account
            AccountMeta::new(user_quote_token_account, false),     // user_quote_token_account
            AccountMeta::new(pool_base_token_account, false),      // pool_base_token_account
            AccountMeta::new(pool_quote_token_account, false),     // pool_quote_token_account
            fee_recipient_meta,                                    // fee_recipient (readonly)
            AccountMeta::new(fee_recipient_ata, false),            // fee_recipient_ata
            AccountMeta::new_readonly(base_token_program, false),  // TOKEN_PROGRAM_ID (readonly)
            AccountMeta::new_readonly(quote_token_program, false), // TOKEN_PROGRAM_ID (readonly, duplicated as in JS)
            common::constants::SYSTEM_PROGRAM_META,                // System Program (readonly)
            AccountMeta::new(spl_associated_token_account_program_id, false), // ASSOCIATED_TOKEN_PROGRAM_ID (readonly)
            constants::PUMPSWAP_EVENT_AUTHORITY_META, // event_authority (readonly)
            constants::PUMPSWAP_AMM_PROGRAM_META,     // PUMP_AMM_PROGRAM_ID (readonly)
            AccountMeta::new(coin_creator_vault_ata, false), // coin_creator_vault_ata
            AccountMeta::new_readonly(coin_creator_vault_authority, false), // coin_creator_vault_authority (readonly)
        ]);
        // For BUY instructions, global_volume_accumulator (idx 19) and user_volume_accumulator (idx 20)
        // are ALWAYS present per the IDL. For SELL, they are NOT in the IDL accounts.
        if is_buy_instruction {
            accounts.push(constants::PUMPSWAP_GLOBAL_VOLUME_ACCUMULATOR_META);
            accounts.push(AccountMeta::new(
                get_user_volume_accumulator_pda(&params.user_wallet).unwrap(),
                false,
            ));
        }
        accounts.push(constants::PUMPSWAP_FEE_CONFIG_META);
        accounts.push(constants::PUMPSWAP_FEE_PROGRAM_META);

        // Cashback remaining accounts (per pump-swap-sdk)
        if self.is_cashback {
            let user_volume_acc = get_user_volume_accumulator_pda(&params.user_wallet)
                .ok_or("Failed to derive user_volume_accumulator PDA")?;
            let user_volume_acc_anchor = functions::to_pubkey(&user_volume_acc);
            let quote_token_program_anchor = functions::to_pubkey(&quote_token_program);

            if is_buy_instruction {
                // SDK buy: ATA(NATIVE_MINT, userVolumeAccumulatorPda(user), true, quoteTokenProgram)
                let native_mint_anchor =
                    functions::to_pubkey(&common::constants::WSOL_TOKEN_ACCOUNT);
                let spl_token_program_anchor =
                    functions::to_pubkey(&common::constants::TOKEN_PROGRAM);
                let user_vol_acc_ata = functions::to_address(
                    &spl_associated_token_account::get_associated_token_address_with_program_id(
                        &user_volume_acc_anchor,
                        &native_mint_anchor,
                        &spl_token_program_anchor,
                    ),
                );
                accounts.push(AccountMeta::new(user_vol_acc_ata, false));
            } else {
                // SDK sell: ATA(quoteMint, userVolumeAccumulatorPda(user), true, quoteTokenProgram)
                //         + userVolumeAccumulatorPda(user)
                let user_vol_acc_ata = functions::to_address(
                    &spl_associated_token_account::get_associated_token_address_with_program_id(
                        &user_volume_acc_anchor,
                        &quote_mint_anchor,
                        &quote_token_program_anchor,
                    ),
                );
                accounts.push(AccountMeta::new(user_vol_acc_ata, false));
                accounts.push(AccountMeta::new(user_volume_acc, false));
            }
        }

        // pool-v2 PDA (appended as last remaining account, per TS SDK pumpfunAmm.ts:161)
        let (pool_v2, _) = Pubkey::find_program_address(
            &[b"pool-v2", base_mint.as_ref()],
            &pumpf_functions::accounts::AMM_PROGRAM,
        );
        accounts.push(AccountMeta::new_readonly(pool_v2, false));

        // Create instruction data dynamically based on buy/sell sizes
        let mut data = Vec::with_capacity(26);

        if is_buy_instruction {
            data.extend_from_slice(&BUY_DISCRIMINATOR);
        } else {
            data.extend_from_slice(&SELL_DISCRIMINATOR);
        }

        // Populate amounts into the data slice (Always Exact In for our aggregator params)
        if is_buy_instruction {
            // base_amount_out
            data.extend_from_slice(&token_amount.to_le_bytes());
            // max_quote_amount_in
            data.extend_from_slice(&sol_amount.to_le_bytes());
        } else {
            // base_amount_in
            data.extend_from_slice(&token_amount.to_le_bytes());
            // min_quote_amount_out
            data.extend_from_slice(&sol_amount.to_le_bytes());
        }

        if is_buy_instruction {
            // track_volume parameter: OptionBool is a STRUCT { bool } in the IDL,
            // NOT Option<bool>. Borsh serializes it as a single byte.
            data.push(0x01); // value: true
        }

        let instruction = Instruction {
            program_id: accounts::AMM_PROGRAM,
            accounts: accounts.clone(),
            data,
        };

        instructions.push(instruction);

        instructions.extend(sol_trade_sdk::trading::common::close_wsol(
            &params.user_wallet,
        ));
        Ok(instructions)
    }
}
