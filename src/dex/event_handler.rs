use rust_decimal::Decimal;
use solana_sdk::pubkey::Pubkey;
use solana_streamer_sdk::{
    match_event,
    streaming::event_parser::{
        core::account_event_parser::{NonceAccountEvent, TokenAccountEvent, TokenInfoEvent},
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
                RaydiumCpmmInitializeEvent, RaydiumCpmmPoolStateAccountEvent, RaydiumCpmmSwapEvent,
                RaydiumCpmmWithdrawEvent,
            },
            BlockMetaEvent,
        },
        UnifiedEvent,
    },
};
use tokio::sync::mpsc;

use crate::{utils::get_sol_mint, PoolUpdateEvent};

pub fn handle_dex_event(
    event: Box<dyn UnifiedEvent>,
    pool_update_tx: mpsc::UnboundedSender<PoolUpdateEvent>,
) {
    match_event!(event, {
        // -------------------------- block meta -----------------------
        BlockMetaEvent => |e: BlockMetaEvent| {
            // println!("BlockMetaEvent: {:?}", e.metadata.handle_us);
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
            let _ = pool_update_tx.send(PoolUpdateEvent::PumpfunPoolUpdate( crate::PumpfunPoolUpdate {pool_address:Pubkey::new_from_array(e.bonding_curve.as_array().clone()),dex:crate::DexType::PumpFun,base_reserve:e.virtual_token_reserves,quote_reserve:e.virtual_sol_reserves,fee_rate:0,lp_supply:0,last_updated:0, mint: Pubkey::new_from_array(e.mint.as_array().clone()), real_base_reserve: e.real_token_reserves }));
        },
        PumpFunMigrateEvent => |e: PumpFunMigrateEvent| {
        },
        PumpFunCreateTokenEvent => |e: PumpFunCreateTokenEvent| {
        },
        // -------------------------- pumpswap -----------------------
        PumpSwapBuyEvent => |e: PumpSwapBuyEvent| {
            println!("Buy event: {e:?}");
        },
        PumpSwapSellEvent => |e: PumpSwapSellEvent| {
            println!("Sell event: {e:?}");
        },
        PumpSwapCreatePoolEvent => |e: PumpSwapCreatePoolEvent| {
            println!("CreatePool event: {e:?}");
        },
        PumpSwapDepositEvent => |e: PumpSwapDepositEvent| {
            println!("Deposit event: {e:?}");
        },
        PumpSwapWithdrawEvent => |e: PumpSwapWithdrawEvent| {
            println!("Withdraw event: {e:?}");
        },
        // -------------------------- raydium_cpmm -----------------------
        RaydiumCpmmSwapEvent => |e: RaydiumCpmmSwapEvent| {
            println!("RaydiumCpmmSwapEvent: {e:?}");
        },
        RaydiumCpmmDepositEvent => |e: RaydiumCpmmDepositEvent| {
            println!("RaydiumCpmmDepositEvent: {e:?}");
        },
        RaydiumCpmmInitializeEvent => |e: RaydiumCpmmInitializeEvent| {
            println!("RaydiumCpmmInitializeEvent: {e:?}");
        },
        RaydiumCpmmWithdrawEvent => |e: RaydiumCpmmWithdrawEvent| {
            println!("RaydiumCpmmWithdrawEvent: {e:?}");
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
            println!("RaydiumAmmV4SwapEvent: {e:?}");
        },
        RaydiumAmmV4DepositEvent => |e: RaydiumAmmV4DepositEvent| {
            println!("RaydiumAmmV4DepositEvent: {e:?}");
        },
        RaydiumAmmV4Initialize2Event => |e: RaydiumAmmV4Initialize2Event| {
            println!("RaydiumAmmV4Initialize2Event: {e:?}");
        },
        RaydiumAmmV4WithdrawEvent => |e: RaydiumAmmV4WithdrawEvent| {
            println!("RaydiumAmmV4WithdrawEvent: {e:?}");
        },
        RaydiumAmmV4WithdrawPnlEvent => |e: RaydiumAmmV4WithdrawPnlEvent| {
            println!("RaydiumAmmV4WithdrawPnlEvent: {e:?}");
        },
        // -------------------------- account -----------------------
        BonkPoolStateAccountEvent => |e: BonkPoolStateAccountEvent| {
            println!("BonkPoolStateAccountEvent: {e:?}");
        },
        BonkGlobalConfigAccountEvent => |e: BonkGlobalConfigAccountEvent| {
            println!("BonkGlobalConfigAccountEvent: {e:?}");
        },
        BonkPlatformConfigAccountEvent => |e: BonkPlatformConfigAccountEvent| {
            println!("BonkPlatformConfigAccountEvent: {e:?}");
        },
        PumpSwapGlobalConfigAccountEvent => |e: PumpSwapGlobalConfigAccountEvent| {
            println!("PumpSwapGlobalConfigAccountEvent: {e:?}");
        },
        PumpSwapPoolAccountEvent => |e: PumpSwapPoolAccountEvent| {
            println!("PumpSwapPoolAccountEvent: {e:?}");
        },
        PumpFunBondingCurveAccountEvent => |e: PumpFunBondingCurveAccountEvent| {
            // println!("PumpFunBondingCurveAccountEvent: {e:?}");
        },
        PumpFunGlobalAccountEvent => |e: PumpFunGlobalAccountEvent| {
            // println!("PumpFunGlobalAccountEvent: {e:?}");
        },
        RaydiumAmmV4AmmInfoAccountEvent => |e: RaydiumAmmV4AmmInfoAccountEvent| {
            println!("RaydiumAmmV4AmmInfoAccountEvent: {e:?}");
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
            println!("RaydiumCpmmAmmConfigAccountEvent: {e:?}");
        },
        RaydiumCpmmPoolStateAccountEvent => |e: RaydiumCpmmPoolStateAccountEvent| {
            println!("RaydiumCpmmPoolStateAccountEvent: {e:?}");
        },
        TokenAccountEvent => |e: TokenAccountEvent| {
            // println!("TokenAccountEvent: {e:?}");
        },
        NonceAccountEvent => |e: NonceAccountEvent| {
            println!("NonceAccountEvent: {e:?}");
        },
        TokenInfoEvent => |e: TokenInfoEvent| {
            println!("TokenInfoEvent: {e:?}");
        },
    });
}
