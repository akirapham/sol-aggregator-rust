use std::collections::HashMap;
use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use solana_streamer_sdk::streaming::event_parser::protocols::orca_whirlpools::{
    parser::ORCA_WHIRLPOOL_PROGRAM_ID, types::TickArrayState, types::OracleState
};
use solana_client::nonblocking::rpc_client::RpcClient;
use std::sync::Arc;
use log;

use crate::{
    constants::is_base_token,
    pool_data_types::{
        PoolUpdateEventType, orca::fee_rate_manager::FeeRateManager, GetAmmConfig
    },
    utils::tokens_equal,
};

use crate::pool_data_types::orca::{
    math::*,
    state::*,
};

use orca_whirlpools_client::{
    get_oracle_address, get_tick_array_address, AccountsType, Oracle, RemainingAccountsInfo,
    RemainingAccountsSlice, SwapV2, SwapV2InstructionArgs, TickArray, Whirlpool,
};
use orca_whirlpools_core::{
    get_tick_array_start_tick_index, swap_quote_by_input_token, swap_quote_by_output_token,
    ExactInSwapQuote, ExactOutSwapQuote, TickArrayFacade, TickFacade, TICK_ARRAY_SIZE,
};
use orca_whirlpools_core::TransferFee;

use std::time::SystemTime;
use std::time::UNIX_EPOCH;
use std::error::Error;
use std::iter::zip;
use solana_sdk::account::Account as SolanaAccount;
use spl_token_2022::extension::transfer_fee::TransferFeeConfig;
use spl_token_2022::extension::{BaseStateWithExtensions, ExtensionType, StateWithExtensions};
use spl_token_2022::state::{Account, Mint};

