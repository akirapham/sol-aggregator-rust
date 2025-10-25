use crate::pool_data_types::{
    clmm::pda, RaydiumClmmPoolState, TickArrayBitmapExtension, TickArrayState, TickState,
};
use anyhow::anyhow;
use anyhow::Result;
use borsh::BorshDeserialize;
use serde::{Deserialize, Serialize};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

const TICK_ARRAY_SIZE: i32 = 60;
const TICK_ARRAY_BITMAP_SIZE: i32 = 512;
const TICK_ARRAY_SEED: &str = "tick_array";
const EXTENSION_TICKARRAY_BITMAP_SIZE: usize = 14;

#[allow(unused)]
#[derive(Clone, Debug)]
pub struct TickArrayInfo {
    pub start_tick_index: i32,
    pub address: Pubkey,
    pub account_data: Option<Vec<u8>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshDeserialize, Default)]
pub struct TickStateLayout {
    pub tick: i32,
    pub liquidity_net: i128,
    pub liquidity_gross: u128,
    pub fee_growth_outside0_x64: u128,
    pub fee_growth_outside1_x64: u128,
    pub reward_growths_outside_x64: [u128; 3],
    pub padding: [u32; 13],
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshDeserialize)]
pub struct TickArrayStateLayout {
    pub pool_id: Pubkey,
    pub start_tick_index: i32,
    #[serde(with = "serde_big_array::BigArray")]
    pub ticks: [TickStateLayout; 60],
    pub initialized_tick_count: u8,
    pub recent_epoch: u64,
    #[serde(with = "serde_big_array::BigArray")]
    pub padding: [u8; 107],
}

pub struct TickArrayFetcher {
    rpc_client: Arc<RpcClient>,
    pub program_id: Pubkey,
}

#[allow(unused)]
impl TickArrayFetcher {
    pub fn new(rpc_client: Arc<RpcClient>, program_id: Pubkey) -> Self {
        Self {
            rpc_client,
            program_id,
        }
    }

