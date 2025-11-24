use crate::{
    constants::is_base_token,
    pool_data_types::{GetAmmConfig, PoolUpdateEventType},

};
use borsh::{BorshDeserialize, BorshSerialize};

use orca_whirlpools_core::{
    TickArrayFacade, TickFacade,
    TransferFee, TICK_ARRAY_SIZE,
};
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use solana_streamer_sdk::streaming::event_parser::protocols::orca_whirlpools::{
    parser::ORCA_WHIRLPOOL_PROGRAM_ID, types::OracleState, types::TickArrayState,
};
use std::collections::HashMap;
use std::sync::Arc;

use solana_sdk::account::Account as SolanaAccount;
use spl_token_2022::extension::transfer_fee::TransferFeeConfig;
use spl_token_2022::extension::{BaseStateWithExtensions, StateWithExtensions};
use spl_token_2022::state::Mint;
use std::error::Error;

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
    pub is_token_mint_a_2022: bool, // Whether token_mint_a uses Token-2022 program
    pub is_token_mint_b_2022: bool, // Whether token_mint_b uses Token-2022 program
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
        // Return 0 on any errors to avoid stack overflow
        let result = self
            .calculate_output_amount_internal(input_token, input_amount)
            .await;

        match result {
            Ok(amount) => amount,
            Err(_e) => 0, // Return 0 on any error instead of unwrapping
        }
    }

    async fn calculate_output_amount_internal(
        &self,
        _input_token: &Pubkey,
        _input_amount: u64,
    ) -> Result<u64, Box<dyn Error>> {
        // let whirlpool_address = self.address;
        // let slippage_tolerance_bps = 50;

        // let whirlpool_info = rpc.get_account(&whirlpool_address).await?;

        // let whirlpool = Whirlpool::from_bytes(&whirlpool_info.data)?;
        // let specified_input = swap_type == SwapType::ExactIn;
        // let specified_token_a = specified_mint == whirlpool.token_mint_a;
        // let a_to_b = specified_token_a == specified_input;

        // let tick_arrays = fetch_tick_arrays_or_default(rpc, whirlpool_address, &whirlpool).await?;

        // let mint_infos = rpc
        //     .get_multiple_accounts(&[whirlpool.token_mint_a, whirlpool.token_mint_b])
        //     .await?;

        // let mint_a_info = mint_infos[0]
        //     .as_ref()
        //     .ok_or(format!("Mint a not found: {}", whirlpool.token_mint_a))?;

        // let mint_b_info = mint_infos[1]
        //     .as_ref()
        //     .ok_or(format!("Mint b not found: {}", whirlpool.token_mint_b))?;

        // let oracle_address = get_oracle_address(&whirlpool_address)?.0;
        // let oracle = fetch_oracle(rpc, oracle_address, &whirlpool).await?;

        // let current_epoch = rpc.get_epoch_info().await?.epoch;
        // let transfer_fee_a = get_current_transfer_fee(Some(mint_a_info), current_epoch);
        // let transfer_fee_b = get_current_transfer_fee(Some(mint_b_info), current_epoch);

        // let timestamp = SystemTime::now()
        //     .duration_since(UNIX_EPOCH)
        //     .unwrap()
        //     .as_secs();
        // let trade_enable_timestamp = oracle
        //     .as_ref()
        //     .map(|x| x.trade_enable_timestamp)
        //     .unwrap_or(0);

        // let quote = match swap_type {
        //     SwapType::ExactIn => SwapQuote::ExactIn(swap_quote_by_input_token(
        //         amount,
        //         specified_token_a,
        //         slippage_tolerance_bps,
        //         whirlpool.clone().into(),
        //         oracle.map(|oracle| oracle.into()),
        //         tick_arrays.map(|x| x.1).into(),
        //         timestamp,
        //         transfer_fee_a,
        //         transfer_fee_b,
        //     )?),
        //     SwapType::ExactOut => SwapQuote::ExactOut(swap_quote_by_output_token(
        //         amount,
        //         specified_token_a,
        //         slippage_tolerance_bps,
        //         whirlpool.clone().into(),
        //         oracle.map(|oracle| oracle.into()),
        //         tick_arrays.map(|x| x.1).into(),
        //         timestamp,
        //         transfer_fee_a,
        //         transfer_fee_b,
        //     )?),
        // };
        // Ok(quote.token_est_out)
        Ok(0)

        // let whirlpool_address = self.address;
        // let slippage_tolerance_bps = 50;

        // let whirlpool_addr = Address::from_str(&whirlpool_address.to_string()).unwrap();
        // let whirlpool_info = rpc_client.get_account(&whirlpool_address).await?;
        // let whirlpool = Whirlpool::from_bytes(&whirlpool_info.data)?;
        // let (token_a, _) = (self.token_mint_a, self.token_mint_b);
        // let specified_token_a = tokens_equal(input_token, &token_a);
        // let mint_infos = rpc_client
        //     .get_multiple_accounts(&[Pubkey::from_str(&whirlpool.token_mint_a.to_string()).unwrap(), Pubkey::from_str(&whirlpool.token_mint_b.to_string()).unwrap()])
        //     .await?;

        // let mint_a_info = mint_infos[0]
        //     .as_ref()
        //     .ok_or(format!("Mint a not found: {}", whirlpool.token_mint_a))?;

        // let mint_b_info = mint_infos[1]
        //     .as_ref()
        //     .ok_or(format!("Mint b not found: {}", whirlpool.token_mint_b))?;

        //     //             let whirlpool_addr = Address::from_str(&whirlpool_address.to_string()).unwrap();
        //     // get_tick_array_address(&whirlpool_addr, x)
        //     //     .map(|(addr, _)| Pubkey::from_str(&addr.to_string()).unwrap())

        // let oracle_address = get_oracle_address(&whirlpool_addr)?.0;
        // let oracle_info = rpc_client.get_account(&Pubkey::from_str(&oracle_address.to_string()).unwrap()).await;
        // let oracle = oracle_info.ok().and_then(|acc| Oracle::from_bytes(&acc.data).ok());

        // let current_epoch = rpc_client.get_epoch_info().await?.epoch;
        // let transfer_fee_a = get_current_transfer_fee(Some(mint_a_info), current_epoch);
        // let transfer_fee_b = get_current_transfer_fee(Some(mint_b_info), current_epoch);

        // let timestamp = SystemTime::now()
        //     .duration_since(UNIX_EPOCH)?
        //     .as_secs();

        // let tick_arrays = fetch_tick_arrays_or_default(rpc_client, whirlpool_address, &whirlpool).await?;

        // let quote = tokio::task::spawn_blocking(move || {
        //     swap_quote_by_input_token(
        //         input_amount,
        //         specified_token_a,
        //         slippage_tolerance_bps,
        //         whirlpool.into(),
        //         oracle.map(|o| o.into()),
        //         tick_arrays.map(|x| x.1).into(),
        //         timestamp,
        //         transfer_fee_a,
        //         transfer_fee_b,
        //     )
        // })
        // .await
        // .map_err(|e| format!("spawn_blocking error: {}", e))?
        // .map_err(|e| format!("swap quote error: {:?}", e))?;

        // Ok(quote.token_est_out)
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

// async fn fetch_tick_arrays_or_default(
//     rpc_client: &RpcClient,
//     whirlpool_address: Pubkey,
//     whirlpool: &Whirlpool,
// ) -> Result<[(Pubkey, TickArrayFacade); 5], Box<dyn Error>> {
//     let tick_array_start_index =
//         get_tick_array_start_tick_index(whirlpool.tick_current_index, whirlpool.tick_spacing);
//     let offset = whirlpool.tick_spacing as i32 * TICK_ARRAY_SIZE as i32;

//     let tick_array_indexes = [
//         tick_array_start_index,
//         tick_array_start_index + offset,
//         tick_array_start_index + offset * 2,
//         tick_array_start_index - offset,
//         tick_array_start_index - offset * 2,
//     ];

//     let tick_array_addresses: Vec<Pubkey> = tick_array_indexes
//         .iter()
//         .map(|&x| get_tick_array_address(&whirlpool_address, x).map(|y| y.0))
//         .collect::<Result<Vec<Pubkey>, _>>()?;

//     let tick_array_infos = rpc.get_multiple_accounts(&tick_array_addresses).await?;

//     let maybe_tick_arrays: Vec<Option<TickArrayFacade>> = tick_array_infos
//         .iter()
//         .map(|x| x.as_ref().and_then(|y| TickArray::from_bytes(&y.data).ok()))
//         .map(|x| x.map(|y| y.into()))
//         .collect();

//     let tick_arrays: Vec<TickArrayFacade> = maybe_tick_arrays
//         .iter()
//         .enumerate()
//         .map(|(i, x)| x.unwrap_or(uninitialized_tick_array(tick_array_indexes[i])))
//         .collect::<Vec<TickArrayFacade>>();

//     let result: [(Pubkey, TickArrayFacade); 5] = zip(tick_array_addresses, tick_arrays)
//         .collect::<Vec<(Pubkey, TickArrayFacade)>>()
//         .try_into()
//         .map_err(|_| "Failed to convert tick arrays to array".to_string())?;

//     Ok(result)
// }

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
