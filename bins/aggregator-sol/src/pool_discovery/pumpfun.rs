use super::PoolDiscovery;
use crate::pool_data_types::{pumpfun::PumpfunPoolState, PoolState};
use anyhow::Result;
use async_trait::async_trait;
use borsh::BorshDeserialize;
use reqwest::Client;
use serde::Deserialize;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use std::collections::HashSet;
use std::sync::Arc;

use crate::fetchers::common::fetch_multiple_accounts;
use solana_streamer_sdk::streaming::event_parser::common::high_performance_clock::get_high_perf_clock;

const PUMPFUN_API_BASE: &str = "https://frontend-api-v3.pump.fun";

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct CoinRecord {
    mint: String,
    #[serde(alias = "bondingCurve")]
    bonding_curve: String,
    name: String,
    symbol: String,
    #[serde(default)]
    image_uri: String,
    #[serde(default, alias = "usd_market_cap")]
    market_cap: f64,
    #[serde(default)]
    complete: bool,
}

#[derive(Deserialize)]
struct TopRunnerItem {
    coin: CoinRecord,
}

// Recommended list endpoint `recommended` likely returns a list.

/// Raw bonding curve state from on-chain account (post-cashback upgrade)
#[derive(Debug, Clone, BorshDeserialize)]
pub struct BondingCurveRaw {
    pub virtual_token_reserves: u64,
    pub virtual_sol_reserves: u64,
    pub real_token_reserves: u64,
    pub real_sol_reserves: u64,
    pub token_total_supply: u64,
    pub complete: bool,
    pub creator: Pubkey,
    pub is_mayhem_mode: bool,
    pub is_cashback: bool,
}

/// Legacy bonding curve state (pre-cashback upgrade)
#[derive(Debug, Clone, BorshDeserialize)]
pub struct BondingCurveRawLegacy {
    pub virtual_token_reserves: u64,
    pub virtual_sol_reserves: u64,
    pub real_token_reserves: u64,
    pub real_sol_reserves: u64,
    pub token_total_supply: u64,
    pub complete: bool,
    pub creator: Pubkey,
    pub is_mayhem_mode: bool,
}

fn decode_bonding_curve_raw(data: &[u8]) -> Option<BondingCurveRaw> {
    // Try new format first (with is_cashback)
    let mut slice = data;
    if let Ok(raw) = BondingCurveRaw::deserialize(&mut slice) {
        return Some(raw);
    }
    // Fallback to legacy format
    let mut slice = data;
    if let Ok(legacy) = BondingCurveRawLegacy::deserialize(&mut slice) {
        return Some(BondingCurveRaw {
            virtual_token_reserves: legacy.virtual_token_reserves,
            virtual_sol_reserves: legacy.virtual_sol_reserves,
            real_token_reserves: legacy.real_token_reserves,
            real_sol_reserves: legacy.real_sol_reserves,
            token_total_supply: legacy.token_total_supply,
            complete: legacy.complete,
            creator: legacy.creator,
            is_mayhem_mode: legacy.is_mayhem_mode,
            is_cashback: false,
        });
    }
    None
}

pub struct PumpFunDiscovery {
    http_client: Client,
    rpc_client: Arc<RpcClient>,
}

impl PumpFunDiscovery {
    pub fn new(rpc_client: Arc<RpcClient>) -> Self {
        Self {
            http_client: Client::new(),
            rpc_client,
        }
    }

    async fn fetch_coins_from_api(&self, url: &str) -> Result<Vec<CoinRecord>> {
        let resp = self.http_client.get(url).send().await?;
        let text = resp.text().await?;

        // 1. Try deserializing as Vec<CoinRecord> (flat list, e.g. recommended)
        if let Ok(coins) = serde_json::from_str::<Vec<CoinRecord>>(&text) {
            return Ok(coins);
        }

        // 2. Try deserializing as Vec<TopRunnerItem> (nested coin object, e.g. top-runners)
        if let Ok(items) = serde_json::from_str::<Vec<TopRunnerItem>>(&text) {
            return Ok(items.into_iter().map(|item| item.coin).collect());
        }

        // 3. Fallback: Wrapped response { coins: [...] }
        #[derive(Deserialize)]
        struct WrappedResponse {
            coins: Vec<CoinRecord>,
        }
        if let Ok(wrapped) = serde_json::from_str::<WrappedResponse>(&text) {
            return Ok(wrapped.coins);
        }

        // If all generic attempts fail, try to log snippet and error
        // For now, return error
        Err(anyhow::anyhow!(
            "Failed to parse PumpFun API response: {}",
            text.chars().take(200).collect::<String>()
        ))
    }
}

