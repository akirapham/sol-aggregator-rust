use solana_streamer_sdk::streaming::event_parser::protocols::meteora_dlmm::types::{
    BinArray, BinArrayBitmapExtension, LbPair,
};
use std::collections::HashMap;

/// Find the next bin array with liquidity using bitmap traversal
/// This is a port of the TypeScript SDK's findNextBinArrayWithLiquidity function
pub fn find_next_bin_array_with_liquidity(
    swap_for_y: bool,
    active_id: i32,
    lb_pair: &LbPair,
    bitmap_extension: Option<&BinArrayBitmapExtension>,
    bin_arrays: &HashMap<i32, BinArray>,
) -> Option<i32> {
    // Use the meteora_dlmm_sdk to find the next bin array index with liquidity
    let lb_pair_anchor = anchor_lang::prelude::Pubkey::from(lb_pair.address.to_bytes());
    
    // Convert to SDK types
    let sdk_lb_pair = crate::pool_data_types::dlmm::functions::to_commons_lb_pair_from_raw(lb_pair);
    let sdk_bitmap_ext = bitmap_extension.map(|ext| {
        crate::pool_data_types::dlmm::functions::to_commons_bitmap_extension_from_raw(ext)
    });
    
    // Use SDK's bitmap traversal to find next bin array index
    let next_index = meteora_dlmm_sdk::quote::find_next_bin_array_index_with_liquidity(
        swap_for_y,
        active_id,
        &sdk_lb_pair,
        sdk_bitmap_ext.as_ref(),
    )?;
    
    // Check if we have this bin array in our cache
    if bin_arrays.contains_key(&(next_index as i32)) {
        Some(next_index as i32)
    } else {
        // Bin array not in cache - partial quote
        None
    }
}
