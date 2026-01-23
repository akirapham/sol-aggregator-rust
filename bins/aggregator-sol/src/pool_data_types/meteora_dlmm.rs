use std::collections::HashMap;

use crate::pool_data_types::dlmm::functions;
use crate::pool_data_types::{GetAmmConfig, PoolUpdateEventType};
use meteora_dlmm_sdk::{BinArrayExtension, BinExtension, LbPairExtension};
use serde::{Deserialize, Serialize};
use serde_with::{json::JsonString, serde_as};
use solana_sdk::pubkey::Pubkey;
use solana_streamer_sdk::streaming::event_parser::protocols::meteora_dlmm::{
    parser::METEORA_DLMM_PROGRAM_ID,
    types::{BinArray, BinArrayBitmapExtension, LbPair},
};

/// Meteora DLMM Pool State
#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeteoraDlmmPoolState {
    pub slot: u64,
    pub transaction_index: Option<u64>,
    pub address: Pubkey,
    #[serde_as(as = "JsonString")]
    pub lbpair: LbPair,
    #[serde(skip)]
    pub bin_arrays: HashMap<i32, BinArray>,
    pub bitmap_extension: Option<BinArrayBitmapExtension>,
    pub reserve_x: Option<u64>,
    pub reserve_y: Option<u64>,
    pub liquidity_usd: f64,
    pub is_state_keys_initialized: bool,
    pub last_updated: u64,
}

/// Pool update event from stream
#[derive(Debug, Clone)]
pub struct MeteoraDlmmPoolUpdate {
    pub slot: u64,
    pub transaction_index: Option<u64>,
    pub address: Pubkey,
    pub lbpair: LbPair,
    pub bin_arrays: Option<HashMap<i32, BinArray>>,
    pub bitmap_extension: Option<BinArrayBitmapExtension>,
    pub is_account_state_update: bool,
    pub pool_update_event_type: PoolUpdateEventType,
    pub additional_event_type: i32,
    pub last_updated: u64,
    pub reserve_x: Option<u64>,
    pub reserve_y: Option<u64>,
}

impl MeteoraDlmmPoolState {
    pub fn get_program_id() -> Pubkey {
        METEORA_DLMM_PROGRAM_ID
    }

