#![allow(warnings)]
use std::{collections::HashMap, str::FromStr};

use anchor_lang::Event;
use rust_decimal::Decimal;
use solana_sdk::pubkey::Pubkey;
use solana_streamer_sdk::{
    match_event,
    streaming::{
        event_parser::{
            core::{
                account_event_parser::{NonceAccountEvent, TokenAccountEvent, TokenInfoEvent},
                event_parser::{PubkeyData, SimplifiedTokenBalance},
            },
            protocols::{
                bonk::{
                    BonkGlobalConfigAccountEvent, BonkMigrateToAmmEvent, BonkMigrateToCpswapEvent,
                    BonkPlatformConfigAccountEvent, BonkPoolCreateEvent, BonkPoolStateAccountEvent,
                    BonkTradeEvent,
                },
                pumpfun::{
                    PumpFunBondingCurveAccountEvent, PumpFunCreateTokenEvent,
                    PumpFunGlobalAccountEvent, PumpFunMigrateEvent, PumpFunTradeEvent,
                },
                pumpswap::{
                    PumpSwapBuyEvent, PumpSwapCreatePoolEvent, PumpSwapDepositEvent,
                    PumpSwapGlobalConfigAccountEvent, PumpSwapPoolAccountEvent, PumpSwapSellEvent,
                    PumpSwapWithdrawEvent,
                },
                raydium_amm_v4::{
                    RaydiumAmmV4AmmInfoAccountEvent, RaydiumAmmV4DepositEvent,
                    RaydiumAmmV4Initialize2Event, RaydiumAmmV4SwapEvent, RaydiumAmmV4WithdrawEvent,
                    RaydiumAmmV4WithdrawPnlEvent,
                },
                raydium_clmm::{
                    RaydiumClmmAmmConfigAccountEvent, RaydiumClmmClosePositionEvent,
                    RaydiumClmmCreatePoolEvent, RaydiumClmmDecreaseLiquidityV2Event,
                    RaydiumClmmIncreaseLiquidityV2Event, RaydiumClmmOpenPositionV2Event,
                    RaydiumClmmOpenPositionWithToken22NftEvent, RaydiumClmmPoolStateAccountEvent,
                    RaydiumClmmSwapEvent, RaydiumClmmSwapV2Event,
                    RaydiumClmmTickArrayStateAccountEvent,
                },
                raydium_cpmm::{
                    RaydiumCpmmAmmConfigAccountEvent, RaydiumCpmmDepositEvent,
                    RaydiumCpmmInitializeEvent, RaydiumCpmmPoolStateAccountEvent,
                    RaydiumCpmmSwapEvent, RaydiumCpmmWithdrawEvent,
                },
                BlockMetaEvent,
            },
            UnifiedEvent,
        },
        grpc::pool,
    },
};
use tokio::sync::mpsc;

use crate::{
    constants::is_base_token,
    pool_data_types::{
        PumpSwapPoolUpdate, PumpfunPoolUpdate, RaydiumAmmV4PoolUpdate, RaydiumCpmmPoolUpdate,
    },
    types::PoolUpdateEvent,
    utils::get_sol_mint,
};
use crate::pool_data_types::BonkPoolUpdate;

