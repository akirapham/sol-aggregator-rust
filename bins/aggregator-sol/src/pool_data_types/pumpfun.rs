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

const BUY_EXACT_QUOTE_IN_V2_DISCRIMINATOR: [u8; 8] = [194, 171, 28, 70, 104, 77, 91, 47];
const SELL_V2_DISCRIMINATOR: [u8; 8] = [93, 246, 130, 60, 231, 233, 64, 178];

fn default_quote_mint() -> Pubkey {
    common::constants::WSOL_TOKEN_ACCOUNT
}

fn associated_token_address(owner: &Pubkey, mint: &Pubkey, token_program: &Pubkey) -> Pubkey {
    let owner_anchor = functions::to_pubkey(owner);
    let mint_anchor = functions::to_pubkey(mint);
    let token_program_anchor = functions::to_pubkey(token_program);
    let ata = spl_associated_token_account::get_associated_token_address_with_program_id(
        &owner_anchor,
        &mint_anchor,
        &token_program_anchor,
    );
    functions::to_address(&ata)
}

fn create_ata_instruction_for_owner(
    funder: Pubkey,
    owner: Pubkey,
    token_account: Pubkey,
    mint: Pubkey,
    token_program: Pubkey,
) -> Instruction {
    Instruction {
        program_id: common::constants::ASSOCIATED_TOKEN_PROGRAM,
        accounts: vec![
            AccountMeta::new(funder, true),
            AccountMeta::new(token_account, false),
            AccountMeta::new_readonly(owner, false),
            AccountMeta::new_readonly(mint, false),
            common::constants::SYSTEM_PROGRAM_META,
            AccountMeta::new_readonly(token_program, false),
        ],
        data: vec![1],
    }
}

