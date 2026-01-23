use serde::{Deserialize, Serialize};
use sol_trade_sdk::utils::calc::bonk::{
    get_buy_token_amount_from_sol_amount, get_sell_sol_amount_from_token_amount,
};
use solana_sdk::pubkey::Pubkey;
use solana_streamer_sdk::streaming::event_parser::protocols::bonk::parser::BONK_PROGRAM_ID;

use crate::{
    pool_data_types::{GetAmmConfig, PoolUpdateEventType},
    utils::tokens_equal,
};
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BonkPoolState {
    pub slot: u64,
    pub transaction_index: Option<u64>,
    pub address: Pubkey,
    pub status: u8,
    pub total_base_sell: u64,
    pub base_reserve: u64,  // virtual_base
    pub quote_reserve: u64, // virtual_quote
    pub liquidity_usd: f64, // base liquidity, one side
    pub real_base: u64,
    pub real_quote: u64,
    pub quote_protocol_fee: u64,
    pub platform_fee: u64,
    pub global_config: Pubkey,
    pub platform_config: Pubkey,
    pub platform_fee_wallet: Pubkey, // Cached from platform_config
    pub base_mint: Pubkey,
    pub quote_mint: Pubkey,
    pub base_vault: Pubkey,
    pub quote_vault: Pubkey,
    pub creator: Pubkey,
    pub last_updated: u64,
    pub is_state_keys_initialized: bool,
}

#[derive(Debug, Clone)]
pub struct BonkPoolUpdate {
    pub slot: u64,
    pub transaction_index: Option<u64>,
    pub address: Pubkey,
    pub status: u8,
    pub total_base_sell: u64,
    pub base_reserve: u64,  // virtual_base
    pub quote_reserve: u64, // virtual_quote
    pub real_base: u64,
    pub real_quote: u64,
    pub quote_protocol_fee: u64,
    pub platform_fee: u64,
    pub global_config: Pubkey,
    pub platform_config: Pubkey,
    pub base_mint: Pubkey,
    pub quote_mint: Pubkey,
    pub base_vault: Pubkey,
    pub quote_vault: Pubkey,
    pub creator: Pubkey,
    pub last_updated: u64,
    pub is_account_state_update: bool,
    pub pool_update_event_type: PoolUpdateEventType,
    pub additional_event_type: i32,
}

#[allow(dead_code)]
impl BonkPoolState {
    pub fn get_program_id() -> Pubkey {
        Pubkey::new_from_array(*BONK_PROGRAM_ID.as_array())
    }

    /// Calculate output amount for PumpFun bonding curve
    pub fn calculate_output_amount(
        &self,
        input_token: &Pubkey,
        input_amount: u64,
        _: &dyn GetAmmConfig,
    ) -> u64 {
        if self.status != 0 {
            // funding ended
            return 0;
        }
        let is_buy = tokens_equal(input_token, &self.quote_mint);

        let result = if is_buy {
            get_buy_token_amount_from_sol_amount(
                input_amount,
                self.base_reserve as u128,
                self.quote_reserve as u128,
                self.real_base as u128,
                self.real_quote as u128,
                0,
            )
        } else {
            // Manual calculation for Sell (Base -> Quote) because SDK function returns incorrect values (15x off).
            // On-Chain behavior matches Simple CPMM (xy=k) with ~1% fee.
            // Formula: out = (quote_reserve * input_amount) / (base_reserve + input_amount)
            let x = self.base_reserve as u128;
            let y = self.quote_reserve as u128;
            let dx = input_amount as u128;

            if x == 0 || y == 0 {
                return 0;
            }

            let numerator = y.checked_mul(dx).unwrap_or(0);
            let denominator = x.checked_add(dx).unwrap_or(u128::MAX);

            let amount_out_gross = numerator.checked_div(denominator).unwrap_or(0);

            // Deduct 1% Fee (Standard Bonk/PumpFun fee)
            let fee = amount_out_gross / 100;
            let amount_out_net = amount_out_gross.saturating_sub(fee);

            amount_out_net as u64
        };

        result
    }

    pub fn calculate_token_prices(
        &self,
        _sol_price: f64,
        _base_decimals: u8,
        _quote_decimals: u8,
    ) -> (f64, f64) {
        (0.0, 0.0) // Bonk does not provide reliable price info
    }
}

