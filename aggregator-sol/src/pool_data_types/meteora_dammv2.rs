use crate::pool_data_types::{GetAmmConfig, PoolUpdateEventType};
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use solana_streamer_sdk::streaming::event_parser::protocols::meteora_dammv2::parser::METEORA_DAMM_V2_PROGRAM_ID;
pub use solana_streamer_sdk::streaming::event_parser::protocols::meteora_dammv2::types::{
    PoolFeesStruct, PoolMetrics, RewardInfo,
};
use std::str::FromStr;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeteoraDammv2PoolState {
    pub slot: u64,
    pub transaction_index: Option<u64>,
    pub address: Pubkey,
    /// Pool fee
    pub pool_fees: PoolFeesStruct,
    /// token a mint
    pub token_a_mint: Pubkey,
    /// token b mint
    pub token_b_mint: Pubkey,
    /// token a vault
    pub token_a_vault: Pubkey,
    /// token b vault
    pub token_b_vault: Pubkey,
    /// Whitelisted vault to be able to buy pool before activation_point
    pub whitelisted_vault: Pubkey,
    /// partner
    pub partner: Pubkey,
    /// liquidity share
    pub liquidity: u128,
    /// protocol a fee
    pub protocol_a_fee: u64,
    /// protocol b fee
    pub protocol_b_fee: u64,
    /// partner a fee
    pub partner_a_fee: u64,
    /// partner b fee
    pub partner_b_fee: u64,
    /// min price
    pub sqrt_min_price: u128,
    /// max price
    pub sqrt_max_price: u128,
    /// current price
    pub sqrt_price: u128,
    /// Activation point, can be slot or timestamp
    pub activation_point: u64,
    /// Activation type, 0 means by slot, 1 means by timestamp
    pub activation_type: u8,
    /// pool status, 0: enable, 1 disable
    pub pool_status: u8,
    /// token a flag
    pub token_a_flag: u8,
    /// token b flag
    pub token_b_flag: u8,
    /// 0 is collect fee in both token, 1 only collect fee in token a, 2 only collect fee in token b
    pub collect_fee_mode: u8,
    /// pool type
    pub pool_type: u8,
    /// pool version, 0: max_fee is still capped at 50%, 1: max_fee is capped at 99%
    pub version: u8,
    /// cumulative fees
    pub fee_a_per_liquidity: [u8; 32],
    pub fee_b_per_liquidity: [u8; 32],
    /// permanent lock liquidity
    pub permanent_lock_liquidity: u128,
    /// metrics
    pub metrics: PoolMetrics,
    /// pool creator
    pub creator: Pubkey,
    /// Farming reward information
    pub reward_infos: [RewardInfo; 2],
    pub liquidity_usd: f64,
    pub last_updated: u64,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct MeteoraDammv2PoolUpdate {
    pub slot: u64,
    pub transaction_index: Option<u64>,
    pub address: Pubkey,
    /// Pool fee
    pub pool_fees: PoolFeesStruct,
    /// token a mint
    pub token_a_mint: Pubkey,
    /// token b mint
    pub token_b_mint: Pubkey,
    /// token a vault
    pub token_a_vault: Pubkey,
    /// token b vault
    pub token_b_vault: Pubkey,
    /// Whitelisted vault
    pub whitelisted_vault: Pubkey,
    /// partner
    pub partner: Pubkey,
    /// liquidity share
    pub liquidity: u128,
    /// protocol a fee
    pub protocol_a_fee: u64,
    /// protocol b fee
    pub protocol_b_fee: u64,
    /// partner a fee
    pub partner_a_fee: u64,
    /// partner b fee
    pub partner_b_fee: u64,
    /// min price
    pub sqrt_min_price: u128,
    /// max price
    pub sqrt_max_price: u128,
    /// current price
    pub sqrt_price: u128,
    /// Activation point
    pub activation_point: u64,
    /// Activation type
    pub activation_type: u8,
    /// pool status
    pub pool_status: u8,
    /// token a flag
    pub token_a_flag: u8,
    /// token b flag
    pub token_b_flag: u8,
    /// collect fee mode
    pub collect_fee_mode: u8,
    /// pool type
    pub pool_type: u8,
    /// pool version
    pub version: u8,
    /// cumulative fees
    pub fee_a_per_liquidity: [u8; 32],
    pub fee_b_per_liquidity: [u8; 32],
    /// permanent lock liquidity
    pub permanent_lock_liquidity: u128,
    /// metrics
    pub metrics: PoolMetrics,
    /// pool creator
    pub creator: Pubkey,
    /// reward infos
    pub reward_infos: [RewardInfo; 2],
    pub is_account_state_update: bool,
    pub pool_update_event_type: PoolUpdateEventType,
    pub additional_event_type: i32,
    pub last_updated: u64,
}

#[allow(dead_code)]
impl MeteoraDammv2PoolState {
    pub fn get_program_id() -> Pubkey {
        Pubkey::from_str(&METEORA_DAMM_V2_PROGRAM_ID.to_string()).unwrap_or_else(|_| Pubkey::default())
    }

    /// Calculate output amount for Meteora Damm V2 pool using cp-amm SDK
    pub fn calculate_output_amount(
        &self,
        input_token: &Pubkey,
        input_amount: u64,
        _: Arc<dyn GetAmmConfig>,
    ) -> u64 {
        // Helper to convert solana_sdk::pubkey::Pubkey to anchor_lang::prelude::Pubkey
        let to_anchor_pubkey = |p: Pubkey| anchor_lang::prelude::Pubkey::from(p.to_bytes());

        // Construct Pool using cp-amm types
        let pool = cp_amm::state::Pool {
            pool_fees: cp_amm::state::fee::PoolFeesStruct {
                base_fee: cp_amm::state::fee::BaseFeeStruct {
                    cliff_fee_numerator: self.pool_fees.base_fee.cliff_fee_numerator,
                    base_fee_mode: self.pool_fees.base_fee.base_fee_mode,
                    padding_0: self.pool_fees.base_fee.padding_0,
                    first_factor: self.pool_fees.base_fee.first_factor,
                    second_factor: self.pool_fees.base_fee.second_factor,
                    third_factor: self.pool_fees.base_fee.third_factor,
                    padding_1: self.pool_fees.base_fee.padding_1,
                },
                protocol_fee_percent: self.pool_fees.protocol_fee_percent,
                partner_fee_percent: self.pool_fees.partner_fee_percent,
                referral_fee_percent: self.pool_fees.referral_fee_percent,
                padding_0: self.pool_fees.padding_0,
                dynamic_fee: cp_amm::state::fee::DynamicFeeStruct {
                    initialized: self.pool_fees.dynamic_fee.initialized,
                    padding: self.pool_fees.dynamic_fee.padding,
                    max_volatility_accumulator: self.pool_fees.dynamic_fee.max_volatility_accumulator,
                    variable_fee_control: self.pool_fees.dynamic_fee.variable_fee_control,
                    bin_step: self.pool_fees.dynamic_fee.bin_step,
                    filter_period: self.pool_fees.dynamic_fee.filter_period,
                    decay_period: self.pool_fees.dynamic_fee.decay_period,
                    reduction_factor: self.pool_fees.dynamic_fee.reduction_factor,
                    last_update_timestamp: self.pool_fees.dynamic_fee.last_update_timestamp,
                    bin_step_u128: self.pool_fees.dynamic_fee.bin_step_u128,
                    sqrt_price_reference: self.pool_fees.dynamic_fee.sqrt_price_reference,
                    volatility_accumulator: self.pool_fees.dynamic_fee.volatility_accumulator,
                    volatility_reference: self.pool_fees.dynamic_fee.volatility_reference,
                },
                padding_1: self.pool_fees.padding_1,
            },
            token_a_mint: to_anchor_pubkey(self.token_a_mint),
            token_b_mint: to_anchor_pubkey(self.token_b_mint),
            token_a_vault: to_anchor_pubkey(self.token_a_vault),
            token_b_vault: to_anchor_pubkey(self.token_b_vault),
            whitelisted_vault: to_anchor_pubkey(self.whitelisted_vault),
            partner: to_anchor_pubkey(self.partner),
            liquidity: self.liquidity,
            _padding: 0,
            protocol_a_fee: self.protocol_a_fee,
            protocol_b_fee: self.protocol_b_fee,
            partner_a_fee: self.partner_a_fee,
            partner_b_fee: self.partner_b_fee,
            sqrt_min_price: self.sqrt_min_price,
            sqrt_max_price: self.sqrt_max_price,
            sqrt_price: self.sqrt_price,
            activation_point: self.activation_point,
            activation_type: self.activation_type,
            pool_status: self.pool_status,
            token_a_flag: self.token_a_flag,
            token_b_flag: self.token_b_flag,
            collect_fee_mode: self.collect_fee_mode,
            pool_type: self.pool_type,
            version: self.version,
            _padding_0: 0,
            fee_a_per_liquidity: self.fee_a_per_liquidity,
            fee_b_per_liquidity: self.fee_b_per_liquidity,
            permanent_lock_liquidity: self.permanent_lock_liquidity,
            metrics: cp_amm::state::PoolMetrics {
                total_lp_a_fee: self.metrics.total_lp_a_fee,
                total_lp_b_fee: self.metrics.total_lp_b_fee,
                total_protocol_a_fee: self.metrics.total_protocol_a_fee,
                total_protocol_b_fee: self.metrics.total_protocol_b_fee,
                total_partner_a_fee: self.metrics.total_partner_a_fee,
                total_partner_b_fee: self.metrics.total_partner_b_fee,
                total_position: self.metrics.total_position,
                padding: self.metrics.padding,
            },
            creator: to_anchor_pubkey(self.creator),
            _padding_1: [0; 6],
            reward_infos: std::array::from_fn(|i| {
                let ri = &self.reward_infos[i];
                cp_amm::state::RewardInfo {
                    initialized: ri.initialized,
                    reward_token_flag: ri.reward_token_flag,
                    _padding_0: ri._padding_0,
                    _padding_1: ri._padding_1,
                    mint: to_anchor_pubkey(ri.mint),
                    vault: to_anchor_pubkey(ri.vault),
                    funder: to_anchor_pubkey(ri.funder),
                    reward_duration: ri.reward_duration,
                    reward_duration_end: ri.reward_duration_end,
                    reward_rate: ri.reward_rate,
                    reward_per_token_stored: ri.reward_per_token_stored,
                    last_update_time: ri.last_update_time,
                    cumulative_seconds_with_empty_liquidity_reward: ri.cumulative_seconds_with_empty_liquidity_reward,
                }
            }),
        };

        let a_to_b = *input_token == self.token_a_mint;
        let current_slot = self.slot;
        let current_timestamp = self.last_updated / 1_000_000;

        let result = cp_amm_sdk::quote_exact_in::get_quote(
            &pool,
            current_timestamp,
            current_slot,
            input_amount,
            a_to_b,
            false, // has_referral
        );

        match result {
            Ok(swap_result) => swap_result.output_amount,
            Err(e) => {
                log::warn!("Meteora Damm V2 quote calculation failed: {}", e);
                0
            }
        }
    }

    pub fn calculate_token_prices(
        &self,
        sol_price: f64,
        token_a_decimals: u8,
        token_b_decimals: u8,
    ) -> (f64, f64) {
        // sqrt_price is in Q64.64 format (price of token A in terms of token B)
        // price = (sqrt_price / 2^64)^2
        
        if self.sqrt_price == 0 {
            return (0.0, 0.0);
        }

        // Convert sqrt_price to actual price
        let q64 = 1u128 << 64;
        let sqrt_price_f64 = self.sqrt_price as f64 / q64 as f64;
        let price_a_in_b = sqrt_price_f64 * sqrt_price_f64;

        // Adjust for decimals
        let decimal_scale = 10_f64.powi(token_a_decimals as i32 - token_b_decimals as i32);
        let adjusted_price_a_in_b = price_a_in_b * decimal_scale;

        // If token B is SOL, calculate token A price in USD
        // Otherwise, return the ratio
        let token_a_price = adjusted_price_a_in_b * sol_price;
        let token_b_price = sol_price;

        (token_a_price, token_b_price)
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
struct SwapParameters2 {
    /// When SwapMode::ExactIn: amount_in. When SwapMode::ExactOut: amount_out
    pub amount_0: u64,
    /// When SwapMode::ExactIn: minimum_amount_out. When SwapMode::ExactOut: maximum_amount_in
    pub amount_1: u64,
    /// Swap mode: 0 = ExactIn, 1 = PartialFill, 2 = ExactOut
    pub swap_mode: u8,
}

#[async_trait]
impl BuildSwapInstruction for MeteoraDammv2PoolState {
    async fn build_swap_instruction(
        &self,
        params: &SwapParams,
        _amm_config_fetcher: Arc<dyn GetAmmConfig>,
    ) -> Result<Vec<Instruction>, String> {
        let input_mint = params.input_token.address;

        // Determine swap direction (a_to_b = true if input is token A)
        let a_to_b = input_mint == self.token_a_mint;

        // Determine token programs based on token flags
        let token_a_program = if self.token_a_flag == 1 {
            spl_token_2022::id()
        } else {
            spl_token::id()
        };

        let token_b_program = if self.token_b_flag == 1 {
            spl_token_2022::id()
        } else {
            spl_token::id()
        };

        // Convert to anchor Pubkey for ATA derivation
        let user_wallet_anchor = common_functions::to_pubkey(&params.user_wallet);
        let token_a_mint_anchor = common_functions::to_pubkey(&self.token_a_mint);
        let token_b_mint_anchor = common_functions::to_pubkey(&self.token_b_mint);

        // Derive user token accounts
        let user_token_a_account = get_associated_token_address_with_program_id(
            &user_wallet_anchor,
            &token_a_mint_anchor,
            &token_a_program,
        );
        let user_token_b_account = get_associated_token_address_with_program_id(
            &user_wallet_anchor,
            &token_b_mint_anchor,
            &token_b_program,
        );

        // Determine which account is input and which is output
        let (input_token_account, output_token_account) = if a_to_b {
            (user_token_a_account, user_token_b_account)
        } else {
            (user_token_b_account, user_token_a_account)
        };

        // Derive pool authority PDA
        let program_id = Self::get_program_id();
        let (pool_authority, _) = Pubkey::find_program_address(
            &[b"pool_authority"],
            &program_id,
        );

        // Calculate minimum output based on slippage
        let estimated_output = self.calculate_output_amount(
            &input_mint,
            params.input_amount,
            _amm_config_fetcher.clone(),
        );
        
        let minimum_amount_out = estimated_output
            .saturating_mul(10000 - params.slippage_bps as u64)
            / 10000;

        // Build accounts list for swap2 instruction
        // Account order from damm-v2/programs/cp-amm/src/instructions/swap/ix_swap.rs:
        // 1. pool_authority (PDA with seed "pool_authority")
        // 2. pool
        // 3. input_token_account (user's token account for input)
        // 4. output_token_account (user's token account for output)  
        // 5. token_a_vault
        // 6. token_b_vault
        // 7. token_a_mint
        // 8. token_b_mint
        // 9. payer (signer)
        // 10. token_a_program
        // 11. token_b_program
        let accounts = vec![
            AccountMeta::new_readonly(pool_authority, false),                              // pool_authority
            AccountMeta::new(self.address, false),                                         // pool
            AccountMeta::new(common_functions::to_address(&input_token_account), false),   // input_token_account
            AccountMeta::new(common_functions::to_address(&output_token_account), false),  // output_token_account
            AccountMeta::new(self.token_a_vault, false),                                   // token_a_vault
            AccountMeta::new(self.token_b_vault, false),                                   // token_b_vault
            AccountMeta::new_readonly(self.token_a_mint, false),                           // token_a_mint
            AccountMeta::new_readonly(self.token_b_mint, false),                           // token_b_mint
            AccountMeta::new_readonly(params.user_wallet, true),                           // payer (signer)
            AccountMeta::new_readonly(common_functions::to_address(&token_a_program), false), // token_a_program
            AccountMeta::new_readonly(common_functions::to_address(&token_b_program), false), // token_b_program
        ];

        // Build instruction data for swap2
        // Discriminator for swap2: sha256("global:swap2")[:8]
        let discriminator: [u8; 8] = [0x41, 0x4b, 0x3f, 0x4c, 0xeb, 0x5b, 0x5b, 0x88];
        let swap_mode_exact_in: u8 = 0; // SwapMode::ExactIn
        
        let args = SwapParameters2 {
            amount_0: params.input_amount,
            amount_1: minimum_amount_out,
            swap_mode: swap_mode_exact_in,
        };

        let mut data = Vec::with_capacity(8 + 17); // 8 discriminator + 8 + 8 + 1
        data.extend_from_slice(&discriminator);
        args.serialize(&mut data).map_err(|e| e.to_string())?;

        let swap_ix = Instruction {
            program_id,
            accounts,
            data,
        };

        // Build instruction list with compute budget and ATA creation
        let mut instructions = vec![
            ComputeBudgetInstruction::set_compute_unit_limit(600_000),
        ];

        // Determine which token program to use for input and output
        let (input_token_program_id, output_token_program_id, input_mint, output_mint) = if a_to_b {
            (token_a_program, token_b_program, self.token_a_mint, self.token_b_mint)
        } else {
            (token_b_program, token_a_program, self.token_b_mint, self.token_a_mint)
        };

        // Create ATA for input token if needed (idempotent)
        instructions.push(common_functions::create_ata_instruction(
            params.user_wallet,
            common_functions::to_address(&input_token_account),
            input_mint,
            input_token_program_id == spl_token_2022::id(),
        ));

        // Create ATA for output token if needed (idempotent)
        instructions.push(common_functions::create_ata_instruction(
            params.user_wallet,
            common_functions::to_address(&output_token_account),
            output_mint,
            output_token_program_id == spl_token_2022::id(),
        ));

        // Add swap instruction
        instructions.push(swap_ix);

        Ok(instructions)
    }
}