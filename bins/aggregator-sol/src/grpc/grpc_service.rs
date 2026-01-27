use std::collections::HashMap;
use std::time::Duration;

/// Maximum number of pubkeys allowed per gRPC filter (Yellowstone limit)
const MAX_PUBKEYS_PER_FILTER: usize = 10;

use crate::dex::handle_dex_event;
use crate::types::AggregatorConfig;
use crate::types::ChainStateUpdate;
use crate::types::PoolUpdateEvent;
use solana_streamer_sdk::streaming::event_parser::core::event_parser::{
    PubkeyData, SimplifiedTokenBalance,
};
use solana_streamer_sdk::streaming::event_parser::protocols::meteora_dammv2::parser::METEORA_DAMM_V2_PROGRAM_ID;
use solana_streamer_sdk::streaming::event_parser::protocols::meteora_dbc::parser::DBC_PROGRAM_ID;
use solana_streamer_sdk::streaming::event_parser::protocols::meteora_dlmm::parser::METEORA_DLMM_PROGRAM_ID;
use solana_streamer_sdk::streaming::event_parser::protocols::orca_whirlpools::parser::ORCA_WHIRLPOOL_PROGRAM_ID;
use solana_streamer_sdk::streaming::{
    event_parser::{
        protocols::{
            bonk::parser::BONK_PROGRAM_ID, pumpfun::parser::PUMPFUN_PROGRAM_ID,
            pumpswap::parser::PUMPSWAP_PROGRAM_ID,
            raydium_amm_v4::parser::RAYDIUM_AMM_V4_PROGRAM_ID,
            raydium_clmm::parser::RAYDIUM_CLMM_PROGRAM_ID,
            raydium_cpmm::parser::RAYDIUM_CPMM_PROGRAM_ID,
        },
        Protocol, UnifiedEvent,
    },
    grpc::ClientConfig,
    yellowstone_grpc::{AccountFilter, TransactionFilter},
    YellowstoneGrpc,
};
use yellowstone_grpc_proto::geyser::CommitmentLevel;

use tokio::{sync::mpsc, time::interval};

/// Type alias for the complex event data tuple used in batch processing
type EventBatch = (
    Vec<Box<dyn UnifiedEvent>>,
    Vec<PubkeyData>,
    Vec<u64>,
    HashMap<String, SimplifiedTokenBalance>,
);

pub struct BatchProcessor {
    batch_size: usize,
    timeout_duration: Duration,
    event_tx: mpsc::UnboundedSender<EventBatch>,
}

impl BatchProcessor {
    pub fn new(
        batch_size: usize,
        timeout_duration: Duration,
    ) -> (Self, mpsc::UnboundedReceiver<Vec<EventBatch>>) {
        let (event_tx, event_rx) = mpsc::unbounded_channel::<EventBatch>();
        let (batch_tx, batch_rx) = mpsc::unbounded_channel::<Vec<EventBatch>>();

        let processor = Self {
            batch_size,
            timeout_duration,
            event_tx,
        };

        // Start the batch processing task
        tokio::spawn(Self::process_batches(
            event_rx,
            batch_tx,
            batch_size,
            timeout_duration,
        ));

        (processor, batch_rx)
    }

    pub fn send_event(
        &self,
        events: Vec<Box<dyn UnifiedEvent>>,
        accounts: Vec<PubkeyData>,
        post_balances: Vec<u64>,
        post_token_balances: HashMap<String, SimplifiedTokenBalance>,
    ) {
        let _ = self
            .event_tx
            .send((events, accounts, post_balances, post_token_balances));
    }

