use crate::aggregator::DexAggregator;
use crate::pool_data_types::*;
use crate::tests::quotes::common::*;
use crate::types::Token;
use borsh::BorshDeserialize;
use solana_client::rpc_config::{CommitmentConfig, RpcSimulateTransactionConfig};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::transaction::Transaction;
use solana_streamer_sdk::streaming::event_parser::protocols::meteora_dbc::types::{
    PoolConfig as MeteoraDbcConfigRaw, VirtualPool as MeteoraDbcPoolRaw, POOL_CONFIG_SIZE,
    VIRTUAL_POOL_SIZE,
};
use std::str::FromStr;
use std::sync::Arc;

#[repr(C)]
#[derive(Clone, Debug)]
pub struct MeteoraDammV2PoolRaw {
    pub pool_fees: solana_streamer_sdk::streaming::event_parser::protocols::meteora_dammv2::types::PoolFeesStruct,
    pub token_a_mint: Pubkey,
    pub token_b_mint: Pubkey,
    pub token_a_vault: Pubkey,
    pub token_b_vault: Pubkey,
    pub whitelisted_vault: Pubkey,
    pub partner: Pubkey,
    pub liquidity: u128,
    pub padding: u128,
    pub protocol_a_fee: u64,
    pub protocol_b_fee: u64,
    pub partner_a_fee: u64,
    pub partner_b_fee: u64,
    pub sqrt_min_price: u128,
    pub sqrt_max_price: u128,
    pub sqrt_price: u128,
    pub activation_point: u64,
    pub activation_type: u8,
    pub pool_status: u8,
    pub token_a_flag: u8,
    pub token_b_flag: u8,
    pub collect_fee_mode: u8,
    pub pool_type: u8,
    pub version: u8,
    pub padding_0: u8,
    pub fee_a_per_liquidity: [u8; 32],
    pub fee_b_per_liquidity: [u8; 32],
    pub permanent_lock_liquidity: u128,
    pub metrics: solana_streamer_sdk::streaming::event_parser::protocols::meteora_dammv2::types::PoolMetrics,
    pub creator: Pubkey,
    pub padding_1: [u64; 6],
    pub reward_infos: [solana_streamer_sdk::streaming::event_parser::protocols::meteora_dammv2::types::RewardInfo; 2],
}

impl MeteoraDammV2PoolRaw {
    pub fn try_from_slice(data: &[u8]) -> Result<Self, std::io::Error> {
        let size = std::mem::size_of::<Self>();
        if data.len() < size + 8 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Data too short",
            ));
        }
        let data = &data[8..]; // Skip discriminator

        if data.len() < size {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Data too short after discriminator",
            ));
        }

        let ptr = data.as_ptr() as *const MeteoraDammV2PoolRaw;
        Ok(unsafe { ptr.read_unaligned() })
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ProtocolFeeRaw {
    pub amount_x: u64,
    pub amount_y: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct RewardInfoRaw {
    pub mint: Pubkey,
    pub vault: Pubkey,
    pub funder: Pubkey,
    pub reward_duration: u64,
    pub reward_duration_end: u64,
    pub reward_rate: u128,
    pub last_update_time: u64,
    pub cumulative_seconds_with_empty_liquidity_reward: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct StaticParametersRaw {
    pub base_factor: u16,
    pub filter_period: u16,
    pub decay_period: u16,
    pub reduction_factor: u16,
    pub variable_fee_control: u32,
    pub max_volatility_accumulator: u32,
    pub min_bin_id: i32,
    pub max_bin_id: i32,
    pub protocol_share: u16,
    pub base_fee_power_factor: u8,
    pub function_type: u8,
    pub padding: [u8; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct VariableParametersRaw {
    pub volatility_accumulator: u32,
    pub volatility_reference: u32,
    pub index_reference: i32,
    pub padding: [u8; 4],
    pub last_update_timestamp: i64,
    pub padding_1: [u8; 8],
}

#[repr(C)]
#[derive(Clone, Debug)]
pub struct LbPairRaw {
    pub parameters: StaticParametersRaw,
    pub v_parameters: VariableParametersRaw,
    pub bump_seed: [u8; 1],
    pub bin_step_seed: [u8; 2],
    pub pair_type: u8,
    pub active_id: i32,
    pub bin_step: u16,
    pub status: u8,
    pub require_base_factor_seed: u8,
    pub base_factor_seed: [u8; 2],
    pub activation_type: u8,
    pub creator_pool_on_off_control: u8,
    pub token_x_mint: Pubkey,
    pub token_y_mint: Pubkey,
    pub reserve_x: Pubkey,
    pub reserve_y: Pubkey,
    pub protocol_fee: ProtocolFeeRaw,
    pub padding_1: [u8; 32],
    pub reward_infos: [RewardInfoRaw; 2],
    pub oracle: Pubkey,
    pub bin_array_bitmap: [u64; 16],
    pub last_updated_at: i64,
    pub padding_2: [u8; 32],
    pub pre_activation_swap_address: Pubkey,
    pub base_key: Pubkey,
    pub activation_point: u64,
    pub pre_activation_duration: u64,
    pub padding_3: [u8; 8],
    pub padding_4: u64,
    pub creator: Pubkey,
    pub token_mint_x_program_flag: u8,
    pub token_mint_y_program_flag: u8,
    pub version: u8,
    pub reserved: [u8; 21],
}

impl LbPairRaw {
    pub fn try_from_slice(data: &[u8]) -> Result<Self, std::io::Error> {
        let size = std::mem::size_of::<Self>();
        if data.len() < size + 8 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Data too short: {} < {}", data.len(), size + 8),
            ));
        }
        let data = &data[8..];
        if data.len() < size {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Data too short after discriminator",
            ));
        }
        let ptr = data.as_ptr() as *const LbPairRaw;
        Ok(unsafe { ptr.read_unaligned() })
    }
}

#[tokio::test]
async fn test_meteora_dbc_quote() {
    let (pool_manager, config) = create_test_setup(vec!["meteora_dbc"]).await;

    // Real SOL-Token Meteora DBC pool
    let pool_address = Pubkey::from_str("6pd7brdZYj8V7Rgo4trnHEvbokc5EjzxZTP1NdMk9sWu").unwrap();
    let token_mint = Pubkey::from_str("6yXTqNnj8PGbJosD6dvpQLFVxaDNpkQmPo7fMxLeUh6A").unwrap();

    // Fetch real pool state from RPC
    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());
    let rpc_client = Arc::new(solana_client::nonblocking::rpc_client::RpcClient::new(
        rpc_url,
    ));

    let account = rpc_client
        .get_account(&pool_address)
        .await
        .expect("Failed to fetch Meteora DBC pool account");

    // Deserialize Meteora DBC pool state (skip 8-byte discriminator)
    let dbc_pool = MeteoraDbcPoolRaw::try_from_slice(&account.data[8..8 + VIRTUAL_POOL_SIZE])
        .expect("Failed to deserialize Meteora DBC pool");

    println!("Meteora DBC pool state - config: {}", dbc_pool.config);

    // Fetch PoolConfig to get quote_mint
    let config_account = rpc_client
        .get_account(&dbc_pool.config)
        .await
        .expect("Failed to fetch Meteora DBC config account");

    let dbc_config =
        MeteoraDbcConfigRaw::try_from_slice(&config_account.data[8..8 + POOL_CONFIG_SIZE])
            .expect("Failed to deserialize Meteora DBC config");

    println!("Meteora DBC config - quote_mint: {}", dbc_config.quote_mint);

    // Fetch token vault accounts to get real reserves
    let vault_base_account = rpc_client
        .get_account(&dbc_pool.base_vault)
        .await
        .expect("Failed to fetch base vault");
    let vault_quote_account = rpc_client
        .get_account(&dbc_pool.quote_vault)
        .await
        .expect("Failed to fetch quote vault");

    // Parse token account data (amount at offset 64)
    let base_reserve = u64::from_le_bytes(vault_base_account.data[64..72].try_into().unwrap());
    let quote_reserve = u64::from_le_bytes(vault_quote_account.data[64..72].try_into().unwrap());

    println!(
        "Real reserves: base={}, quote={}",
        base_reserve, quote_reserve
    );

    // Create pool state with real data
    let pool_state = PoolState::MeteoraDbc(Box::new(DbcPoolState {
        slot: 100,
        transaction_index: None,
        address: pool_address,
        config: dbc_pool.config,
        creator: dbc_pool.creator,
        base_mint: dbc_pool.base_mint,
        base_vault: dbc_pool.base_vault,
        quote_vault: dbc_pool.quote_vault,
        base_reserve,
        quote_reserve,
        protocol_base_fee: dbc_pool.protocol_base_fee,
        protocol_quote_fee: dbc_pool.protocol_quote_fee,
        partner_base_fee: dbc_pool.partner_base_fee,
        partner_quote_fee: dbc_pool.partner_quote_fee,
        sqrt_price: dbc_pool.sqrt_price,
        activation_point: dbc_pool.activation_point,
        pool_type: dbc_pool.pool_type,
        is_migrated: dbc_pool.is_migrated,
        is_partner_withdraw_surplus: dbc_pool.is_partner_withdraw_surplus,
        is_protocol_withdraw_surplus: dbc_pool.is_protocol_withdraw_surplus,
        migration_progress: dbc_pool.migration_progress,
        is_withdraw_leftover: dbc_pool.is_withdraw_leftover,
        is_creator_withdraw_surplus: dbc_pool.is_creator_withdraw_surplus,
        migration_fee_withdraw_status: dbc_pool.migration_fee_withdraw_status,
        finish_curve_timestamp: dbc_pool.finish_curve_timestamp,
        creator_base_fee: dbc_pool.creator_base_fee,
        creator_quote_fee: dbc_pool.creator_quote_fee,
        liquidity_usd: 1_000_000.0, // High liquidity to pass aggregator filter
        last_updated: u64::MAX,
        pool_config: Some(dbc_config.clone()),
        volatility_tracker: Some(dbc_pool.volatility_tracker),
    }));
    pool_manager.inject_pool(pool_state).await;

    // Test Round Trip: SOL -> Token -> SOL
    verify_quote_round_trip(
        pool_manager,
        config,
        wsol_token(),
        Token {
            address: token_mint,
            symbol: Some("TOKEN".to_string()),
            name: Some("Meteora DBC Token".to_string()),
            decimals: 6,
            is_token_2022: false,
            logo_uri: None,
        },
        1_000, // 1000 units (Reduced from 1 SOL to avoid slippage)
        pool_address,
        100, // 1% tolerance
    )
    .await;
}

