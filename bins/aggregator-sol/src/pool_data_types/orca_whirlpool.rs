use crate::{
    constants::is_base_token,
    pool_data_types::{
        common::{constants, functions},
        GetAmmConfig, PoolUpdateEventType,
    },
};
use borsh::{BorshDeserialize, BorshSerialize};

use crate::pool_data_types::traits::BuildSwapInstruction;
use crate::types::SwapParams;
use crate::utils::tokens_equal;
use async_trait::async_trait;
use orca_whirlpools_core::{
    compute_swap, AdaptiveFeeConstantsFacade, AdaptiveFeeInfo, AdaptiveFeeVariablesFacade,
    TickArrayFacade, TickArraySequence, TickFacade, WhirlpoolFacade, WhirlpoolRewardInfoFacade,
    NUM_REWARDS, TICK_ARRAY_SIZE,
};
use serde::{Deserialize, Serialize};
use solana_program::instruction::{AccountMeta, Instruction};
use solana_sdk::pubkey::Pubkey;
use solana_streamer_sdk::streaming::event_parser::protocols::orca_whirlpools::{
    parser::ORCA_WHIRLPOOL_PROGRAM_ID, types::OracleState, types::TickArrayState,
};
use std::collections::HashMap;
use std::sync::Arc;

use serde_with::{serde_as, DisplayFromStr};

#[serde_as]
#[derive(Debug, Clone, Default, Serialize, Deserialize, BorshDeserialize, BorshSerialize)]
pub struct WhirlpoolPoolState {
    pub slot: u64,
    pub transaction_index: Option<u64>,
    pub address: Pubkey,
    pub whirlpool_config: Pubkey,
    pub tick_spacing: u16,
    pub tick_spacing_seed: [u8; 2],
    pub fee_rate: u16,
    pub protocol_fee_rate: u16,
    #[serde_as(as = "DisplayFromStr")]
    pub liquidity: u128,
    pub liquidity_usd: f64,
    #[serde_as(as = "DisplayFromStr")]
    pub sqrt_price: u128,
    pub tick_current_index: i32,
    pub token_mint_a: Pubkey,
    pub token_vault_a: Pubkey,
    pub token_mint_b: Pubkey,
    pub token_vault_b: Pubkey,

    #[serde(skip)]
    pub tick_array_state: HashMap<i32, TickArrayState>,
    pub last_updated: u64, // Unix timestamp
    pub token_a_reserve: u64,
    pub token_b_reserve: u64,
    pub is_state_keys_initialized: bool,
    #[serde(skip)]
    pub oracle_state: OracleState,
}

#[derive(Clone, Debug)]
pub struct WhirlpoolPoolStatePart {
    pub whirlpool_config: Pubkey,
    pub tick_spacing: u16,
    pub tick_spacing_seed: [u8; 2],
    pub fee_rate: u16,
    pub protocol_fee_rate: u16,
    pub liquidity: u128,
    pub sqrt_price: u128,
    pub tick_current_index: i32,
    pub token_mint_a: Pubkey,
    pub token_vault_a: Pubkey,
    pub token_mint_b: Pubkey,
    pub token_vault_b: Pubkey,
}

#[derive(Clone, Debug)]
pub struct WhirlpoolPoolReservePart {
    pub token_a_reserve: u64,
    pub token_b_reserve: u64,
}

#[derive(Debug, Clone)]
pub struct WhirlpoolPoolUpdate {
    pub slot: u64,
    pub transaction_index: Option<u64>,
    pub address: Pubkey,
    pub pool_state_part: Option<WhirlpoolPoolStatePart>,
    pub reserve_part: Option<WhirlpoolPoolReservePart>,
    pub tick_array_state: Option<TickArrayState>,
    pub oracle_state: Option<OracleState>,
    pub last_updated: u64,
    pub is_account_state_update: bool,
    pub pool_update_event_type: PoolUpdateEventType,
    pub additional_event_type: i32, // for tick array index tracking, 0 for others
}

impl WhirlpoolPoolState {
    pub fn get_program_id() -> Pubkey {
        Pubkey::new_from_array(*ORCA_WHIRLPOOL_PROGRAM_ID.as_array())
    }

