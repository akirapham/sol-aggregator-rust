use serde::{Deserialize, Serialize};
use sol_trade_sdk::utils::calc::bonk::{
    get_buy_token_amount_from_sol_amount, get_sell_sol_amount_from_token_amount,
};
use solana_sdk::pubkey::Pubkey;
use solana_streamer_sdk::streaming::event_parser::protocols::bonk::parser::BONK_PROGRAM_ID;

use crate::{
    pool_data_types::{GetAmmConfig, PoolUpdateEventType},
    utils::{get_sol_mint, tokens_equal},
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
        let is_buy = tokens_equal(input_token, &self.quote_mint);
        if is_buy {
            get_buy_token_amount_from_sol_amount(
                input_amount,
                self.base_reserve as u128,
                self.quote_reserve as u128,
                self.real_base as u128,
                self.real_quote as u128,
                0,
            )
        } else {
            get_sell_sol_amount_from_token_amount(
                input_amount,
                self.base_reserve as u128,
                self.quote_reserve as u128,
                self.real_base as u128,
                self.real_quote as u128,
                0,
            )
        }
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
use solana_sdk::instruction::Instruction;
// use spl_associated_token_account; // Removed as SDK likely handles this or we rely on SDK output

use crate::utils::utils_functions::replace_key_in_instructions;
use sol_trade_sdk::common::gas_fee_strategy::GasFeeStrategy;
use sol_trade_sdk::instruction::bonk::BonkInstructionBuilder;
use sol_trade_sdk::swqos::TradeType;
use sol_trade_sdk::trading::core::params::{BonkParams, SwapParams as SdkSwapParams};
use sol_trade_sdk::trading::core::traits::InstructionBuilder;
use solana_sdk::signature::{Keypair, Signer};
use std::sync::Arc;

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
        let dummy_keypair = Keypair::new();
        let dummy_payer = Arc::new(dummy_keypair);

        // Calculate minimum output based on slippage
        let estimated_output = self.calculate_output_amount(
            &params.input_token.address,
            params.input_amount,
            amm_config_fetcher,
        );

        // Note: BonkParams needs real_base/quote which are u64 in struct but u128 in BonkParams
        // The struct has: pub base_reserve: u64, pub quote_reserve: u64, pub real_base: u64, pub real_quote: u64

        // We need to derive ATAs for platform fee wallet and creator for BonkParams
        // Assuming fees are in Quote token (mostly USDC/SOL).
        let quote_mint = self.quote_mint;

        // Recalculate ATAs for BonkParams
        // spl_associated_token_account expects Anchor Pubkeys
        let wallet_anchor = anchor_lang::prelude::Pubkey::from(self.platform_fee_wallet.to_bytes());
        let quote_anchor = anchor_lang::prelude::Pubkey::from(quote_mint.to_bytes());
        let creator_anchor = anchor_lang::prelude::Pubkey::from(self.creator.to_bytes());

        let platform_fee_wallet_ata = spl_associated_token_account::get_associated_token_address(
            &wallet_anchor,
            &quote_anchor,
        );
        let creator_ata = spl_associated_token_account::get_associated_token_address(
            &creator_anchor,
            &quote_anchor,
        );
        let protocol_params = Box::new(BonkParams {
            virtual_base: self.base_reserve as u128,
            virtual_quote: self.quote_reserve as u128,
            real_base: self.real_base as u128,
            real_quote: self.real_quote as u128,
            pool_state: self.address.to_bytes().into(),
            base_vault: self.base_vault.to_bytes().into(),
            quote_vault: self.quote_vault.to_bytes().into(),
            mint_token_program: spl_token::id().to_bytes().into(),
            platform_config: self.platform_config.to_bytes().into(),
            platform_associated_account: platform_fee_wallet_ata.to_bytes().into(),
            creator_associated_account: creator_ata.to_bytes().into(),
            global_config: self.global_config.to_bytes().into(),
        });

        let amount_out = functions::calculate_slippage(estimated_output, params.slippage_bps)?;

        let trade_type = if params.input_token.address == self.quote_mint {
            TradeType::Buy
        } else {
            TradeType::Sell
        };

        let sdk_params = SdkSwapParams {
            rpc: rpc_client.cloned(),
            payer: dummy_payer.clone(),
            trade_type,
            input_mint: params.input_token.address,
            input_token_program: None,
            output_mint: params.output_token.address,
            output_token_program: None,
            input_amount: Some(params.input_amount),
            slippage_basis_points: Some(params.slippage_bps as u64),
            address_lookup_table_account: None,
            recent_blockhash: None,
            data_size_limit: 200_000,
            wait_transaction_confirmed: false,
            protocol_params,
            open_seed_optimize: false,
            swqos_clients: vec![],
            middleware_manager: None,
            durable_nonce: None,
            with_tip: false,
            create_input_mint_ata: false,
            close_input_mint_ata: false,
            create_output_mint_ata: true,
            close_output_mint_ata: false,
            fixed_output_amount: Some(amount_out),
            gas_fee_strategy: GasFeeStrategy::new(),
            simulate: false,
        };

        let builder = BonkInstructionBuilder;

        // Determine Swap Direction
        // If query input == quote_mint => Buy (Quote -> Base)
        // If query input == base_mint => Sell (Base -> Quote)

        let instructions_future = if params.input_token.address == self.quote_mint {
            builder.build_buy_instructions(&sdk_params)
        } else {
            builder.build_sell_instructions(&sdk_params)
        };

        let mut instructions = instructions_future.await.map_err(|e| e.to_string())?;

        // Patch Instructions
        replace_key_in_instructions(
            &mut instructions,
            &dummy_payer.pubkey(),
            &params.user_wallet,
        );

        // Replace Input Token ATA (Source)
        let dummy_input_ata_anchor = spl_associated_token_account::get_associated_token_address(
            &sdk_to_anchor(&dummy_payer.pubkey()),
            &sdk_to_anchor(&params.input_token.address),
        );
        let user_input_ata_anchor = spl_associated_token_account::get_associated_token_address(
            &sdk_to_anchor(&params.user_wallet),
            &sdk_to_anchor(&params.input_token.address),
        );

        let dummy_input_ata = anchor_to_sdk(&dummy_input_ata_anchor);
        let user_input_ata = anchor_to_sdk(&user_input_ata_anchor);

        replace_key_in_instructions(&mut instructions, &dummy_input_ata, &user_input_ata);

        // Replace Output Token ATA (Destination)
        let dummy_output_ata_anchor = spl_associated_token_account::get_associated_token_address(
            &sdk_to_anchor(&dummy_payer.pubkey()),
            &sdk_to_anchor(&params.output_token.address),
        );
        let user_output_ata_anchor = spl_associated_token_account::get_associated_token_address(
            &sdk_to_anchor(&params.user_wallet),
            &sdk_to_anchor(&params.output_token.address),
        );

        let dummy_output_ata = anchor_to_sdk(&dummy_output_ata_anchor);
        let user_output_ata = anchor_to_sdk(&user_output_ata_anchor);

        replace_key_in_instructions(&mut instructions, &dummy_output_ata, &user_output_ata);

        Ok(instructions)
    }
}

// Helper functions for type conversion
fn sdk_to_anchor(pubkey: &Pubkey) -> anchor_lang::prelude::Pubkey {
    anchor_lang::prelude::Pubkey::from(pubkey.to_bytes())
}

fn anchor_to_sdk(pubkey: &anchor_lang::prelude::Pubkey) -> Pubkey {
    Pubkey::new_from_array(pubkey.to_bytes())
}