    async fn process_batches(
        mut event_rx: mpsc::UnboundedReceiver<EventBatch>, // Receive individual events
        batch_tx: mpsc::UnboundedSender<Vec<EventBatch>>,  // Send batches
        batch_size: usize,
        timeout_duration: Duration,
    ) {
        let mut current_batch = Vec::with_capacity(batch_size);
        let mut timeout_interval = interval(timeout_duration);
        timeout_interval.tick().await; // Start the timer

        loop {
            tokio::select! {
                // Process events as they come in
                event = event_rx.recv() => {
                    match event {
                        Some(e) => {

                            current_batch.push(e);

                            // If batch is full, process it immediately
                            if current_batch.len() >= batch_size {
                                let batch_to_send = std::mem::take(&mut current_batch);
                                if let Err(e) = batch_tx.send(batch_to_send) {
                                    log::error!("Failed to send batch: {}", e);
                                }
                                timeout_interval.reset(); // Reset timer for next batch
                            }
                        }
                        None => {
                            log::error!("BatchProcessor event_rx channel closed!");
                            break;
                        },
                    }
                }

                // Timeout reached - process current batch even if not full
                _ = timeout_interval.tick() => {
                    if !current_batch.is_empty() {
                        let batch_to_send = std::mem::take(&mut current_batch);
                        if let Err(e) = batch_tx.send(batch_to_send) {
                            log::error!("Failed to send timed-out batch: {}", e);
                        }
                    }
                    // Timer automatically resets
                }
            }
        }

        // Process any remaining events when shutting down
        if !current_batch.is_empty() {
            if let Err(e) = batch_tx.send(current_batch) {
                log::error!("Failed to send final batch: {}", e);
            }
        }
    }

    pub async fn process_batch(
        batch: Vec<EventBatch>,
        pool_update_tx: mpsc::UnboundedSender<Vec<PoolUpdateEvent>>,
        chain_state_update_tx: mpsc::UnboundedSender<ChainStateUpdate>,
    ) {
        // Process events concurrently within the batch
        let tasks: Vec<_> = batch
            .into_iter()
            .map(|(events, accounts, post_balances, post_token_balances)| {
                let pool_update_tx_clone = pool_update_tx.clone();
                let chain_state_update_tx_clone = chain_state_update_tx.clone();
                tokio::spawn(async move {
                    Self::process_single_event(
                        events,
                        accounts,
                        post_balances,
                        post_token_balances,
                        pool_update_tx_clone,
                        chain_state_update_tx_clone,
                    )
                    .await;
                })
            })
            .collect();

        // Wait for all events in this batch to be processed
        for task in tasks {
            let _ = task.await;
        }
    }

    async fn process_single_event(
        events: Vec<Box<dyn UnifiedEvent>>,
        accounts: Vec<PubkeyData>,
        post_balances: Vec<u64>,
        post_token_balances: HashMap<String, SimplifiedTokenBalance>,
        pool_update_tx: mpsc::UnboundedSender<Vec<PoolUpdateEvent>>,
        chain_state_update_tx: mpsc::UnboundedSender<ChainStateUpdate>,
    ) {
        handle_dex_event(
            events,
            accounts,
            post_balances,
            post_token_balances,
            pool_update_tx,
            chain_state_update_tx,
        );
    }
}

use std::sync::Arc;
use tokio::sync::Mutex;

/// Holds a gRPC client with its specific filters for a chunk of pools
struct SubscriptionInfo {
    grpc: YellowstoneGrpc,
    transaction_filter: TransactionFilter,
    account_filter: AccountFilter,
}

pub struct GrpcService {
    /// Multiple gRPC clients for multi-subscription (one per pool chunk)
    subscriptions: Vec<SubscriptionInfo>,
    batch_processor: Arc<BatchProcessor>,
    protocols: Vec<Protocol>,
    batch_rx: Arc<Mutex<Option<mpsc::UnboundedReceiver<Vec<EventBatch>>>>>,
}

use crate::grpc::traits::GrpcServiceTrait;
use async_trait::async_trait;