fn is_native_quote_mint(quote_mint: &Pubkey) -> bool {
    tokens_equal(quote_mint, &common::constants::WSOL_TOKEN_ACCOUNT)
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PumpfunPoolState {
    pub slot: u64,
    pub transaction_index: Option<u64>,
    pub address: Pubkey, // bonding curve address
    pub mint: Pubkey,
    #[serde(default = "default_quote_mint")]
    pub quote_mint: Pubkey,
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
    pub quote_mint: Pubkey,
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

        let quote_mint = if self.quote_mint == Pubkey::default() {
            default_quote_mint()
        } else {
            self.quote_mint
        };
        let is_buy = tokens_equal(input_token, &quote_mint);
        if !is_buy && self.mint != Pubkey::default() && !tokens_equal(input_token, &self.mint) {
            return 0;
        }
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
        let quote_mint = if self.quote_mint == Pubkey::default() {
            get_sol_mint()
        } else {
            self.quote_mint
        };
        let is_buy = tokens_equal(&params.input_token.address, &quote_mint);
        if !is_buy && !tokens_equal(&params.output_token.address, &quote_mint) {
            return Err(format!(
                "PumpFun swap must be between base mint and quote mint {}",
                quote_mint
            ));
        }

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
        let buyback_fee_recipient = constants::pick_buyback_fee_recipient();
        let fee_recipient = if is_mayhem_mode {
            constants::MAYHEM_FEE_RECIPIENT
        } else {
            constants::FEE_RECIPIENT
        };

        let fee_recipient_meta = if is_mayhem_mode {
            constants::MAYHEM_FEE_RECIPIENT_META
        } else {
            constants::FEE_RECIPIENT_META
        };

        let quote_token_program = common::constants::TOKEN_PROGRAM;
        let quote_token_program_meta = common::constants::TOKEN_PROGRAM_META;

        let base_mint = if is_buy {
            params.output_token.address
        } else {
            params.input_token.address
        };
        let bonding_curve_addr = if self.address == Pubkey::default() {
            pumpf_functions::get_bonding_curve_pda(&base_mint)
                .ok_or("Failed to get bonding curve PDA".to_string())?
        } else {
            self.address
        };

        let associated_base_bonding_curve =
            associated_token_address(&bonding_curve_addr, &base_mint, &token_program);
        let associated_quote_bonding_curve =
            associated_token_address(&bonding_curve_addr, &quote_mint, &quote_token_program);
        let associated_base_user =
            associated_token_address(&params.user_wallet, &base_mint, &token_program);
        let associated_quote_user =
            associated_token_address(&params.user_wallet, &quote_mint, &quote_token_program);
        let associated_quote_fee_recipient =
            associated_token_address(&fee_recipient, &quote_mint, &quote_token_program);
        let associated_quote_buyback_fee_recipient =
            associated_token_address(&buyback_fee_recipient, &quote_mint, &quote_token_program);
        let associated_creator_vault =
            associated_token_address(&creator_vault_pda, &quote_mint, &quote_token_program);

        let (sharing_config, _) = Pubkey::find_program_address(
            &[b"sharing-config", base_mint.as_ref()],
            &constants::PUMPFUN_FEE_PROGRAM,
        );
        let (user_volume_accumulator, _) = Pubkey::find_program_address(
            &[b"user_volume_accumulator", params.user_wallet.as_ref()],
            &pumpf_functions::accounts::PUMPFUN,
        );
        let (global_volume_accumulator, _) = Pubkey::find_program_address(
            &[b"global_volume_accumulator"],
            &pumpf_functions::accounts::PUMPFUN,
        );
        let associated_user_volume_accumulator =
            associated_token_address(&user_volume_accumulator, &quote_mint, &quote_token_program);

        let mut instructions = Vec::with_capacity(6);

        if is_buy {
            instructions.push(create_ata_instruction_for_owner(
                params.user_wallet,
                params.user_wallet,
                associated_base_user,
                base_mint,
                token_program,
            ));
        }

        if !is_native_quote_mint(&quote_mint) {
            instructions.push(create_ata_instruction_for_owner(
                params.user_wallet,
                params.user_wallet,
                associated_quote_user,
                quote_mint,
                quote_token_program,
            ));
            instructions.push(create_ata_instruction_for_owner(
                params.user_wallet,
                creator_vault_pda,
                associated_creator_vault,
                quote_mint,
                quote_token_program,
            ));
            instructions.push(create_ata_instruction_for_owner(
                params.user_wallet,
                bonding_curve_addr,
                associated_quote_bonding_curve,
                quote_mint,
                quote_token_program,
            ));
            if self.is_cashback {
                instructions.push(create_ata_instruction_for_owner(
                    params.user_wallet,
                    user_volume_accumulator,
                    associated_user_volume_accumulator,
                    quote_mint,
                    quote_token_program,
                ));
            }
        }

        if is_buy {
            // ========================================
            // BUY: Quote -> Token (Exact Quote In)
            // ========================================
            // 1. Calculate expected token output for the given input quote amount
            // Note: pumpf_functions::get_buy_token_amount_from_sol_amount calculates the virtual curve output.
            // Manual calc consistent with calculate_output_amount
            let x = self.virtual_token_reserves as u128;
            let y = self.virtual_sol_reserves as u128;
            let dy = params.input_amount as u128; // Input quote amount

            let fee = dy / 100;
            let dy_net = dy.saturating_sub(fee);

            let numerator = x.saturating_mul(dy_net);
            let denominator = y.saturating_add(dy_net);

            let expected_token_amount = (numerator.checked_div(denominator).unwrap_or(0)) as u64;

            // 2. Calculate min_tokens_out (Apply slippage)
            let min_tokens_out =
                functions::calculate_slippage(expected_token_amount, params.slippage_bps)?;

            // Build instruction data for buy_exact_quote_in_v2
            // Args: spendable_quote_in (u64), min_tokens_out (u64)
            let mut buy_data = Vec::with_capacity(24);
            buy_data.extend_from_slice(&BUY_EXACT_QUOTE_IN_V2_DISCRIMINATOR);
            buy_data.extend_from_slice(&params.input_amount.to_le_bytes()); // spendable_quote_in
            buy_data.extend_from_slice(&min_tokens_out.to_le_bytes()); // min_tokens_out

            let buy_accounts: Vec<AccountMeta> = vec![
                constants::PUMPFUN_GLOBAL_ACCOUNT_META,
                AccountMeta::new_readonly(base_mint, false),
                AccountMeta::new_readonly(quote_mint, false),
                token_program_meta,
                quote_token_program_meta,
                common::constants::ASSOCIATED_TOKEN_PROGRAM_META,
                fee_recipient_meta,
                AccountMeta::new(associated_quote_fee_recipient, false),
                constants::buyback_fee_recipient_meta(buyback_fee_recipient),
                AccountMeta::new(associated_quote_buyback_fee_recipient, false),
                AccountMeta::new(bonding_curve_addr, false),
                AccountMeta::new(associated_base_bonding_curve, false),
                AccountMeta::new(associated_quote_bonding_curve, false),
                AccountMeta::new(params.user_wallet, true),
                AccountMeta::new(associated_base_user, false),
                AccountMeta::new(associated_quote_user, false),
                AccountMeta::new(creator_vault_pda, false),
                AccountMeta::new(associated_creator_vault, false),
                AccountMeta::new_readonly(sharing_config, false),
                AccountMeta::new_readonly(global_volume_accumulator, false),
                AccountMeta::new(user_volume_accumulator, false),
                AccountMeta::new(associated_user_volume_accumulator, false),
                constants::PUMPFUN_FEE_CONFIG_META,
                constants::PUMPFUN_FEE_PROGRAM_META,
                common::constants::SYSTEM_PROGRAM_META,
                constants::PUMPFUN_EVENT_AUTHORITY_META,
                constants::PUMPFUN_META,
            ];

            instructions.push(Instruction::new_with_bytes(
                Self::get_program_id(),
                &buy_data,
                buy_accounts,
            ));
            Ok(instructions)
        } else {
            // ========================================
            // SELL: Token -> Quote
            // ========================================
            // Calculate expected quote output
            let quote_amount = get_sell_sol_amount_from_token_amount(
                self.virtual_token_reserves as u128,
                self.virtual_sol_reserves as u128,
                self.creator,
                params.input_amount,
            );
            let min_quote_output =
                functions::calculate_slippage(quote_amount, params.slippage_bps)?;

            // Build instruction data (8 byte discriminator + 8 byte amount + 8 byte min output)
            let mut sell_data = [0u8; 24];
            sell_data[..8].copy_from_slice(&SELL_V2_DISCRIMINATOR);
            sell_data[8..16].copy_from_slice(&params.input_amount.to_le_bytes());
            sell_data[16..24].copy_from_slice(&min_quote_output.to_le_bytes());

            let sell_accounts: Vec<AccountMeta> = vec![
                constants::PUMPFUN_GLOBAL_ACCOUNT_META,
                AccountMeta::new_readonly(base_mint, false),
                AccountMeta::new_readonly(quote_mint, false),
                token_program_meta,
                quote_token_program_meta,
                common::constants::ASSOCIATED_TOKEN_PROGRAM_META,
                fee_recipient_meta,
                AccountMeta::new(associated_quote_fee_recipient, false),
                constants::buyback_fee_recipient_meta(buyback_fee_recipient),
                AccountMeta::new(associated_quote_buyback_fee_recipient, false),
                AccountMeta::new(bonding_curve_addr, false),
                AccountMeta::new(associated_base_bonding_curve, false),
                AccountMeta::new(associated_quote_bonding_curve, false),
                AccountMeta::new(params.user_wallet, true),
                AccountMeta::new(associated_base_user, false),
                AccountMeta::new(associated_quote_user, false),
                AccountMeta::new(creator_vault_pda, false),
                AccountMeta::new(associated_creator_vault, false),
                AccountMeta::new_readonly(sharing_config, false),
                AccountMeta::new(user_volume_accumulator, false),
                AccountMeta::new(associated_user_volume_accumulator, false),
                constants::PUMPFUN_FEE_CONFIG_META,
                constants::PUMPFUN_FEE_PROGRAM_META,
                common::constants::SYSTEM_PROGRAM_META,
                constants::PUMPFUN_EVENT_AUTHORITY_META,
                constants::PUMPFUN_META,
            ];

            instructions.push(Instruction::new_with_bytes(
                Self::get_program_id(),
                &sell_data,
                sell_accounts,
            ));

            Ok(instructions)
        }
    }
}
