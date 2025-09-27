use crate::pool_data_types::{RaydiumClmmAmmConfig, RaydiumCpmmAmmConfig};
use anyhow::Result;
use async_trait::async_trait;
use solana_sdk::pubkey::Pubkey;
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
}
