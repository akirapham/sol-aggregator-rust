use crate::pool_data_types::traits::BuildSwapInstruction;
use crate::types::SwapParams;
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
    pub complete: bool,
    pub creator: Pubkey,
    pub is_mayhem_mode: bool,
    #[serde(default)]
    pub is_cashback: bool,
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
    pub complete: bool,
    pub creator: Pubkey,
    pub is_mayhem_mode: bool,
    pub last_updated: u64,
    pub is_account_state_update: bool,
    pub pool_update_event_type: PoolUpdateEventType,
    pub additional_event_type: i32, // for tick array index tracking, 0 for others
    pub is_cashback: Option<bool>,
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
        _: &dyn GetAmmConfig,
    ) -> u64 {
        if self.complete {
            // complete
            return 0;
        }

        let is_buy = tokens_equal(input_token, &get_sol_mint());
        if is_buy {
            let x = self.virtual_token_reserves as u128;
            let y = self.virtual_sol_reserves as u128;
            let dy = input_amount as u128;

            if x == 0 || y == 0 {
                return 0;
            }

            // 1% Fee on Input SOL
            let fee = dy / 100;
            let dy_net = dy.saturating_sub(fee);

            // xy = k
            // (y + dy_net) * (x - dx) = xy
            // x - dx = xy / (y + dy_net)
            // dx = x - (xy / (y + dy_net))
            // dx = (x * (y + dy_net) - xy) / (y + dy_net)
            // dx = (xy + x*dy_net - xy) / (y + dy_net)
            // dx = (x * dy_net) / (y + dy_net)

            let numerator = x.saturating_mul(dy_net);
            let denominator = y.saturating_add(dy_net);

            let amount_out = numerator.checked_div(denominator).unwrap_or(0);
            amount_out as u64
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
        let token_price = (self.real_sol_reserves as f64 / self.real_token_reserves as f64)
            * decimal_scale
            * sol_price;

        (token_price, sol_price)
    }
}

use solana_client::nonblocking::rpc_client::RpcClient;
use std::sync::Arc;

