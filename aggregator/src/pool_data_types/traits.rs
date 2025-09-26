use crate::pool_data_types::{RaydiumClmmAmmConfig, RaydiumCpmmAmmConfig};
use async_trait::async_trait;
use solana_sdk::pubkey::Pubkey;

#[async_trait]
pub trait GetAmmConfig: Send + Sync {
    async fn get_raydium_clmm_amm_config(
        &self,
        amm_config: &Pubkey,
    ) -> Option<RaydiumClmmAmmConfig>;
    async fn get_raydium_cpmm_amm_config(
        &self,
        amm_config: &Pubkey,
    ) -> Option<RaydiumCpmmAmmConfig>;
}