    pub fn calculate_output_amount(
        &self,
        input_token: &Pubkey,
        input_amount: u64,
        _amm_config_fetcher: &dyn GetAmmConfig,
    ) -> u64 {
        if input_amount == 0 {
            return 0;
        }

        let swap_for_y = *input_token == self.lbpair.token_x_mint;

        // Convert to commons types
        let mut lb_pair = functions::to_commons_lb_pair(self);
        let bin_arrays = functions::get_commons_bin_arrays(self);

        // Convert Pubkey to anchor_lang::prelude::Pubkey
        let lb_pair_pubkey = anchor_lang::prelude::Pubkey::from(self.address.to_bytes());

        // Skip transfer fee calculation - use amount_in directly as transfer_fee_excluded_amount_in
        let mut amount_left = input_amount;
        let mut total_amount_out: u64 = 0;
        let mut total_fee: u64 = 0;

        println!(
            "DEBUG: calculate_output_amount START. Input: {}, SwapForY: {}",
            input_amount, swap_for_y
        );

        // Create Clock using anchor types
        let current_timestamp = self.last_updated;

        // Update references
        if let Err(e) = lb_pair.update_references(current_timestamp as i64) {
            println!("DEBUG: Error updating references: {:?}", e);
            log::debug!("Error updating references: {:?}", e);
            return 0;
        }

        // Convert bitmap_extension if present
        let bitmap_extension_commons = self
            .bitmap_extension
            .as_ref()
            .map(|ext| functions::to_commons_bitmap_extension(self, ext));

        println!("DEBUG: Total bin arrays in cache: {}", self.bin_arrays.len());
        println!("DEBUG: Bin array indices: {:?}", self.bin_arrays.keys().collect::<Vec<_>>());

        while amount_left > 0 {
            // Calculate which bin array contains the current active bin
            let current_bin_array_index = match meteora_dlmm_sdk::dlmm::accounts::BinArray::bin_id_to_bin_array_index(
                lb_pair.active_id,
            ) {
                Ok(idx) => idx as i32,
                Err(e) => {
                    println!("DEBUG: Error calculating bin array index: {:?}", e);
                    log::debug!("Error calculating bin array index: {:?}", e);
                    break;
                }
            };

            println!("DEBUG: Active bin ID: {}, Bin array index: {}", lb_pair.active_id, current_bin_array_index);

            // Look up the bin array directly in our HashMap by index
            let active_bin_array_raw = match self.bin_arrays.get(&current_bin_array_index) {
                Some(arr) => arr.clone(),
                None => {
                    println!(
                        "DEBUG: Bin array {} not in cache (partial quote). Cached indices: {:?}",
                        current_bin_array_index,
                        self.bin_arrays.keys().collect::<Vec<_>>()
                    );
                    log::debug!("Bin array {} not in cache", current_bin_array_index);
                    break; // Partial quote - bin array not in cache
                }
            };

            let mut active_bin_array = functions::get_commons_bin_array_from_raw(&active_bin_array_raw);

            println!(
                "DEBUG: Active Bin Array Found. Index: {}",
                active_bin_array.index
            );


            // Shift active bin if there's an empty gap
            let lb_pair_bin_array_index =
                match meteora_dlmm_sdk::dlmm::accounts::BinArray::bin_id_to_bin_array_index(
                    lb_pair.active_id,
                ) {
                    Ok(idx) => idx,
                    Err(e) => {
                        println!("DEBUG: Error getting bin array index: {:?}", e);
                        log::debug!("Error getting bin array index: {:?}", e);
                        return 0;
                    }
                };

            if i64::from(lb_pair_bin_array_index) != active_bin_array.index {
                println!(
                    "DEBUG: BinIndex mismatch: LbPair implies {}, Found {}",
                    lb_pair_bin_array_index, active_bin_array.index
                );
                if swap_for_y {
                    if let Ok((_, upper_bin_id)) =
                        meteora_dlmm_sdk::dlmm::accounts::BinArray::get_bin_array_lower_upper_bin_id(
                            active_bin_array.index as i32,
                        )
                    {
                        lb_pair.active_id = upper_bin_id;
                    }
                } else if let Ok((lower_bin_id, _)) =
                    meteora_dlmm_sdk::dlmm::accounts::BinArray::get_bin_array_lower_upper_bin_id(
                        active_bin_array.index as i32,
                    )
                {
                    lb_pair.active_id = lower_bin_id;
                }
            }

            loop {
                // Wait, skipping loop start debugging for now.
                // Just log around swap_quote_exact_in
                let is_within_range =
                    match active_bin_array.is_bin_id_within_range(lb_pair.active_id) {
                        Ok(within) => within,
                        Err(e) => {
                            println!("DEBUG: Error checking range: {:?}", e);
                            return 0;
                        }
                    };

                if !is_within_range || amount_left == 0 {
                    log::debug!(
                        "Bin ID {} is not within range or amount left is 0",
                        lb_pair.active_id
                    );
                    break;
                }

                if let Err(e) = lb_pair.update_volatility_accumulator() {
                    log::debug!("Error updating volatility accumulator: {:?}", e);
                    return 0;
                }

                let active_bin = match active_bin_array.get_bin_mut(lb_pair.active_id) {
                    Ok(bin) => bin,
                    Err(e) => {
                        log::debug!("Error getting active bin: {:?}", e);
                        return 0;
                    }
                };

                let price =
                    match active_bin.get_or_store_bin_price(lb_pair.active_id, lb_pair.bin_step) {
                        Ok(p) => p,
                        Err(e) => {
                            log::debug!("Error getting bin price: {:?}", e);
                            return 0;
                        }
                    };

                if !active_bin.is_empty(!swap_for_y) {
                    match active_bin.swap(amount_left, price, swap_for_y, &lb_pair, None) {
                        Ok(swap_result) => {
                            amount_left = match amount_left
                                .checked_sub(swap_result.amount_in_with_fees)
                            {
                                Some(val) => val,
                                None => {
                                    log::debug!("Math overflow subtracting amount_in_with_fees");
                                    return 0;
                                }
                            };

                            total_amount_out =
                                match total_amount_out.checked_add(swap_result.amount_out) {
                                    Some(val) => val,
                                    None => {
                                        log::debug!("Math overflow adding amount_out");
                                        return 0;
                                    }
                                };

                            total_fee = match total_fee.checked_add(swap_result.fee) {
                                Some(val) => val,
                                None => {
                                    log::debug!("Math overflow adding fee");
                                    return 0;
                                }
                            };
                        }
                        Err(e) => {
                            log::debug!("Error during swap: {:?}", e);
                            return 0;
                        }
                    }
                }

                if amount_left > 0 {
                    println!("DEBUG: Advancing. Amount left: {}, Current active_id: {}", amount_left, lb_pair.active_id);
                    if let Err(e) = lb_pair.advance_active_bin(swap_for_y) {
                        println!("DEBUG: Error advancing active bin: {:?}", e);
                        log::debug!("Error advancing active bin: {:?}", e);
                        return 0;
                    }
                    println!("DEBUG: After advance, new active_id: {}", lb_pair.active_id);
                }
            }
            println!("DEBUG: Inner loop ended. Amount left: {}", amount_left);
        }

        total_amount_out
    }