// Whirlpool sqrt price limits (same as Raydium CLMM)
const MIN_SQRT_PRICE_X64: u128 = 4295048016;
const MAX_SQRT_PRICE_X64: u128 = 79226673521066979257578248091;

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
    pub liquidity: u128,
    pub liquidity_usd: f64,
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

    /// Calculate output amount using official Whirlpool swap loop logic
    /// 
    /// Implements the exact same amount_calculated accumulation as the official
    /// swap() function in /programs/whirlpool/src/manager/swap_manager.rs
    /// 
    /// Key features mirrored from official implementation:
    /// - Multi-step swap loop: while amount_remaining > 0 && curr_sqrt_price != limit
    /// - Dynamic fee rate calculation via FeeRateManager
    /// - Proper amount_calculated accumulation (output for exact-input mode)
    /// - Tick crossing and liquidity updates (simplified for single-tick scenarios)
    pub async fn calculate_output_amount(
        &self,
        input_token: &Pubkey,
        input_amount: u64,
        _amm_config_fetcher: Arc<dyn GetAmmConfig>,
        rpc_client: &RpcClient,
    ) -> u64 {
        // Return 0 on any errors to avoid stack overflow
        let result = self.calculate_output_amount_internal(
            input_token,
            input_amount,
            rpc_client,
        )
        .await;
        
        match result {
            Ok(amount) => amount,
            Err(_e) => 0, // Return 0 on any error instead of unwrapping
        }
    }

    async fn calculate_output_amount_internal(
        &self,
        input_token: &Pubkey,
        input_amount: u64,
        rpc_client: &RpcClient,
    ) -> Result<u64, Box<dyn Error>> {
        let whirlpool_address = self.address;
        let slippage_tolerance_bps = 50;
        
        let whirlpool_info = rpc_client.get_account(&whirlpool_address).await?;
        let whirlpool = Whirlpool::from_bytes(&whirlpool_info.data)?;
        let (token_a, _) = (self.token_mint_a, self.token_mint_b);
        let specified_token_a = tokens_equal(input_token, &token_a);
        let mint_infos = rpc_client
            .get_multiple_accounts(&[whirlpool.token_mint_a, whirlpool.token_mint_b])
            .await?;

        let mint_a_info = mint_infos[0]
            .as_ref()
            .ok_or(format!("Mint a not found: {}", whirlpool.token_mint_a))?;

        let mint_b_info = mint_infos[1]
            .as_ref()
            .ok_or(format!("Mint b not found: {}", whirlpool.token_mint_b))?;

        let oracle_address = get_oracle_address(&whirlpool_address)?.0;
        let oracle_info = rpc_client.get_account(&oracle_address).await;
        let oracle = oracle_info.ok().and_then(|acc| Oracle::from_bytes(&acc.data).ok());

        let current_epoch = rpc_client.get_epoch_info().await?.epoch;
        let transfer_fee_a = get_current_transfer_fee(Some(mint_a_info), current_epoch);
        let transfer_fee_b = get_current_transfer_fee(Some(mint_b_info), current_epoch);

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)?
            .as_secs();

        let tick_arrays = fetch_tick_arrays_or_default(rpc_client, whirlpool_address, &whirlpool).await?;

        
        let quote = tokio::task::spawn_blocking(move || {
            swap_quote_by_input_token(
                input_amount,
                specified_token_a,
                slippage_tolerance_bps,
                whirlpool.into(),
                oracle.map(|o| o.into()),
                tick_arrays.map(|x| x.1).into(),
                timestamp,
                transfer_fee_a,
                transfer_fee_b,
            )
        })
        .await
        .map_err(|e| format!("spawn_blocking error: {}", e))?
        .map_err(|e| format!("swap quote error: {:?}", e))?;
        
        Ok(quote.token_est_out)
    }

       /// Helper function to get tick array start index
    fn get_tick_array_start_tick_index(current_tick: i32, tick_spacing: u16) -> i32 {
        let tick_spacing = tick_spacing as i32;
        let array_size = 72; // TICK_ARRAY_SIZE
        let range_size = tick_spacing * array_size;
        (current_tick / range_size) * range_size
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

/// Result of a single swap step computation
#[allow(unused)]
struct SwapStepResult {
    amount_in: u64,
    amount_out: u64,
    next_sqrt_price: u128,
    fee_amount: u64,
}

async fn fetch_tick_arrays_or_default(
    rpc_client: &RpcClient,
    whirlpool_address: Pubkey,
    whirlpool: &Whirlpool,
) -> Result<[(Pubkey, TickArrayFacade); 5], Box<dyn Error>> {
    let tick_array_start_index =
        get_tick_array_start_tick_index(whirlpool.tick_current_index, whirlpool.tick_spacing);
    let offset = whirlpool.tick_spacing as i32 * TICK_ARRAY_SIZE as i32;

    let tick_array_indexes = [
        tick_array_start_index,
        tick_array_start_index + offset,
        tick_array_start_index + offset * 2,
        tick_array_start_index - offset,
        tick_array_start_index - offset * 2,
    ];

    let tick_array_addresses: Vec<Pubkey> = tick_array_indexes
        .iter()
        .map(|&x| get_tick_array_address(&whirlpool_address, x).map(|y| y.0))
        .collect::<Result<Vec<Pubkey>, _>>()?;

    let tick_array_infos = rpc_client.get_multiple_accounts(&tick_array_addresses).await?;

    let maybe_tick_arrays: Vec<Option<TickArrayFacade>> = tick_array_infos
        .iter()
        .map(|x| x.as_ref().and_then(|y| TickArray::from_bytes(&y.data).ok()))
        .map(|x| x.map(|y| y.into()))
        .collect();

    let tick_arrays: Vec<TickArrayFacade> = maybe_tick_arrays
        .iter()
        .enumerate()
        .map(|(i, x)| x.unwrap_or(uninitialized_tick_array(tick_array_indexes[i])))
        .collect::<Vec<TickArrayFacade>>();

    let result: [(Pubkey, TickArrayFacade); 5] = zip(tick_array_addresses, tick_arrays)
        .collect::<Vec<(Pubkey, TickArrayFacade)>>()
        .try_into()
        .map_err(|_| "Failed to convert tick arrays to array".to_string())?;

    Ok(result)
}

async fn fetch_oracle(
    rpc_client: &RpcClient,
    oracle_address: Pubkey,
    whirlpool: &Whirlpool,
) -> Result<Option<Oracle>, Box<dyn Error>> {
    // no need to fetch oracle for non-adaptive fee whirlpools
    if whirlpool.tick_spacing == u16::from_le_bytes(whirlpool.fee_tier_index_seed) {
        return Ok(None);
    }
    match rpc_client.get_account(&oracle_address).await {
        Ok(oracle_info) => {
            match Oracle::from_bytes(&oracle_info.data) {
                Ok(oracle) => Ok(Some(oracle)),
                Err(e) => {
                    log::warn!("Failed to deserialize oracle: {}", e);
                    Ok(None)
                }
            }
        }
        Err(e) => {
            log::warn!("Failed to fetch oracle: {}", e);
            Ok(None)
        }
    }
}

pub fn get_current_transfer_fee(
    mint_account_info: Option<&SolanaAccount>,
    current_epoch: u64,
) -> Option<TransferFee> {
    let token_mint_data = &mint_account_info?.data;
    let token_mint_unpacked = StateWithExtensions::<Mint>::unpack(token_mint_data).ok()?;

    if let Ok(transfer_fee_config) = token_mint_unpacked.get_extension::<TransferFeeConfig>() {
        let fee = transfer_fee_config.get_epoch_fee(current_epoch);
        return Some(TransferFee {
            fee_bps: fee.transfer_fee_basis_points.into(),
            max_fee: fee.maximum_fee.into(),
        });
    }

    None
}

fn uninitialized_tick_array(start_tick_index: i32) -> TickArrayFacade {
    TickArrayFacade {
        start_tick_index,
        ticks: [TickFacade::default(); TICK_ARRAY_SIZE],
    }
}