impl GrpcService {
    /// Start all gRPC subscriptions with batch processing
    pub async fn start(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        log::info!(
            "Starting {} gRPC subscription(s) with batch processing...",
            self.subscriptions.len()
        );
        log::info!(
            "Batch size: {}, Timeout: {}ms",
            self.batch_processor.batch_size,
            self.batch_processor.timeout_duration.as_millis()
        );

        // Start each subscription in parallel
        for (idx, sub) in self.subscriptions.iter().enumerate() {
            let batch_processor = Arc::clone(&self.batch_processor);
            let protocols = self.protocols.clone();
            let tx_filter = sub.transaction_filter.clone();
            let acc_filter = sub.account_filter.clone();

            log::info!(
                "📡 Subscription {}: Monitoring {} pools",
                idx + 1,
                acc_filter.account.len()
            );

            // Create callback that sends events to shared batch processor
            let callback = move |events: Vec<Box<dyn UnifiedEvent>>,
                                 accounts,
                                 post_balances,
                                 post_token_balances| {
                batch_processor.send_event(events, accounts, post_balances, post_token_balances);
            };

            sub.grpc
                .subscribe_events_immediate(
                    protocols,
                    None,
                    vec![tx_filter],
                    vec![acc_filter],
                    None,
                    Some(CommitmentLevel::Processed),
                    callback,
                )
                .await?;
        }

        log::info!(
            "✅ All {} gRPC subscriptions started.",
            self.subscriptions.len()
        );

        Ok(())
    }

    pub async fn stop(&self) {
        for sub in &self.subscriptions {
            sub.grpc.stop().await;
        }
        log::info!("All gRPC subscriptions stopped.");
    }
}

#[async_trait]
impl GrpcServiceTrait for GrpcService {
    async fn subscribe_pool_updates(
        &self,
        pool_update_sender: mpsc::UnboundedSender<Vec<PoolUpdateEvent>>,
        chain_state_sender: mpsc::UnboundedSender<ChainStateUpdate>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Spawn consumer loop FIRST to ensure it's ready to handle events
        // even if start() blocks or events arrive immediately.
        {
            let mut rx_guard = self.batch_rx.lock().await;
            if let Some(mut rx) = rx_guard.take() {
                // Spawn consumer loop
                tokio::spawn(async move {
                    while let Some(batch) = rx.recv().await {
                        // log::info!("Consumer loop received batch of size {}", batch.len());
                        BatchProcessor::process_batch(
                            batch,
                            pool_update_sender.clone(),
                            chain_state_sender.clone(),
                        )
                        .await;
                    }
                    log::warn!("Consumer loop exited - channel closed");
                });
            } else {
                log::error!("FAILED TO TAKE BATCH RX - Already taken?");
            }
        }

        // THEN Start gRPC subscription
        // If this blocks, it's fine because the consumer is already running in a separate task.
        if let Err(e) = self.start().await {
            log::error!("GrpcService failed to start: {}", e);
            return Err(e.into());
        }
        Ok(())
    }

    async fn stop(&self) {
        self.stop().await;
    }
}