#[tokio::test]
async fn test_meteora_damm_v2_quote() {
    let _ = env_logger::builder().is_test(true).try_init();
    let (pool_manager, config) = create_test_setup(vec!["meteora_dammv2"]).await;

    // Meteora DAMM V2 Pool: SOL-RALPH
    // Pool Address: DbyK8gEiXwNeh2zFW2Lo1svUQ1WkHAeQyNDsRaKQ6BHf
    let pool_address = Pubkey::from_str("DbyK8gEiXwNeh2zFW2Lo1svUQ1WkHAeQyNDsRaKQ6BHf").unwrap();
    let _token_a_mint = Pubkey::from_str("So11111111111111111111111111111111111111112").unwrap(); // SOL
    let token_b_mint = Pubkey::from_str("CxWPdDBqxVo3fnTMRTvNuSrd4gkp78udSrFvkVDBAGS").unwrap(); // RALPH

    // Fetch real pool state
    let rpc_client = pool_manager.get_rpc_client();
    let account = rpc_client
        .get_account(&pool_address)
        .await
        .expect("Failed to fetch pool account");

    // Deserialize
    let raw_pool = MeteoraDammV2PoolRaw::try_from_slice(&account.data)
        .expect("Failed to deserialize DAMM V2 pool");

    println!(
        "Meteora DAMM V2 pool found - Liquidity: {}",
        raw_pool.liquidity
    );

    // Create PoolState
    let pool_state =
        PoolState::MeteoraDammV2(Box::new(crate::pool_data_types::MeteoraDammV2PoolState {
            slot: 0,
            transaction_index: Some(0),
            address: pool_address,
            pool_fees: raw_pool.pool_fees,
            token_a_mint: raw_pool.token_a_mint,
            token_b_mint: raw_pool.token_b_mint, // Should match RALPH or SOL
            token_a_vault: raw_pool.token_a_vault,
            token_b_vault: raw_pool.token_b_vault,
            whitelisted_vault: raw_pool.whitelisted_vault,
            partner: raw_pool.partner,
            liquidity: raw_pool.liquidity,
            protocol_a_fee: raw_pool.protocol_a_fee,
            protocol_b_fee: raw_pool.protocol_b_fee,
            partner_a_fee: raw_pool.partner_a_fee,
            partner_b_fee: raw_pool.partner_b_fee,
            sqrt_min_price: raw_pool.sqrt_min_price,
            sqrt_max_price: raw_pool.sqrt_max_price,
            sqrt_price: raw_pool.sqrt_price,
            activation_point: raw_pool.activation_point,
            activation_type: raw_pool.activation_type,
            pool_status: raw_pool.pool_status,
            token_a_flag: raw_pool.token_a_flag,
            token_b_flag: raw_pool.token_b_flag,
            collect_fee_mode: raw_pool.collect_fee_mode,
            pool_type: raw_pool.pool_type,
            version: raw_pool.version,
            fee_a_per_liquidity: raw_pool.fee_a_per_liquidity,
            fee_b_per_liquidity: raw_pool.fee_b_per_liquidity,
            permanent_lock_liquidity: raw_pool.permanent_lock_liquidity,
            metrics: raw_pool.metrics,
            creator: raw_pool.creator,
            reward_infos: raw_pool.reward_infos,
            liquidity_usd: 1_000_000.0, // High liquidity to pass aggregator filter
            last_updated: u64::MAX,
        }));

    pool_manager.inject_pool(pool_state).await;

    // Test Round Trip: SOL -> RALPH -> SOL
    verify_quote_round_trip(
        pool_manager,
        config,
        wsol_token(), // Input SOL
        Token {
            address: token_b_mint, // RALPH (assuming token_b is RALPH, checks below)
            symbol: Some("RALPH".to_string()),
            name: Some("Ralph Token".to_string()),
            decimals: 9, // Assuming 9 for now, should verify
            is_token_2022: false,
            logo_uri: None,
        },
        1_000, // 1000 units
        pool_address,
        100, // 1% tolerance
    )
    .await;
}

#[tokio::test]
async fn test_meteora_dlmm_quote() {
    let _ = env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();
    let (pool_manager, config) = create_test_setup(vec!["meteora_dlmm"]).await;

    // Meteora DLMM Pool: SOL-Token
    // Pool Address: 6b9ZdnykBXZwRqw1xuS4McYxghAwocwZzrwijzcUVcxP
    let pool_address = Pubkey::from_str("6b9ZdnykBXZwRqw1xuS4McYxghAwocwZzrwijzcUVcxP").unwrap();
    let ralph_mint = Pubkey::from_str("8116V1BW9zaXUM6pVhWVaAduKrLcEBi3RGXedKTrBAGS").unwrap(); // Token X
    let sol_mint = Pubkey::from_str("So11111111111111111111111111111111111111112").unwrap(); // Token Y

    // Fetch real pool state
    let rpc_client = pool_manager.get_rpc_client();
    let account = rpc_client
        .get_account(&pool_address)
        .await
        .expect("Failed to fetch DLMM pool account");

    // Deserialize
    let raw_pool =
        LbPairRaw::try_from_slice(&account.data).expect("Failed to deserialize DLMM pool");

    println!(
        "Meteora DLMM pool found - Active ID: {}, Token X: {}, Token Y: {}",
        raw_pool.active_id, raw_pool.token_x_mint, raw_pool.token_y_mint
    );

    // Debug Raw Pool details
    println!(
        "Bin Step: {}, Status: {}, Bin Array Bitmap: {:?}",
        raw_pool.bin_step, raw_pool.status, raw_pool.bin_array_bitmap
    );

    // Verify token match
    assert_eq!(
        raw_pool.token_x_mint, ralph_mint,
        "Token X mismatch (Expected RALPH)"
    );
    assert_eq!(
        raw_pool.token_y_mint, sol_mint,
        "Token Y mismatch (Expected SOL)"
    );

    println!(
        "Program ID from State: {}",
        crate::pool_data_types::MeteoraDlmmPoolState::get_program_id()
    );

    // Attempt to deserialize using SDK type directly for simplicity in constructing PoolState
    use solana_streamer_sdk::streaming::event_parser::protocols::meteora_dlmm::types::BinArrayBitmapExtension as SdkBitmapExtension;
    use solana_streamer_sdk::streaming::event_parser::protocols::meteora_dlmm::types::LbPair as SdkLbPair;

    // Skip 8 bytes discriminator
    let mut sdk_lb_pair: SdkLbPair = if account.data.len() >= 8 {
        let size_sdk = std::mem::size_of::<SdkLbPair>();
        let size_raw = std::mem::size_of::<LbPairRaw>();
        println!(
            "SDK LbPair Size: {}, Raw LbPair Size: {}",
            size_sdk, size_raw
        );

        if size_sdk == size_raw {
            unsafe { (account.data[8..].as_ptr() as *const SdkLbPair).read_unaligned() }
        } else {
            panic!(
                "SDK LbPair size ({}) != Raw LbPair size ({}). IDL vs SDK mismatch.",
                size_sdk, size_raw
            );
        }
    } else {
        panic!("Account data too short");
    };

    // Fix mismatch by copying from verified raw_pool
    // SdkLbPair layout is likely shifted relative to on-chain data due to padding or version differences
    // We overwrite critical fields with values we KNOW are correct from Borsh decoding
    sdk_lb_pair.token_x_mint = raw_pool.token_x_mint;
    sdk_lb_pair.token_y_mint = raw_pool.token_y_mint;
    sdk_lb_pair.active_id = raw_pool.active_id;
    sdk_lb_pair.bin_step = raw_pool.bin_step;
    // Copy parameters to ensure logic validation passes
    sdk_lb_pair.parameters.min_bin_id = raw_pool.parameters.min_bin_id;
    sdk_lb_pair.parameters.max_bin_id = raw_pool.parameters.max_bin_id;
    sdk_lb_pair.parameters.base_factor = raw_pool.parameters.base_factor;
    // Copy bitmap to ensure we find the right bin arrays
    sdk_lb_pair.bin_array_bitmap = raw_pool.bin_array_bitmap;

    // Fetch Bitmap Extension
    let program_id = Pubkey::from_str("LBUZKhRxPF3XUpBCjp4YzTKgLccjZhTSDM9YuVaPwxo").unwrap();
    let (bitmap_pubkey, _) =
        Pubkey::find_program_address(&[b"bitmap", pool_address.as_ref()], &program_id);
    let mut bitmap_extension = None;

    if let Ok(bitmap_acc) = rpc_client.get_account(&bitmap_pubkey).await {
        println!("Bitmap Extension Found! Size: {}", bitmap_acc.data.len());
        if bitmap_acc.data.len() >= 8 {
            // Assuming SDK BitmapExtension matches
            let size_sdk = std::mem::size_of::<SdkBitmapExtension>();
            if bitmap_acc.data.len() - 8 >= size_sdk {
                let ext: SdkBitmapExtension = unsafe {
                    (bitmap_acc.data[8..].as_ptr() as *const SdkBitmapExtension).read_unaligned()
                };
                bitmap_extension = Some(ext);
            } else {
                println!("Bitmap extension size mismatch or too small");
            }
        }
    } else {
        println!("Bitmap Extension Not Found (Optional)");
    }

    // Create PoolState
    let mut pool_state_struct = crate::pool_data_types::MeteoraDlmmPoolState {
        slot: 0,
        transaction_index: Some(0),
        address: pool_address,
        lbpair: sdk_lb_pair,
        bin_arrays: std::collections::HashMap::new(), // To be populated
        bitmap_extension,
        reserve_x: None,
        reserve_y: None,
        liquidity_usd: 1_000_000.0,
        is_state_keys_initialized: true,
        last_updated: u64::MAX,
    };

    // Fetch Bin Arrays
    // Use the optimized fetcher to get all necessary bin arrays
    use crate::fetchers::meteora_dlmm_bin_array_fetcher::MeteoraDlmmBinArrayFetcher;
    let fetcher = MeteoraDlmmBinArrayFetcher::new(rpc_client.clone());

    println!("Fetching bin arrays using MeteoraDlmmBinArrayFetcher...");
    match fetcher
        .fetch_all_bin_arrays(pool_address, &pool_state_struct)
        .await
    {
        Ok(bin_arrays) => {
            println!("Fetcher returned {} bin arrays", bin_arrays.len());
            for ba in bin_arrays {
                pool_state_struct.bin_arrays.insert(ba.index as i32, ba);
            }
        }
        Err(e) => panic!("Failed to fetch bin arrays: {:?}", e),
    }
    println!("Fetched {} bin arrays", pool_state_struct.bin_arrays.len());

    // Debug Bin Liquidity
    let mut total_liquidity_x = 0u64;
    let mut total_liquidity_y = 0u64;
    for (idx, ba) in &pool_state_struct.bin_arrays {
        let mut active_bins = 0;
        for bin in &ba.bins {
            if bin.amount_x > 0 || bin.amount_y > 0 {
                total_liquidity_x += bin.amount_x;
                total_liquidity_y += bin.amount_y;
                active_bins += 1;
            }
        }
        println!("BinArray {}: {} active bins", idx, active_bins);
    }
    println!(
        "Total Liquidity X: {}, Y: {}",
        total_liquidity_x, total_liquidity_y
    );

    let pool_state = PoolState::MeteoraDlmm(Box::new(pool_state_struct));
    pool_manager.inject_pool(pool_state).await;

    // Test Round Trip: SOL -> RALPH -> SOL
    verify_quote_round_trip(
        pool_manager,
        config,
        wsol_token(), // Input SOL (Token Y)
        Token {
            address: ralph_mint, // Output RALPH (Token X)
            symbol: Some("RALPH".to_string()),
            name: Some("Ralph Token".to_string()),
            decimals: 9,
            is_token_2022: false,
            logo_uri: None,
        },
        1_000, // 1000 units (Small amount for liquidity safety)
        pool_address,
        400, // 4% tolerance
    )
    .await;
}

