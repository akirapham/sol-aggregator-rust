use std::collections::HashMap;
use solana_sdk::pubkey::Pubkey;
use solana_streamer_sdk::streaming::event_parser::protocols::meteora_dlmm::types::BinArrayBitmapExtension;
use crate::pool_data_types::meteora_dlmm::MeteoraDlmmPoolState;

/// Convert LbPair to meteora_dlmm_sdk format
pub fn to_commons_lb_pair(pool: &MeteoraDlmmPoolState) -> meteora_dlmm_sdk::dlmm::accounts::LbPair {
    let to_anchor_pubkey = |p: Pubkey| anchor_lang::prelude::Pubkey::from(p.to_bytes());

    meteora_dlmm_sdk::dlmm::accounts::LbPair {
        parameters: meteora_dlmm_sdk::dlmm::types::StaticParameters {
            base_factor: pool.lbpair.parameters.base_factor,
            filter_period: pool.lbpair.parameters.filter_period,
            decay_period: pool.lbpair.parameters.decay_period,
            reduction_factor: pool.lbpair.parameters.reduction_factor,
            variable_fee_control: pool.lbpair.parameters.variable_fee_control,
            max_volatility_accumulator: pool.lbpair.parameters.max_volatility_accumulator,
            min_bin_id: pool.lbpair.parameters.min_bin_id,
            max_bin_id: pool.lbpair.parameters.max_bin_id,
            protocol_share: pool.lbpair.parameters.protocol_share,
            base_fee_power_factor: pool.lbpair.parameters.base_fee_power_factor,
            _padding: pool.lbpair.parameters._padding,
        },
        v_parameters: meteora_dlmm_sdk::dlmm::types::VariableParameters {
            volatility_accumulator: pool.lbpair.v_parameters.volatility_accumulator,
            volatility_reference: pool.lbpair.v_parameters.volatility_reference,
            index_reference: pool.lbpair.v_parameters.index_reference,
            _padding: pool.lbpair.v_parameters._padding,
            last_update_timestamp: pool.lbpair.v_parameters.last_update_timestamp,
            _padding_1: pool.lbpair.v_parameters._padding_1,
        },
        bump_seed: pool.lbpair.bump_seed,
        bin_step_seed: pool.lbpair.bin_step_seed,
        pair_type: pool.lbpair.pair_type,
        active_id: pool.lbpair.active_id,
        bin_step: pool.lbpair.bin_step,
        status: pool.lbpair.status,
        require_base_factor_seed: pool.lbpair.require_base_factor_seed,
        base_factor_seed: pool.lbpair.base_factor_seed,
        activation_type: pool.lbpair.activation_type,
        creator_pool_on_off_control: pool.lbpair.creator_pool_on_off_control,
        token_x_mint: to_anchor_pubkey(pool.lbpair.token_x_mint),
        token_y_mint: to_anchor_pubkey(pool.lbpair.token_y_mint),
        reserve_x: to_anchor_pubkey(pool.lbpair.reserve_x),
        reserve_y: to_anchor_pubkey(pool.lbpair.reserve_y),
        protocol_fee: meteora_dlmm_sdk::dlmm::types::ProtocolFee {
            amount_x: pool.lbpair.protocol_fee.amount_x,
            amount_y: pool.lbpair.protocol_fee.amount_y,
        },
        _padding_1: pool.lbpair._padding_1,
        reward_infos: [
            meteora_dlmm_sdk::dlmm:: types::RewardInfo {
                mint: to_anchor_pubkey(pool.lbpair.reward_infos[0].mint),
                vault: to_anchor_pubkey(pool.lbpair.reward_infos[0].vault),
                funder: to_anchor_pubkey(pool.lbpair.reward_infos[0].funder),
                reward_duration: pool.lbpair.reward_infos[0].reward_duration,
                reward_duration_end: pool.lbpair.reward_infos[0].reward_duration_end,
                reward_rate: pool.lbpair.reward_infos[0].reward_rate,
                last_update_time: pool.lbpair.reward_infos[0].last_update_time,
                cumulative_seconds_with_empty_liquidity_reward: pool.lbpair.reward_infos[0].cumulative_seconds_with_empty_liquidity_reward,
            },
            meteora_dlmm_sdk::dlmm::types::RewardInfo {
                mint: to_anchor_pubkey(pool.lbpair.reward_infos[1].mint),
                vault: to_anchor_pubkey(pool.lbpair.reward_infos[1].vault),
                funder: to_anchor_pubkey(pool.lbpair.reward_infos[1].funder),
                reward_duration: pool.lbpair.reward_infos[1].reward_duration,
                reward_duration_end: pool.lbpair.reward_infos[1].reward_duration_end,
                reward_rate: pool.lbpair.reward_infos[1].reward_rate,
                last_update_time: pool.lbpair.reward_infos[1].last_update_time,
                cumulative_seconds_with_empty_liquidity_reward: pool.lbpair.reward_infos[1].cumulative_seconds_with_empty_liquidity_reward,
            },
        ],
        oracle: to_anchor_pubkey(pool.lbpair.oracle),
        bin_array_bitmap: pool.lbpair.bin_array_bitmap,
        last_updated_at: pool.lbpair.last_updated_at,
        _padding_2: pool.lbpair._padding_2,
        pre_activation_swap_address: to_anchor_pubkey(pool.lbpair.pre_activation_swap_address),
        base_key: to_anchor_pubkey(pool.lbpair.base_key),
        activation_point: pool.lbpair.activation_point,
        pre_activation_duration: pool.lbpair.pre_activation_duration,
        _padding_3: pool.lbpair._padding_3,
        _padding_4: pool.lbpair._padding_4,
        creator: to_anchor_pubkey(pool.lbpair.creator),
        token_mint_x_program_flag: pool.lbpair.token_mint_x_program_flag,
        token_mint_y_program_flag: pool.lbpair.token_mint_y_program_flag,
        _reserved: pool.lbpair._reserved,
    }
}