pub async fn create_grpc_service(
    batch_size: usize,
    batch_timeout_ms: u64,
    monitored_pool_addresses: Option<Vec<String>>,
) -> Result<Arc<GrpcService>, Box<dyn std::error::Error>> {
    let agg_config = AggregatorConfig::from_env().unwrap();

    // Create batch processor (shared across all subscriptions)
    let (batch_processor, batch_rx) =
        BatchProcessor::new(batch_size, Duration::from_millis(batch_timeout_ms));
    let batch_processor = Arc::new(batch_processor);

    // Dynamically build account_include and protocols based on .env flags
    let mut account_include = Vec::new();
    let mut protocols = Vec::new();

    if agg_config.enable_pumpfun {
        account_include.push(PUMPFUN_PROGRAM_ID.to_string());
        protocols.push(Protocol::PumpFun);
    }
    if agg_config.enable_pumpfun_swap {
        account_include.push(PUMPSWAP_PROGRAM_ID.to_string());
        protocols.push(Protocol::PumpSwap);
    }
    if agg_config.enable_bonk {
        account_include.push(BONK_PROGRAM_ID.to_string());
        protocols.push(Protocol::Bonk);
    }
    if agg_config.enable_raydium_cpmm {
        account_include.push(RAYDIUM_CPMM_PROGRAM_ID.to_string());
        protocols.push(Protocol::RaydiumCpmm);
    }
    if agg_config.enable_raydium_clmm {
        account_include.push(RAYDIUM_CLMM_PROGRAM_ID.to_string());
        protocols.push(Protocol::RaydiumClmm);
    }
    if agg_config.enable_raydium_amm_v4 {
        account_include.push(RAYDIUM_AMM_V4_PROGRAM_ID.to_string());
        protocols.push(Protocol::RaydiumAmmV4);
    }
    if agg_config.enable_meteora_dbc {
        account_include.push(DBC_PROGRAM_ID.to_string());
        protocols.push(Protocol::MeteoraDbc);
    }
    if agg_config.enable_meteora_dammv2 {
        account_include.push(METEORA_DAMM_V2_PROGRAM_ID.to_string());
        protocols.push(Protocol::MeteoraDammV2);
    }
    if agg_config.enable_orca_whirlpools {
        account_include.push(ORCA_WHIRLPOOL_PROGRAM_ID.to_string());
        protocols.push(Protocol::OrcaWhirlpools);
    }
    if agg_config.enable_meteora_dlmm {
        account_include.push(METEORA_DLMM_PROGRAM_ID.to_string());
        protocols.push(Protocol::MeteoraDlmm);
    }

    log::info!("Enabled DEXes: {:?}", protocols);

    // Build subscriptions based on mode
    let subscriptions: Vec<SubscriptionInfo> =
        if let Some(ref pool_addresses) = monitored_pool_addresses {
            // Filtered mode: Chunk pools into groups of MAX_PUBKEYS_PER_FILTER
            let pool_chunks: Vec<Vec<String>> = pool_addresses
                .chunks(MAX_PUBKEYS_PER_FILTER)
                .map(|c| c.to_vec())
                .collect();

            log::info!(
                "🎯 Filtered subscription mode: {} pools → {} subscription(s) (max {} per filter)",
                pool_addresses.len(),
                pool_chunks.len(),
                MAX_PUBKEYS_PER_FILTER
            );

            let mut subs = Vec::with_capacity(pool_chunks.len());
            for (idx, chunk) in pool_chunks.into_iter().enumerate() {
                // Create a new gRPC client for each chunk
                let mut config: ClientConfig = ClientConfig::high_throughput();
                config.enable_metrics = true;
                let grpc = YellowstoneGrpc::new_with_config(
                    agg_config.yellowstone_grpc_url.clone(),
                    None,
                    agg_config.backup_grpc_url.clone(),
                    config,
                )?;

                log::info!(
                    "📡 Created gRPC client {} for {} pools",
                    idx + 1,
                    chunk.len()
                );

                let tx_filter = TransactionFilter {
                    account_include: chunk.clone(),
                    account_exclude: vec![],
                    account_required: vec![],
                };

                let acc_filter = AccountFilter {
                    account: chunk,
                    owner: vec![],
                    filters: vec![],
                };

                subs.push(SubscriptionInfo {
                    grpc,
                    transaction_filter: tx_filter,
                    account_filter: acc_filter,
                });
            }
            subs
        } else {
            // Legacy mode: Single subscription monitoring all DEX programs
            log::info!("📡 Full subscription mode: Monitoring all pools from enabled DEXes");

            let mut config: ClientConfig = ClientConfig::high_throughput();
            config.enable_metrics = true;
            let grpc = YellowstoneGrpc::new_with_config(
                agg_config.yellowstone_grpc_url.clone(),
                None,
                agg_config.backup_grpc_url.clone(),
                config,
            )?;

            let tx_filter = TransactionFilter {
                account_include: account_include.clone(),
                account_exclude: vec![],
                account_required: vec![],
            };

            let acc_filter = AccountFilter {
                account: vec![],
                owner: account_include.clone(),
                filters: vec![],
            };

            log::info!("Monitoring programs: {:?}", account_include);

            vec![SubscriptionInfo {
                grpc,
                transaction_filter: tx_filter,
                account_filter: acc_filter,
            }]
        };

    log::info!("Created {} gRPC subscription(s)", subscriptions.len());

    Ok(Arc::new(GrpcService {
        subscriptions,
        batch_processor,
        protocols,
        batch_rx: Arc::new(Mutex::new(Some(batch_rx))),
    }))
}
