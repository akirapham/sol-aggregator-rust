use std::{collections::HashMap, sync::Arc};

use crate::{
    constants::is_base_token,
    pool_data_types::{
        clmm::{pool::PoolUtils, tpe::ComputeClmmPoolInfo},
        GetAmmConfig, PoolUpdateEventType,
        common,
    },
    utils::tokens_equal,
};

use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use solana_streamer_sdk::streaming::event_parser::protocols::raydium_clmm::parser::RAYDIUM_CLMM_PROGRAM_ID;

use crate::types::SwapParams;
use crate::pool_data_types::traits::BuildSwapInstruction;
use async_trait::async_trait;
use solana_program::instruction::{AccountMeta, Instruction};
use spl_associated_token_account;
use solana_compute_budget_interface::ComputeBudgetInstruction;

const MIN_SQRT_PRICE_X64: u128 = 4295048016;
const MAX_SQRT_PRICE_X64: u128 = 79226673521066979257578248091;

#[derive(BorshSerialize)]
struct SwapV2Args {
    amount: u64,
    other_amount_threshold: u64,
    sqrt_price_limit_x64: u128,
    is_base_input: bool,
}

#[derive(Clone, Debug, Copy, Default)]
#[allow(dead_code)]
pub struct TickState {
    pub tick: i32,
    pub liquidity_net: i128,
    pub liquidity_gross: u128,
}

#[allow(unused)]
#[derive(Clone, Debug)]
pub struct TickArrayState {
    pub start_tick_index: i32,
    pub ticks: [TickState; 60],
    pub initialized_tick_count: u8,
}

impl Default for TickArrayState {
    fn default() -> Self {
        Self {
            start_tick_index: 0,
            ticks: [TickState::default(); 60],
            initialized_tick_count: 0,
        }
    }
}

const EXTENSION_TICKARRAY_BITMAP_SIZE: usize = 14;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TickArrayBitmapExtension {
    /// Packed initialized tick array state for start_tick_index is positive
    pub positive_tick_array_bitmap: [[u64; 8]; EXTENSION_TICKARRAY_BITMAP_SIZE],
    /// Packed initialized tick array state for start_tick_index is negitive
    pub negative_tick_array_bitmap: [[u64; 8]; EXTENSION_TICKARRAY_BITMAP_SIZE],
}

impl Default for TickArrayBitmapExtension {
    fn default() -> Self {
        Self {
            positive_tick_array_bitmap: core::array::from_fn(|_| [0u64; 8]),
            negative_tick_array_bitmap: core::array::from_fn(|_| [0u64; 8]),
        }
    }
}

#[derive(Clone, Debug)]
pub struct RaydiumClmmPoolStatePart {
    pub amm_config: Pubkey,
    pub token_mint0: Pubkey,
    pub token_mint1: Pubkey,
    pub token_vault0: Pubkey,
    pub token_vault1: Pubkey,
    pub observation_key: Pubkey,
    pub tick_spacing: u16,
    pub liquidity: u128,
    pub sqrt_price_x64: u128,
    pub tick_current_index: i32,
    pub status: u8,
    pub tick_array_bitmap: [u64; 16],
    pub open_time: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, BorshDeserialize)]
pub struct RaydiumClmmAmmConfig {
    pub bump: u8,
    pub index: u16,
    pub owner: Pubkey,
    pub protocol_fee_rate: u32,
    pub trade_fee_rate: u32,
    pub tick_spacing: u16,
    pub fund_fee_rate: u32,
    pub padding_u32: u32,
    pub fund_owner: Pubkey,
    pub padding: [u64; 3],
}

