use std::collections::HashMap;
use std::time::Duration;

use crate::dex::handle_dex_event;
use crate::types::AggregatorConfig;
use crate::types::ChainStateUpdate;
use crate::types::PoolUpdateEvent;
use solana_streamer_sdk::streaming::event_parser::core::event_parser::{
    PubkeyData, SimplifiedTokenBalance,
};
use solana_streamer_sdk::streaming::event_parser::protocols::meteora_dbc::parser::DBC_PROGRAM_ID;
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
                                let _ = batch_tx.send(batch_to_send);
                                timeout_interval.reset(); // Reset timer for next batch
                            }
                        }
                        None => break, // Channel closed
                    }
                }

                // Timeout reached - process current batch even if not full
                _ = timeout_interval.tick() => {
                    if !current_batch.is_empty() {
                        let batch_to_send = std::mem::take(&mut current_batch);
                        let _ = batch_tx.send(batch_to_send);
                    }
                    // Timer automatically resets
                }
            }
        }

        // Process any remaining events when shutting down
        if !current_batch.is_empty() {
            let _ = batch_tx.send(current_batch);
        }
    }

    pub async fn process_batch(
        batch: Vec<EventBatch>,
        pool_update_tx: mpsc::UnboundedSender<Vec<PoolUpdateEvent>>,
        chain_state_update_tx: mpsc::UnboundedSender<ChainStateUpdate>,
    ) {
        log::trace!("Processing batch of {} events", batch.len());

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

pub struct GrpcService {
    grpc: YellowstoneGrpc,
    batch_processor: Arc<BatchProcessor>,
    transaction_filter: TransactionFilter,
    account_filter: AccountFilter,
    protocols: Vec<Protocol>,
}

impl GrpcService {
    /// Start the gRPC service with batch processing
    pub async fn start(self: Arc<Self>) -> Result<(), Box<dyn std::error::Error>> {
        // Clone Arc for the callback
        let batch_processor = Arc::clone(&self.batch_processor);

        // Create callback that sends events to batch processor
        let callback = move |events: Vec<Box<dyn UnifiedEvent>>,
                             accounts,
                             post_balances,
                             post_token_balances| {
            batch_processor.send_event(events, accounts, post_balances, post_token_balances);
        };

        log::info!("Starting gRPC subscription with batch processing...");
        log::info!(
            "Batch size: {}, Timeout: {}ms",
            self.batch_processor.batch_size,
            self.batch_processor.timeout_duration.as_millis()
        );
        log::info!("Monitoring programs: {:?}", self.account_filter.owner);
        self.grpc
            .subscribe_events_immediate(
                self.protocols.clone(),
                None,
                vec![self.transaction_filter.clone()],
                vec![self.account_filter.clone()],
                None,
                Some(CommitmentLevel::Processed),
                callback,
            )
            .await?;

        log::info!("gRPC subscription started. Event handling loop should be run separately.");

        Ok(())
    }

    pub async fn stop(&self) {
        self.grpc.stop().await;
        log::info!("gRPC subscription stopped.");
    }
}
pub async fn create_grpc_service(
    batch_size: usize,
    batch_timeout_ms: u64,
) -> Result<(Arc<GrpcService>, mpsc::UnboundedReceiver<Vec<EventBatch>>), Box<dyn std::error::Error>>
{
    let agg_config = AggregatorConfig::from_env().unwrap();

    // Create low-latency configuration
    let mut config: ClientConfig = ClientConfig::high_throughput();
    // Enable performance monitoring, has performance overhead, disabled by default
    config.enable_metrics = true;
    let grpc = YellowstoneGrpc::new_with_config(
        agg_config.yellowstone_grpc_url,
        None,
        agg_config.backup_grpc_url,
        config,
    )?;
    log::info!("GRPC client created successfully");

    // Create batch processor
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

    if agg_config.enable_orca_whirlpools {
        account_include.push(ORCA_WHIRLPOOL_PROGRAM_ID.to_string());
        protocols.push(Protocol::OrcaWhirlpools);
    }

    let account_exclude = vec![];
    let account_required = vec![];

    // Listen to transaction data
    let transaction_filter = TransactionFilter {
        account_include: account_include.clone(),
        account_exclude,
        account_required,
    };

    // Listen to account data belonging to owner programs -> account event monitoring
    let account_filter = AccountFilter {
        account: vec![], // Raydium AMM V4 program's authority account
        owner: account_include.clone(),
        filters: vec![],
    };

    log::info!("Enabled DEXes: {:?}", protocols);
    log::info!("Monitoring programs: {:?}", account_include);

    Ok((
        Arc::new(GrpcService {
            grpc,
            batch_processor,
            transaction_filter,
            account_filter,
            protocols,
        }),
        batch_rx,
    ))
}
