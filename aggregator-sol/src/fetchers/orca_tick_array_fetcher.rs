use anyhow::{anyhow, Result};
use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use solana_streamer_sdk::streaming::event_parser::protocols::orca_whirlpools::parser::ORCA_WHIRLPOOL_PROGRAM_ID;
use std::sync::Arc;

use crate::pool_data_types::orca_whirlpool::WhirlpoolPoolState;

const TICK_ARRAY_SIZE: usize = 88; // For array declarations
const TICK_ARRAY_SIZE_I32: i32 = 88; // For arithmetic
const TICK_ARRAY_SEED: &str = "tick_array";

// Tick limits from Whirlpool protocol
const MIN_TICK_INDEX: i32 = -443636;
const MAX_TICK_INDEX: i32 = 443636;

#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct AdaptiveFeeConstants {
    pub filter_period: u16,
    pub decay_period: u16,
    pub reduction_factor: u16,
    pub adaptive_fee_control_factor: u32,
    pub max_volatility_accumulator: u32,
    pub tick_group_size: u16,
    pub major_swap_threshold_ticks: u16,
    pub reserved: [u8; 16],
}

#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Oracle {
    pub discriminator: [u8; 8],
    #[cfg_attr(feature = "serde", serde(with = "serde_with::As::<serde_with::DisplayFromStr>"))]
    pub whirlpool: Pubkey,
    pub trade_enable_timestamp: u64,
    pub adaptive_fee_constants: AdaptiveFeeConstants,
    pub adaptive_fee_variables: AdaptiveFeeVariables,
    #[cfg_attr(feature = "serde", serde(with = "serde_with::As::<serde_with::Bytes>"))]
    pub reserved: [u8; 128],
}

#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct AdaptiveFeeVariables {
    pub last_reference_update_timestamp: u64,
    pub last_major_swap_timestamp: u64,
    pub volatility_reference: u32,
    pub tick_group_index_reference: i32,
    pub volatility_accumulator: u32,
    pub reserved: [u8; 16],
}


#[allow(unused)]
#[derive(Clone, Debug)]
pub struct OrcaTickArrayInfo {
    pub start_tick_index: i32,
    pub address: Pubkey,
    pub account_data: Option<Vec<u8>>,
}

/// Represents an individual tick in Orca Whirlpool (for fixed arrays)
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshDeserialize, Copy, Default)]
pub struct OrcaTickState {
    pub initialized: bool,
    pub liquidity_net: i128,
    pub liquidity_gross: u128,
    pub fee_growth_outside_a: u128,
    pub fee_growth_outside_b: u128,
    #[serde(with = "serde_big_array::BigArray")]
    pub reward_growths_outside: [u128; 3],
}

/// Dynamic tick data (for initialized ticks in dynamic arrays)
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshDeserialize, Copy)]
pub struct DynamicTickData {
    pub liquidity_net: i128,
    pub liquidity_gross: u128,
    pub fee_growth_outside_a: u128,
    pub fee_growth_outside_b: u128,
    #[serde(with = "serde_big_array::BigArray")]
    pub reward_growths_outside: [u128; 3],
}

/// Dynamic tick enum - either uninitialized (1 byte) or initialized (113 bytes)
#[derive(Clone, Debug, PartialEq, Eq, BorshDeserialize)]
pub enum DynamicTick {
    Uninitialized,
    Initialized(DynamicTickData),
}

impl DynamicTick {
    pub fn to_tick_state(&self) -> OrcaTickState {
        match self {
            DynamicTick::Uninitialized => OrcaTickState::default(),
            DynamicTick::Initialized(data) => OrcaTickState {
                initialized: true,
                liquidity_net: data.liquidity_net,
                liquidity_gross: data.liquidity_gross,
                fee_growth_outside_a: data.fee_growth_outside_a,
                fee_growth_outside_b: data.fee_growth_outside_b,
                reward_growths_outside: data.reward_growths_outside,
            },
        }
    }
}

