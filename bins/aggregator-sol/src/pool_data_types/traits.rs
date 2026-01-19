use crate::pool_data_types::{RaydiumClmmAmmConfig, RaydiumCpmmAmmConfig};
use anyhow::Result;
use async_trait::async_trait;
use solana_sdk::pubkey::Pubkey;
use solana_streamer_sdk::streaming::event_parser::protocols::meteora_dbc::types::PoolConfig;
use std::sync::Arc;

#[allow(unused)]
#[async_trait]
pub trait GetAmmConfig: Send + Sync {
    async fn get_raydium_clmm_amm_config(
        &self,
        amm_config: &Pubkey,
    ) -> Result<Option<RaydiumClmmAmmConfig>>;
    async fn get_raydium_cpmm_amm_config(
        &self,
        amm_config: &Pubkey,
    ) -> Result<Option<RaydiumCpmmAmmConfig>>;

    async fn get_dbc_pool_config(&self, dbc_config: &Pubkey) -> Result<Option<PoolConfig>>;
}

use crate::types::SwapParams;
use solana_sdk::instruction::Instruction;

#[async_trait]
pub trait BuildSwapInstruction: Send + Sync {
    async fn build_swap_instruction(
        &self,
        params: &SwapParams,
        amm_config_fetcher: Arc<dyn GetAmmConfig>,
    ) -> std::result::Result<Vec<Instruction>, String>;
}