    /// Calculate token prices
    pub fn calculate_token_prices(
        &self,
        sol_price: f64,
        _token_x_decimals: u8,
        _token_y_decimals: u8,
    ) -> (f64, f64) {
        // Compute token prices based on active bin and bin step
        // Price formula: price = (1 + bin_step / BASIS_POINT_MAX) ^ active_id
        let basis_point_max: f64 = 10_000.0;
        let bin_step = self.lbpair.bin_step as f64;
        let active_id = self.lbpair.active_id;
        let price_x_in_y = (1.0 + bin_step / basis_point_max).powi(active_id);
        // Adjust for token decimals difference
        let decimal_scale = 10_f64.powi(_token_x_decimals as i32 - _token_y_decimals as i32);
        let adjusted_price = price_x_in_y * decimal_scale;
        // Determine if either token is SOL (native mint) using anchor_pubkey type
        let sol_mint_anchor =
            anchor_lang::prelude::Pubkey::from(spl_token::native_mint::ID.to_bytes());
        let token_x_pubkey =
            anchor_lang::prelude::Pubkey::from(self.lbpair.token_x_mint.to_bytes());
        let token_y_pubkey =
            anchor_lang::prelude::Pubkey::from(self.lbpair.token_y_mint.to_bytes());
        let token_x_is_sol = token_x_pubkey == sol_mint_anchor;
        let token_y_is_sol = token_y_pubkey == sol_mint_anchor;
        // Compute USD prices
        let (price_x_usd, price_y_usd) = if token_y_is_sol {
            // token Y is SOL
            (adjusted_price * sol_price, sol_price)
        } else if token_x_is_sol {
            // token X is SOL
            (sol_price, sol_price / adjusted_price)
        } else {
            // Neither token is SOL; return relative prices
            (adjusted_price, 1.0)
        };
        (price_x_usd, price_y_usd)
    }
}

use crate::pool_data_types::common::functions as common_functions;
use crate::pool_data_types::traits::BuildSwapInstruction;
use crate::types::SwapParams;
use async_trait::async_trait;
use borsh::BorshSerialize;
use solana_program::instruction::{AccountMeta, Instruction};
use spl_associated_token_account::get_associated_token_address_with_program_id;