/// Represents a fixed-size tick array in Orca Whirlpool (88 ticks)
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshDeserialize)]
pub struct FixedOrcaTickArrayState {
    #[serde(with = "serde_big_array::BigArray")]
    pub discriminator: [u8; 8],
    pub start_tick_index: i32,
    #[serde(with = "serde_big_array::BigArray")]
    pub ticks: [OrcaTickState; TICK_ARRAY_SIZE],
    pub whirlpool: Pubkey,
}

/// Represents a dynamic tick array in Orca Whirlpool (variable size with bitmap)
/// Note: We don't use BorshDeserialize for the whole struct because ticks need custom parsing
#[allow(unused)]
#[derive(Clone, Debug)]
pub struct DynamicOrcaTickArrayState {
    pub discriminator: [u8; 8],
    pub start_tick_index: i32,
    pub whirlpool: Pubkey,
    pub tick_bitmap: u128,
    pub ticks: Vec<DynamicTick>, // Exactly 88 ticks, variable size encoding
}

impl DynamicOrcaTickArrayState {
    /// Manually deserialize from account data
    pub fn deserialize_from_account_data(data: &[u8]) -> Result<Self, std::io::Error> {
        if data.len() < 60 {
            // 8 (discriminator) + 4 (start_tick) + 32 (whirlpool) + 16 (bitmap) = 60
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Account data too small",
            ));
        }

        let mut offset = 0;

        // Read discriminator
        let discriminator: [u8; 8] = data[offset..offset + 8].try_into().map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "Bad discriminator")
        })?;
        offset += 8;

        // Read start_tick_index
        let start_tick_index =
            i32::from_le_bytes(data[offset..offset + 4].try_into().map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, "Bad start_tick_index")
            })?);
        offset += 4;

        // Read whirlpool pubkey
        let whirlpool = Pubkey::try_from(&data[offset..offset + 32]).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "Bad whirlpool pubkey")
        })?;
        offset += 32;

        // Read tick_bitmap
        let tick_bitmap =
            u128::from_le_bytes(data[offset..offset + 16].try_into().map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, "Bad tick_bitmap")
            })?);
        offset += 16;

        // Read exactly 88 ticks
        let mut ticks = Vec::with_capacity(TICK_ARRAY_SIZE);
        for _ in 0..TICK_ARRAY_SIZE {
            if offset >= data.len() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "Not enough data for ticks",
                ));
            }

            // Deserialize DynamicTick from remaining data
            let mut tick_data = &data[offset..];
            let tick = DynamicTick::deserialize(&mut tick_data)?;

            // Calculate how many bytes were consumed
            let consumed = data.len() - offset - tick_data.len();
            offset += consumed;

            ticks.push(tick);
        }

        Ok(Self {
            discriminator,
            start_tick_index,
            whirlpool,
            tick_bitmap,
            ticks,
        })
    }
}

/// Enum to represent either fixed or dynamic tick array
#[derive(Clone, Debug)]
pub enum OrcaTickArrayState {
    Fixed(FixedOrcaTickArrayState),
    Dynamic(DynamicOrcaTickArrayState),
}

#[allow(unused)]
impl OrcaTickArrayState {
    pub fn start_tick_index(&self) -> i32 {
        match self {
            Self::Fixed(arr) => arr.start_tick_index,
            Self::Dynamic(arr) => arr.start_tick_index,
        }
    }

    pub fn whirlpool(&self) -> Pubkey {
        match self {
            Self::Fixed(arr) => arr.whirlpool,
            Self::Dynamic(arr) => arr.whirlpool,
        }
    }

    pub fn ticks(&self) -> Vec<OrcaTickState> {
        match self {
            Self::Fixed(arr) => arr.ticks.to_vec(),
            Self::Dynamic(arr) => arr.ticks.iter().map(|t| t.to_tick_state()).collect(),
        }
    }

    pub fn is_dynamic(&self) -> bool {
        matches!(self, Self::Dynamic(_))
    }