#[async_trait]
impl BuildSwapInstruction for PumpfunPoolState {
    /// Build PumpFun swap instruction
    async fn build_swap_instruction(
        &self,
        params: &SwapParams,
        amm_config_fetcher: &dyn GetAmmConfig,
        rpc_client: Option<&Arc<RpcClient>>,
    ) -> std::result::Result<Vec<Instruction>, String> {
        let _ = amm_config_fetcher;
        let _ = rpc_client;
        // Determine if this is a buy (SOL -> Token) or sell (Token -> SOL)
        let is_buy = tokens_equal(&params.input_token.address, &get_sol_mint());

        let creator_vault_pda =
            sol_trade_sdk::instruction::utils::pumpfun::get_creator_vault_pda(&self.creator)
                .ok_or("Failed to derive creator vault")?;

        let is_mayhem_mode = self.is_mayhem_mode;

        // Determine token program based on the token itself (Token2022 support)
        let is_token_2022 = if is_buy {
            params.output_token.is_token_2022
        } else {
            params.input_token.is_token_2022
        };

        // Use Token2022 program if token is 2022, otherwise fallback to standard or mayhem logic
        let token_program = if is_token_2022 {
            Pubkey::new_from_array(spl_token_2022::ID.to_bytes())
        } else {
            functions::get_token_program(is_mayhem_mode)
        };

        let token_program_meta = AccountMeta::new_readonly(token_program, false);

        let fee_recipient_meta = if is_mayhem_mode {
            constants::MAYHEM_FEE_RECIPIENT_META
        } else {
            constants::FEE_RECIPIENT_META
        };
        if is_buy {
            // ========================================
            // BUY: SOL -> Token (Exact SOL In)
            // ========================================
            // 1. Calculate expected token output for the given input SOL
            // Note: pumpf_functions::get_buy_token_amount_from_sol_amount calculates the virtual curve output.
            // Manual calc consistent with calculate_output_amount
            let x = self.virtual_token_reserves as u128;
            let y = self.virtual_sol_reserves as u128;
            let dy = params.input_amount as u128; // Input SOL

            let fee = dy / 100;
            let dy_net = dy.saturating_sub(fee);

            let numerator = x.saturating_mul(dy_net);
            let denominator = y.saturating_add(dy_net);

            let expected_token_amount = (numerator.checked_div(denominator).unwrap_or(0)) as u64;

            // 2. Calculate min_tokens_out (Apply slippage)
            let min_tokens_out =
                functions::calculate_slippage(expected_token_amount, params.slippage_bps)?;

            let bonding_curve_addr = if self.address == Pubkey::default() {
                pumpf_functions::get_bonding_curve_pda(&params.output_token.address)
                    .ok_or("Failed to get bonding curve PDA".to_string())?
            } else {
                self.address
            };

            // Convert Address types to anchor_lang Pubkey for compatibility
            let user_wallet_anchor = functions::to_pubkey(&params.user_wallet);
            let output_mint_anchor = functions::to_pubkey(&params.output_token.address);
            let bonding_curve_anchor = functions::to_pubkey(&bonding_curve_addr);
            // Use the token_program we determined earlier
            let token_program_anchor = functions::to_pubkey(&token_program);

            // Get associated token accounts
            let associated_bonding_curve_anchor =
                spl_associated_token_account::get_associated_token_address_with_program_id(
                    &bonding_curve_anchor,
                    &output_mint_anchor,
                    &token_program_anchor,
                );
            let user_token_account_anchor =
                spl_associated_token_account::get_associated_token_address_with_program_id(
                    &user_wallet_anchor,
                    &output_mint_anchor,
                    &token_program_anchor,
                );

            // Convert back to Address for AccountMeta
            let associated_bonding_curve = functions::to_address(&associated_bonding_curve_anchor);
            let user_token_account = functions::to_address(&user_token_account_anchor);

            let mut instructions = Vec::with_capacity(2);

            // Create ATA using common function
            instructions.push(functions::create_ata_instruction(
                params.user_wallet,
                user_token_account,
                params.output_token.address,
                is_token_2022,
            ));

            // Derive user_volume_accumulator from PumpFun Program ID
            let (user_volume_accumulator, _) = Pubkey::find_program_address(
                &[b"user_volume_accumulator", params.user_wallet.as_ref()],
                &pumpf_functions::accounts::PUMPFUN,
            );

            // Derive global_volume_accumulator from PumFun Program ID
            let (global_volume_accumulator, _) = Pubkey::find_program_address(
                &[b"global_volume_accumulator"],
                &pumpf_functions::accounts::PUMPFUN,
            );

            // Build instruction data for buy_exact_sol_in
            // Discriminator: [56, 252, 116, 8, 158, 223, 205, 95]
            // Args: spendable_sol_in (u64), min_tokens_out (u64), track_volume (OptionBool)
            let mut buy_data = Vec::with_capacity(25);
            buy_data.extend_from_slice(&[56, 252, 116, 8, 158, 223, 205, 95]);
            buy_data.extend_from_slice(&params.input_amount.to_le_bytes()); // spendable_sol_in
            buy_data.extend_from_slice(&min_tokens_out.to_le_bytes()); // min_tokens_out
            buy_data.push(0); // track_volume: OptionBool { val: false } (encoded as 0u8)

            // Build accounts array
            // Account Structure for buy_exact_sol_in (Same as buy):
            // 0: Global
            // 1: Fee Recipient
            // 2: Mint
            // 3: Bonding Curve
            // 4: Associated Bonding Curve
            // 5: Associated User
            // 6: User
            // 7: System Program
            // 8: Token Program
            // 9: Creator Vault
            // 10: Event Authority
            // 11: Program
            // 12: Global Volume Accumulator
            // 13: User Volume Accumulator
            // 14: Fee Config
            // 15: Fee Program
            let (bonding_curve_v2, _) = Pubkey::find_program_address(
                &[b"bonding-curve-v2", params.output_token.address.as_ref()],
                &pumpf_functions::accounts::PUMPFUN,
            );

            let mut buy_accounts: Vec<AccountMeta> = vec![
                constants::PUMPFUN_GLOBAL_ACCOUNT_META,
                fee_recipient_meta,
                AccountMeta::new_readonly(params.output_token.address, false),
                AccountMeta::new(bonding_curve_addr, false),
                AccountMeta::new(associated_bonding_curve, false),
                AccountMeta::new(user_token_account, false),
                AccountMeta::new(params.user_wallet, true),
                common::constants::SYSTEM_PROGRAM_META,
                token_program_meta,
                AccountMeta::new(creator_vault_pda, false),
                constants::PUMPFUN_EVENT_AUTHORITY_META,
                constants::PUMPFUN_META,
                AccountMeta::new(global_volume_accumulator, false),
                AccountMeta::new(user_volume_accumulator, false),
                constants::PUMPFUN_FEE_CONFIG_META,
                constants::PUMPFUN_FEE_PROGRAM_META,
            ];
            buy_accounts.push(AccountMeta::new_readonly(bonding_curve_v2, false));

            instructions.push(Instruction::new_with_bytes(
                Self::get_program_id(),
                &buy_data,
                buy_accounts,
            ));
            Ok(instructions)
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
            let min_sol_output = functions::calculate_slippage(sol_amount, params.slippage_bps)?;
            // Get bonding curve PDA
            let bonding_curve_addr = if self.address == Pubkey::default() {
                pumpf_functions::get_bonding_curve_pda(&params.input_token.address)
                    .ok_or("Failed to get bonding curve PDA".to_string())?
            } else {
                self.address
            };

            // Convert Address types to anchor_lang Pubkey for compatibility
            let user_wallet_anchor = functions::to_pubkey(&params.user_wallet);
            let input_mint_anchor = functions::to_pubkey(&params.input_token.address);
            let bonding_curve_anchor = functions::to_pubkey(&bonding_curve_addr);
            // Use the token_program we determined earlier
            let token_program_anchor = functions::to_pubkey(&token_program);

            // Get associated token accounts
            let associated_bonding_curve_anchor =
                spl_associated_token_account::get_associated_token_address_with_program_id(
                    &bonding_curve_anchor,
                    &input_mint_anchor,
                    &token_program_anchor,
                );
            let user_token_account_anchor =
                spl_associated_token_account::get_associated_token_address_with_program_id(
                    &user_wallet_anchor,
                    &input_mint_anchor,
                    &token_program_anchor,
                );

            // Convert back to Address for AccountMeta
            let associated_bonding_curve = functions::to_address(&associated_bonding_curve_anchor);
            let user_token_account = functions::to_address(&user_token_account_anchor);

            // ========================================
            // Build instructions
            // ========================================
            let mut instructions = Vec::with_capacity(2);

            // Create ATA using common function
            instructions.push(functions::create_ata_instruction(
                params.user_wallet,
                user_token_account,
                params.input_token.address,
                is_token_2022,
            ));
            // Derive user_volume_accumulator from PumpFun Program ID
            let (_user_volume_accumulator, _) = Pubkey::find_program_address(
                &[b"user_volume_accumulator", params.user_wallet.as_ref()],
                &pumpf_functions::accounts::PUMPFUN,
            );

            // Derive global_volume_accumulator from PumpFun Program ID
            let (_global_volume_accumulator, _) = Pubkey::find_program_address(
                &[b"global_volume_accumulator"],
                &pumpf_functions::accounts::PUMPFUN,
            );

            // Build instruction data (8 byte discriminator + 8 byte amount + 8 byte min output)
            let mut sell_data = [0u8; 24];
            sell_data[..8].copy_from_slice(&[51, 230, 133, 164, 1, 127, 131, 173]); // Sell method ID
            sell_data[8..16].copy_from_slice(&params.input_amount.to_le_bytes());
            sell_data[16..24].copy_from_slice(&min_sol_output.to_le_bytes());

            // Account Structure for Standard Sell (Post-Migration):
            // 0..7: Standard
            // 8: Associated Token Program
            // 9: Token Program
            // 10: Creator Vault
            // ...
            let (bonding_curve_v2, _) = Pubkey::find_program_address(
                &[b"bonding-curve-v2", params.input_token.address.as_ref()],
                &pumpf_functions::accounts::PUMPFUN,
            );

            let mut sell_accounts: Vec<AccountMeta> = vec![
                constants::PUMPFUN_GLOBAL_ACCOUNT_META,
                fee_recipient_meta,
                AccountMeta::new_readonly(params.input_token.address, false),
                AccountMeta::new(bonding_curve_addr, false),
                AccountMeta::new(associated_bonding_curve, false),
                AccountMeta::new(user_token_account, false),
                AccountMeta::new(params.user_wallet, true),
                common::constants::SYSTEM_PROGRAM_META,
                AccountMeta::new(creator_vault_pda, false),
                token_program_meta,
                constants::PUMPFUN_EVENT_AUTHORITY_META,
                constants::PUMPFUN_META,
                constants::PUMPFUN_FEE_CONFIG_META,
                constants::PUMPFUN_FEE_PROGRAM_META,
            ];
            if self.is_cashback {
                let (user_volume_accumulator, _) = Pubkey::find_program_address(
                    &[b"user_volume_accumulator", params.user_wallet.as_ref()],
                    &pumpf_functions::accounts::PUMPFUN,
                );
                sell_accounts.push(AccountMeta::new(user_volume_accumulator, false));
            }
            sell_accounts.push(AccountMeta::new_readonly(bonding_curve_v2, false));

            instructions.push(Instruction::new_with_bytes(
                Self::get_program_id(),
                &sell_data,
                sell_accounts,
            ));

            Ok(instructions)
        }
    }
}