    /// Fetch all active tick array accounts for a CLMM pool
    pub async fn fetch_all_tick_arrays(
        &self,
        pool_id: Pubkey,
        pool_state: Arc<&RaydiumClmmPoolState>,
    ) -> Result<Vec<TickArrayState>> {
        // first we fetch tick array bitmap and then tick array bitmap extension if any
        // derive tick array bitmap extension address from pool state
        let tick_array_bitmap_extension_address =
            pda::get_pda_ex_bitmap_account(&RaydiumClmmPoolState::get_program_id(), &pool_id).0;
        let pool_and_extension_accounts = self
            .rpc_client
            .get_multiple_accounts(&[pool_id, tick_array_bitmap_extension_address])
            .await?;
        let pool_account = pool_and_extension_accounts
            .first()
            .and_then(|opt| opt.as_ref())
            .ok_or_else(|| anyhow!("Failed to fetch pool account"))?; // Changed to anyhow! macro
                                                                      // decode pool account data
        let tick_array_bitmap = match solana_streamer_sdk::streaming::event_parser::protocols::raydium_clmm::types::PoolState::try_from_slice(&pool_account.data[8..]) {
            Ok(decoded_pool_state) => {
                decoded_pool_state.tick_array_bitmap
            }
            Err(e) => {
                return Err(anyhow!("Failed to decode pool state: {:?}", e));  // Changed to anyhow! macro
            }
        };

        let tick_array_bitmap_extension_account = pool_and_extension_accounts
            .get(1)
            .and_then(|opt| opt.as_ref());
        let tick_array_bitmap_extension = if let Some(extension_account) =
            tick_array_bitmap_extension_account
        {
            match solana_streamer_sdk::streaming::event_parser::protocols::raydium_clmm::types::TickArraysBitampExtension::try_from_slice(&extension_account.data[8..]) {
                Ok(extension) => Some(extension),
                Err(e) => {
                    log::warn!("Failed to decode tick array bitmap extension: {:?}", e);
                    None
                }
            }
        } else {
            log::info!(
                "No tick array bitmap extension account found for pool {}",
                pool_id
            );
            None
        };

        // Get all initialized tick array start indices
        let mut tick_array_indices = Vec::new();

        // 1. Scan pool-level bitmap (central 1024 bits)
        let pool_bitmap_indices =
            self.get_tick_arrays_from_pool_bitmap(&tick_array_bitmap, pool_state.tick_spacing);
        tick_array_indices.extend(pool_bitmap_indices);

        // 2. Scan extension bitmaps if present (for extreme ranges)
        if let Some(extension) = &tick_array_bitmap_extension {
            let extension_indices = self.get_tick_arrays_from_extension_bitmap(
                &TickArrayBitmapExtension {
                    positive_tick_array_bitmap: extension.positive_tick_array_bitmap,
                    negative_tick_array_bitmap: extension.negative_tick_array_bitmap,
                },
                pool_state.tick_spacing,
            );
            tick_array_indices.extend(extension_indices);
        }

        // 3. Generate tick array addresses from indices
        let mut tick_arrays = Vec::new();
        for start_index in tick_array_indices {
            let address = self.get_tick_array_address(pool_id, start_index);
            tick_arrays.push(TickArrayInfo {
                start_tick_index: start_index,
                address,
                account_data: None,
            });
        }

        // 4. Fetch account data in batches (optional - for performance)
        self.fetch_tick_array_accounts_batch(&mut tick_arrays)
            .await?;

        // parse tick arrays to TickArrayStateLayout
        let tick_array_states = tick_arrays
            .iter()
            .filter_map(|ta| {
                ta.account_data.as_ref().and_then(|data| {
                    TickArrayStateLayout::try_from_slice(&data[8..]).ok().map(
                        |layout: TickArrayStateLayout| TickArrayState {
                            start_tick_index: layout.start_tick_index,
                            ticks: std::array::from_fn(|i| TickState {
                                tick: layout.ticks[i].tick,
                                liquidity_net: layout.ticks[i].liquidity_net,
                                liquidity_gross: layout.ticks[i].liquidity_gross,
                            }),
                            initialized_tick_count: layout.initialized_tick_count,
                        },
                    )
                })
            })
            .collect::<Vec<TickArrayState>>();

        Ok(tick_array_states)
    }

    /// Extract initialized tick array indices from pool-level bitmap
    fn get_tick_arrays_from_pool_bitmap(
        &self,
        bitmap: &[u64; 16], // 1024 bits total
        tick_spacing: u16,
    ) -> Vec<i32> {
        let mut indices = Vec::new();
        let tick_count = Self::tick_count(tick_spacing);

        // Scan each 64-bit word in the bitmap
        for (word_idx, word) in bitmap.iter().enumerate() {
            if *word == 0 {
                continue; // Skip empty words
            }

            // Check each bit in the word
            for bit_idx in 0..64 {
                if (word & (1u64 << bit_idx)) != 0 {
                    // Calculate the absolute bit position
                    let bit_position = (word_idx * 64 + bit_idx) as i32;

                    // Convert bit position to tick array start index
                    // Pool bitmap is centered, so adjust by subtracting the center offset
                    let tick_array_start_index =
                        (bit_position - TICK_ARRAY_BITMAP_SIZE) * tick_count;

                    indices.push(tick_array_start_index);
                }
            }
        }

        indices
    }

    /// Extract initialized tick array indices from extension bitmaps
    fn get_tick_arrays_from_extension_bitmap(
        &self,
        extension: &TickArrayBitmapExtension,
        tick_spacing: u16,
    ) -> Vec<i32> {
        let mut indices = Vec::new();
        let tick_count = Self::tick_count(tick_spacing);

        // Scan positive extension bitmaps
        for (bitmap_idx, bitmap) in extension.positive_tick_array_bitmap.iter().enumerate() {
            indices.extend(self.scan_extension_bitmap_word(
                bitmap,
                bitmap_idx,
                tick_spacing,
                false, // positive
            ));
        }

        // Scan negative extension bitmaps
        for (bitmap_idx, bitmap) in extension.negative_tick_array_bitmap.iter().enumerate() {
            indices.extend(self.scan_extension_bitmap_word(
                bitmap,
                bitmap_idx,
                tick_spacing,
                true, // negative
            ));
        }

        indices
    }