    pub fn tick_bitmap(&self) -> Option<u128> {
        match self {
            Self::Fixed(_) => None,
            Self::Dynamic(arr) => Some(arr.tick_bitmap),
        }
    }
}

pub struct OrcaTickArrayFetcher {
    rpc_client: Arc<RpcClient>,
    pub program_id: Pubkey,
}

#[allow(unused)]
impl OrcaTickArrayFetcher {
    pub fn new(rpc_client: Arc<RpcClient>, program_id: Pubkey) -> Self {
        Self {
            rpc_client,
            program_id,
        }
    }

    /// Fetch a single tick array by address
    pub async fn fetch_tick_array(&self, address: Pubkey) -> Result<OrcaTickArrayState> {
        let account = self
            .rpc_client
            .get_account(&address)
            .await
            .map_err(|e| anyhow!("Failed to fetch tick array account: {}", e))?;

        if account.owner != self.program_id {
            return Err(anyhow!("Account not owned by Orca Whirlpool program"));
        }

        // Account structure: 8-byte discriminator + data
        if account.data.len() < 8 {
            return Err(anyhow!("Account data too small to contain discriminator"));
        }

        let discriminator = &account.data[0..8];

        // Fixed tick array discriminator: [69, 97, 189, 190, 110, 7, 66, 187]
        let fixed_discriminator: [u8; 8] = [69, 97, 189, 190, 110, 7, 66, 187];
        // Dynamic tick array discriminator: [17, 216, 246, 142, 225, 199, 218, 56]
        let dynamic_discriminator: [u8; 8] = [17, 216, 246, 142, 225, 199, 218, 56];

        if discriminator == fixed_discriminator {
            let tick_array: FixedOrcaTickArrayState =
                BorshDeserialize::deserialize(&mut &account.data[..])?;
            Ok(OrcaTickArrayState::Fixed(tick_array))
        } else if discriminator == dynamic_discriminator {
            let tick_array =
                DynamicOrcaTickArrayState::deserialize_from_account_data(&account.data)?;
            Ok(OrcaTickArrayState::Dynamic(tick_array))
        } else {
            Err(anyhow!("Unknown tick array discriminator"))
        }
    }

    /// Fetch multiple tick arrays by their addresses
    pub async fn fetch_multiple_tick_arrays(
        &self,
        addresses: Vec<Pubkey>,
    ) -> Result<Vec<OrcaTickArrayState>> {
        let accounts = self
            .rpc_client
            .get_multiple_accounts(&addresses)
            .await
            .map_err(|e| anyhow!("Failed to fetch multiple tick array accounts: {}", e))?;

        let mut tick_arrays = Vec::new();

        for (idx, account_opt) in accounts.iter().enumerate() {
            match account_opt {
                Some(account) => {
                    if account.owner != self.program_id {
                        log::warn!(
                            "Account {} not owned by Orca Whirlpool program, skipping",
                            addresses[idx]
                        );
                        continue;
                    }

                    if account.data.len() < 8 {
                        log::warn!("Account {} data too small, skipping", addresses[idx]);
                        continue;
                    }

                    let discriminator = &account.data[0..8];
                    let fixed_discriminator: [u8; 8] = [69, 97, 189, 190, 110, 7, 66, 187];
                    let dynamic_discriminator: [u8; 8] = [17, 216, 246, 142, 225, 199, 218, 56];

                    if discriminator == fixed_discriminator {
                        match BorshDeserialize::deserialize(&mut &account.data[..]) {
                            Ok(tick_array) => {
                                tick_arrays.push(OrcaTickArrayState::Fixed(tick_array));
                            }
                            Err(e) => {
                                log::warn!(
                                    "Failed to deserialize fixed tick array {}: {}",
                                    addresses[idx],
                                    e
                                );
                            }
                        }
                    } else if discriminator == dynamic_discriminator {
                        match DynamicOrcaTickArrayState::deserialize_from_account_data(
                            &account.data,
                        ) {
                            Ok(tick_array) => {
                                tick_arrays.push(OrcaTickArrayState::Dynamic(tick_array));
                            }
                            Err(e) => {
                                log::warn!(
                                    "Failed to deserialize dynamic tick array {}: {}",
                                    addresses[idx],
                                    e
                                );
                            }
                        }
                    } else {
                        log::warn!("Unknown discriminator for tick array {}", addresses[idx]);
                    }
                }
                None => {
                    log::debug!(
                        "Tick array account {} not found (not initialized on-chain)",
                        addresses[idx]
                    );
                }
            }
        }

        Ok(tick_arrays)
    }