    pub async fn calculate_output_amount(
        &self,
        input_token: &Pubkey,
        input_amount: u64,
        _amm_config_fetcher: Arc<dyn GetAmmConfig>,
    ) -> u64 {
        if input_amount == 0 {
            return 0;
        }

        let specified_token_a = tokens_equal(input_token, &self.token_mint_a);
        let a_to_b = specified_token_a; // ExactIn: Input A -> Output B (A to B)

        // Construct WhirlpoolFacade
        let whirlpool = WhirlpoolFacade {
            fee_tier_index_seed: self.tick_spacing_seed,
            tick_spacing: self.tick_spacing,
            fee_rate: self.fee_rate,
            protocol_fee_rate: self.protocol_fee_rate,
            liquidity: self.liquidity,
            sqrt_price: self.sqrt_price,
            tick_current_index: self.tick_current_index,
            fee_growth_global_a: 0,
            fee_growth_global_b: 0,
            reward_last_updated_timestamp: 0,
            reward_infos: [WhirlpoolRewardInfoFacade::default(); NUM_REWARDS],
        };

        // Construct AdaptiveFeeInfo
        let adaptive_fee_info = if self.oracle_state.whirlpool == Pubkey::default() {
            None
        } else {
            Some(AdaptiveFeeInfo {
                constants: AdaptiveFeeConstantsFacade {
                    filter_period: self.oracle_state.adaptive_fee_constants.filter_period,
                    decay_period: self.oracle_state.adaptive_fee_constants.decay_period,
                    reduction_factor: self.oracle_state.adaptive_fee_constants.reduction_factor,
                    adaptive_fee_control_factor: self
                        .oracle_state
                        .adaptive_fee_constants
                        .adaptive_fee_control_factor,
                    max_volatility_accumulator: self
                        .oracle_state
                        .adaptive_fee_constants
                        .max_volatility_accumulator,
                    tick_group_size: self.oracle_state.adaptive_fee_constants.tick_group_size,
                    major_swap_threshold_ticks: self
                        .oracle_state
                        .adaptive_fee_constants
                        .major_swap_threshold_ticks,
                },
                variables: AdaptiveFeeVariablesFacade {
                    last_reference_update_timestamp: self
                        .oracle_state
                        .adaptive_fee_variables
                        .last_reference_update_timestamp,
                    last_major_swap_timestamp: self
                        .oracle_state
                        .adaptive_fee_variables
                        .last_major_swap_timestamp,
                    volatility_reference: self
                        .oracle_state
                        .adaptive_fee_variables
                        .volatility_reference,
                    tick_group_index_reference: self
                        .oracle_state
                        .adaptive_fee_variables
                        .tick_group_index_reference,
                    volatility_accumulator: self
                        .oracle_state
                        .adaptive_fee_variables
                        .volatility_accumulator,
                },
            })
        };

        // Construct TickArraySequence
        let start_tick_index = orca_whirlpools_core::get_tick_array_start_tick_index(
            self.tick_current_index,
            self.tick_spacing,
        );
        let offset = self.tick_spacing as i32 * TICK_ARRAY_SIZE as i32;

        // We need a sequence of tick arrays. Let's try to get 5 arrays centered around current.
        let mut tick_arrays: [Option<TickArrayFacade>; 5] = [None, None, None, None, None];

        for i in 0..5 {
            let index = start_tick_index + (i as i32 - 2) * offset;
            if let Some(state) = self.tick_array_state.get(&index) {
                // Convert TickArrayState to TickArrayFacade
                let mut ticks = [TickFacade::default(); TICK_ARRAY_SIZE];
                for (j, t) in state.ticks.iter().enumerate() {
                    if j >= TICK_ARRAY_SIZE {
                        break;
                    }
                    ticks[j] = TickFacade {
                        initialized: t.initialized,
                        liquidity_net: t.liquidity_net,
                        liquidity_gross: t.liquidity_gross,
                        fee_growth_outside_a: t.fee_growth_outside_a,
                        fee_growth_outside_b: t.fee_growth_outside_b,
                        reward_growths_outside: t.reward_growths_outside,
                    };
                }

                tick_arrays[i] = Some(TickArrayFacade {
                    start_tick_index: state.start_tick_index,
                    ticks,
                });
            }
        }

        let tick_sequence = match TickArraySequence::new(tick_arrays, self.tick_spacing) {
            Ok(seq) => seq,
            Err(_) => return 0,
        };

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let result = compute_swap(
            input_amount,
            0, // sqrt_price_limit (0 means default min/max)
            whirlpool,
            tick_sequence,
            a_to_b,
            true, // specified_input = true (ExactIn)
            timestamp,
            adaptive_fee_info,
        );

        match result {
            Ok(swap_result) => {
                if a_to_b {
                    swap_result.token_b
                } else {
                    swap_result.token_a
                }
            }
            Err(_) => 0,
        }
    }

