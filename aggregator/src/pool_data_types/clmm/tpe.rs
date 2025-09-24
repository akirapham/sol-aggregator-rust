use solana_sdk::pubkey::Pubkey;

use crate::pool_data_types::{
    RadyiumClmmPoolState, RaydiumClmmAmmConfig, TickArrayBitmapExtension,
};

#[derive(Debug)]
pub struct ComputeClmmPoolInfo {
    pub id: Pubkey,
    pub program_id: Pubkey,
    pub pool_state: RadyiumClmmPoolState,
    pub ex_bitmap_info: Option<TickArrayBitmapExtension>,
    pub amm_config: Option<RaydiumClmmAmmConfig>,
}

impl ComputeClmmPoolInfo {
    pub fn new(
        id: Pubkey,
        program_id: Pubkey,
        pool_state: RadyiumClmmPoolState,
        ex_bitmap_info: Option<TickArrayBitmapExtension>,
        amm_config: Option<RaydiumClmmAmmConfig>,
    ) -> Self {
        Self {
            id,
            program_id,
            pool_state,
            ex_bitmap_info,
            amm_config,
        }
    }
}