use crate::pool_data_types::common::functions;
use crate::pool_data_types::traits::BuildSwapInstruction;
use crate::types::SwapParams;
use async_trait::async_trait;
use solana_sdk::instruction::{AccountMeta, Instruction};
// use crate::utils::tokens_equal; // Already imported at line 10
// use solana_sdk::pubkey::Pubkey; // Already imported at line 5

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BonkSwapArgs {
    pub discriminator: [u8; 8],
    pub amount_in: u64,
    pub min_amount_out: u64,
}

#[async_trait]
impl BuildSwapInstruction for BonkPoolState {
    async fn build_swap_instruction(
        &self,
        params: &SwapParams,
        amm_config_fetcher: &dyn GetAmmConfig,
        rpc_client: Option<&std::sync::Arc<solana_client::nonblocking::rpc_client::RpcClient>>,
    ) -> std::result::Result<Vec<Instruction>, String> {
        let _ = amm_config_fetcher;
        let _ = rpc_client;

        // Constants from reference
        let raydium_launchpad_authority =
            solana_sdk::pubkey!("WLHv2UAZm6z4KyaaELi5pjdbJh6RESMva1Rnn8pJVVh");
        let event_authority = solana_sdk::pubkey!("2DPAtwB8L12vrMRExbLuyGnC7n2J5LNoZQSejeQGpwkr");
        let raydium_launchpad_program = Self::get_program_id();

        // Discriminators
        let buy_discriminator: [u8; 8] = [250, 234, 13, 123, 213, 156, 19, 236];
        let sell_discriminator: [u8; 8] = [149, 39, 222, 155, 211, 124, 152, 26];

        // Determine if Buy or Sell
        // Input == Quote Mint (e.g. SOL) => Buy
        let is_buy = tokens_equal(&params.input_token.address, &self.quote_mint);

        // 1. Calculate Estimated Output & Slippage
        let expected_output = self.calculate_output_amount(
            &params.input_token.address,
            params.input_amount,
            amm_config_fetcher,
        );
        let min_amount_out = functions::calculate_slippage(expected_output, params.slippage_bps)?;

        // 2. Build Instruction Data
        // Layout: Discriminator (8) + Amount In (8) + Min Amount Out (8) + Share Fee Rate (8)
        let mut data = Vec::with_capacity(32);
        if is_buy {
            data.extend_from_slice(&buy_discriminator);
        } else {
            data.extend_from_slice(&sell_discriminator);
        }
        data.extend_from_slice(&params.input_amount.to_le_bytes());
        data.extend_from_slice(&min_amount_out.to_le_bytes());
        // Share fee rate = 0 as per reference
        data.extend_from_slice(&0u64.to_le_bytes());

        // 3. Resolve Accounts
        let user = params.user_wallet;

        let mut instructions = Vec::new();

        // Determine input/output tokens and create ATAs
        // If Buy: Input = Quote (SOL/USDC), Output = Base (Token)
        // If Sell: Input = Base (Token), Output = Quote (SOL/USDC)

        let (input_mint, output_mint, is_input_token2022, is_output_token2022) = if is_buy {
            (
                self.quote_mint,
                self.base_mint,
                false,
                params.output_token.is_token_2022,
            )
        } else {
            (
                self.base_mint,
                self.quote_mint,
                params.input_token.is_token_2022,
                false,
            )
        };

        // Resolve ATAs
        // We use spl_associated_token_account to derive addresses.
        // We need to know which Token Program to use for derivation.
        // Typically: Base uses its program (Token or Token2022), Quote (SOL) uses Token Program.

        // User ATAs
        // spl_associated_token_account here expects anchor_lang::prelude::Pubkey (based on compiler errors and project pattern)
        let user_anchor = functions::to_pubkey(&user);
        let input_mint_anchor = functions::to_pubkey(&input_mint);

        let output_mint_anchor = functions::to_pubkey(&output_mint);

        // Helper for Program ID (Anchor Type for spl_associated_token_account)
        let get_program_id_anchor = |is_2022: bool| {
            let id = if is_2022 {
                spl_token_2022::ID
            } else {
                spl_token::ID
            };
            anchor_lang::prelude::Pubkey::new_from_array(id.to_bytes())
        };

        let user_input_ata_anchor =
            spl_associated_token_account::get_associated_token_address_with_program_id(
                &user_anchor,
                &input_mint_anchor,
                &get_program_id_anchor(is_input_token2022),
            );
        let user_input_ata = functions::to_address(&user_input_ata_anchor);

        let user_output_ata_anchor =
            spl_associated_token_account::get_associated_token_address_with_program_id(
                &user_anchor,
                &output_mint_anchor,
                &get_program_id_anchor(is_output_token2022),
            );
        let user_output_ata = functions::to_address(&user_output_ata_anchor);

        // Create Input ATA if needed (usually handled by caller or pre-steps, but aggregator standard is to add create ix if output missing)
        // Aggregator `create_ata_instruction` handles output ATA creation.
        instructions.push(functions::create_ata_instruction(
            user,
            user_output_ata,
            output_mint,
            is_output_token2022,
        ));

        // Prepare Account Metas matching reference order:
        // 1. User (Signer, Writable)
        // 2. Authority (Readonly)
        // 3. Global Config (Readonly)
        // 4. Platform Config (Readonly)
        // 5. Pool ID/State (Writable)
        // 6. User Base ATA (Writable)
        // 7. User Quote ATA (Writable)
        // 8. Pool Base ATA/Vault (Writable)
        // 9. Pool Quote ATA/Vault (Writable)
        // 10. Base Mint (Readonly)
        // 11. Quote Mint (Readonly)
        // 12. Base Token Program (Readonly)
        // 13. Quote Token Program (Readonly)
        // 14. Event Authority (Readonly)
        // 15. Program (Readonly)

        // Identify which user ATA is Base and which is Quote
        let (user_base_ata, user_quote_ata) = if is_buy {
            // Buy: Input=Quote, Output=Base
            (user_output_ata, user_input_ata)
        } else {
            // Sell: Input=Base, Output=Quote
            (user_input_ata, user_output_ata)
        };

        // Helper to get SDK Pubkey for Token Program
        let get_token_program_sdk = |is_2022: bool| {
            let id = if is_2022 {
                spl_token_2022::ID
            } else {
                spl_token::ID
            };
            solana_sdk::pubkey::Pubkey::new_from_array(id.to_bytes())
        };

        // Token Programs
        // Base Token Program
        let base_token_program = if params.output_token.address == self.base_mint {
            // Buy: Params Output is Base
            get_token_program_sdk(params.output_token.is_token_2022)
        } else {
            // Sell: Params Input is Base
            get_token_program_sdk(params.input_token.is_token_2022)
        };

        // Quote Token Program (Usually Legacy for SOL/USDC)
        let quote_token_program =
            solana_sdk::pubkey::Pubkey::new_from_array(spl_token::ID.to_bytes());

        let mut accounts = vec![
            AccountMeta::new(user, true),
            AccountMeta::new_readonly(raydium_launchpad_authority, false),
            AccountMeta::new_readonly(self.global_config, false),
            AccountMeta::new_readonly(self.platform_config, false),
            AccountMeta::new(self.address, false),
            AccountMeta::new(user_base_ata, false),
            AccountMeta::new(user_quote_ata, false),
            AccountMeta::new(self.base_vault, false),
            AccountMeta::new(self.quote_vault, false),
            AccountMeta::new_readonly(self.base_mint, false),
            AccountMeta::new_readonly(self.quote_mint, false),
            AccountMeta::new_readonly(base_token_program, false),
            AccountMeta::new_readonly(quote_token_program, false),
            AccountMeta::new_readonly(event_authority, false),
            AccountMeta::new_readonly(raydium_launchpad_program, false),
            AccountMeta::new_readonly(
                crate::pool_data_types::common::constants::SYSTEM_PROGRAM,
                false,
            ),
        ];

        // CALCULATE FEE ACCOUNTS using PDA derivation (as per reference)
        // Reference: getPdaCreatorVault(RAYDIUM_LAUNCHLAB_MAINNET_ADDR, target, quote_mint)
        // Seeds: [target, quote_mint]
        let derive_fee_vault = |target: &Pubkey,
                                quote_mint: &Pubkey,
                                program_id: &Pubkey|
         -> Pubkey {
            let (pda, _) =
                Pubkey::find_program_address(&[target.as_ref(), quote_mint.as_ref()], program_id);
            pda
        };

        let platform_fee_ata = derive_fee_vault(
            &self.platform_config,
            &self.quote_mint,
            &raydium_launchpad_program,
        );
        let creator_fee_ata =
            derive_fee_vault(&self.creator, &self.quote_mint, &raydium_launchpad_program);

        // Append Fee Accounts as Writable (since they receive fees)
        accounts.push(AccountMeta::new(platform_fee_ata, false));
        accounts.push(AccountMeta::new(creator_fee_ata, false));

        instructions.push(Instruction {
            program_id: raydium_launchpad_program,
            accounts,
            data,
        });

        Ok(instructions)
    }
}

// Helper functions for type conversion