#[async_trait]
impl PoolDiscovery for PumpFunDiscovery {
    async fn discover_for_token(&self, _token: &Pubkey) -> Result<Vec<PoolState>> {
        // TODO: Implement specific token lookup API if available
        Ok(vec![])
    }

    async fn discover_top_pools(&self, limit: usize) -> Result<Vec<PoolState>> {
        let top_runners_url = format!("{}/coins/top-runners", PUMPFUN_API_BASE);
        let recommended_url = format!(
            "{}/coins/recommended?limit={}&includeNsfw=false",
            PUMPFUN_API_BASE, limit
        );

        let mut all_coins = Vec::new();

        // Fetch top runners
        match self.fetch_coins_from_api(&top_runners_url).await {
            Ok(coins) => all_coins.extend(coins),
            Err(e) => log::warn!("Failed to fetch top runners: {}", e),
        }

        // Fetch recommended
        match self.fetch_coins_from_api(&recommended_url).await {
            Ok(coins) => all_coins.extend(coins),
            Err(e) => log::warn!("Failed to fetch recommended: {}", e),
        }

        log::info!("Found {} coins", all_coins.len());

        // Deduplicate by mint
        let mut unique_coins = Vec::new();
        let mut seen_mints = HashSet::new();
        for coin in all_coins {
            if seen_mints.insert(coin.mint.clone()) {
                unique_coins.push(coin);
            }
        }

        log::info!(
            "Processing {} unique coins (batch fetching on-chain data)",
            unique_coins.len()
        );

        let mut discovered_pools = Vec::new();

        // Prepare list of bonding curve addresses to fetch
        let mut bonding_curves = Vec::with_capacity(unique_coins.len());
        let mut bonding_curve_to_coin_map = std::collections::HashMap::new();

        for coin in &unique_coins {
            if let Ok(pubkey) = Pubkey::try_from(coin.bonding_curve.as_str()) {
                bonding_curves.push(pubkey);
                bonding_curve_to_coin_map.insert(pubkey, coin);
            } else {
                log::warn!("Invalid bonding curve pubkey for coin {}", coin.mint);
            }
        }

        let batch_results = fetch_multiple_accounts(&self.rpc_client, &bonding_curves).await?;

        for (i, account_option) in batch_results.into_iter().enumerate() {
            if let Some(data) = account_option {
                let curve_pubkey = bonding_curves[i];
                if let Some(coin) = bonding_curve_to_coin_map.get(&curve_pubkey) {
                    // Parse data
                    if data.len() < 8 {
                        log::warn!("Account data too short for bonding curve {}", curve_pubkey);
                        continue;
                    }

                    let data_slice = &data[8..];
                    match decode_bonding_curve_raw(data_slice) {
                        Some(raw_state) => {
                            if let Ok(mint) = Pubkey::try_from(coin.mint.as_str()) {
                                let state = PumpfunPoolState {
                                    slot: 0,
                                    transaction_index: None,
                                    address: curve_pubkey,
                                    mint,
                                    last_updated: get_high_perf_clock() as u64,
                                    liquidity_usd: coin.market_cap,
                                    is_state_keys_initialized: true,
                                    virtual_token_reserves: raw_state.virtual_token_reserves,
                                    virtual_sol_reserves: raw_state.virtual_sol_reserves,
                                    real_token_reserves: raw_state.real_token_reserves,
                                    real_sol_reserves: raw_state.real_sol_reserves,
                                    complete: raw_state.complete,
                                    creator: raw_state.creator,
                                    is_mayhem_mode: raw_state.is_mayhem_mode,
                                    is_cashback: raw_state.is_cashback,
                                };
                                discovered_pools.push(PoolState::Pumpfun(state));
                            }
                        }
                        None => {
                            log::warn!("Failed to deserialize bonding curve {}", curve_pubkey,);
                        }
                    }
                }
            }
        }

        Ok(discovered_pools)
    }
}