    /// Derive tick array PDA for a given whirlpool and start tick index
    pub fn derive_tick_array_pda(
        &self,
        whirlpool: &Pubkey,
        start_tick_index: i32,
    ) -> Result<(Pubkey, u8)> {
        // Whirlpool uses the string representation of the start tick index for PDA derivation
        let start_tick_index_str = start_tick_index.to_string();
        let seeds = [
            TICK_ARRAY_SEED.as_bytes(),
            whirlpool.as_ref(),
            start_tick_index_str.as_bytes(),
        ];

        let (pda, bump) = Pubkey::find_program_address(&seeds, &self.program_id);
        Ok((pda, bump))
    }

    /// Get initialized tick indices from a tick array
    pub fn get_initialized_ticks(tick_array: &OrcaTickArrayState) -> Vec<(i32, OrcaTickState)> {
        let start_tick = tick_array.start_tick_index();
        let mut initialized_ticks = Vec::new();

        match tick_array {
            OrcaTickArrayState::Dynamic(dynamic) => {
                // For dynamic arrays, ticks vector contains exactly the ticks as stored
                // The bitmap tells us which tick offsets are initialized
                for (i, tick) in dynamic.ticks.iter().enumerate() {
                    let tick_state = tick.to_tick_state();
                    if tick_state.initialized {
                        let tick_index = start_tick + i as i32;
                        initialized_ticks.push((tick_index, tick_state));
                    }
                }
            }
            OrcaTickArrayState::Fixed(fixed) => {
                for (i, tick) in fixed.ticks.iter().enumerate() {
                    if tick.initialized {
                        let tick_index = start_tick + i as i32;
                        initialized_ticks.push((tick_index, *tick));
                    }
                }
            }
        }

        initialized_ticks
    }

    /// Calculate price from adjacent ticks in the pool
    pub fn calculate_price_from_sqrt_price(sqrt_price_x64: u128) -> f64 {
        // Convert from fixed-point X64 format to price
        // sqrt_price = sqrt_price_x64 / 2^64
        // price = sqrt_price^2
        const Q64: f64 = 18446744073709551616.0; // 2^64
        let sqrt_price = (sqrt_price_x64 as f64) / Q64;
        sqrt_price * sqrt_price
    }