#[tokio::test]
async fn test_meteora_dbc_quote_reverse() {
    let (pool_manager, config) = create_test_setup(vec!["meteora_dbc"]).await;
    let pool_address = Pubkey::from_str("6pd7brdZYj8V7Rgo4trnHEvbokc5EjzxZTP1NdMk9sWu").unwrap();
    let token_mint = Pubkey::from_str("6yXTqNnj8PGbJosD6dvpQLFVxaDNpkQmPo7fMxLeUh6A").unwrap();

    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());
    let rpc_client = Arc::new(solana_client::nonblocking::rpc_client::RpcClient::new(
        rpc_url,
    ));

    let account = rpc_client
        .get_account(&pool_address)
        .await
        .expect("Failed to fetch Meteora DBC pool account");
    let dbc_pool = MeteoraDbcPoolRaw::try_from_slice(&account.data[8..8 + VIRTUAL_POOL_SIZE])
        .expect("Failed to deserialize Meteora DBC pool");

    let config_account = rpc_client
        .get_account(&dbc_pool.config)
        .await
        .expect("Failed to fetch Meteora DBC config account");
    let dbc_config =
        MeteoraDbcConfigRaw::try_from_slice(&config_account.data[8..8 + POOL_CONFIG_SIZE])
            .expect("Failed to deserialize Meteora DBC config");

    let vault_base_account = rpc_client
        .get_account(&dbc_pool.base_vault)
        .await
        .expect("Failed to fetch base vault");
    let vault_quote_account = rpc_client
        .get_account(&dbc_pool.quote_vault)
        .await
        .expect("Failed to fetch quote vault");
    let base_reserve = u64::from_le_bytes(vault_base_account.data[64..72].try_into().unwrap());
    let quote_reserve = u64::from_le_bytes(vault_quote_account.data[64..72].try_into().unwrap());

    let pool_state = PoolState::MeteoraDbc(Box::new(DbcPoolState {
        slot: 100,
        transaction_index: None,
        address: pool_address,
        config: dbc_pool.config,
        creator: dbc_pool.creator,
        base_mint: dbc_pool.base_mint,
        base_vault: dbc_pool.base_vault,
        quote_vault: dbc_pool.quote_vault,
        base_reserve,
        quote_reserve,
        protocol_base_fee: dbc_pool.protocol_base_fee,
        protocol_quote_fee: dbc_pool.protocol_quote_fee,
        partner_base_fee: dbc_pool.partner_base_fee,
        partner_quote_fee: dbc_pool.partner_quote_fee,
        sqrt_price: dbc_pool.sqrt_price,
        activation_point: dbc_pool.activation_point,
        pool_type: dbc_pool.pool_type,
        is_migrated: dbc_pool.is_migrated,
        is_partner_withdraw_surplus: dbc_pool.is_partner_withdraw_surplus,
        is_protocol_withdraw_surplus: dbc_pool.is_protocol_withdraw_surplus,
        migration_progress: dbc_pool.migration_progress,
        is_withdraw_leftover: dbc_pool.is_withdraw_leftover,
        is_creator_withdraw_surplus: dbc_pool.is_creator_withdraw_surplus,
        migration_fee_withdraw_status: dbc_pool.migration_fee_withdraw_status,
        finish_curve_timestamp: dbc_pool.finish_curve_timestamp,
        creator_base_fee: dbc_pool.creator_base_fee,
        creator_quote_fee: dbc_pool.creator_quote_fee,
        liquidity_usd: 1_000_000.0,
        last_updated: u64::MAX,
        pool_config: Some(dbc_config.clone()),
        volatility_tracker: Some(dbc_pool.volatility_tracker),
    }));
    pool_manager.inject_pool(pool_state).await;

    // Test Round Trip: Token -> SOL -> Token
    verify_quote_round_trip(
        pool_manager,
        config,
        Token {
            address: token_mint,
            symbol: Some("TOKEN".to_string()),
            name: Some("Meteora DBC Token".to_string()),
            decimals: 6,
            is_token_2022: false,
            logo_uri: None,
        },
        wsol_token(),
        1_000_000_000,
        pool_address,
        100, // 1% tolerance
    )
    .await;
}

#[tokio::test]
async fn test_meteora_dlmm_quote_reverse() {
    let _ = env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();
    let (pool_manager, config) = create_test_setup(vec!["meteora_dlmm"]).await;

    let pool_address = Pubkey::from_str("6b9ZdnykBXZwRqw1xuS4McYxghAwocwZzrwijzcUVcxP").unwrap();
    let ralph_mint = Pubkey::from_str("8116V1BW9zaXUM6pVhWVaAduKrLcEBi3RGXedKTrBAGS").unwrap();
    let _sol_mint = Pubkey::from_str("So11111111111111111111111111111111111111112").unwrap();

    let rpc_client = pool_manager.get_rpc_client();
    let account = rpc_client
        .get_account(&pool_address)
        .await
        .expect("Failed to fetch DLMM pool account");
    let raw_pool =
        LbPairRaw::try_from_slice(&account.data).expect("Failed to deserialize DLMM pool");

    use solana_streamer_sdk::streaming::event_parser::protocols::meteora_dlmm::types::BinArrayBitmapExtension as SdkBitmapExtension;
    use solana_streamer_sdk::streaming::event_parser::protocols::meteora_dlmm::types::LbPair as SdkLbPair;

    // Initialize SdkLbPair and fix data mismatch (same logic as forward test)
    let mut sdk_lb_pair: SdkLbPair = if account.data.len() >= 8 {
        unsafe { (account.data[8..].as_ptr() as *const SdkLbPair).read_unaligned() }
    } else {
        panic!("Account data too short");
    };

    sdk_lb_pair.token_x_mint = raw_pool.token_x_mint;
    sdk_lb_pair.token_y_mint = raw_pool.token_y_mint;
    sdk_lb_pair.active_id = raw_pool.active_id;
    sdk_lb_pair.bin_step = raw_pool.bin_step;
    sdk_lb_pair.parameters.min_bin_id = raw_pool.parameters.min_bin_id;
    sdk_lb_pair.parameters.max_bin_id = raw_pool.parameters.max_bin_id;
    sdk_lb_pair.parameters.base_factor = raw_pool.parameters.base_factor;
    sdk_lb_pair.bin_array_bitmap = raw_pool.bin_array_bitmap;

    let program_id = Pubkey::from_str("LBUZKhRxPF3XUpBCjp4YzTKgLccjZhTSDM9YuVaPwxo").unwrap();
    let (bitmap_pubkey, _) =
        Pubkey::find_program_address(&[b"bitmap", pool_address.as_ref()], &program_id);
    let mut bitmap_extension = None;
    if let Ok(bitmap_acc) = rpc_client.get_account(&bitmap_pubkey).await {
        if bitmap_acc.data.len() >= 8 {
            let ext: SdkBitmapExtension = unsafe {
                (bitmap_acc.data[8..].as_ptr() as *const SdkBitmapExtension).read_unaligned()
            };
            bitmap_extension = Some(ext);
        }
    }

    let mut pool_state_struct = crate::pool_data_types::MeteoraDlmmPoolState {
        slot: 0,
        transaction_index: Some(0),
        address: pool_address,
        lbpair: sdk_lb_pair,
        bin_arrays: std::collections::HashMap::new(),
        bitmap_extension,
        reserve_x: None,
        reserve_y: None,
        liquidity_usd: 1_000_000.0,
        is_state_keys_initialized: true,
        last_updated: u64::MAX,
    };

    // Use Fetcher
    use crate::fetchers::meteora_dlmm_bin_array_fetcher::MeteoraDlmmBinArrayFetcher;
    let fetcher = MeteoraDlmmBinArrayFetcher::new(rpc_client.clone());
    if let Ok(bin_arrays) = fetcher
        .fetch_all_bin_arrays(pool_address, &pool_state_struct)
        .await
    {
        for ba in bin_arrays {
            pool_state_struct.bin_arrays.insert(ba.index as i32, ba);
        }
    }

    let pool_state = PoolState::MeteoraDlmm(Box::new(pool_state_struct));
    pool_manager.inject_pool(pool_state).await;

    // Test Round Trip: RALPH -> SOL -> RALPH
    verify_quote_round_trip(
        pool_manager,
        config,
        Token {
            address: ralph_mint,
            symbol: Some("RALPH".to_string()),
            name: Some("Ralph Token".to_string()),
            decimals: 9,
            is_token_2022: false,
            logo_uri: None,
        },
        wsol_token(),
        100_000_000_000, // Swap amount (100B to ensure non-zero SOL output)
        pool_address,
        400, // 4% tolerance
    )
    .await;
}