/// Convert BinArrayBitmapExtension to meteora_dlmm_sdk format
pub fn to_commons_bitmap_extension(_pool: &MeteoraDlmmPoolState, ext: &BinArrayBitmapExtension) -> meteora_dlmm_sdk::dlmm::accounts::BinArrayBitmapExtension {
    let to_anchor_pubkey = |p: Pubkey| anchor_lang::prelude::Pubkey::from(p.to_bytes());
    
    meteora_dlmm_sdk::dlmm::accounts::BinArrayBitmapExtension {
        lb_pair: to_anchor_pubkey(ext.lb_pair),
        positive_bin_array_bitmap: ext.positive_bin_array_bitmap,
        negative_bin_array_bitmap: ext.negative_bin_array_bitmap,
    }
}

/// Get bin arrays in meteora_dlmm_sdk format
pub fn get_commons_bin_arrays(pool: &MeteoraDlmmPoolState) -> HashMap<anchor_lang::prelude::Pubkey, meteora_dlmm_sdk::dlmm::accounts::BinArray> {
    let to_anchor_pubkey = |p: Pubkey| anchor_lang::prelude::Pubkey::from(p.to_bytes());
    let mut bin_arrays = HashMap::new();
    for (index, state) in &pool.bin_arrays {
         let address_anchor = to_anchor_pubkey(pool.address);
         let (pubkey, _) = meteora_dlmm_sdk::pda::derive_bin_array_pda(address_anchor, *index as i64);
         
         let mut bins = [meteora_dlmm_sdk::dlmm::types::Bin::default(); 70];
         for (i, bin) in state.bins.iter().enumerate() {
             if i < 70 {
                 bins[i] = meteora_dlmm_sdk::dlmm::types::Bin {
                     amount_x: bin.amount_x,
                     amount_y: bin.amount_y,
                     price: bin.price,
                     liquidity_supply: bin.liquidity_supply as u128,
                     reward_per_token_stored: bin.reward_per_token_stored,
                     fee_amount_x_per_token_stored: bin.fee_amount_x_per_token_stored,
                     fee_amount_y_per_token_stored: bin.fee_amount_y_per_token_stored,
                     amount_x_in: bin.amount_x_in,
                     amount_y_in: bin.amount_y_in,
                 };
             }
         }

         let bin_array = meteora_dlmm_sdk::dlmm::accounts::BinArray {
             index: state.index as i64,
             version: 0,
             lb_pair: to_anchor_pubkey(pool.address),
             bins,
             _padding: [0; 7],
         };
         bin_arrays.insert(pubkey, bin_array);
    }
    bin_arrays
}