    /// Scan a single extension bitmap word (512 bits)
    fn scan_extension_bitmap_word(
        &self,
        bitmap: &[u64; 8], // 512 bits = 8 * 64 bits
        bitmap_index: usize,
        tick_spacing: u16,
        is_negative: bool,
    ) -> Vec<i32> {
        let mut indices = Vec::new();
        let tick_count = Self::tick_count(tick_spacing);

        for (word_idx, word) in bitmap.iter().enumerate() {
            if *word == 0 {
                continue;
            }

            for bit_idx in 0..64 {
                if (word & (1u64 << bit_idx)) != 0 {
                    let bit_position_in_bitmap = word_idx * 64 + bit_idx;
                    let absolute_bit_position = bitmap_index * 512 + bit_position_in_bitmap;

                    let tick_array_start_index = if is_negative {
                        // For negative side: start from pool boundary and go further negative
                        -(TICK_ARRAY_BITMAP_SIZE + 1 + absolute_bit_position as i32) * tick_count
                    } else {
                        // For positive side: start from pool boundary and go further positive
                        (TICK_ARRAY_BITMAP_SIZE + 1 + absolute_bit_position as i32) * tick_count
                    };

                    indices.push(tick_array_start_index);
                }
            }
        }

        indices
    }

    /// Generate PDA for tick array account
    fn get_tick_array_address(&self, pool_id: Pubkey, start_tick_index: i32) -> Pubkey {
        let (address, _) = Pubkey::find_program_address(
            &[
                TICK_ARRAY_SEED.as_bytes(),
                pool_id.as_ref(),
                &start_tick_index.to_be_bytes(),
            ],
            &self.program_id,
        );
        address
    }

    /// Batch fetch tick array account data with retry logic
    async fn fetch_tick_array_accounts_batch(
        &self,
        tick_arrays: &mut [TickArrayInfo],
    ) -> Result<()> {
        let addresses: Vec<Pubkey> = tick_arrays.iter().map(|ta| ta.address).collect();

        // Fetch accounts in batches to avoid RPC limits
        const BATCH_SIZE: usize = 100;
        const MAX_RETRIES: usize = 3;
        const RETRY_DELAY: Duration = Duration::from_secs(1);

        for chunk in addresses.chunks(BATCH_SIZE) {
            let mut attempts = 0;
            loop {
                match self.rpc_client.get_multiple_accounts(chunk).await {
                    Ok(accounts) => {
                        // Process successful accounts
                        for (i, account_option) in accounts.iter().enumerate() {
                            if let Some(account) = account_option {
                                if let Some(tick_array_idx) =
                                    tick_arrays.iter().position(|ta| ta.address == chunk[i])
                                {
                                    tick_arrays[tick_array_idx].account_data =
                                        Some(account.data.clone());
                                }
                            }
                        }
                        break; // Success, exit retry loop
                    }
                    Err(e) => {
                        attempts += 1;
                        if attempts >= MAX_RETRIES {
                            log::error!(
                                "Failed to fetch accounts after {} attempts: {:?}",
                                MAX_RETRIES,
                                e
                            );
                            return Err(e.into());
                        }
                        log::warn!(
                            "Failed to fetch accounts (attempt {}/{}), retrying in {:?}: {:?}",
                            attempts,
                            MAX_RETRIES,
                            RETRY_DELAY,
                            e
                        );
                        sleep(RETRY_DELAY).await;
                    }
                }
            }
        }

        Ok(())
    }

    /// Helper function to calculate tick count per array
    fn tick_count(tick_spacing: u16) -> i32 {
        TICK_ARRAY_SIZE * tick_spacing as i32
    }

    /// Check if tick array start index is valid
    fn is_valid_start_index(tick_index: i32, tick_spacing: u16) -> bool {
        // Check if tick is within bounds and aligned to array boundaries
        tick_index % Self::tick_count(tick_spacing) == 0
    }