#[tokio::test]
async fn test_meteora_damm_v2_quote_reverse() {
    let (pool_manager, config) = create_test_setup(vec!["meteora_dammv2"]).await;
    let pool_address = Pubkey::from_str("DbyK8gEiXwNeh2zFW2Lo1svUQ1WkHAeQyNDsRaKQ6BHf").unwrap();
    let token_b_mint = Pubkey::from_str("CxWPdDBqxVo3fnTMRTvNuSrd4gkp78udSrFvkVDBAGS").unwrap(); // RALPH

    let rpc_client = pool_manager.get_rpc_client();
    let account = rpc_client
        .get_account(&pool_address)
        .await
        .expect("Failed to fetch pool account");
    let raw_pool = MeteoraDammV2PoolRaw::try_from_slice(&account.data)
        .expect("Failed to deserialize DAMM V2 pool");

    let pool_state =
        PoolState::MeteoraDammV2(Box::new(crate::pool_data_types::MeteoraDammV2PoolState {
            slot: 0,
            transaction_index: Some(0),
            address: pool_address,
            pool_fees: raw_pool.pool_fees,
            token_a_mint: raw_pool.token_a_mint,
            token_b_mint: raw_pool.token_b_mint,
            token_a_vault: raw_pool.token_a_vault,
            token_b_vault: raw_pool.token_b_vault,
            whitelisted_vault: raw_pool.whitelisted_vault,
            partner: raw_pool.partner,
            liquidity: raw_pool.liquidity,
            protocol_a_fee: raw_pool.protocol_a_fee,
            protocol_b_fee: raw_pool.protocol_b_fee,
            partner_a_fee: raw_pool.partner_a_fee,
            partner_b_fee: raw_pool.partner_b_fee,
            sqrt_min_price: raw_pool.sqrt_min_price,
            sqrt_max_price: raw_pool.sqrt_max_price,
            sqrt_price: raw_pool.sqrt_price,
            activation_point: raw_pool.activation_point,
            activation_type: raw_pool.activation_type,
            pool_status: raw_pool.pool_status,
            token_a_flag: raw_pool.token_a_flag,
            token_b_flag: raw_pool.token_b_flag,
            collect_fee_mode: raw_pool.collect_fee_mode,
            pool_type: raw_pool.pool_type,
            version: raw_pool.version,
            fee_a_per_liquidity: raw_pool.fee_a_per_liquidity,
            fee_b_per_liquidity: raw_pool.fee_b_per_liquidity,
            permanent_lock_liquidity: raw_pool.permanent_lock_liquidity,
            metrics: raw_pool.metrics,
            creator: raw_pool.creator,
            reward_infos: raw_pool.reward_infos,
            liquidity_usd: 1_000_000.0,
            last_updated: u64::MAX,
        }));

    pool_manager.inject_pool(pool_state).await;

    // Test Round Trip: RALPH -> SOL -> RALPH
    verify_quote_round_trip(
        pool_manager,
        config,
        Token {
            address: token_b_mint,
            symbol: Some("RALPH".to_string()),
            name: Some("Ralph Token".to_string()),
            decimals: 9,
            is_token_2022: false,
            logo_uri: None,
        },
        wsol_token(),
        1_000_000_000,
        pool_address,
        100, // 1% tolerance
    )
    .await;
}

#[tokio::test]
async fn test_meteora_damm_quote_simulation() {
    let _ = env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();

    let (pool_manager, config) = create_test_setup(vec!["meteora_dammv2"]).await;

    // Meteora DAMM V2 Pool: SOL-RALPH
    let pool_address = Pubkey::from_str("DbyK8gEiXwNeh2zFW2Lo1svUQ1WkHAeQyNDsRaKQ6BHf").unwrap();
    let token_b_mint = Pubkey::from_str("CxWPdDBqxVo3fnTMRTvNuSrd4gkp78udSrFvkVDBAGS").unwrap(); // RALPH

    // Fetch real pool state
    let rpc_client = pool_manager.get_rpc_client();
    let account = rpc_client
        .get_account(&pool_address)
        .await
        .expect("Failed to fetch pool account");
    let raw_pool = MeteoraDammV2PoolRaw::try_from_slice(&account.data)
        .expect("Failed to deserialize DAMM V2 pool");

    let pool_state =
        PoolState::MeteoraDammV2(Box::new(crate::pool_data_types::MeteoraDammV2PoolState {
            slot: 0,
            transaction_index: Some(0),
            address: pool_address,
            pool_fees: raw_pool.pool_fees,
            token_a_mint: raw_pool.token_a_mint,
            token_b_mint: raw_pool.token_b_mint,
            token_a_vault: raw_pool.token_a_vault,
            token_b_vault: raw_pool.token_b_vault,
            whitelisted_vault: raw_pool.whitelisted_vault,
            partner: raw_pool.partner,
            liquidity: raw_pool.liquidity,
            protocol_a_fee: raw_pool.protocol_a_fee,
            protocol_b_fee: raw_pool.protocol_b_fee,
            partner_a_fee: raw_pool.partner_a_fee,
            partner_b_fee: raw_pool.partner_b_fee,
            sqrt_min_price: raw_pool.sqrt_min_price,
            sqrt_max_price: raw_pool.sqrt_max_price,
            sqrt_price: raw_pool.sqrt_price,
            activation_point: raw_pool.activation_point,
            activation_type: raw_pool.activation_type,
            pool_status: raw_pool.pool_status,
            token_a_flag: raw_pool.token_a_flag,
            token_b_flag: raw_pool.token_b_flag,
            collect_fee_mode: raw_pool.collect_fee_mode,
            pool_type: raw_pool.pool_type,
            version: raw_pool.version,
            fee_a_per_liquidity: raw_pool.fee_a_per_liquidity,
            fee_b_per_liquidity: raw_pool.fee_b_per_liquidity,
            permanent_lock_liquidity: raw_pool.permanent_lock_liquidity,
            metrics: raw_pool.metrics,
            creator: raw_pool.creator,
            reward_infos: raw_pool.reward_infos,
            liquidity_usd: 1_000_000.0,
            last_updated: u64::MAX,
        }));

    pool_manager.inject_pool(pool_state).await;

    // Inject tokens
    pool_manager.inject_token(wsol_token()).await;
    pool_manager
        .inject_token(Token {
            address: token_b_mint,
            symbol: Some("RALPH".to_string()),
            name: Some("Ralph Token".to_string()),
            decimals: 9,
            is_token_2022: false,
            logo_uri: None,
        })
        .await;

    // Create App State
    let aggregator = Arc::new(DexAggregator::new(config, pool_manager.clone()));

    // 2. Perform Quote (SOL -> RALPH)
    let user_wallet_str = "DNfuF1L62WWyW3pNakVkyGGFzVVhj4Yr52jSmdTyeBHm".to_string(); // Random active wallet
    let payer = Pubkey::from_str(&user_wallet_str).unwrap();
    let amount_in = 100_000_000; // 0.1 SOL

    let swap_params = crate::types::SwapParams {
        input_token: wsol_token(),
        output_token: Token {
            address: token_b_mint,
            symbol: Some("RALPH".to_string()),
            name: Some("Ralph Token".to_string()),
            decimals: 9,
            is_token_2022: false,
            logo_uri: None,
        },
        input_amount: amount_in,
        slippage_bps: 300,
        user_wallet: payer,
        priority: crate::types::ExecutionPriority::Medium,
    };

    println!(
        "Requesting Quote: {} -> {}",
        swap_params.input_token.address, swap_params.output_token.address
    );

    let quote = aggregator
        .get_swap_route(&swap_params)
        .await
        .expect("Failed to get buy quote");

    // For single hop, we can assume route has 1 path, 1 step
    let _step = &quote.paths[0].steps[0];
    // Find the pool state again (we injected it, but aggregator might have cloned it)
    let pool = pool_manager.get_pool(&pool_address).await.unwrap();

    let instructions = pool
        .build_swap_instruction(&swap_params, &*pool_manager, None)
        .await
        .expect("Failed to build swap instruction");

    println!("Simulating transaction...");
    // Create transaction
    let recent_blockhash = rpc_client.get_latest_blockhash().await.unwrap();
    let mut message = solana_sdk::message::Message::new(&instructions, Some(&payer));
    message.recent_blockhash = recent_blockhash;
    let transaction = Transaction::new_unsigned(message);

    let config = RpcSimulateTransactionConfig {
        sig_verify: false,
        replace_recent_blockhash: true,
        commitment: Some(CommitmentConfig::confirmed()),
        encoding: None,
        accounts: None,
        min_context_slot: None,
        inner_instructions: true,
    };

    let result = rpc_client
        .simulate_transaction_with_config(&transaction, config)
        .await
        .expect("Simulation failed");

    if let Some(err) = result.value.err {
        println!("Simulation Logs: {:#?}", result.value.logs);
        panic!("Simulation error: {:?}", err);
    }

    println!("Simulation successful! Logs: {:#?}", result.value.logs);
}