#[derive(Clone, Debug)]
pub struct RaydiumClmmPoolReservePart {
    pub token0_reserve: u64,
    pub token1_reserve: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaydiumClmmPoolState {
    pub slot: u64,
    pub transaction_index: Option<u64>,
    pub address: Pubkey,
    pub amm_config: Pubkey,
    pub token_mint0: Pubkey,
    pub token_mint1: Pubkey,
    pub token_vault0: Pubkey,
    pub token_vault1: Pubkey,
    pub observation_key: Pubkey,
    pub tick_spacing: u16,
    pub liquidity: u128,
    pub liquidity_usd: f64,
    pub sqrt_price_x64: u128,
    pub tick_current_index: i32,
    pub status: u8,
    pub tick_array_bitmap: [u64; 16],
    pub open_time: u64,
    #[serde(skip)]
    pub tick_array_state: HashMap<i32, TickArrayState>,
    pub tick_array_bitmap_extension: Option<TickArrayBitmapExtension>,
    pub last_updated: u64, // Unix timestamp
    pub token0_reserve: u64,
    pub token1_reserve: u64,
    pub is_state_keys_initialized: bool,
    pub is_token_mint0_2022: bool, // Whether token_mint0 uses Token-2022 program
    pub is_token_mint1_2022: bool, // Whether token_mint1 uses Token-2022 program
}

#[derive(Debug, Clone)]
pub struct RaydiumClmmPoolUpdate {
    pub slot: u64,
    pub transaction_index: Option<u64>,
    pub address: Pubkey,
    pub pool_state_part: Option<RaydiumClmmPoolStatePart>,
    pub reserve_part: Option<RaydiumClmmPoolReservePart>,
    pub tick_array_state: Option<TickArrayState>,
    pub tick_array_bitmap_extension: Option<TickArrayBitmapExtension>,
    pub last_updated: u64,
    pub is_account_state_update: bool,
    pub pool_update_event_type: PoolUpdateEventType,
    pub additional_event_type: i32, // for tick array index tracking, 0 for others
}

impl RaydiumClmmPoolState {
    pub fn get_program_id() -> Pubkey {
        Pubkey::new_from_array(*RAYDIUM_CLMM_PROGRAM_ID.as_array())
    }

    /// Calculate output amount using Raydium CLMM pool state
    pub async fn calculate_output_amount(
        &self,
        input_token: &Pubkey,
        input_amount: u64,
        amm_config_fetcher: Arc<dyn GetAmmConfig>,
    ) -> u64 {
        if input_amount == 0 {
            return 0;
        }

        let (token0, _) = (self.token_mint0, self.token_mint1);
        let input_is_token0 = tokens_equal(input_token, &token0);
        let sqrt_price_limit_x64 = if input_is_token0 {
            MIN_SQRT_PRICE_X64 + 1
        } else {
            MAX_SQRT_PRICE_X64 - 1
        };

        let real_input_amount = input_amount;
        self.get_output_amount(
            real_input_amount,
            input_token,
            sqrt_price_limit_x64,
            amm_config_fetcher,
        )
        .await
    }

    async fn get_output_amount(
        &self,
        input_amount: u64,
        input_token: &Pubkey,
        _sqrt_price_limit_x64: u128,
        amm_config_fetcher: Arc<dyn GetAmmConfig>,
    ) -> u64 {
        let amm_config = match amm_config_fetcher
            .get_raydium_clmm_amm_config(&self.amm_config)
            .await
        {
            Ok(Some(config)) => config,
            _ => return 0,
        };

        let pool_info = ComputeClmmPoolInfo::new(
            self.address,
            Self::get_program_id(),
            self,
            self.tick_array_bitmap_extension.as_ref(),
            Some(amm_config),
        );

        match PoolUtils::get_output_amount_and_remain_accounts(
            &pool_info,
            &self.tick_array_state,
            input_token,
            rug::Integer::from(input_amount),
        ) {
            Ok(result) => result.expected_amount_out.abs().to_u64().unwrap_or(0),
            Err(_) => 0,
        }
    }