    pub fn calculate_token_prices(
        &self,
        sol_price: f64,
        base_decimals: u8,
        quote_decimals: u8,
    ) -> (f64, f64) {
        // For concentrated liquidity (CLMM), price is derived from sqrt_price
        // sqrt_price is in Q64 format (fixed point with 64 fractional bits)
        // price = (sqrt_price / 2^64)^2 * (10^(quote_decimals - base_decimals))

        if self.sqrt_price == 0 {
            return (0.0, 0.0);
        }

        let token_a_str = self.token_mint_a.to_string();
        let token_b_str = self.token_mint_b.to_string();

        let is_token_a_base_token = is_base_token(&token_a_str);
        let is_token_b_base_token = is_base_token(&token_b_str);

        // Convert sqrt_price from Q64 to float (Q64 == 2^64)
        let q64 = 2f64.powi(64);
        let sqrt_price = self.sqrt_price as f64 / q64;

        // Price = sqrt_price^2 * (10^(quote_decimals - base_decimals))
        let decimal_scale = 10_f64.powi(quote_decimals as i32 - base_decimals as i32);
        let price_ratio = sqrt_price * sqrt_price * decimal_scale;

        // If token_b is a base token (like USDC, SOL), use its price
        if is_token_b_base_token {
            let token_b_price = if token_b_str == "So11111111111111111111111111111111111111112" {
                sol_price // SOL
            } else {
                1.0 // Assume USDC/USDT are ~$1
            };

            let token_a_price = price_ratio * token_b_price;
            (token_a_price, token_b_price)
        } else if is_token_a_base_token {
            // If token_a is a base token, use its price
            let token_a_price = if token_a_str == "So11111111111111111111111111111111111111112" {
                sol_price // SOL
            } else {
                1.0 // Assume USDC/USDT are ~$1
            };

            let token_b_price = token_a_price / price_ratio;
            (token_a_price, token_b_price)
        } else {
            // Neither token is a base token, assume relative pricing
            (price_ratio, 1.0)
        }
    }
}

#[derive(BorshSerialize)]
struct SwapV2InstructionArgs {
    amount: u64,
    other_amount_threshold: u64,
    sqrt_price_limit: u128,
    amount_specified_is_input: bool,
    a_to_b: bool,
    remaining_accounts_info: Option<RemainingAccountsInfo>,
}

#[derive(BorshSerialize)]
struct RemainingAccountsInfo {
    slices: Vec<RemainingAccountsSlice>,
}

#[derive(BorshSerialize)]
struct RemainingAccountsSlice {
    accounts_type: u8, // 0 = SupplementalTickArrays
    length: u8,
}