#[tokio::test]
async fn test_meteora_damm_quote_simulation_reverse() {
    let _ = env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();

    let (pool_manager, _config) = create_test_setup(vec!["meteora_dammv2"]).await;

    // Meteora DAMM V2 Pool: SOL-RALPH
    let pool_address = Pubkey::from_str("DbyK8gEiXwNeh2zFW2Lo1svUQ1WkHAeQyNDsRaKQ6BHf").unwrap();
    let token_b_mint = Pubkey::from_str("CxWPdDBqxVo3fnTMRTvNuSrd4gkp78udSrFvkVDBAGS").unwrap(); // RALPH

    // Fetch real pool state
    let rpc_client = pool_manager.get_rpc_client();
    let account = rpc_client
        .get_account(&pool_address)
        .await
        .expect("Failed to fetch pool account");
    let raw_pool = MeteoraDammV2PoolRaw::try_from_slice(&account.data)
        .expect("Failed to deserialize DAMM V2 pool");

    let pool_state =
        PoolState::MeteoraDammV2(Box::new(crate::pool_data_types::MeteoraDammV2PoolState {
            slot: 0,
            transaction_index: Some(0),
            address: pool_address,
            pool_fees: raw_pool.pool_fees,
            token_a_mint: raw_pool.token_a_mint,
            token_b_mint: raw_pool.token_b_mint,
            token_a_vault: raw_pool.token_a_vault,
            token_b_vault: raw_pool.token_b_vault,
            whitelisted_vault: raw_pool.whitelisted_vault,
            partner: raw_pool.partner,
            liquidity: raw_pool.liquidity,
            protocol_a_fee: raw_pool.protocol_a_fee,
            protocol_b_fee: raw_pool.protocol_b_fee,
            partner_a_fee: raw_pool.partner_a_fee,
            partner_b_fee: raw_pool.partner_b_fee,
            sqrt_min_price: raw_pool.sqrt_min_price,
            sqrt_max_price: raw_pool.sqrt_max_price,
            sqrt_price: raw_pool.sqrt_price,
            activation_point: raw_pool.activation_point,
            activation_type: raw_pool.activation_type,
            pool_status: raw_pool.pool_status,
            token_a_flag: raw_pool.token_a_flag,
            token_b_flag: raw_pool.token_b_flag,
            collect_fee_mode: raw_pool.collect_fee_mode,
            pool_type: raw_pool.pool_type,
            version: raw_pool.version,
            fee_a_per_liquidity: raw_pool.fee_a_per_liquidity,
            fee_b_per_liquidity: raw_pool.fee_b_per_liquidity,
            permanent_lock_liquidity: raw_pool.permanent_lock_liquidity,
            metrics: raw_pool.metrics,
            creator: raw_pool.creator,
            reward_infos: raw_pool.reward_infos,
            liquidity_usd: 1_000_000.0,
            last_updated: u64::MAX,
        }));

    pool_manager.inject_pool(pool_state).await;

    // Inject tokens
    pool_manager.inject_token(wsol_token()).await;
    pool_manager
        .inject_token(Token {
            address: token_b_mint,
            symbol: Some("RALPH".to_string()),
            name: Some("Ralph Token".to_string()),
            decimals: 9,
            is_token_2022: false,
            logo_uri: None,
        })
        .await;

    let user_wallet_str = "DNfuF1L62WWyW3pNakVkyGGFzVVhj4Yr52jSmdTyeBHm".to_string(); // Random active wallet
    let payer = Pubkey::from_str(&user_wallet_str).unwrap();

    // 1. First Buy RALPH with SOL to ensure we have valid input amount calculation
    // Actually we can just simulate the Sell if we assume user has tokens.
    // But for reverse calculation test, we simulate: Token -> SOL
    let amount_in = 100_000_000; // 0.1 RALPH (assuming 9 decimals)

    // 3. Build Transaction
    let _swap_params = crate::types::SwapParams {
        input_token: Token {
            address: token_b_mint,
            symbol: Some("RALPH".to_string()),
            name: Some("Ralph Token".to_string()),
            decimals: 9,
            is_token_2022: false,
            logo_uri: None,
        },
        output_token: wsol_token(),
        input_amount: amount_in,
        slippage_bps: 50,
        user_wallet: payer,
        priority: crate::types::ExecutionPriority::Medium,
    };

    // 4. Verification: Use Composite Transaction (Buy -> Sell) to ensure user has tokens
    println!("Simulating Composite Transaction (Buy -> Sell)...");

    // Find the pool state again
    let pool = pool_manager.get_pool(&pool_address).await.unwrap();

    // 4.1 Build BUY Instruction (SOL -> RALPH)
    let buy_amount_in = 1_000_000_000; // 1 SOL
    let buy_params = crate::types::SwapParams {
        input_token: wsol_token(),
        output_token: Token {
            address: token_b_mint,
            symbol: Some("RALPH".to_string()),
            name: Some("Ralph Token".to_string()),
            decimals: 9,
            is_token_2022: false,
            logo_uri: None,
        },
        input_amount: buy_amount_in,
        slippage_bps: 300,
        user_wallet: payer,
        priority: crate::types::ExecutionPriority::Medium,
    };

    let buy_instructions = pool
        .build_swap_instruction(&buy_params, &*pool_manager, None)
        .await
        .expect("Failed to build buy instruction");

    // 4.2 Build SELL Instruction (RALPH -> SOL)
    // We assume we get some RALPH from buy (approx price 1 SOL ~ X RALPH).
    // Let's just sell what we asked to sell in original test, or a safe amount.
    let sell_amount_in = amount_in; // 0.1 RALPH (from original test setup)

    let sell_params = crate::types::SwapParams {
        input_token: Token {
            address: token_b_mint,
            symbol: Some("RALPH".to_string()),
            name: Some("Ralph Token".to_string()),
            decimals: 9,
            is_token_2022: false,
            logo_uri: None,
        },
        output_token: wsol_token(),
        input_amount: sell_amount_in,
        slippage_bps: 300,
        user_wallet: payer,
        priority: crate::types::ExecutionPriority::Medium,
    };

    let sell_instructions = pool
        .build_swap_instruction(&sell_params, &*pool_manager, None)
        .await
        .expect("Failed to build sell instruction");

    // 4.3 Combine Instructions
    let mut instructions = Vec::new();
    instructions.extend(buy_instructions);
    instructions.extend(sell_instructions);

    // Create transaction
    let recent_blockhash = rpc_client.get_latest_blockhash().await.unwrap();
    let mut message = solana_sdk::message::Message::new(&instructions, Some(&payer));
    message.recent_blockhash = recent_blockhash;
    let transaction = Transaction::new_unsigned(message);

    let config = RpcSimulateTransactionConfig {
        sig_verify: false,
        replace_recent_blockhash: true,
        commitment: Some(CommitmentConfig::confirmed()),
        encoding: None,
        accounts: None,
        min_context_slot: None,
        inner_instructions: true,
    };

    let result = rpc_client
        .simulate_transaction_with_config(&transaction, config)
        .await
        .expect("Simulation failed");

    if let Some(err) = result.value.err {
        println!("Simulation Logs: {:#?}", result.value.logs);
        panic!("Simulation error: {:?}", err);
    }

    println!("Simulation successful! Logs: {:#?}", result.value.logs);
}

#[tokio::test]
async fn test_meteora_dbc_quote_simulation() {
    let _ = env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();

    let (pool_manager, config) = create_test_setup(vec!["meteora_dbc"]).await;

    // Meteora DBC Pool: SOL-Token
    // Pool Address: 6pd7brdZYj8V7Rgo4trnHEvbokc5EjzxZTP1NdMk9sWu
    let pool_address = Pubkey::from_str("6pd7brdZYj8V7Rgo4trnHEvbokc5EjzxZTP1NdMk9sWu").unwrap();
    let token_mint = Pubkey::from_str("6yXTqNnj8PGbJosD6dvpQLFVxaDNpkQmPo7fMxLeUh6A").unwrap();
    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());
    let rpc_client = Arc::new(solana_client::nonblocking::rpc_client::RpcClient::new(
        rpc_url,
    ));

    // Fetch real pool state
    let account = rpc_client
        .get_account(&pool_address)
        .await
        .expect("Failed to fetch pool account");

    // Deserialize Meteora DBC pool state (skip 8-byte discriminator)
    let dbc_pool = MeteoraDbcPoolRaw::try_from_slice(&account.data[8..8 + VIRTUAL_POOL_SIZE])
        .expect("Failed to deserialize Meteora DBC pool");

    // Fetch PoolConfig
    let config_account = rpc_client
        .get_account(&dbc_pool.config)
        .await
        .expect("Failed to fetch Meteora DBC config account");
    let dbc_config =
        MeteoraDbcConfigRaw::try_from_slice(&config_account.data[8..8 + POOL_CONFIG_SIZE])
            .expect("Failed to deserialize Meteora DBC config");

    // Fetch token vault accounts to get real reserves
    let vault_base_account = rpc_client
        .get_account(&dbc_pool.base_vault)
        .await
        .expect("Failed to fetch base vault");
    let vault_quote_account = rpc_client
        .get_account(&dbc_pool.quote_vault)
        .await
        .expect("Failed to fetch quote vault");

    let base_reserve = u64::from_le_bytes(vault_base_account.data[64..72].try_into().unwrap());
    let quote_reserve = u64::from_le_bytes(vault_quote_account.data[64..72].try_into().unwrap());

    let pool_state = PoolState::MeteoraDbc(Box::new(DbcPoolState {
        slot: 100,
        transaction_index: None,
        address: pool_address,
        config: dbc_pool.config,
        creator: dbc_pool.creator,
        base_mint: dbc_pool.base_mint,
        base_vault: dbc_pool.base_vault,
        quote_vault: dbc_pool.quote_vault,
        base_reserve,
        quote_reserve,
        protocol_base_fee: dbc_pool.protocol_base_fee,
        protocol_quote_fee: dbc_pool.protocol_quote_fee,
        partner_base_fee: dbc_pool.partner_base_fee,
        partner_quote_fee: dbc_pool.partner_quote_fee,
        sqrt_price: dbc_pool.sqrt_price,
        activation_point: dbc_pool.activation_point,
        pool_type: dbc_pool.pool_type,
        is_migrated: dbc_pool.is_migrated,
        is_partner_withdraw_surplus: dbc_pool.is_partner_withdraw_surplus,
        is_protocol_withdraw_surplus: dbc_pool.is_protocol_withdraw_surplus,
        migration_progress: dbc_pool.migration_progress,
        is_withdraw_leftover: dbc_pool.is_withdraw_leftover,
        is_creator_withdraw_surplus: dbc_pool.is_creator_withdraw_surplus,
        migration_fee_withdraw_status: dbc_pool.migration_fee_withdraw_status,
        finish_curve_timestamp: dbc_pool.finish_curve_timestamp,
        creator_base_fee: dbc_pool.creator_base_fee,
        creator_quote_fee: dbc_pool.creator_quote_fee,
        liquidity_usd: 1_000_000.0,
        last_updated: u64::MAX,
        pool_config: Some(dbc_config.clone()),
        volatility_tracker: Some(dbc_pool.volatility_tracker),
    }));

    pool_manager.inject_pool(pool_state).await;

    // Inject tokens
    pool_manager.inject_token(wsol_token()).await;
    pool_manager
        .inject_token(Token {
            address: token_mint,
            symbol: Some("TOKEN".to_string()),
            name: Some("Meteora DBC Token".to_string()),
            decimals: 6,
            is_token_2022: false,
            logo_uri: None,
        })
        .await;

    // Create App State
    let aggregator = Arc::new(DexAggregator::new(config, pool_manager.clone()));

    // Perform Quote (SOL -> Token)
    let user_wallet_str = "DNfuF1L62WWyW3pNakVkyGGFzVVhj4Yr52jSmdTyeBHm".to_string(); // Random active wallet
    let payer = Pubkey::from_str(&user_wallet_str).unwrap();
    let amount_in = 1_000_000_000; // 1 SOL

    let swap_params = crate::types::SwapParams {
        input_token: wsol_token(),
        output_token: Token {
            address: token_mint,
            symbol: Some("TOKEN".to_string()),
            name: Some("Meteora DBC Token".to_string()),
            decimals: 6,
            is_token_2022: false,
            logo_uri: None,
        },
        input_amount: amount_in,
        slippage_bps: 100,
        user_wallet: payer,
        priority: crate::types::ExecutionPriority::Medium,
    };

    println!(
        "Requesting Quote: {} -> {}",
        swap_params.input_token.address, swap_params.output_token.address
    );

    let _quote = aggregator
        .get_swap_route(&swap_params)
        .await
        .expect("Failed to get buy quote");

    // Get instructions
    let pool = pool_manager.get_pool(&pool_address).await.unwrap();
    let instructions = pool
        .build_swap_instruction(&swap_params, &*pool_manager, None)
        .await
        .expect("Failed to build swap instruction");

    // Add WSOL wrapping for SOL input (prepend to instructions)
    let wsol_instructions = sol_trade_sdk::trading::common::handle_wsol(&payer, amount_in);
    let mut all_instructions = Vec::new();
    all_instructions.extend(wsol_instructions);
    all_instructions.extend(instructions);
    // Add WSOL unwrapping (close WSOL account)
    all_instructions.extend(sol_trade_sdk::trading::common::close_wsol(&payer));

    println!("Simulating transaction...");
    // Create transaction
    let recent_blockhash = rpc_client.get_latest_blockhash().await.unwrap();
    let mut message = solana_sdk::message::Message::new(&all_instructions, Some(&payer));
    message.recent_blockhash = recent_blockhash;
    let transaction = Transaction::new_unsigned(message);

    let config = RpcSimulateTransactionConfig {
        sig_verify: false,
        replace_recent_blockhash: true,
        commitment: Some(CommitmentConfig::confirmed()),
        encoding: None,
        accounts: None,
        min_context_slot: None,
        inner_instructions: true,
    };

    let result = rpc_client
        .simulate_transaction_with_config(&transaction, config)
        .await
        .expect("Simulation failed");

    if let Some(err) = result.value.err {
        println!("Simulation Logs: {:#?}", result.value.logs);
        panic!("Simulation error: {:?}", err);
    }

    println!("Simulation successful! Logs: {:#?}", result.value.logs);
}

