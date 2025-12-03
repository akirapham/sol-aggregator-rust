use std::{collections::HashMap, sync::Arc};

use crate::pool_data_types::{GetAmmConfig, PoolUpdateEventType};
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use solana_streamer_sdk::streaming::event_parser::protocols::meteora_dlmm::{parser::METEORA_DLMM_PROGRAM_ID, types::{StaticParameters, VariableParameters, ProtocolFee, RewardInfo}};

/// Bin data structure - represents liquidity at a specific price point
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Bin {
    pub amount_x: u64,
    pub amount_y: u64,
    pub liquidity_supply: u64,
    #[serde(skip)]
    #[allow(dead_code)]
    pub price: f64, // Cached price for this bin
}

/// BinArray - collection of 70 consecutive bins
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BinArrayState {
    pub index: i32, // Bin array index
    pub bins: Vec<Bin>, // 70 bins
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BinArrayBitmapExtension {
    pub lb_pair: Pubkey,
    pub positive_bin_array_bitmap: [[u64; 8];12],
    pub negative_bin_array_bitmap: [[u64; 8];12],
}

/// Meteora DLMM Pool State
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeteoraDlmmPoolState {
    pub slot: u64,
    pub transaction_index: Option<u64>,
    pub address: Pubkey,

    //lbpair account
    pub parameters: StaticParameters,
    pub v_parameters: VariableParameters,
    pub bump_seed: [u8; 1],
    pub bin_step_seed: [u8; 2],
    pub pair_type: u8,
    pub active_id: i32,
    pub bin_step: u16,
    pub status: u8,
    pub require_base_factor_seed: u8,
    pub base_factor_seed: [u8; 2],
    pub activation_type: u8,
    pub creator_pool_on_off_control: u8,
    pub token_x_mint: Pubkey,
    pub token_y_mint: Pubkey,
    pub reserve_x: Pubkey,
    pub reserve_y: Pubkey,
    pub protocol_fee: ProtocolFee,
    pub _padding_1: [u8; 32],
    pub reward_infos: RewardInfo,
    pub oracle: Pubkey,
    pub bin_array_bitmap: [u64; 16],
    pub last_updated: u64,
    pub _padding_2: [u8; 32],
    pub pre_activation_swap_address: Pubkey,
    pub base_key: Pubkey,
    pub activation_point: u64,
    pub pre_activation_duration: u64,
    pub _padding_3: [u8; 8],
    pub _padding_4: u64,
    pub creator: Pubkey,
    pub token_mint_x_program_flag: u8,
    pub token_mint_y_program_flag: u8,
    pub _reserved: [u8; 22],

    // Liquidity tracking
    pub liquidity_usd: f64,
    
    
    // Bin arrays (cached from events)
    #[serde(skip)]
    pub bin_arrays: HashMap<i32, BinArrayState>,
    pub bitmap_extension: Option<BinArrayBitmapExtension>,
    pub is_state_keys_initialized: bool,
}

/// Pool update event from stream
#[derive(Debug, Clone)]
pub struct MeteoraDlmmPoolUpdate {
    pub slot: u64,
    pub transaction_index: Option<u64>,

    //lbpair account
    pub address: Pubkey,
    pub parameters: StaticParameters,
    pub v_parameters: VariableParameters,
    pub bump_seed: [u8; 1],
    pub bin_step_seed: [u8; 2],
    pub pair_type: u8,
    pub active_id: i32,
    pub bin_step: u16,
    pub status: u8,
    pub require_base_factor_seed: u8,
    pub base_factor_seed: [u8; 2],
    pub activation_type: u8,
    pub creator_pool_on_off_control: u8,
    pub token_x_mint: Pubkey,
    pub token_y_mint: Pubkey,
    pub reserve_x: Pubkey,
    pub reserve_y: Pubkey,
    pub protocol_fee: ProtocolFee,
    pub _padding_1: [u8; 32],
    pub reward_infos: RewardInfo,
    pub oracle: Pubkey,
    pub bin_array_bitmap: [u64; 16],
    pub last_updated: u64,
    pub _padding_2: [u8; 32],
    pub pre_activation_swap_address: Pubkey,
    pub base_key: Pubkey,
    pub activation_point: u64,
    pub pre_activation_duration: u64,
    pub _padding_3: [u8; 8],
    pub _padding_4: u64,
    pub creator: Pubkey,
    pub token_mint_x_program_flag: u8,
    pub token_mint_y_program_flag: u8,
    pub _reserved: [u8; 22],
    
    pub bin_arrays: HashMap<i32, BinArrayState>, // bin_array_index -> BinArray
    pub bitmap_extension: Option<BinArrayBitmapExtension>,
    pub is_account_state_update: bool,
    pub pool_update_event_type: PoolUpdateEventType,
    pub additional_event_type: i32,
}

impl MeteoraDlmmPoolState {
    pub fn get_program_id() -> Pubkey {
        METEORA_DLMM_PROGRAM_ID
    }
    
    pub fn calculate_output_amount(
        &self,
        input_token: &Pubkey,
        input_amount: u64,
        _amm_config_fetcher: Arc<dyn GetAmmConfig>,
    ) -> u64 {
        if input_amount == 0 {
            return 0;
        }
        0
    }
    
    /// Calculate token prices
    pub fn calculate_token_prices(
        &self,
        sol_price: f64,
        _token_x_decimals: u8,
        _token_y_decimals: u8,
    ) -> (f64, f64) {
        (0.0, 0.0)
    }
}

use crate::types::SwapParams;
use async_trait::async_trait;
use crate::pool_data_types::traits::BuildSwapInstruction;
use crate::pool_data_types::common::functions as common_functions;
use solana_program::instruction::{AccountMeta, Instruction};
use spl_associated_token_account::get_associated_token_address_with_program_id;
use borsh::BorshSerialize;
use solana_compute_budget_interface::ComputeBudgetInstruction;

#[derive(BorshSerialize)]
struct Swap2Args {
    pub amount_in: u64,
    pub min_amount_out: u64,
}

#[async_trait]
impl BuildSwapInstruction for MeteoraDlmmPoolState {
    async fn build_swap_instruction(
        &self,
        params: &SwapParams,
        _amm_config_fetcher: Arc<dyn GetAmmConfig>,
    ) -> Result<Vec<Instruction>, String> {
        let input_mint = params.input_token.address;
        
        // Determine swap direction
        let swap_for_y = input_mint == self.token_x_mint;
        
        // Determine token programs
        let token_x_program = common_functions::get_token_program(params.input_token.is_token_2022);
        let token_y_program = common_functions::get_token_program(params.output_token.is_token_2022);
        
        // Convert to anchor Pubkey for ATA derivation
        let user_wallet_anchor = common_functions::to_pubkey(&params.user_wallet);
        let token_x_mint_anchor = common_functions::to_pubkey(&self.token_x_mint);
        let token_y_mint_anchor = common_functions::to_pubkey(&self.token_y_mint);
        let token_x_program_anchor = common_functions::to_pubkey(&token_x_program);
        let token_y_program_anchor = common_functions::to_pubkey(&token_y_program);
        
        // Derive user token accounts
        let user_token_x = get_associated_token_address_with_program_id(
            &user_wallet_anchor,
            &token_x_mint_anchor,
            &token_x_program_anchor,
        );
        let user_token_y = get_associated_token_address_with_program_id(
            &user_wallet_anchor,
            &token_y_mint_anchor,
            &token_y_program_anchor,
        );
        
        // Determine input/output accounts
        let (user_token_in, user_token_out) = if swap_for_y {
            (user_token_x, user_token_y)
        } else {
            (user_token_y, user_token_x)
        };
        
        // Derive event authority PDA
        let program_id = Self::get_program_id();
        let (event_authority, _) = Pubkey::find_program_address(
            &[b"__event_authority"],
            &program_id,
        );
        
        // Calculate minimum output with slippage
        let estimated_output = self.calculate_output_amount(
            &input_mint,
            params.input_amount,
            _amm_config_fetcher.clone(),
        );
        
        let min_amount_out = estimated_output
            .saturating_mul(10000 - params.slippage_bps as u64)
            / 10000;
        
        // Build main accounts
        let mut accounts = vec![
            AccountMeta::new(self.address, false),                                     // lb_pair
            AccountMeta::new(self.oracle, false),                                      // oracle (can be written to)
            AccountMeta::new(common_functions::to_address(&user_token_in), false),    // user_token_in
            AccountMeta::new(common_functions::to_address(&user_token_out), false),   // user_token_out
            AccountMeta::new(self.reserve_x, false),                                   // reserve_x
            AccountMeta::new(self.reserve_y, false),                                   // reserve_y
            AccountMeta::new_readonly(self.token_x_mint, false),                       // token_x_mint
            AccountMeta::new_readonly(self.token_y_mint, false),                       // token_y_mint
            AccountMeta::new_readonly(params.user_wallet, true),                       // user (signer)
            AccountMeta::new_readonly(token_x_program, false),                         // token_x_program
            AccountMeta::new_readonly(token_y_program, false),                         // token_y_program
            AccountMeta::new_readonly(event_authority, false),                         // event_authority
            AccountMeta::new_readonly(program_id, false),                              // program
        ];
        
        // // Add bin arrays as remaining accounts
        // // Get bin arrays needed for this swap (up to 4 arrays)
        // let bin_arrays = self.get_bin_arrays_for_swap(swap_for_y, 4);
        // for bin_array_pubkey in bin_arrays {
        //     accounts.push(AccountMeta::new(bin_array_pubkey, false));
        // }
        
        // Build instruction data
        let discriminator: [u8; 8] = [0x9c, 0x8a, 0xaa, 0xea, 0xd9, 0xfa, 0x02, 0x62]; // swap2
        let args = Swap2Args {
            amount_in: params.input_amount,
            min_amount_out,
        };
        
        let mut data = Vec::with_capacity(8 + 16);
        data.extend_from_slice(&discriminator);
        args.serialize(&mut data).map_err(|e| e.to_string())?;
        
        let swap_ix = Instruction {
            program_id,
            accounts,
            data,
        };
        
        // Build instruction list
        let mut instructions = vec![
            ComputeBudgetInstruction::set_compute_unit_limit(1_400_000),
        ];
        
        // Determine token programs for ATAs
        let is_x_token_2022 = params.input_token.is_token_2022;
        let is_y_token_2022 = params.output_token.is_token_2022;
        
        // Create ATA for input token if needed
        instructions.push(common_functions::create_ata_instruction(
            params.user_wallet,
            common_functions::to_address(&user_token_in),
            input_mint,
            if swap_for_y { is_x_token_2022 } else { is_y_token_2022 },
        ));
        
        // Create ATA for output token if needed
        let output_mint = if swap_for_y { self.token_y_mint } else { self.token_x_mint };
        instructions.push(common_functions::create_ata_instruction(
            params.user_wallet,
            common_functions::to_address(&user_token_out),
            output_mint,
            if swap_for_y { is_y_token_2022 } else { is_x_token_2022 },
        ));
        
        // Add swap instruction
        instructions.push(swap_ix);
        
        Ok(instructions)
    }
}