#[async_trait]
impl BuildSwapInstruction for MeteoraDlmmPoolState {
    async fn build_swap_instruction(
        &self,
        params: &SwapParams,
        amm_config_fetcher: &dyn GetAmmConfig,
        _rpc_client: Option<&std::sync::Arc<solana_client::nonblocking::rpc_client::RpcClient>>,
    ) -> std::result::Result<Vec<Instruction>, String> {
        let input_mint = params.input_token.address;

        // Determine swap direction
        let swap_for_y = input_mint == self.lbpair.token_x_mint;

        // Determine token programs
        let token_x_program = common_functions::get_token_program(params.input_token.is_token_2022);
        let token_y_program =
            common_functions::get_token_program(params.output_token.is_token_2022);

        // Convert to anchor Pubkey for ATA derivation
        let user_wallet_anchor = common_functions::to_pubkey(&params.user_wallet);
        let token_x_mint_anchor = common_functions::to_pubkey(&self.lbpair.token_x_mint);
        let token_y_mint_anchor = common_functions::to_pubkey(&self.lbpair.token_y_mint);
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
        let (event_authority, _) =
            Pubkey::find_program_address(&[b"__event_authority"], &program_id);

        // Calculate minimum output with slippage
        let estimated_output =
            self.calculate_output_amount(&input_mint, params.input_amount, amm_config_fetcher);

        let min_amount_out =
            common_functions::calculate_slippage(estimated_output, params.slippage_bps)?;

        // Memo program address
        let memo_program = solana_sdk::pubkey!("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr");

        // Build accounts according to swap2 IDL specification
        let mut accounts = vec![
            AccountMeta::new(self.address, false), // 0: lb_pair
        ];

        // Derive and add bin_array_bitmap_extension
        // This is an optional account, but must always be present in the account list
        // If bitmap extension doesn't exist, we pass the program ID as a placeholder
        let lb_pair_anchor = common_functions::to_pubkey(&self.address);
        let (bitmap_extension_pda, _) = anchor_lang::prelude::Pubkey::find_program_address(
            &[b"bitmap", lb_pair_anchor.as_ref()],
            &common_functions::to_pubkey(&program_id),
        );

        // If we have bitmap extension data, use the derived PDA, otherwise use program ID as placeholder
        let bitmap_extension_account = if self.bitmap_extension.is_some() {
            common_functions::to_address(&bitmap_extension_pda)
        } else {
            program_id // Use program ID as placeholder for optional account
        };
        accounts.push(AccountMeta::new_readonly(bitmap_extension_account, false));

        // Continue with remaining accounts
        accounts.extend_from_slice(&[
            AccountMeta::new(self.lbpair.reserve_x, false),
            AccountMeta::new(self.lbpair.reserve_y, false),
            AccountMeta::new(common_functions::to_address(&user_token_in), false),
            AccountMeta::new(common_functions::to_address(&user_token_out), false),
            AccountMeta::new_readonly(self.lbpair.token_x_mint, false),
            AccountMeta::new_readonly(self.lbpair.token_y_mint, false),
            AccountMeta::new(self.lbpair.oracle, false),
            AccountMeta::new(program_id, false), // host_fee_in (optional, using program ID as placeholder)
            AccountMeta::new_readonly(params.user_wallet, true),
            AccountMeta::new_readonly(token_x_program, false),
            AccountMeta::new_readonly(token_y_program, false),
            AccountMeta::new_readonly(memo_program, false),
            AccountMeta::new_readonly(event_authority, false),
            AccountMeta::new_readonly(program_id, false),
        ]);

        // Get bin arrays needed for the swap
        let lb_pair_anchor = common_functions::to_pubkey(&self.address);
        let bitmap_extension_commons = self
            .bitmap_extension
            .as_ref()
            .map(|ext| functions::to_commons_bitmap_extension(self, ext));

        let bin_array_pubkeys = match meteora_dlmm_sdk::quote::get_bin_array_pubkeys_for_swap(
            lb_pair_anchor,
            &functions::to_commons_lb_pair(self),
            bitmap_extension_commons.as_ref(),
            swap_for_y,
            1, // Number of bin arrays to fetch
        ) {
            Ok(keys) => keys,
            Err(e) => {
                return Err(format!("Failed to get bin arrays for swap: {:?}", e));
            }
        };

        // Add bin arrays as remaining accounts
        for bin_array_pubkey in bin_array_pubkeys {
            accounts.push(AccountMeta::new(
                common_functions::to_address(&bin_array_pubkey),
                false,
            ));
        }

        // Build instruction data with correct discriminator
        let discriminator: [u8; 8] = [65, 75, 63, 76, 235, 91, 91, 136]; // swap2

        // RemainingAccountsInfo with empty slices (no Token2022 transfer hooks for now)
        #[derive(BorshSerialize)]
        struct RemainingAccountsSlice {
            accounts_type: u8,
            length: u8,
        }

        #[derive(BorshSerialize)]
        struct RemainingAccountsInfo {
            slices: Vec<RemainingAccountsSlice>,
        }

        let remaining_accounts_info = RemainingAccountsInfo {
            slices: vec![], // Empty for now - would contain transfer hook info for Token2022
        };

        let mut data = Vec::new();
        data.extend_from_slice(&discriminator);
        borsh::BorshSerialize::serialize(&params.input_amount, &mut data)
            .map_err(|e| e.to_string())?;
        borsh::BorshSerialize::serialize(&min_amount_out, &mut data).map_err(|e| e.to_string())?;
        borsh::BorshSerialize::serialize(&remaining_accounts_info, &mut data)
            .map_err(|e| e.to_string())?;

        let swap_ix = Instruction {
            program_id,
            accounts,
            data,
        };

        // Build instruction list
        let mut instructions = Vec::new();

        // Determine token programs for ATAs
        let is_x_token_2022 = params.input_token.is_token_2022;
        let is_y_token_2022 = params.output_token.is_token_2022;

        // Determine the correct token type for each ATA based on swap direction
        let input_is_token_2022 = if swap_for_y {
            is_x_token_2022
        } else {
            is_y_token_2022
        };
        let output_is_token_2022 = if swap_for_y {
            is_y_token_2022
        } else {
            is_x_token_2022
        };

        // Create ATA for input token (ensures account exists for transfer)
        instructions.push(common_functions::create_ata_instruction(
            params.user_wallet,
            common_functions::to_address(&user_token_in),
            input_mint,
            input_is_token_2022,
        ));

        // Create ATA for output token (ensures account exists to receive tokens)
        let output_mint = if swap_for_y {
            self.lbpair.token_y_mint
        } else {
            self.lbpair.token_x_mint
        };
        instructions.push(common_functions::create_ata_instruction(
            params.user_wallet,
            common_functions::to_address(&user_token_out),
            output_mint,
            output_is_token_2022,
        ));

        // Add swap instruction
        instructions.push(swap_ix);

        Ok(instructions)
    }
}