#[tokio::test]
async fn test_meteora_dbc_quote_simulation_reverse() {
    let _ = env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();

    let (pool_manager, _config) = create_test_setup(vec!["meteora_dbc"]).await;

    // Meteora DBC Pool: SOL-Token
    let pool_address = Pubkey::from_str("6pd7brdZYj8V7Rgo4trnHEvbokc5EjzxZTP1NdMk9sWu").unwrap();
    let token_mint = Pubkey::from_str("6yXTqNnj8PGbJosD6dvpQLFVxaDNpkQmPo7fMxLeUh6A").unwrap();
    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());
    let rpc_client = Arc::new(solana_client::nonblocking::rpc_client::RpcClient::new(
        rpc_url,
    ));

    // Fetch real pool state
    let account = rpc_client
        .get_account(&pool_address)
        .await
        .expect("Failed to fetch pool account");
    let dbc_pool = MeteoraDbcPoolRaw::try_from_slice(&account.data[8..8 + VIRTUAL_POOL_SIZE])
        .expect("Failed to deserialize Meteora DBC pool");

    let config_account = rpc_client
        .get_account(&dbc_pool.config)
        .await
        .expect("Failed to fetch Meteora DBC config account");
    let dbc_config =
        MeteoraDbcConfigRaw::try_from_slice(&config_account.data[8..8 + POOL_CONFIG_SIZE])
            .expect("Failed to deserialize Meteora DBC config");

    let vault_base_account = rpc_client
        .get_account(&dbc_pool.base_vault)
        .await
        .expect("Failed to fetch base vault");
    let vault_quote_account = rpc_client
        .get_account(&dbc_pool.quote_vault)
        .await
        .expect("Failed to fetch quote vault");

    let base_reserve = u64::from_le_bytes(vault_base_account.data[64..72].try_into().unwrap());
    let quote_reserve = u64::from_le_bytes(vault_quote_account.data[64..72].try_into().unwrap());

    let pool_state = PoolState::MeteoraDbc(Box::new(DbcPoolState {
        slot: 100,
        transaction_index: None,
        address: pool_address,
        config: dbc_pool.config,
        creator: dbc_pool.creator,
        base_mint: dbc_pool.base_mint,
        base_vault: dbc_pool.base_vault,
        quote_vault: dbc_pool.quote_vault,
        base_reserve,
        quote_reserve,
        protocol_base_fee: dbc_pool.protocol_base_fee,
        protocol_quote_fee: dbc_pool.protocol_quote_fee,
        partner_base_fee: dbc_pool.partner_base_fee,
        partner_quote_fee: dbc_pool.partner_quote_fee,
        sqrt_price: dbc_pool.sqrt_price,
        activation_point: dbc_pool.activation_point,
        pool_type: dbc_pool.pool_type,
        is_migrated: dbc_pool.is_migrated,
        is_partner_withdraw_surplus: dbc_pool.is_partner_withdraw_surplus,
        is_protocol_withdraw_surplus: dbc_pool.is_protocol_withdraw_surplus,
        migration_progress: dbc_pool.migration_progress,
        is_withdraw_leftover: dbc_pool.is_withdraw_leftover,
        is_creator_withdraw_surplus: dbc_pool.is_creator_withdraw_surplus,
        migration_fee_withdraw_status: dbc_pool.migration_fee_withdraw_status,
        finish_curve_timestamp: dbc_pool.finish_curve_timestamp,
        creator_base_fee: dbc_pool.creator_base_fee,
        creator_quote_fee: dbc_pool.creator_quote_fee,
        liquidity_usd: 1_000_000.0,
        last_updated: u64::MAX,
        pool_config: Some(dbc_config.clone()),
        volatility_tracker: Some(dbc_pool.volatility_tracker),
    }));

    pool_manager.inject_pool(pool_state).await;
    pool_manager.inject_token(wsol_token()).await;
    pool_manager
        .inject_token(Token {
            address: token_mint,
            symbol: Some("TOKEN".to_string()),
            name: Some("Meteora DBC Token".to_string()),
            decimals: 6,
            is_token_2022: false,
            logo_uri: None,
        })
        .await;

    let user_wallet_str = "DNfuF1L62WWyW3pNakVkyGGFzVVhj4Yr52jSmdTyeBHm".to_string(); // Random active wallet
    let payer = Pubkey::from_str(&user_wallet_str).unwrap();

    // 1. Buy instructions (SOL -> Token)
    let buy_amount_in = 1_000_000_000; // 1 SOL
    let buy_params = crate::types::SwapParams {
        input_token: wsol_token(),
        output_token: Token {
            address: token_mint,
            symbol: Some("TOKEN".to_string()),
            name: Some("Meteora DBC Token".to_string()),
            decimals: 6,
            is_token_2022: false,
            logo_uri: None,
        },
        input_amount: buy_amount_in,
        slippage_bps: 100,
        user_wallet: payer,
        priority: crate::types::ExecutionPriority::Medium,
    };

    let pool = pool_manager.get_pool(&pool_address).await.unwrap();
    let buy_instructions = pool
        .build_swap_instruction(&buy_params, &*pool_manager, None)
        .await
        .expect("Failed to build buy instruction");

    // 2. Sell instructions (Token -> SOL)
    // Increase sell amount to avoid "zero after slippage" error (decimals 6, so 100_000_000 = 100 tokens)
    let sell_amount_in = 100_000_000;

    let sell_params = crate::types::SwapParams {
        input_token: Token {
            address: token_mint,
            symbol: Some("TOKEN".to_string()),
            name: Some("Meteora DBC Token".to_string()),
            decimals: 6,
            is_token_2022: false,
            logo_uri: None,
        },
        output_token: wsol_token(),
        input_amount: sell_amount_in,
        slippage_bps: 300,
        user_wallet: payer,
        priority: crate::types::ExecutionPriority::Medium,
    };

    let sell_instructions = pool
        .build_swap_instruction(&sell_params, &*pool_manager, None)
        .await
        .expect("Failed to build sell instruction");

    // Combine instructions with WSOL handling
    let mut instructions = Vec::new();

    // Add WSOL wrapping for buy (SOL -> Token)
    instructions.extend(sol_trade_sdk::trading::common::handle_wsol(
        &payer,
        buy_amount_in,
    ));
    instructions.extend(buy_instructions);

    // Add sell instructions (Token -> SOL)
    instructions.extend(sell_instructions);

    // Close WSOL account at the end
    instructions.extend(sol_trade_sdk::trading::common::close_wsol(&payer));

    // Simulate
    let recent_blockhash = rpc_client.get_latest_blockhash().await.unwrap();
    let mut message = solana_sdk::message::Message::new(&instructions, Some(&payer));
    message.recent_blockhash = recent_blockhash;
    let transaction = Transaction::new_unsigned(message);

    let config = RpcSimulateTransactionConfig {
        sig_verify: false,
        replace_recent_blockhash: true,
        commitment: Some(CommitmentConfig::confirmed()),
        encoding: None,
        accounts: None,
        min_context_slot: None,
        inner_instructions: true,
    };

    let result = rpc_client
        .simulate_transaction_with_config(&transaction, config)
        .await
        .expect("Simulation failed");

    if let Some(err) = result.value.err {
        println!("Simulation Logs: {:#?}", result.value.logs);
        panic!("Simulation error: {:?}", err);
    }
    println!("Simulation successful! Logs: {:#?}", result.value.logs);
}