    /// Fetch all relevant tick arrays for a given Whirlpool
    ///
    /// Unlike Raydium which uses a pool-level bitmap, Whirlpool requires calculating
    /// which tick arrays are needed based on the current tick position.
    /// This implementation fetches tick arrays in a wider range around the current tick
    /// to ensure we have all necessary data for accurate swap routing.
    ///
    /// # Arguments
    /// * `pool_id` - The Whirlpool address
    /// * `pool_state` - The Whirlpool pool state containing tick_current_index and tick_spacing
    ///
    /// # Returns
    /// Vector of OrcaTickArrayState objects for all fetched tick arrays
    pub async fn fetch_all_tick_arrays(
        &self,
        pool_id: Pubkey,
        pool_state: &WhirlpoolPoolState,
    ) -> Result<Vec<OrcaTickArrayState>> {
        let tick_current_index = pool_state.tick_current_index;
        let tick_spacing = pool_state.tick_spacing as i32;

        // Calculate ticks per array: TICK_ARRAY_SIZE_I32 * tick_spacing
        let ticks_per_array = TICK_ARRAY_SIZE_I32 * tick_spacing;

        // Calculate the start tick index for the array containing current tick
        let current_array_start = (tick_current_index / ticks_per_array) * ticks_per_array;

        // Fetch arrays in a wider range to support larger swaps
        // We fetch 5 arrays in each direction plus the current array (11 total)
        // This provides good coverage for most swap scenarios and reduces quote inaccuracy
        let offsets = [-5, -4, -3, -2, -1, 0, 1, 2, 3, 4, 5];

        let start_tick_indices: Vec<i32> = offsets
            .iter()
            .map(|&offset| current_array_start + (offset * ticks_per_array))
            .filter(|&start_index| {
                // Filter out invalid tick array indices
                (MIN_TICK_INDEX..MAX_TICK_INDEX).contains(&start_index)
            })
            .collect();

        log::debug!(
            "Fetching {} tick arrays for Whirlpool {} (current tick: {}, spacing: {})",
            start_tick_indices.len(),
            pool_id,
            tick_current_index,
            tick_spacing
        );

        // Generate tick array addresses
        let addresses: Vec<Pubkey> = start_tick_indices
            .iter()
            .filter_map(|&start_index| {
                self.derive_tick_array_pda(&pool_id, start_index)
                    .ok()
                    .map(|(pda, _)| pda)
            })
            .collect();

        // Batch fetch all tick arrays (including uninitialized ones)
        let tick_arrays = self.fetch_multiple_tick_arrays(addresses).await?;

        log::debug!(
            "Successfully fetched {} initialized tick arrays for Whirlpool {}",
            tick_arrays.len(),
            pool_id
        );

        Ok(tick_arrays)
    }

    pub async fn fetch_oracle(
        &self,
        whirlpool: &Pubkey,
        tick_spacing: u16,
        fee_tier_index_seed: [u8; 2],
    ) -> Result<Option<Oracle>> {
        // no need to fetch oracle for non-adaptive fee whirlpools
        if tick_spacing == u16::from_le_bytes(fee_tier_index_seed) {
            return Ok(None);
        }
        let oracle_address = get_oracle_address(whirlpool).0;
        let oracle_info = self.rpc_client.get_account(&oracle_address).await?;
        // Ok(Some(Oracle::from_bytes(&oracle_info.data)?))

        let oracle = Oracle::try_from_slice(&oracle_info.data)
            .map_err(|e| anyhow!("Failed to deserialize oracle: {}", e))?;
        Ok(Some(oracle))
    }
}

pub fn get_oracle_address(whirlpool: &Pubkey) -> (Pubkey, u8) {
    let seeds: &[&[u8]; 2] = &[b"oracle", whirlpool.as_ref()];

    Pubkey::try_find_program_address(seeds, &ORCA_WHIRLPOOL_PROGRAM_ID).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_orca_tick_state_default() {
        let tick = OrcaTickState::default();
        assert!(!tick.initialized);
        assert_eq!(tick.liquidity_net, 0);
        assert_eq!(tick.liquidity_gross, 0);
    }

    #[test]
    fn test_orca_tick_array_state_methods() {
        let fixed_array = FixedOrcaTickArrayState {
            discriminator: [69, 97, 189, 190, 110, 7, 66, 187],
            start_tick_index: 100,
            ticks: [OrcaTickState::default(); TICK_ARRAY_SIZE],
            whirlpool: Pubkey::new_unique(),
        };

        let tick_array = OrcaTickArrayState::Fixed(fixed_array.clone());
        assert_eq!(tick_array.start_tick_index(), 100);
        assert_eq!(tick_array.whirlpool(), fixed_array.whirlpool);
        assert!(!tick_array.is_dynamic());
        assert!(tick_array.tick_bitmap().is_none());
    }

    #[test]
    fn test_price_calculation() {
        // Example: sqrt_price_x64 = 2^64 (represents 1 in fixed point)
        // This should give price ~1.0
        let sqrt_price_x64: u128 = 18446744073709551616; // 2^64
        let price = OrcaTickArrayFetcher::calculate_price_from_sqrt_price(sqrt_price_x64);
        assert!((price - 1.0).abs() < 0.001);
    }
}