    /// Get tick array start index for any tick
    pub fn get_array_start_index(tick_index: i32, tick_spacing: u16) -> i32 {
        let ticks_in_array = Self::tick_count(tick_spacing);
        let mut start = tick_index / ticks_in_array;
        if tick_index < 0 && tick_index % ticks_in_array != 0 {
            start -= 1;
        }
        start * ticks_in_array
    }

    /// Find initialized tick arrays within a range around current tick
    pub async fn fetch_tick_arrays_in_range(
        &self,
        pool_id: Pubkey,
        tick_current: i32,
        tick_spacing: u16,
        tick_array_bitmap: [u64; 16],
        extension: Option<&TickArrayBitmapExtension>,
        range_size: usize,
    ) -> Result<Vec<TickArrayInfo>> {
        let current_array_start = Self::get_array_start_index(tick_current, tick_spacing);
        let tick_count = Self::tick_count(tick_spacing);

        let mut tick_arrays = Vec::new();

        // Check arrays around current tick
        let half_range = (range_size / 2) as i32;
        for offset in -half_range..=half_range {
            let start_index = current_array_start + (offset * tick_count);

            // Check if this tick array is initialized
            if self.is_tick_array_initialized(
                start_index,
                tick_spacing,
                tick_array_bitmap,
                extension,
            ) {
                let address = self.get_tick_array_address(pool_id, start_index);
                tick_arrays.push(TickArrayInfo {
                    start_tick_index: start_index,
                    address,
                    account_data: None,
                });
            }
        }

        // Fetch account data
        self.fetch_tick_array_accounts_batch(&mut tick_arrays)
            .await?;

        Ok(tick_arrays)
    }

    /// Check if a tick array is initialized by examining the bitmaps
    fn is_tick_array_initialized(
        &self,
        start_index: i32,
        tick_spacing: u16,
        pool_bitmap: [u64; 16],
        extension: Option<&TickArrayBitmapExtension>,
    ) -> bool {
        // Determine if we need to check pool bitmap or extension
        let boundary = TICK_ARRAY_BITMAP_SIZE * Self::tick_count(tick_spacing);

        if start_index >= -boundary && start_index < boundary {
            // Check pool-level bitmap
            let offset =
                (start_index / Self::tick_count(tick_spacing) + TICK_ARRAY_BITMAP_SIZE) as usize;
            if offset < 1024 {
                let word_idx = offset / 64;
                let bit_idx = offset % 64;
                return (pool_bitmap[word_idx] & (1u64 << bit_idx)) != 0;
            }
        } else if let Some(ext) = extension {
            // Check extension bitmap
            return self.check_extension_bitmap(start_index, tick_spacing, ext);
        }

        false
    }

    /// Check extension bitmap for initialization
    fn check_extension_bitmap(
        &self,
        start_index: i32,
        tick_spacing: u16,
        extension: &TickArrayBitmapExtension,
    ) -> bool {
        let tick_count = Self::tick_count(tick_spacing);
        let boundary = TICK_ARRAY_BITMAP_SIZE * tick_count;

        if start_index >= boundary {
            // Positive extension
            let offset_from_boundary = (start_index - boundary) / tick_count;
            let bitmap_idx = (offset_from_boundary / 512) as usize;
            let bit_position = (offset_from_boundary % 512) as usize;

            if bitmap_idx < EXTENSION_TICKARRAY_BITMAP_SIZE {
                let word_idx = bit_position / 64;
                let bit_idx = bit_position % 64;
                return (extension.positive_tick_array_bitmap[bitmap_idx][word_idx]
                    & (1u64 << bit_idx))
                    != 0;
            }
        } else if start_index < -boundary {
            // Negative extension
            let offset_from_boundary = (-boundary - start_index) / tick_count - 1;
            let bitmap_idx = (offset_from_boundary / 512) as usize;
            let bit_position = (offset_from_boundary % 512) as usize;

            if bitmap_idx < EXTENSION_TICKARRAY_BITMAP_SIZE {
                let word_idx = bit_position / 64;
                let bit_idx = bit_position % 64;
                return (extension.negative_tick_array_bitmap[bitmap_idx][word_idx]
                    & (1u64 << bit_idx))
                    != 0;
            }
        }
        false
    }
}
