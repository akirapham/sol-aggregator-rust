use crate::pool_data_types::{dlmm::functions, MeteoraDlmmPoolState};
use anyhow::Result;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use solana_streamer_sdk::streaming::event_parser::protocols::meteora_dlmm::types::{
    binarray_decode, BinArray,
};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

pub struct MeteoraDlmmBinArrayFetcher {
    rpc_client: Arc<RpcClient>,
}

impl MeteoraDlmmBinArrayFetcher {
    pub fn new(rpc_client: Arc<RpcClient>) -> Self {
        Self { rpc_client }
    }

    pub async fn fetch_all_bin_arrays(
        &self,
        pool_id: Pubkey,
        pool_state: &MeteoraDlmmPoolState,
    ) -> Result<Vec<BinArray>> {
        let pool_id_anchor = anchor_lang::prelude::Pubkey::from(pool_id.to_bytes());
        let commons_lbpair = functions::to_commons_lb_pair(pool_state);
        let commons_bitmap_extension = pool_state
            .bitmap_extension
            .as_ref()
            .map(|ext| functions::to_commons_bitmap_extension(pool_state, ext));

        let mut all_bin_array_pubkeys = std::collections::HashSet::new();

        const TAKE_COUNT: u8 = 50; // Increased from 10 to fetch more bin arrays

        // Swap for Y (X -> Y)
        if let Ok(pubkeys_for_y) = meteora_dlmm_sdk::quote::get_bin_array_pubkeys_for_swap(
            pool_id_anchor,
            &commons_lbpair,
            commons_bitmap_extension.as_ref(),
            true, // swap_for_y
            TAKE_COUNT,
        ) {
            for pubkey in pubkeys_for_y {
                all_bin_array_pubkeys.insert(Pubkey::from(pubkey.to_bytes()));
            }
        } else {
            log::debug!("DLMM Fetcher: Failed to get pubkeys for Y swap");
        }

        // Swap for X (Y -> X)
        if let Ok(pubkeys_for_x) = meteora_dlmm_sdk::quote::get_bin_array_pubkeys_for_swap(
            pool_id_anchor,
            &commons_lbpair,
            commons_bitmap_extension.as_ref(),
            false, // swap_for_y
            TAKE_COUNT,
        ) {
            for pubkey in pubkeys_for_x {
                all_bin_array_pubkeys.insert(Pubkey::from(pubkey.to_bytes()));
            }
        } else {
            log::debug!("DLMM Fetcher: Failed to get pubkeys for X swap");
        }

        if all_bin_array_pubkeys.is_empty() {
            log::debug!("DLMM Fetcher: No bin arrays found for pool {}", pool_id);
            return Ok(Vec::new());
        }

        // Create BinArrayInfo structs
        let mut bin_arrays: Vec<BinArrayInfo> = all_bin_array_pubkeys
            .into_iter()
            .map(|address| BinArrayInfo {
                index: 0, // Will be set after fetching
                address,
                account_data: None,
            })
            .collect();

        // Fetch account data in batches
        self.fetch_bin_array_accounts_batch(&mut bin_arrays).await?;

        // Parse bin arrays
        let fetched_bin_arrays: Vec<BinArray> = bin_arrays
            .into_iter()
            .filter_map(|ba| {
                ba.account_data.and_then(|data| {
                    // Skip 8 byte discriminator and use binarray_decode
                    if data.len() >= 8 {
                        match binarray_decode(&data[8..]) {
                            Some(bin_array) => {
                                Some(bin_array)
                            }
                            None => {
                                log::debug!("DLMM Fetcher: Failed to parse bin array, data length: {} bytes", data.len());
                                None
                            }
                        }
                    } else {
                        log::debug!("DLMM Fetcher: Bin array data too short: {} bytes", data.len());
                        None
                    }
                })
            })
            .collect();

        Ok(fetched_bin_arrays)
    }

    async fn fetch_bin_array_accounts_batch(&self, bin_arrays: &mut [BinArrayInfo]) -> Result<()> {
        let addresses: Vec<Pubkey> = bin_arrays.iter().map(|ba| ba.address).collect();

        const BATCH_SIZE: usize = 100;
        const MAX_RETRIES: usize = 3;
        const INITIAL_BACKOFF: u64 = 200;

        for chunk in addresses.chunks(BATCH_SIZE) {
            let mut attempts = 0;
            loop {
                match self.rpc_client.get_multiple_accounts(chunk).await {
                    Ok(accounts) => {
                        for (i, account_option) in accounts.iter().enumerate() {
                            if let Some(account) = account_option {
                                if let Some(bin_array_idx) =
                                    bin_arrays.iter().position(|ba| ba.address == chunk[i])
                                {
                                    bin_arrays[bin_array_idx].account_data =
                                        Some(account.data.clone());
                                }
                            }
                        }
                        break;
                    }
                    Err(e) => {
                        attempts += 1;
                        if attempts >= MAX_RETRIES {
                            log::debug!(
                                "Failed to fetch accounts after {} attempts: {:?}",
                                MAX_RETRIES,
                                e
                            );
                            return Err(e.into());
                        }
                        log::debug!(
                            "Failed to fetch accounts (attempt {}/{}), retrying...: {:?}",
                            attempts,
                            MAX_RETRIES,
                            e
                        );
                        sleep(Duration::from_millis(
                            INITIAL_BACKOFF * 2u64.pow((attempts - 1) as u32),
                        ))
                        .await;
                    }
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug)]
struct BinArrayInfo {
    #[allow(dead_code)]
    index: i32,
    address: Pubkey,
    account_data: Option<Vec<u8>>,
}