pub fn handle_dex_event(
    events: Vec<Box<dyn UnifiedEvent>>,
    accounts: Vec<PubkeyData>,
    post_balances: Vec<u64>,
    post_token_balances: HashMap<String, SimplifiedTokenBalance>,
    pool_update_tx: mpsc::UnboundedSender<Vec<PoolUpdateEvent>>,
) {
    let mut pool_update_events = vec![];
    // loop over events and match their types
    for event in events {
        match_event!(event, {
            // -------------------------- block meta -----------------------
            BlockMetaEvent => |e: BlockMetaEvent| {
                // println!("BlockMetaEvent: {:?}", e.metadata.recv_us/1000000);
            },
            // -------------------------- bonk -----------------------
            BonkPoolCreateEvent => |e: BonkPoolCreateEvent| {
                // When using grpc, you can get block_time from each event
                println!("block_time: {:?}, block_time_ms: {:?}", e.metadata.block_time, e.metadata.block_time_ms);
                println!("BonkPoolCreateEvent: {:?}", e.base_mint_param.symbol);
            },
            BonkTradeEvent => |e: BonkTradeEvent| {
                println!("BonkTradeEvent: {e:?}");
            },
            BonkMigrateToAmmEvent => |e: BonkMigrateToAmmEvent| {
                println!("BonkMigrateToAmmEvent: {e:?}");
            },
            BonkMigrateToCpswapEvent => |e: BonkMigrateToCpswapEvent| {
                println!("BonkMigrateToCpswapEvent: {e:?}");
            },
            // -------------------------- pumpfun -----------------------
            PumpFunTradeEvent => |e: PumpFunTradeEvent| {
                pool_update_events.push(PoolUpdateEvent::PumpfunPoolUpdate(
                    PumpfunPoolUpdate {
                        address: Pubkey::new_from_array(e.bonding_curve.as_array().clone()),
                        last_updated: e.timestamp as u64,
                        mint:Pubkey::new_from_array(e.mint.as_array().clone()),
                        token_reserve: e.virtual_token_reserves,
                        sol_reserve: e.virtual_sol_reserves,
                        real_token_reserve: e.real_token_reserves,
                        slot: e.metadata.slot,
                        transaction_index: e.metadata.transaction_index,
                        complete: false,
                    }));
            },
            PumpFunMigrateEvent => |e: PumpFunMigrateEvent| {
                pool_update_events.push(PoolUpdateEvent::PumpfunPoolUpdate(
                    PumpfunPoolUpdate {
                        address: Pubkey::new_from_array(e.bonding_curve.as_array().clone()),
                        last_updated: e.timestamp as u64,
                        mint:Pubkey::new_from_array(e.mint.as_array().clone()),
                        token_reserve: 0,
                        sol_reserve: 0,
                        real_token_reserve: 0,
                        slot: e.metadata.slot,
                        transaction_index: e.metadata.transaction_index,
                        complete: true,
                    }));
            },
            PumpFunCreateTokenEvent => |e: PumpFunCreateTokenEvent| {
                pool_update_events.push(PoolUpdateEvent::PumpfunPoolUpdate(
                    PumpfunPoolUpdate {
                        address: Pubkey::new_from_array(e.bonding_curve.as_array().clone()),
                        last_updated: e.timestamp as u64,
                        mint:Pubkey::new_from_array(e.mint.as_array().clone()),
                        token_reserve: e.virtual_token_reserves,
                        sol_reserve: e.virtual_sol_reserves,
                        real_token_reserve: e.real_token_reserves,
                        slot: e.metadata.slot,
                        transaction_index: e.metadata.transaction_index,
                        complete: false,
                    }));
            },
            // -------------------------- pumpswap -----------------------
            PumpSwapBuyEvent => |e: PumpSwapBuyEvent| {
                pool_update_events.push(PoolUpdateEvent::PumpSwapPoolUpdate(
                    PumpSwapPoolUpdate {
                        address: Pubkey::new_from_array(e.pool.as_array().clone()),
                        index: None,
                        creator: None,
                        base_mint: Pubkey::new_from_array(e.base_mint.as_array().clone()),
                        quote_mint: Pubkey::new_from_array(e.quote_mint.as_array().clone()),
                        pool_base_token_account: Pubkey::new_from_array(e.pool_base_token_account.as_array().clone()),
                        pool_quote_token_account: Pubkey::new_from_array(e.pool_quote_token_account.as_array().clone()),
                        last_updated: e.last_update_timestamp as u64,
                        base_reserve: e.pool_base_token_reserves,
                        quote_reserve: e.pool_quote_token_reserves,
                        slot: e.metadata.slot,
                        transaction_index: e.metadata.transaction_index,
                    }));
            },
            PumpSwapSellEvent => |e: PumpSwapSellEvent| {
                pool_update_events.push(PoolUpdateEvent::PumpSwapPoolUpdate(
                    PumpSwapPoolUpdate {
                        address: Pubkey::new_from_array(e.pool.as_array().clone()),
                        index: None,
                        creator: None,
                        base_mint: Pubkey::new_from_array(e.base_mint.as_array().clone()),
                        quote_mint: Pubkey::new_from_array(e.quote_mint.as_array().clone()),
                        pool_base_token_account: Pubkey::new_from_array(e.pool_base_token_account.as_array().clone()),
                        pool_quote_token_account: Pubkey::new_from_array(e.pool_quote_token_account.as_array().clone()),
                        last_updated: e.timestamp as u64,
                        base_reserve: e.pool_base_token_reserves,
                        quote_reserve: e.pool_quote_token_reserves,
                        slot: e.metadata.slot,
                        transaction_index: e.metadata.transaction_index,
                    }));
            },
            PumpSwapCreatePoolEvent => |e: PumpSwapCreatePoolEvent| {
                pool_update_events.push(PoolUpdateEvent::PumpSwapPoolUpdate(
                    PumpSwapPoolUpdate {
                        address: Pubkey::new_from_array(e.pool.as_array().clone()),
                        index: Some(e.index),
                        creator: Some(Pubkey::new_from_array(e.creator.as_array().clone())),
                        base_mint: Pubkey::new_from_array(e.base_mint.as_array().clone()),
                        quote_mint: Pubkey::new_from_array(e.quote_mint.as_array().clone()),
                        pool_base_token_account: Pubkey::new_from_array(e.pool_base_token_account.as_array().clone()),
                        pool_quote_token_account: Pubkey::new_from_array(e.pool_quote_token_account.as_array().clone()),
                        last_updated: e.timestamp as u64,
                        base_reserve: e.pool_base_amount,
                        quote_reserve: e.pool_quote_amount,
                        slot: e.metadata.slot,
                        transaction_index: e.metadata.transaction_index,
                    }));
            },
            PumpSwapDepositEvent => |e: PumpSwapDepositEvent| {
                pool_update_events.push(PoolUpdateEvent::PumpSwapPoolUpdate(
                    PumpSwapPoolUpdate {
                        address: Pubkey::new_from_array(e.pool.as_array().clone()),
                        index: None,
                        creator: None,
                        base_mint: Pubkey::new_from_array(e.base_mint.as_array().clone()),
                        quote_mint: Pubkey::new_from_array(e.quote_mint.as_array().clone()),
                        pool_base_token_account: Pubkey::new_from_array(e.pool_base_token_account.as_array().clone()),
                        pool_quote_token_account: Pubkey::new_from_array(e.pool_quote_token_account.as_array().clone()),
                        last_updated: e.timestamp as u64,
                        base_reserve: e.pool_base_token_reserves,
                        quote_reserve: e.pool_quote_token_reserves,
                        slot: e.metadata.slot,
                        transaction_index: e.metadata.transaction_index,
                    }));
            },
            PumpSwapWithdrawEvent => |e: PumpSwapWithdrawEvent| {
                pool_update_events.push(PoolUpdateEvent::PumpSwapPoolUpdate(
                    PumpSwapPoolUpdate {
                        address: Pubkey::new_from_array(e.pool.as_array().clone()),
                        index: None,
                        creator: None,
                        base_mint: Pubkey::new_from_array(e.base_mint.as_array().clone()),
                        quote_mint: Pubkey::new_from_array(e.quote_mint.as_array().clone()),
                        pool_base_token_account: Pubkey::new_from_array(e.pool_base_token_account.as_array().clone()),
                        pool_quote_token_account: Pubkey::new_from_array(e.pool_quote_token_account.as_array().clone()),
                        last_updated: e.timestamp as u64,
                        base_reserve: e.pool_base_token_reserves,
                        quote_reserve: e.pool_quote_token_reserves,
                        slot: e.metadata.slot,
                        transaction_index: e.metadata.transaction_index,
                    }));
            },
            // -------------------------- raydium_cpmm -----------------------
            RaydiumCpmmSwapEvent => |e: RaydiumCpmmSwapEvent| {
                let token0_is_input = e.input_token_mint < e.output_token_mint;
                let (token0_balance, token1_balance) = if token0_is_input {
                    (post_token_balances.get(e.input_vault.to_string().as_str()), post_token_balances.get(e.output_vault.to_string().as_str()))
                } else {
                    (post_token_balances.get(e.output_vault.to_string().as_str()), post_token_balances.get(e.input_vault.to_string().as_str()))
                };

                if let (Some(t0b), Some(t1b)) = (token0_balance, token1_balance) {
                    if is_base_token(&t0b.mint) || is_base_token(&t1b.mint) {
                        let (token0_vault, token1_vault) = if token0_is_input {
                            (e.input_vault, e.output_vault)
                        } else {
                            (e.output_vault, e.input_vault)
                        };
                        let raydium_pool_update = RaydiumCpmmPoolUpdate {
                            address: Pubkey::new_from_array(e.pool_state.as_array().clone()),
                            slot: e.metadata.slot,
                            transaction_index: e.metadata.transaction_index,
                            token0: Pubkey::from_str(&t0b.mint).unwrap(),
                            token1: Pubkey::from_str(&t1b.mint).unwrap(),
                            token0_vault: Pubkey::new_from_array(token0_vault.as_array().clone()),
                            token1_vault: Pubkey::new_from_array(token1_vault.as_array().clone()),
                            token0_reserve: t0b.amount,
                            token1_reserve: t1b.amount,
                            amm_config: Pubkey::new_from_array(e.amm_config.as_array().clone()),
                            observation_state: Pubkey::new_from_array(e.observation_state.as_array().clone()),
                            last_updated: (e.metadata.recv_us/1000000) as u64,
                            status: None,
                        };
                        pool_update_events.push(PoolUpdateEvent::RaydiumCpmmPoolUpdate(raydium_pool_update));
                    }
                }
            },
            RaydiumCpmmDepositEvent => |e: RaydiumCpmmDepositEvent| {
                let token0_balance = post_token_balances.get(e.token0_vault.to_string().as_str());
                let token1_balance = post_token_balances.get(e.token1_vault.to_string().as_str());

                if let (Some(t0b), Some(t1b)) = (token0_balance, token1_balance) {
                    if is_base_token(&t0b.mint) || is_base_token(&t1b.mint) {
                        let raydium_pool_update = RaydiumCpmmPoolUpdate {
                            address: Pubkey::new_from_array(e.pool_state.as_array().clone()),
                            slot: e.metadata.slot,
                            transaction_index: e.metadata.transaction_index,
                            token0: Pubkey::from_str(&t0b.mint).unwrap(),
                            token1: Pubkey::from_str(&t1b.mint).unwrap(),
                            token0_vault: Pubkey::new_from_array(e.token0_vault.as_array().clone()),
                            token1_vault: Pubkey::new_from_array(e.token1_vault.as_array().clone()),
                            token0_reserve: t0b.amount,
                            token1_reserve: t1b.amount,
                            amm_config: Pubkey::default(),
                            observation_state: Pubkey::default(),
                            last_updated: (e.metadata.recv_us/1000000) as u64,
                            status: None,
                        };
                        pool_update_events.push(PoolUpdateEvent::RaydiumCpmmPoolUpdate(raydium_pool_update));
                    }
                }
            },
            RaydiumCpmmInitializeEvent => |e: RaydiumCpmmInitializeEvent| {
                let token0_balance = post_token_balances.get(e.token0_vault.to_string().as_str());
                let token1_balance = post_token_balances.get(e.token1_vault.to_string().as_str());

                if let (Some(t0b), Some(t1b)) = (token0_balance, token1_balance) {
                    if is_base_token(&t0b.mint) || is_base_token(&t1b.mint) {
                        let raydium_pool_update = RaydiumCpmmPoolUpdate {
                            address: Pubkey::new_from_array(e.pool_state.as_array().clone()),
                            slot: e.metadata.slot,
                            transaction_index: e.metadata.transaction_index,
                            token0: Pubkey::from_str(&t0b.mint).unwrap(),
                            token1: Pubkey::from_str(&t1b.mint).unwrap(),
                            token0_vault: Pubkey::new_from_array(e.token0_vault.as_array().clone()),
                            token1_vault: Pubkey::new_from_array(e.token1_vault.as_array().clone()),
                            token0_reserve: t0b.amount,
                            token1_reserve: t1b.amount,
                            amm_config: Pubkey::default(),
                            observation_state: Pubkey::default(),
                            last_updated: (e.metadata.recv_us/1000000) as u64,
                            status: None,
                        };
                        pool_update_events.push(PoolUpdateEvent::RaydiumCpmmPoolUpdate(raydium_pool_update));
                    }
                }
            },
            RaydiumCpmmWithdrawEvent => |e: RaydiumCpmmWithdrawEvent| {
                let token0_balance = post_token_balances.get(e.token0_vault.to_string().as_str());
                let token1_balance = post_token_balances.get(e.token1_vault.to_string().as_str());

                if let (Some(t0b), Some(t1b)) = (token0_balance, token1_balance) {
                    if is_base_token(&t0b.mint) || is_base_token(&t1b.mint) {
                        let raydium_pool_update = RaydiumCpmmPoolUpdate {
                            address: Pubkey::new_from_array(e.pool_state.as_array().clone()),
                            slot: e.metadata.slot,
                            transaction_index: e.metadata.transaction_index,
                            token0: Pubkey::from_str(&t0b.mint).unwrap(),
                            token1: Pubkey::from_str(&t1b.mint).unwrap(),
                            token0_vault: Pubkey::new_from_array(e.token0_vault.as_array().clone()),
                            token1_vault: Pubkey::new_from_array(e.token1_vault.as_array().clone()),
                            token0_reserve: t0b.amount,
                            token1_reserve: t1b.amount,
                            amm_config: Pubkey::default(),
                            observation_state: Pubkey::default(),
                            last_updated: (e.metadata.recv_us/1000000) as u64,
                            status: None,
                        };
                        pool_update_events.push(PoolUpdateEvent::RaydiumCpmmPoolUpdate(raydium_pool_update));
                    }
                }
            },
            // -------------------------- raydium_clmm -----------------------
            RaydiumClmmSwapEvent => |e: RaydiumClmmSwapEvent| {
                println!("RaydiumClmmSwapEvent: {e:?}");
            },
            RaydiumClmmSwapV2Event => |e: RaydiumClmmSwapV2Event| {
                println!("RaydiumClmmSwapV2Event: {e:?}");
            },
            RaydiumClmmClosePositionEvent => |e: RaydiumClmmClosePositionEvent| {
                println!("RaydiumClmmClosePositionEvent: {e:?}");
            },
            RaydiumClmmDecreaseLiquidityV2Event => |e: RaydiumClmmDecreaseLiquidityV2Event| {
                println!("RaydiumClmmDecreaseLiquidityV2Event: {e:?}");
            },
            RaydiumClmmCreatePoolEvent => |e: RaydiumClmmCreatePoolEvent| {
                println!("RaydiumClmmCreatePoolEvent: {e:?}");
            },
            RaydiumClmmIncreaseLiquidityV2Event => |e: RaydiumClmmIncreaseLiquidityV2Event| {
                println!("RaydiumClmmIncreaseLiquidityV2Event: {e:?}");
            },
            RaydiumClmmOpenPositionWithToken22NftEvent => |e: RaydiumClmmOpenPositionWithToken22NftEvent| {
                println!("RaydiumClmmOpenPositionWithToken22NftEvent: {e:?}");
            },
            RaydiumClmmOpenPositionV2Event => |e: RaydiumClmmOpenPositionV2Event| {
                println!("RaydiumClmmOpenPositionV2Event: {e:?}");
            },
            // -------------------------- raydium_amm_v4 -----------------------
            RaydiumAmmV4SwapEvent => |e: RaydiumAmmV4SwapEvent| {
                // find base mint in post_token_balances
                let base_token_balance = post_token_balances.get(e.pool_coin_token_account.to_string().as_str());
                let quote_token_balance = post_token_balances.get(e.pool_pc_token_account.to_string().as_str());
                if let (Some(btb), Some(qtb)) = (base_token_balance, quote_token_balance) {
                    if is_base_token(&btb.mint) || is_base_token(&qtb.mint) {
                        let raydium_pool_update = RaydiumAmmV4PoolUpdate {
                            address: Pubkey::new_from_array(e.amm.as_array().clone()),
                            slot: e.metadata.slot,
                            transaction_index: e.metadata.transaction_index,
                            base_mint: Pubkey::from_str(&btb.mint).unwrap(),
                            quote_mint: Pubkey::from_str(&qtb.mint).unwrap(),
                            amm_authority: Pubkey::new_from_array(e.amm_authority.as_array().clone()),
                            amm_open_orders: Pubkey::new_from_array(e.amm.as_array().clone()),
                            amm_target_orders: Pubkey::new_from_array(e.amm_target_orders.unwrap_or_default().as_array().clone()),
                            pool_coin_token_account: Pubkey::new_from_array(e.pool_coin_token_account.as_array().clone()),
                            pool_pc_token_account: Pubkey::new_from_array(e.pool_pc_token_account.as_array().clone()),
                            serum_program: Pubkey::new_from_array(e.serum_program.as_array().clone()),
                            serum_market: Pubkey::new_from_array(e.serum_market.as_array().clone()),
                            serum_bids: Pubkey::new_from_array(e.serum_bids.as_array().clone()),
                            serum_asks: Pubkey::new_from_array(e.serum_asks.as_array().clone()),
                            serum_event_queue: Pubkey::new_from_array(e.serum_event_queue.as_array().clone()),
                            serum_coin_vault_account: Pubkey::new_from_array(e.serum_coin_vault_account.as_array().clone()),
                            serum_pc_vault_account: Pubkey::new_from_array(e.serum_pc_vault_account.as_array().clone()),
                            serum_vault_signer: Pubkey::new_from_array(e.serum_vault_signer.as_array().clone()),
                            last_updated: (e.metadata.recv_us/1000000) as u64,
                            base_reserve: btb.amount,
                            quote_reserve: qtb.amount,
                        };
                        pool_update_events.push(PoolUpdateEvent::RaydiumPoolUpdate(raydium_pool_update));
                    }
                }
            },
            RaydiumAmmV4DepositEvent => |e: RaydiumAmmV4DepositEvent| {
                // find base mint in post_token_balances
                let base_token_balance = post_token_balances.get(e.pool_coin_token_account.to_string().as_str());
                let quote_token_balance = post_token_balances.get(e.pool_pc_token_account.to_string().as_str());
                if let (Some(btb), Some(qtb)) = (base_token_balance, quote_token_balance) {
                    if is_base_token(&btb.mint) || is_base_token(&qtb.mint) {
                        let raydium_pool_update = RaydiumAmmV4PoolUpdate {
                            address: Pubkey::new_from_array(e.amm.as_array().clone()),
                            slot: e.metadata.slot,
                            transaction_index: e.metadata.transaction_index,
                            base_mint: Pubkey::from_str(&btb.mint).unwrap(),
                            quote_mint: Pubkey::from_str(&qtb.mint).unwrap(),
                            amm_authority: Pubkey::new_from_array(e.amm_authority.as_array().clone()),
                            amm_open_orders: Pubkey::new_from_array(e.amm.as_array().clone()),
                            amm_target_orders: Pubkey::new_from_array(e.amm_target_orders.as_array().clone()),
                            pool_coin_token_account: Pubkey::new_from_array(e.pool_coin_token_account.as_array().clone()),
                            pool_pc_token_account: Pubkey::new_from_array(e.pool_pc_token_account.as_array().clone()),
                            serum_program: Pubkey::default(),
                            serum_market: Pubkey::new_from_array(e.serum_market.as_array().clone()),
                            serum_bids: Pubkey::default(),
                            serum_asks: Pubkey::default(),
                            serum_event_queue: Pubkey::new_from_array(e.serum_event_queue.as_array().clone()),
                            serum_coin_vault_account: Pubkey::default(),
                            serum_pc_vault_account: Pubkey::default(),
                            serum_vault_signer: Pubkey::default(),
                            last_updated: (e.metadata.recv_us/1000000) as u64,
                            base_reserve: btb.amount,
                            quote_reserve: qtb.amount,
                        };
                        pool_update_events.push(PoolUpdateEvent::RaydiumPoolUpdate(raydium_pool_update));
                    }
                }
            },
            RaydiumAmmV4Initialize2Event => |e: RaydiumAmmV4Initialize2Event| {
                let base_token_balance = post_token_balances.get(e.pool_coin_token_account.to_string().as_str());
                let quote_token_balance = post_token_balances.get(e.pool_pc_token_account.to_string().as_str());
                if let (Some(btb), Some(qtb)) = (base_token_balance, quote_token_balance) {
                    if is_base_token(&btb.mint) || is_base_token(&qtb.mint) {
                        let raydium_pool_update = RaydiumAmmV4PoolUpdate {
                            address: Pubkey::new_from_array(e.amm.as_array().clone()),
                            slot: e.metadata.slot,
                            transaction_index: e.metadata.transaction_index,
                            base_mint: Pubkey::from_str(&btb.mint).unwrap(),
                            quote_mint: Pubkey::from_str(&qtb.mint).unwrap(),
                            amm_authority: Pubkey::new_from_array(e.amm_authority.as_array().clone()),
                            amm_open_orders: Pubkey::new_from_array(e.amm.as_array().clone()),
                            amm_target_orders: Pubkey::new_from_array(e.amm_target_orders.as_array().clone()),
                            pool_coin_token_account: Pubkey::new_from_array(e.pool_coin_token_account.as_array().clone()),
                            pool_pc_token_account: Pubkey::new_from_array(e.pool_pc_token_account.as_array().clone()),
                            serum_program: Pubkey::new_from_array(e.serum_program.as_array().clone()),
                            serum_market: Pubkey::new_from_array(e.serum_market.as_array().clone()),
                            serum_bids: Pubkey::default(),
                            serum_asks: Pubkey::default(),
                            serum_event_queue: Pubkey::default(),
                            serum_coin_vault_account: Pubkey::default(),
                            serum_pc_vault_account: Pubkey::default(),
                            serum_vault_signer: Pubkey::default(),
                            last_updated: (e.metadata.recv_us/1000000) as u64,
                            base_reserve: btb.amount,
                            quote_reserve: qtb.amount,
                        };
                        pool_update_events.push(PoolUpdateEvent::RaydiumPoolUpdate(raydium_pool_update));
                    }
                }
            },
            RaydiumAmmV4WithdrawEvent => |e: RaydiumAmmV4WithdrawEvent| {
                let base_token_balance = post_token_balances.get(e.pool_coin_token_account.to_string().as_str());
                let quote_token_balance = post_token_balances.get(e.pool_pc_token_account.to_string().as_str());
                if let (Some(btb), Some(qtb)) = (base_token_balance, quote_token_balance) {
                    if is_base_token(&btb.mint) || is_base_token(&qtb.mint) {
                        let raydium_pool_update = RaydiumAmmV4PoolUpdate {
                            address: Pubkey::new_from_array(e.amm.as_array().clone()),
                            slot: e.metadata.slot,
                            transaction_index: e.metadata.transaction_index,
                            base_mint: Pubkey::from_str(&btb.mint).unwrap(),
                            quote_mint: Pubkey::from_str(&qtb.mint).unwrap(),
                            amm_authority: Pubkey::new_from_array(e.amm_authority.as_array().clone()),
                            amm_open_orders: Pubkey::new_from_array(e.amm.as_array().clone()),
                            amm_target_orders: Pubkey::new_from_array(e.amm_target_orders.as_array().clone()),
                            pool_coin_token_account: Pubkey::new_from_array(e.pool_coin_token_account.as_array().clone()),
                            pool_pc_token_account: Pubkey::new_from_array(e.pool_pc_token_account.as_array().clone()),
                            serum_program: Pubkey::new_from_array(e.serum_program.as_array().clone()),
                            serum_market: Pubkey::new_from_array(e.serum_market.as_array().clone()),
                            serum_bids: Pubkey::new_from_array(e.serum_bids.as_array().clone()),
                            serum_asks: Pubkey::new_from_array(e.serum_asks.as_array().clone()),
                            serum_event_queue: Pubkey::new_from_array(e.serum_event_queue.as_array().clone()),
                            serum_coin_vault_account: Pubkey::new_from_array(e.serum_coin_vault_account.as_array().clone()),
                            serum_pc_vault_account: Pubkey::new_from_array(e.serum_pc_vault_account.as_array().clone()),
                            serum_vault_signer: Pubkey::new_from_array(e.serum_vault_signer.as_array().clone()),
                            last_updated: (e.metadata.recv_us/1000000) as u64,
                            base_reserve: btb.amount,
                            quote_reserve: qtb.amount,
                        };
                        pool_update_events.push(PoolUpdateEvent::RaydiumPoolUpdate(raydium_pool_update));
                    }
                }
            },
            RaydiumAmmV4WithdrawPnlEvent => |e: RaydiumAmmV4WithdrawPnlEvent| {
                let base_token_balance = post_token_balances.get(e.pool_coin_token_account.to_string().as_str());
                let quote_token_balance = post_token_balances.get(e.pool_pc_token_account.to_string().as_str());
                if let (Some(btb), Some(qtb)) = (base_token_balance, quote_token_balance) {
                    if is_base_token(&btb.mint) || is_base_token(&qtb.mint) {
                        let raydium_pool_update = RaydiumAmmV4PoolUpdate {
                            address: Pubkey::new_from_array(e.amm.as_array().clone()),
                            slot: e.metadata.slot,
                            transaction_index: e.metadata.transaction_index,
                            base_mint: Pubkey::from_str(&btb.mint).unwrap(),
                            quote_mint: Pubkey::from_str(&qtb.mint).unwrap(),
                            amm_authority: Pubkey::new_from_array(e.amm_authority.as_array().clone()),
                            amm_open_orders: Pubkey::new_from_array(e.amm.as_array().clone()),
                            amm_target_orders: Pubkey::new_from_array(e.amm_target_orders.as_array().clone()),
                            pool_coin_token_account: Pubkey::new_from_array(e.pool_coin_token_account.as_array().clone()),
                            pool_pc_token_account: Pubkey::new_from_array(e.pool_pc_token_account.as_array().clone()),
                            serum_program: Pubkey::new_from_array(e.serum_program.as_array().clone()),
                            serum_market: Pubkey::new_from_array(e.serum_market.as_array().clone()),
                            serum_bids: Pubkey::default(),
                            serum_asks: Pubkey::default(),
                            serum_event_queue: Pubkey::new_from_array(e.serum_event_queue.as_array().clone()),
                            serum_coin_vault_account: Pubkey::new_from_array(e.serum_coin_vault_account.as_array().clone()),
                            serum_pc_vault_account: Pubkey::new_from_array(e.serum_pc_vault_account.as_array().clone()),
                            serum_vault_signer: Pubkey::new_from_array(e.serum_vault_signer.as_array().clone()),
                            last_updated: (e.metadata.recv_us/1000000) as u64,
                            base_reserve: btb.amount,
                            quote_reserve: qtb.amount,
                        };
                        pool_update_events.push(PoolUpdateEvent::RaydiumPoolUpdate(raydium_pool_update));
                    }
                }
            },
            // -------------------------- account -----------------------
            BonkPoolStateAccountEvent => |e: BonkPoolStateAccountEvent| {
                println!("BonkPoolStateAccountEvent: {e:?}");

                pool_update_events.push(PoolUpdateEvent::BonkPoolUpdate(
                    BonkPoolUpdate {
                        slot: e.metadata.slot,
                        transaction_index: e.metadata.transaction_index,
                        address: Pubkey::new_from_array(e.pubkey.as_array().clone()), // bondin
                        status: e.pool_state.status,
                        base_decimals: e.pool_state.base_decimals,
                        quote_decimals: e.pool_state.quote_decimals,
                        total_base_sell: e.pool_state.total_base_sell,
                        base_reserve: e.pool_state.virtual_base, // virtual_base
                        quote_reserve: e.pool_state.virtual_quote, // virtual_quote
                        real_base: e.pool_state.real_base,
                        real_quote: e.pool_state.real_quote,
                        quote_protocol_fee: e.pool_state.quote_protocol_fee,
                        platform_fee: e.pool_state.platform_fee,
                        global_config: Pubkey::new_from_array(e.pool_state.global_config.as_array().clone()),
                        platform_config: Pubkey::new_from_array(e.pool_state.platform_config.as_array().clone()),
                        base_mint: Pubkey::new_from_array(e.pool_state.base_mint.as_array().clone()),
                        quote_mint: Pubkey::new_from_array(e.pool_state.quote_mint.as_array().clone()),
                        base_vault: Pubkey::new_from_array(e.pool_state.base_vault.as_array().clone()),
                        quote_vault: Pubkey::new_from_array(e.pool_state.quote_vault.as_array().clone()),
                        creator: Pubkey::new_from_array(e.pool_state.creator.as_array().clone()),
                        last_updated: (e.metadata.recv_us/1000000) as u64, // Unix timestamp
                    }));
            },
            BonkGlobalConfigAccountEvent => |e: BonkGlobalConfigAccountEvent| {
                // println!("BonkGlobalConfigAccountEvent: {e:?}");
            },
            BonkPlatformConfigAccountEvent => |e: BonkPlatformConfigAccountEvent| {
                // println!("BonkPlatformConfigAccountEvent: {e:?}");
            },
            PumpSwapGlobalConfigAccountEvent => |e: PumpSwapGlobalConfigAccountEvent| {
                // do nothing for now
            },
            PumpSwapPoolAccountEvent => |e: PumpSwapPoolAccountEvent| {
                pool_update_events.push(PoolUpdateEvent::PumpSwapPoolUpdate(
                    PumpSwapPoolUpdate {
                        address: Pubkey::new_from_array(e.pubkey.as_array().clone()),
                        index: Some(e.pool.index),
                        creator: Some(Pubkey::new_from_array(e.pool.creator.as_array().clone())),
                        base_mint: Pubkey::new_from_array(e.pool.base_mint.as_array().clone()),
                        quote_mint: Pubkey::new_from_array(e.pool.quote_mint.as_array().clone()),
                        pool_base_token_account: Pubkey::new_from_array(e.pool.pool_base_token_account.as_array().clone()),
                        pool_quote_token_account: Pubkey::new_from_array(e.pool.pool_quote_token_account.as_array().clone()),
                        last_updated: (e.metadata.recv_us/1000000) as u64,
                        base_reserve: 0, // 0 means not updated by this event
                        quote_reserve: 0, // 0 means not updated by this event
                        slot: e.metadata.slot,
                        transaction_index: e.metadata.transaction_index,
                    }));
            },
            PumpFunBondingCurveAccountEvent => |e: PumpFunBondingCurveAccountEvent| {
                // println!("PumpFunBondingCurveAccountEvent: {e:?}");
            },
            PumpFunGlobalAccountEvent => |e: PumpFunGlobalAccountEvent| {
                // println!("PumpFunGlobalAccountEvent: {e:?}");
            },
            RaydiumAmmV4AmmInfoAccountEvent => |e: RaydiumAmmV4AmmInfoAccountEvent| {
                // do nothing for now
            },
            RaydiumClmmAmmConfigAccountEvent => |e: RaydiumClmmAmmConfigAccountEvent| {
                println!("RaydiumClmmAmmConfigAccountEvent: {e:?}");
            },
            RaydiumClmmPoolStateAccountEvent => |e: RaydiumClmmPoolStateAccountEvent| {
                println!("RaydiumClmmPoolStateAccountEvent: {e:?}");
            },
            RaydiumClmmTickArrayStateAccountEvent => |e: RaydiumClmmTickArrayStateAccountEvent| {
                println!("RaydiumClmmTickArrayStateAccountEvent: {e:?}");
            },
            RaydiumCpmmAmmConfigAccountEvent => |e: RaydiumCpmmAmmConfigAccountEvent| {
                // do nothing for now
            },
            RaydiumCpmmPoolStateAccountEvent => |e: RaydiumCpmmPoolStateAccountEvent| {
                let raydium_pool_update = RaydiumCpmmPoolUpdate {
                    address: Pubkey::new_from_array(e.pubkey.as_array().clone()),
                    slot: e.metadata.slot,
                    transaction_index: e.metadata.transaction_index,
                    token0: Pubkey::new_from_array(e.pool_state.token0_mint.as_array().clone()),
                    token1: Pubkey::new_from_array(e.pool_state.token1_mint.as_array().clone()),
                    token0_vault: Pubkey::new_from_array(e.pool_state.token0_vault.as_array().clone()),
                    token1_vault: Pubkey::new_from_array(e.pool_state.token1_vault.as_array().clone()),
                    token0_reserve: 0,
                    token1_reserve: 0,
                    amm_config: Pubkey::new_from_array(e.pool_state.amm_config.as_array().clone()),
                    observation_state: Pubkey::new_from_array(e.pool_state.observation_key.as_array().clone()),
                    last_updated: (e.metadata.recv_us/1000000) as u64,
                    status: Some(e.pool_state.status),
                };
                pool_update_events.push(PoolUpdateEvent::RaydiumCpmmPoolUpdate(raydium_pool_update));
            },
            TokenAccountEvent => |e: TokenAccountEvent| {
                // do nothing for now
            },
            NonceAccountEvent => |e: NonceAccountEvent| {
                println!("NonceAccountEvent: {e:?}");
            },
            TokenInfoEvent => |e: TokenInfoEvent| {
                println!("TokenInfoEvent: {e:?}");
            },
        });
    }

    if !pool_update_events.is_empty() {
        let _ = pool_update_tx.send(pool_update_events);
    }
}