    pub fn calculate_token_prices(
        &self,
        sol_price: f64,
        base_decimals: u8,
        quote_decimals: u8,
    ) -> (f64, f64) {
        // For concentrated liquidity (CLMM), price is derived from sqrt_price_x64
        // sqrt_price_x64 is in Q64 format (fixed point with 64 fractional bits)
        // price = (sqrt_price_x64 / 2^64)^2 * (10^(quote_decimals - base_decimals))

        if self.sqrt_price_x64 == 0 {
            return (0.0, 0.0);
        }

        let token0_str = self.token_mint0.to_string();
        let token1_str = self.token_mint1.to_string();

        let is_token0_a_base_token = is_base_token(&token0_str);
        let is_token1_a_base_token = is_base_token(&token1_str);

        // Convert sqrt_price_x64 from Q64 to float (Q64 == 2^64)
        let q64 = 2f64.powi(64);
        let sqrt_price = self.sqrt_price_x64 as f64 / q64;

        // Price = sqrt_price^2 * (10^(quote_decimals - base_decimals))
        let decimal_scale = 10_f64.powi(quote_decimals as i32 - base_decimals as i32);
        let price_ratio = sqrt_price * sqrt_price * decimal_scale;

        // If token1 is a base token (like USDC, SOL), use its price
        if is_token1_a_base_token {
            let token1_price = if token1_str == "So11111111111111111111111111111111111111112" {
                sol_price // SOL
            } else {
                1.0 // Assume USDC/USDT are ~$1
            };

            let token0_price = price_ratio * token1_price;
            (token0_price, token1_price)
        } else if is_token0_a_base_token {
            // If token0 is a base token, use its price
            let token0_price = if token0_str == "So11111111111111111111111111111111111111112" {
                sol_price // SOL
            } else {
                1.0 // Assume USDC/USDT are ~$1
            };

            let token1_price = token0_price / price_ratio;
            (token0_price, token1_price)
        } else {
            // Neither token is a base token, assume relative pricing
            (price_ratio, 1.0)
        }
    }
}

#[async_trait]
impl BuildSwapInstruction for RaydiumClmmPoolState {
    async fn build_swap_instruction(
        &self,
        params: &SwapParams,
        amm_config_fetcher: Arc<dyn GetAmmConfig>,
    ) -> std::result::Result<Vec<Instruction>, String> {
        // 1. Determine direction (zero_for_one)
        let zero_for_one = params.input_token.address == self.token_mint0;
        if !zero_for_one && params.input_token.address != self.token_mint1 {
            return Err("Input token does not match pool mints".to_string());
        }
        
        // Get AMM config
        let amm_config = match amm_config_fetcher
            .get_raydium_clmm_amm_config(&self.amm_config)
            .await
        {
            Ok(Some(config)) => config,
            _ => return Err("Failed to get AMM config".to_string()),
        };

        // Build pool info for calculations
        let pool_info = ComputeClmmPoolInfo::new(
            self.address,
            Self::get_program_id(),
            self,
            self.tick_array_bitmap_extension.as_ref(),
            Some(amm_config),
        );

        // Calculate output amount and get remaining tick array accounts
        let calculation_result = PoolUtils::get_output_amount_and_remain_accounts(
            &pool_info,
            &self.tick_array_state,
            &params.input_token.address,
            rug::Integer::from(params.input_amount),
        ).map_err(|e| e.to_string())?;

        let expected_amount_out = calculation_result.expected_amount_out.to_u64().unwrap_or(0);
        let all_tick_arrays = calculation_result.remaining_accounts;

        // Calculate slippage
        let slippage_factor = 10000 - params.slippage_bps as u64;
        let other_amount_threshold = (expected_amount_out as u128 * slippage_factor as u128 / 10000) as u64;

        // Sqrt price limit
        let sqrt_price_limit_x64 = if zero_for_one {
            MIN_SQRT_PRICE_X64 + 1
        } else {
            MAX_SQRT_PRICE_X64 - 1
        };

        let (input_vault, output_vault) = if zero_for_one {
            (self.token_vault0, self.token_vault1)
        } else {
            (self.token_vault1, self.token_vault0)
        };

        let (input_mint, output_mint) = if zero_for_one {
            (self.token_mint0, self.token_mint1)
        } else {
            (self.token_mint1, self.token_mint0)
        };

        // Get tick array accounts for remaining_accounts
        if all_tick_arrays.is_empty() {
            return Err("No tick arrays returned from calculation".to_string());
        }

        // Build remaining accounts: tickarray_bitmap_extension + tick_arrays
        let mut remaining_accounts = Vec::new();
        
        // Add all tick arrays as remaining accounts
        for &tick_array_key in &all_tick_arrays {
            remaining_accounts.push(AccountMeta::new(tick_array_key, false));
        }

        let token_program_0 = if self.is_token_mint0_2022 {
            spl_token_2022::id()
        } else {
            spl_token::id() 
        };
        let token_program_1 = if self.is_token_mint1_2022 {
            spl_token_2022::id()
        } else {
            spl_token::id()
        };

        // Determine which token program to use for input and output
        let (input_token_program, output_token_program) = if zero_for_one {
            (token_program_0, token_program_1)
        } else {
            (token_program_1, token_program_0)
        };

        // Convert Address types to anchor_lang Pubkey for compatibility
        let user_wallet_old = anchor_lang::prelude::Pubkey::new_from_array(params.user_wallet.to_bytes());
        let input_mint_old = anchor_lang::prelude::Pubkey::new_from_array(params.input_token.address.to_bytes());
        let output_mint_old = anchor_lang::prelude::Pubkey::new_from_array(params.output_token.address.to_bytes());

        // Get user's associated token accounts
        let user_input_token_old = spl_associated_token_account::get_associated_token_address_with_program_id(
            &user_wallet_old,
            &input_mint_old,
            &input_token_program,
        );
        let user_output_token_old = spl_associated_token_account::get_associated_token_address_with_program_id(
            &user_wallet_old,
            &output_mint_old,
            &output_token_program,
        );
        
        // Convert back to Address for AccountMeta
        let user_input_token = solana_sdk::pubkey::Pubkey::new_from_array(user_input_token_old.to_bytes());
        let user_output_token = solana_sdk::pubkey::Pubkey::new_from_array(user_output_token_old.to_bytes());

        // SwapV2 instruction (supports both SPL Token and Token-2022)
        // Discriminator for SwapV2
        let discriminator: [u8; 8] = [43, 4, 237, 11, 26, 201, 30, 98]; // SwapV2 discriminator
        
        let args = SwapV2Args {
            amount: params.input_amount,
            other_amount_threshold,
            sqrt_price_limit_x64,
            is_base_input: true,
        };
        let mut data = Vec::with_capacity(8 + 32);
        data.extend_from_slice(&discriminator);
        args.serialize(&mut data).map_err(|e| e.to_string())?;

        let mut accounts = vec![
            AccountMeta::new(params.user_wallet, true), // payer
            AccountMeta::new_readonly(self.amm_config, false), // amm_config
            AccountMeta::new(self.address, false), // pool_state
            AccountMeta::new(user_input_token, false), // input_token_account
            AccountMeta::new(user_output_token, false), // output_token_account
            AccountMeta::new(input_vault, false), // input_vault
            AccountMeta::new(output_vault, false), // output_vault
            AccountMeta::new(self.observation_key, false), // observation_state
            common::constants::TOKEN_PROGRAM_META, // token_program
            common::constants::TOKEN_PROGRAM_2022_META, // token_program_2022
            common::constants::SPL_MEMO_PROGRAM_META, // memo_program
            AccountMeta::new_readonly(input_mint, false), // input_vault_mint
            AccountMeta::new_readonly(output_mint, false), // output_vault_mint
        ];
        // Add remaining tick arrays
        accounts.extend(remaining_accounts);
        let swap_instruction = Instruction {
            program_id: Self::get_program_id(),
            accounts,
            data,
        };

        // Compute Budget Instruction
        let compute_budget_instruction = ComputeBudgetInstruction::set_compute_unit_limit(1_400_000);

        let mut instructions = vec![compute_budget_instruction];

        // Create input token ATA instruction (idempotent - creates if doesn't exist)
        let create_input_ata_accounts = vec![
            AccountMeta::new(params.user_wallet, true), // funding
            AccountMeta::new(user_input_token, false), // associated_token
            AccountMeta::new_readonly(params.user_wallet, false), // wallet
            AccountMeta::new_readonly(input_mint, false), // mint
            common::constants::SYSTEM_PROGRAM_META, // system_program
            if self.is_token_mint0_2022 && zero_for_one || self.is_token_mint1_2022 && !zero_for_one {
                common::constants::TOKEN_PROGRAM_2022_META
            } else {
                common::constants::TOKEN_PROGRAM_META
            }, // token_program
        ];
        
        let spl_associated_token_account_program_id = solana_sdk::pubkey::Pubkey::new_from_array(spl_associated_token_account::id().to_bytes());
        let create_input_ata_ix = Instruction {
            program_id: spl_associated_token_account_program_id,
            accounts: create_input_ata_accounts,
            data: vec![1], // Idempotent instruction discriminator
        };
        instructions.push(create_input_ata_ix);

        // Create output token ATA instruction (idempotent - creates if doesn't exist)
        let create_output_ata_accounts = vec![
            AccountMeta::new(params.user_wallet, true), // funding
            AccountMeta::new(user_output_token, false), // associated_token
            AccountMeta::new_readonly(params.user_wallet, false), // wallet
            AccountMeta::new_readonly(output_mint, false), // mint
            common::constants::SYSTEM_PROGRAM_META, // system_program
            if self.is_token_mint1_2022 && zero_for_one || self.is_token_mint0_2022 && !zero_for_one {
                common::constants::TOKEN_PROGRAM_2022_META
            } else {
                common::constants::TOKEN_PROGRAM_META
            }, // token_program
        ];
        
        let create_output_ata_ix = Instruction {
            program_id: spl_associated_token_account_program_id,
            accounts: create_output_ata_accounts,
            data: vec![1], // Idempotent instruction discriminator
        };
        instructions.push(create_output_ata_ix);

        // Add swap instruction
        instructions.push(swap_instruction);

        Ok(instructions)
    }
}
