use anyhow::{anyhow, Result};
use borsh::BorshDeserialize;
use serde::{Deserialize, Serialize};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;
use std::sync::Arc;

const ORCA_WHIRLPOOL_PROGRAM_ID: &str = "whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc";
const TICK_ARRAY_SIZE: usize = 88;
const TICK_ARRAY_SEED: &str = "tick_array";

#[allow(unused)]
#[derive(Clone, Debug)]
pub struct OrcaTickArrayInfo {
    pub start_tick_index: i32,
    pub address: Pubkey,
    pub account_data: Option<Vec<u8>>,
}

/// Represents an individual tick in Orca Whirlpool
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
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshDeserialize)]
pub struct DynamicOrcaTickArrayState {
    #[serde(with = "serde_big_array::BigArray")]
    pub discriminator: [u8; 8],
    pub start_tick_index: i32,
    pub whirlpool: Pubkey,
    pub tick_bitmap: u128, // Bitmap for tracking initialized ticks
    #[serde(with = "serde_big_array::BigArray")]
    pub ticks: [OrcaTickState; TICK_ARRAY_SIZE],
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

    pub fn ticks(&self) -> &[OrcaTickState; TICK_ARRAY_SIZE] {
        match self {
            Self::Fixed(arr) => &arr.ticks,
            Self::Dynamic(arr) => &arr.ticks,
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
    pub fn new(rpc_client: Arc<RpcClient>) -> Result<Self> {
        let program_id = Pubkey::from_str(ORCA_WHIRLPOOL_PROGRAM_ID)
            .map_err(|_| anyhow!("Failed to parse Orca Whirlpool program ID"))?;

        Ok(Self {
            rpc_client,
            program_id,
        })
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
                BorshDeserialize::deserialize(&mut &account.data[8..])?;
            Ok(OrcaTickArrayState::Fixed(tick_array))
        } else if discriminator == dynamic_discriminator {
            let tick_array: DynamicOrcaTickArrayState =
                BorshDeserialize::deserialize(&mut &account.data[8..])?;
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
                        match BorshDeserialize::deserialize(&mut &account.data[8..]) {
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
                        match BorshDeserialize::deserialize(&mut &account.data[8..]) {
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
                    log::warn!("Account {} not found", addresses[idx]);
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
        let seeds = [
            TICK_ARRAY_SEED.as_bytes(),
            whirlpool.as_ref(),
            &start_tick_index.to_le_bytes(),
        ];

        let (pda, bump) = Pubkey::find_program_address(&seeds, &self.program_id);
        Ok((pda, bump))
    }

    /// Get initialized tick indices from a tick array
    pub fn get_initialized_ticks(tick_array: &OrcaTickArrayState) -> Vec<(i32, OrcaTickState)> {
        let start_tick = tick_array.start_tick_index();
        let ticks = tick_array.ticks();
        let mut initialized_ticks = Vec::new();

        // For dynamic arrays, use bitmap; for fixed arrays, check initialized flag
        match tick_array {
            OrcaTickArrayState::Dynamic(dynamic) => {
                for (i, tick) in ticks.iter().enumerate() {
                    let is_initialized = (dynamic.tick_bitmap >> i) & 1 == 1;
                    if is_initialized && tick.initialized {
                        let tick_index = start_tick + i as i32;
                        initialized_ticks.push((tick_index, *tick));
                    }
                }
            }
            OrcaTickArrayState::Fixed(_) => {
                for (i, tick) in ticks.iter().enumerate() {
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
    fn test_dynamic_tick_array_bitmap() {
        let dynamic_array = DynamicOrcaTickArrayState {
            discriminator: [17, 216, 246, 142, 225, 199, 218, 56],
            start_tick_index: 50,
            whirlpool: Pubkey::new_unique(),
            tick_bitmap: 0xFF, // First 8 ticks initialized
            ticks: [OrcaTickState::default(); TICK_ARRAY_SIZE],
        };

        let tick_array = OrcaTickArrayState::Dynamic(dynamic_array);
        assert!(tick_array.is_dynamic());
        assert_eq!(tick_array.tick_bitmap(), Some(0xFF));
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