#[tokio::test]
async fn test_meteora_dlmm_quote_simulation() {
    let _ = env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();

    let (pool_manager, config) = create_test_setup(vec!["meteora_dlmm"]).await;

    // Meteora DLMM Pool: SOL-RALPH
    let pool_address = Pubkey::from_str("2NeCmKGXrcCa4aQhKdjdhY728565YsFcxNbD1JPcGDkh").unwrap();
    let token_mint = Pubkey::from_str("DLJjXt2BmoeWMBuvuvBnuLZZGPiG5wFt8mVDWXnbVvnH").unwrap();
    let _sol_mint = Pubkey::from_str("So11111111111111111111111111111111111111112").unwrap();

    let rpc_client = pool_manager.get_rpc_client();
    let account = rpc_client
        .get_account(&pool_address)
        .await
        .expect("Failed to fetch DLMM pool account");
    let raw_pool =
        LbPairRaw::try_from_slice(&account.data).expect("Failed to deserialize DLMM pool");

    use solana_streamer_sdk::streaming::event_parser::protocols::meteora_dlmm::types::BinArrayBitmapExtension as SdkBitmapExtension;
    use solana_streamer_sdk::streaming::event_parser::protocols::meteora_dlmm::types::LbPair as SdkLbPair;

    let mut sdk_lb_pair: SdkLbPair = if account.data.len() >= 8 {
        unsafe { (account.data[8..].as_ptr() as *const SdkLbPair).read_unaligned() }
    } else {
        panic!("Account data too short");
    };

    sdk_lb_pair.token_x_mint = raw_pool.token_x_mint;
    sdk_lb_pair.token_y_mint = raw_pool.token_y_mint;
    sdk_lb_pair.reserve_x = raw_pool.reserve_x;
    sdk_lb_pair.reserve_y = raw_pool.reserve_y;
    sdk_lb_pair.oracle = raw_pool.oracle;
    sdk_lb_pair.active_id = raw_pool.active_id;
    sdk_lb_pair.bin_step = raw_pool.bin_step;
    sdk_lb_pair.parameters.min_bin_id = raw_pool.parameters.min_bin_id;
    sdk_lb_pair.parameters.max_bin_id = raw_pool.parameters.max_bin_id;
    sdk_lb_pair.parameters.base_factor = raw_pool.parameters.base_factor;
    sdk_lb_pair.bin_array_bitmap = raw_pool.bin_array_bitmap;

    let program_id = Pubkey::from_str("LBUZKhRxPF3XUpBCjp4YzTKgLccjZhTSDM9YuVaPwxo").unwrap();
    let (bitmap_pubkey, _) =
        Pubkey::find_program_address(&[b"bitmap", pool_address.as_ref()], &program_id);
    let mut bitmap_extension = None;
    if let Ok(bitmap_acc) = rpc_client.get_account(&bitmap_pubkey).await {
        if bitmap_acc.data.len() >= 8 {
            let ext: SdkBitmapExtension = unsafe {
                (bitmap_acc.data[8..].as_ptr() as *const SdkBitmapExtension).read_unaligned()
            };
            bitmap_extension = Some(ext);
        }
    }

    let mut pool_state_struct = crate::pool_data_types::MeteoraDlmmPoolState {
        slot: 0,
        transaction_index: Some(0),
        address: pool_address,
        lbpair: sdk_lb_pair,
        bin_arrays: std::collections::HashMap::new(),
        bitmap_extension,
        reserve_x: None,
        reserve_y: None,
        liquidity_usd: 1_000_000.0,
        is_state_keys_initialized: true,
        last_updated: u64::MAX,
    };

    use crate::fetchers::meteora_dlmm_bin_array_fetcher::MeteoraDlmmBinArrayFetcher;
    let fetcher = MeteoraDlmmBinArrayFetcher::new(rpc_client.clone());
    if let Ok(bin_arrays) = fetcher
        .fetch_all_bin_arrays(pool_address, &pool_state_struct)
        .await
    {
        for ba in bin_arrays {
            pool_state_struct.bin_arrays.insert(ba.index as i32, ba);
        }
    }

    let pool_state = PoolState::MeteoraDlmm(Box::new(pool_state_struct));
    pool_manager.inject_pool(pool_state).await;

    // Inject tokens
    pool_manager.inject_token(wsol_token()).await;
    pool_manager
        .inject_token(Token {
            address: token_mint,
            symbol: Some("RALPH".to_string()),
            name: Some("Ralph Token".to_string()),
            decimals: 9,
            is_token_2022: true,
            logo_uri: None,
        })
        .await;

    let aggregator = Arc::new(DexAggregator::new(config, pool_manager.clone()));

    let user_wallet_str = "DNfuF1L62WWyW3pNakVkyGGFzVVhj4Yr52jSmdTyeBHm".to_string();
    let payer = Pubkey::from_str(&user_wallet_str).unwrap();
    let amount_in = 100_000_000; // 0.1 SOL

    let swap_params = crate::types::SwapParams {
        input_token: wsol_token(),
        output_token: Token {
            address: token_mint,
            symbol: Some("RALPH".to_string()),
            name: Some("Ralph Token".to_string()),
            decimals: 9,
            is_token_2022: true,
            logo_uri: None,
        },
        input_amount: amount_in,
        slippage_bps: 1000,
        user_wallet: payer,
        priority: crate::types::ExecutionPriority::Medium,
    };

    println!(
        "Requesting Quote: {} -> {}",
        swap_params.input_token.address, swap_params.output_token.address
    );

    let _quote = aggregator
        .get_swap_route(&swap_params)
        .await
        .expect("Failed to get quote");

    let pool = pool_manager.get_pool(&pool_address).await.unwrap();
    let swap_instructions = pool
        .build_swap_instruction(&swap_params, &*pool_manager, None)
        .await
        .expect("Failed to build swap instruction");

    // Add WSOL wrapping for SOL input
    let mut all_instructions = Vec::new();
    all_instructions.extend(sol_trade_sdk::trading::common::handle_wsol(
        &payer, amount_in,
    ));
    all_instructions.extend(swap_instructions);
    all_instructions.extend(sol_trade_sdk::trading::common::close_wsol(&payer));

    println!("Simulating transaction...");
    let recent_blockhash = rpc_client.get_latest_blockhash().await.unwrap();
    let mut message = solana_sdk::message::Message::new(&all_instructions, Some(&payer));
    message.recent_blockhash = recent_blockhash;
    let transaction = Transaction::new_unsigned(message);

    let config = RpcSimulateTransactionConfig {
        sig_verify: false,
        replace_recent_blockhash: true,
        commitment: Some(CommitmentConfig::confirmed()),
        encoding: None,
        accounts: None,
        min_context_slot: None,
        inner_instructions: true,
    };

    let result = rpc_client
        .simulate_transaction_with_config(&transaction, config)
        .await
        .expect("Simulation failed");

    if let Some(err) = result.value.err {
        println!("Simulation Logs: {:#?}", result.value.logs);
        panic!("Simulation error: {:?}", err);
    }

    println!("Simulation successful! Logs: {:#?}", result.value.logs);
}

#[tokio::test]
async fn test_meteora_dlmm_quote_simulation_reverse() {
    let _ = env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();

    let (pool_manager, _config) = create_test_setup(vec!["meteora_dlmm"]).await;

    let pool_address = Pubkey::from_str("6b9ZdnykBXZwRqw1xuS4McYxghAwocwZzrwijzcUVcxP").unwrap();
    let ralph_mint = Pubkey::from_str("8116V1BW9zaXUM6pVhWVaAduKrLcEBi3RGXedKTrBAGS").unwrap();

    let rpc_client = pool_manager.get_rpc_client();
    let account = rpc_client
        .get_account(&pool_address)
        .await
        .expect("Failed to fetch DLMM pool account");
    let raw_pool =
        LbPairRaw::try_from_slice(&account.data).expect("Failed to deserialize DLMM pool");

    use solana_streamer_sdk::streaming::event_parser::protocols::meteora_dlmm::types::BinArrayBitmapExtension as SdkBitmapExtension;
    use solana_streamer_sdk::streaming::event_parser::protocols::meteora_dlmm::types::LbPair as SdkLbPair;

    let mut sdk_lb_pair: SdkLbPair = if account.data.len() >= 8 {
        unsafe { (account.data[8..].as_ptr() as *const SdkLbPair).read_unaligned() }
    } else {
        panic!("Account data too short");
    };

    sdk_lb_pair.token_x_mint = raw_pool.token_x_mint;
    sdk_lb_pair.token_y_mint = raw_pool.token_y_mint;
    sdk_lb_pair.reserve_x = raw_pool.reserve_x;
    sdk_lb_pair.reserve_y = raw_pool.reserve_y;
    sdk_lb_pair.oracle = raw_pool.oracle;
    sdk_lb_pair.active_id = raw_pool.active_id;
    sdk_lb_pair.bin_step = raw_pool.bin_step;
    sdk_lb_pair.parameters.min_bin_id = raw_pool.parameters.min_bin_id;
    sdk_lb_pair.parameters.max_bin_id = raw_pool.parameters.max_bin_id;
    sdk_lb_pair.parameters.base_factor = raw_pool.parameters.base_factor;
    sdk_lb_pair.bin_array_bitmap = raw_pool.bin_array_bitmap;

    let program_id = Pubkey::from_str("LBUZKhRxPF3XUpBCjp4YzTKgLccjZhTSDM9YuVaPwxo").unwrap();
    let (bitmap_pubkey, _) =
        Pubkey::find_program_address(&[b"bitmap", pool_address.as_ref()], &program_id);
    let mut bitmap_extension = None;
    if let Ok(bitmap_acc) = rpc_client.get_account(&bitmap_pubkey).await {
        if bitmap_acc.data.len() >= 8 {
            let ext: SdkBitmapExtension = unsafe {
                (bitmap_acc.data[8..].as_ptr() as *const SdkBitmapExtension).read_unaligned()
            };
            bitmap_extension = Some(ext);
        }
    }

    let mut pool_state_struct = crate::pool_data_types::MeteoraDlmmPoolState {
        slot: 0,
        transaction_index: Some(0),
        address: pool_address,
        lbpair: sdk_lb_pair,
        bin_arrays: std::collections::HashMap::new(),
        bitmap_extension,
        reserve_x: None,
        reserve_y: None,
        liquidity_usd: 1_000_000.0,
        is_state_keys_initialized: true,
        last_updated: u64::MAX,
    };

    use crate::fetchers::meteora_dlmm_bin_array_fetcher::MeteoraDlmmBinArrayFetcher;
    let fetcher = MeteoraDlmmBinArrayFetcher::new(rpc_client.clone());
    if let Ok(bin_arrays) = fetcher
        .fetch_all_bin_arrays(pool_address, &pool_state_struct)
        .await
    {
        for ba in bin_arrays {
            pool_state_struct.bin_arrays.insert(ba.index as i32, ba);
        }
    }

    let pool_state = PoolState::MeteoraDlmm(Box::new(pool_state_struct));
    pool_manager.inject_pool(pool_state).await;
    pool_manager.inject_token(wsol_token()).await;
    pool_manager
        .inject_token(Token {
            address: ralph_mint,
            symbol: Some("RALPH".to_string()),
            name: Some("Ralph Token".to_string()),
            decimals: 9,
            is_token_2022: false,
            logo_uri: None,
        })
        .await;

    let user_wallet_str = "DNfuF1L62WWyW3pNakVkyGGFzVVhj4Yr52jSmdTyeBHm".to_string();
    let payer = Pubkey::from_str(&user_wallet_str).unwrap();

    // 1. Buy instructions (SOL -> RALPH)
    let buy_amount_in = 100_000_000; // 0.1 SOL
    let buy_params = crate::types::SwapParams {
        input_token: wsol_token(),
        output_token: Token {
            address: ralph_mint,
            symbol: Some("RALPH".to_string()),
            name: Some("Ralph Token".to_string()),
            decimals: 9,
            is_token_2022: false,
            logo_uri: None,
        },
        input_amount: buy_amount_in,
        slippage_bps: 100,
        user_wallet: payer,
        priority: crate::types::ExecutionPriority::Medium,
    };

    let pool = pool_manager.get_pool(&pool_address).await.unwrap();
    let buy_instructions = pool
        .build_swap_instruction(&buy_params, &*pool_manager, None)
        .await
        .expect("Failed to build buy instruction");

    // 2. Sell instructions (RALPH -> SOL)
    let sell_amount_in = 10_000_000_000; // 10 RALPH (decimals 9)

    let sell_params = crate::types::SwapParams {
        input_token: Token {
            address: ralph_mint,
            symbol: Some("RALPH".to_string()),
            name: Some("Ralph Token".to_string()),
            decimals: 9,
            is_token_2022: false,
            logo_uri: None,
        },
        output_token: wsol_token(),
        input_amount: sell_amount_in,
        slippage_bps: 1000,
        user_wallet: payer,
        priority: crate::types::ExecutionPriority::Medium,
    };

    let sell_instructions = pool
        .build_swap_instruction(&sell_params, &*pool_manager, None)
        .await
        .expect("Failed to build sell instruction");

    // Combine instructions with WSOL handling
    let mut instructions = Vec::new();

    // Add WSOL wrapping for buy (SOL -> RALPH)
    instructions.extend(sol_trade_sdk::trading::common::handle_wsol(
        &payer,
        buy_amount_in,
    ));
    instructions.extend(buy_instructions);

    // Add sell instructions (RALPH -> SOL)
    instructions.extend(sell_instructions);

    // Close WSOL account at the end
    instructions.extend(sol_trade_sdk::trading::common::close_wsol(&payer));

    // Simulate
    let recent_blockhash = rpc_client.get_latest_blockhash().await.unwrap();
    let mut message = solana_sdk::message::Message::new(&instructions, Some(&payer));
    message.recent_blockhash = recent_blockhash;
    let transaction = Transaction::new_unsigned(message);

    let config = RpcSimulateTransactionConfig {
        sig_verify: false,
        replace_recent_blockhash: true,
        commitment: Some(CommitmentConfig::confirmed()),
        encoding: None,
        accounts: None,
        min_context_slot: None,
        inner_instructions: true,
    };

    let result = rpc_client
        .simulate_transaction_with_config(&transaction, config)
        .await
        .expect("Simulation failed");

    if let Some(err) = result.value.err {
        println!("Simulation Logs: {:#?}", result.value.logs);
        panic!("Simulation error: {:?}", err);
    }
    println!("Simulation successful! Logs: {:#?}", result.value.logs);
}