#[async_trait]
impl BuildSwapInstruction for WhirlpoolPoolState {
    async fn build_swap_instruction(
        &self,
        params: &SwapParams,
        _amm_config_fetcher: Arc<dyn GetAmmConfig>,
    ) -> std::result::Result<Vec<Instruction>, String> {
        // 1. Determine swap direction
        let specified_token_a = tokens_equal(&params.input_token.address, &self.token_mint_a);
        let a_to_b = specified_token_a; // ExactIn: Input A -> Output B (A to B)

        if !specified_token_a && !tokens_equal(&params.input_token.address, &self.token_mint_b) {
            return Err("Input token does not match pool mints".to_string());
        }

        // 2. Calculate output amount using existing function
        let expected_amount_out = self
            .calculate_output_amount(
                &params.input_token.address,
                params.input_amount,
                _amm_config_fetcher,
            )
            .await;

        if expected_amount_out == 0 {
            return Err("Failed to calculate output amount or output is zero".to_string());
        }

        // 3. Calculate slippage
        let other_amount_threshold =
            functions::calculate_slippage(expected_amount_out, params.slippage_bps)?;

        // 4. Get tick array addresses
        let start_tick_index = orca_whirlpools_core::get_tick_array_start_tick_index(
            self.tick_current_index,
            self.tick_spacing,
        );
        let offset = self.tick_spacing as i32 * TICK_ARRAY_SIZE as i32;

        let tick_array_indexes = [
            start_tick_index,
            start_tick_index + offset,
            start_tick_index + offset * 2,
            start_tick_index - offset,
            start_tick_index - offset * 2,
        ];

        let program_id = Self::get_program_id();
        let tick_array_addresses: [Pubkey; 5] = [
            get_tick_array_pda(&self.address, tick_array_indexes[0], &program_id),
            get_tick_array_pda(&self.address, tick_array_indexes[1], &program_id),
            get_tick_array_pda(&self.address, tick_array_indexes[2], &program_id),
            get_tick_array_pda(&self.address, tick_array_indexes[3], &program_id),
            get_tick_array_pda(&self.address, tick_array_indexes[4], &program_id),
        ];

        // 5. Get oracle address
        let oracle_seeds = &[b"oracle", self.address.as_ref()];
        let (oracle_address, _) = Pubkey::find_program_address(oracle_seeds, &program_id);

        let token_program_0 = functions::get_token_program(params.input_token.is_token_2022);
        let token_program_1 = functions::get_token_program(params.output_token.is_token_2022);

        // Determine which token program to use for input and output
        let (token_program_a, token_program_b) = if a_to_b {
            (token_program_0, token_program_1)
        } else {
            (token_program_1, token_program_0)
        };

        // For ATA creation
        let token_program_0_id = if params.input_token.is_token_2022 {
            spl_token_2022::id()
        } else {
            spl_token::id()
        };
        let token_program_1_id = if params.output_token.is_token_2022 {
            spl_token_2022::id()
        } else {
            spl_token::id()
        };

        // Determine which token program to use for input and output
        let (token_program_a_id, token_program_b_id) = if a_to_b {
            (token_program_0_id, token_program_1_id)
        } else {
            (token_program_1_id, token_program_0_id)
        };

        // 7. Get user's ATAs
        let user_wallet_pubkey = functions::to_pubkey(&params.user_wallet);
        let mint_a_pubkey = functions::to_pubkey(&self.token_mint_a);
        let mint_b_pubkey = functions::to_pubkey(&self.token_mint_b);

        let token_owner_account_a_pubkey =
            spl_associated_token_account::get_associated_token_address_with_program_id(
                &user_wallet_pubkey,
                &mint_a_pubkey,
                &token_program_a_id,
            );
        let token_owner_account_b_pubkey =
            spl_associated_token_account::get_associated_token_address_with_program_id(
                &user_wallet_pubkey,
                &mint_b_pubkey,
                &token_program_b_id,
            );

        let token_owner_account_a = functions::to_address(&token_owner_account_a_pubkey);
        let token_owner_account_b = functions::to_address(&token_owner_account_b_pubkey);

        // 8. Build SwapV2 instruction
        // Discriminator for SwapV2: sha256("global:swap_v2")[..8]
        let discriminator: [u8; 8] = [43, 4, 237, 11, 26, 201, 30, 98];

        let args = SwapV2InstructionArgs {
            amount: params.input_amount,
            other_amount_threshold,
            sqrt_price_limit: 0,             // 0 means default min/max
            amount_specified_is_input: true, // ExactIn
            a_to_b,
            remaining_accounts_info: Some(RemainingAccountsInfo {
                slices: vec![RemainingAccountsSlice {
                    accounts_type: 0, // SupplementalTickArrays
                    length: 2,
                }],
            }),
        };

        let mut data = Vec::with_capacity(8 + 64);
        data.extend_from_slice(&discriminator);
        args.serialize(&mut data).map_err(|e| e.to_string())?;

        let mut accounts = vec![
            AccountMeta::new_readonly(token_program_a, false), // token_program_a
            AccountMeta::new_readonly(token_program_b, false), // token_program_b
            AccountMeta::new_readonly(constants::SPL_MEMO_PROGRAM, false), // memo_program
            AccountMeta::new_readonly(params.user_wallet, true), // token_authority (signer)
            AccountMeta::new(self.address, false),             // whirlpool
            AccountMeta::new_readonly(self.token_mint_a, false), // token_mint_a
            AccountMeta::new_readonly(self.token_mint_b, false), // token_mint_b
            AccountMeta::new(token_owner_account_a, false),    // token_owner_account_a
            AccountMeta::new(self.token_vault_a, false),       // token_vault_a
            AccountMeta::new(token_owner_account_b, false),    // token_owner_account_b
            AccountMeta::new(self.token_vault_b, false),       // token_vault_b
            AccountMeta::new(tick_array_addresses[0], false),  // tick_array_0
            AccountMeta::new(tick_array_addresses[1], false),  // tick_array_1
            AccountMeta::new(tick_array_addresses[2], false),  // tick_array_2
            AccountMeta::new(oracle_address, false),           // oracle
        ];

        // Add supplemental tick arrays as remaining accounts
        accounts.push(AccountMeta::new(tick_array_addresses[3], false));
        accounts.push(AccountMeta::new(tick_array_addresses[4], false));

        let swap_instruction = Instruction {
            program_id: Self::get_program_id(),
            accounts,
            data,
        };

        // 9. Build instruction list
        let mut instructions = Vec::new();

        // Create ATA for token A if needed
        instructions.push(functions::create_ata_instruction(
            params.user_wallet,
            token_owner_account_a,
            self.token_mint_a,
            token_program_a == constants::TOKEN_PROGRAM_2022,
        ));

        // Create ATA for token B if needed
        instructions.push(functions::create_ata_instruction(
            params.user_wallet,
            token_owner_account_b,
            self.token_mint_b,
            token_program_b == constants::TOKEN_PROGRAM_2022,
        ));

        // Add swap instruction
        instructions.push(swap_instruction);

        Ok(instructions)
    }
}
// Helper function to derive tick array PDAs
fn get_tick_array_pda(whirlpool: &Pubkey, start_tick_index: i32, program_id: &Pubkey) -> Pubkey {
    let start_tick_index_str = start_tick_index.to_string();
    let seeds = &[
        b"tick_array",
        whirlpool.as_ref(),
        &start_tick_index_str.as_bytes(),
    ];
    let (pda, _) = Pubkey::find_program_address(seeds, program_id);
    pda
}