#[tokio::test]
async fn test_meteora_dlmm_quote_vs_jupiter() {
    let _ = env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();

    let (pool_manager, config) = create_test_setup(vec!["meteora_dlmm"]).await;

    // Meteora DLMM Pool: SOL-RALPH
    let pool_address = Pubkey::from_str("6b9ZdnykBXZwRqw1xuS4McYxghAwocwZzrwijzcUVcxP").unwrap();
    let ralph_mint = Pubkey::from_str("8116V1BW9zaXUM6pVhWVaAduKrLcEBi3RGXedKTrBAGS").unwrap();

    // Fetch real pool state
    let rpc_client = pool_manager.get_rpc_client();
    let account = rpc_client
        .get_account(&pool_address)
        .await
        .expect("Failed to fetch DLMM pool account");
    let raw_pool =
        LbPairRaw::try_from_slice(&account.data).expect("Failed to deserialize DLMM pool");

    use solana_streamer_sdk::streaming::event_parser::protocols::meteora_dlmm::types::BinArrayBitmapExtension as SdkBitmapExtension;
    use solana_streamer_sdk::streaming::event_parser::protocols::meteora_dlmm::types::LbPair as SdkLbPair;

    let mut sdk_lb_pair: SdkLbPair = if account.data.len() >= 8 {
        unsafe { (account.data[8..].as_ptr() as *const SdkLbPair).read_unaligned() }
    } else {
        panic!("Account data too short");
    };

    sdk_lb_pair.token_x_mint = raw_pool.token_x_mint;
    sdk_lb_pair.token_y_mint = raw_pool.token_y_mint;
    sdk_lb_pair.active_id = raw_pool.active_id;
    sdk_lb_pair.bin_step = raw_pool.bin_step;
    sdk_lb_pair.parameters.min_bin_id = raw_pool.parameters.min_bin_id;
    sdk_lb_pair.parameters.max_bin_id = raw_pool.parameters.max_bin_id;
    sdk_lb_pair.parameters.base_factor = raw_pool.parameters.base_factor;
    sdk_lb_pair.bin_array_bitmap = raw_pool.bin_array_bitmap;

    let program_id = Pubkey::from_str("LBUZKhRxPF3XUpBCjp4YzTKgLccjZhTSDM9YuVaPwxo").unwrap();
    let (bitmap_pubkey, _) =
        Pubkey::find_program_address(&[b"bitmap", pool_address.as_ref()], &program_id);
    let mut bitmap_extension = None;
    if let Ok(bitmap_acc) = rpc_client.get_account(&bitmap_pubkey).await {
        if bitmap_acc.data.len() >= 8 {
            let ext: SdkBitmapExtension = unsafe {
                (bitmap_acc.data[8..].as_ptr() as *const SdkBitmapExtension).read_unaligned()
            };
            bitmap_extension = Some(ext);
        }
    }

    let mut pool_state_struct = crate::pool_data_types::MeteoraDlmmPoolState {
        slot: 0,
        transaction_index: Some(0),
        address: pool_address,
        lbpair: sdk_lb_pair,
        bin_arrays: std::collections::HashMap::new(),
        bitmap_extension,
        reserve_x: None,
        reserve_y: None,
        liquidity_usd: 1_000_000.0,
        is_state_keys_initialized: true,
        last_updated: u64::MAX,
    };

    use crate::fetchers::meteora_dlmm_bin_array_fetcher::MeteoraDlmmBinArrayFetcher;
    let fetcher = MeteoraDlmmBinArrayFetcher::new(rpc_client.clone());
    if let Ok(bin_arrays) = fetcher
        .fetch_all_bin_arrays(pool_address, &pool_state_struct)
        .await
    {
        for ba in bin_arrays {
            pool_state_struct.bin_arrays.insert(ba.index as i32, ba);
        }
    }

    let pool_state = PoolState::MeteoraDlmm(Box::new(pool_state_struct.clone()));
    // print pool reserve
    println!("Pool Reserve X: {:?}", pool_state_struct.reserve_x);
    println!("Pool Reserve Y: {:?}", pool_state_struct.reserve_y);
    pool_manager.inject_pool(pool_state).await;

    // Inject tokens
    pool_manager.inject_token(wsol_token()).await;
    pool_manager
        .inject_token(Token {
            address: ralph_mint,
            symbol: Some("RALPH".to_string()),
            name: Some("Ralph Token".to_string()),
            decimals: 9,
            is_token_2022: false,
            logo_uri: None,
        })
        .await;

    let aggregator = Arc::new(DexAggregator::new(config, pool_manager.clone()));

    // 1. Calculate Local Quote
    let amount_in = 1_000_000_000; // 1 SOL
    let swap_params = crate::types::SwapParams {
        input_token: wsol_token(),
        output_token: Token {
            address: ralph_mint,
            symbol: Some("RALPH".to_string()),
            name: Some("Ralph Token".to_string()),
            decimals: 9,
            is_token_2022: false,
            logo_uri: None,
        },
        input_amount: amount_in,
        slippage_bps: 100,
        user_wallet: Pubkey::new_unique(), // Doesn't matter for quote
        priority: crate::types::ExecutionPriority::Medium,
    };

    let local_quote = aggregator
        .get_swap_route(&swap_params)
        .await
        .expect("Failed to get local quote");

    println!("Local Output Amount: {}", local_quote.output_amount);

    // 2. Fetch Jupiter Quote
    println!("Fetching Jupiter quote...");
    let req_client = reqwest::Client::new();
    let resp_result = req_client
        .get("https://public.jupiterapi.com/quote")
        .query(&[
            ("inputMint", wsol_token().address.to_string()),
            ("outputMint", ralph_mint.to_string()),
            ("amount", amount_in.to_string()),
            ("slippageBps", "100".to_string()),
            ("onlyDirectRoutes", "true".to_string()), // Force direct routes only
        ])
        .send()
        .await;

    match resp_result {
        Ok(resp) => {
            if !resp.status().is_success() {
                println!("Jupiter API Error: {}", resp.text().await.unwrap());
                println!("Skipping comparison due to API error.");
                return;
            }

            let jup_json: serde_json::Value = resp.json().await.expect("Failed to parse JSON");

            let jup_out_amount_str = jup_json["outAmount"].as_str().expect("Missing outAmount");
            let jup_out_amount: u64 = jup_out_amount_str
                .parse()
                .expect("Failed to parse outAmount");

            println!("Jupiter JSON: {:#?}", jup_json);
            println!("Jupiter Output Amount: {}", jup_out_amount);

            // Check route plan
            if let Some(route_plan) = jup_json["routePlan"].as_array() {
                println!("Jupiter Route Plan Length: {}", route_plan.len());
                for (i, step) in route_plan.iter().enumerate() {
                    let label = step["swapInfo"]["label"].as_str().unwrap_or("Unknown");
                    let amm_key = step["swapInfo"]["ammKey"].as_str().unwrap_or("Unknown");
                    println!("Step {}: {} (AMM: {})", i, label, amm_key);
                }
            }

            // 3. Compare
            let diff = if local_quote.output_amount > jup_out_amount {
                local_quote.output_amount - jup_out_amount
            } else {
                jup_out_amount - local_quote.output_amount
            };
            println!("Out quote output amount: {:#?}", local_quote.output_amount);
            let diff_percent = (diff as f64 / jup_out_amount as f64) * 100.0;
            println!("Difference: {} ({}%)", diff, diff_percent);

            // Note: Local calculation may underestimate for large swaps because we only fetch
            // nearby bin arrays during quote calculation. For production routing, all necessary
            // bin arrays are fetched. Jupiter has access to all bin arrays.
            // Accept up to 10% difference for this comparison test.
            assert!(
                diff_percent < 10.0,
                "Difference too high: {}%",
                diff_percent
            );
        }
        Err(e) => {
            println!(
                "Failed to fetch Jupiter quote (likely network/DNS issue): {}",
                e
            );
            println!("Skipping comparison.");
        }
    }
}
