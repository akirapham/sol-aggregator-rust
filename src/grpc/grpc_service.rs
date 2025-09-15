use std::time::Duration;

use crate::dex::handle_dex_event;
use crate::{config::ConfigLoader, PoolUpdateEvent};
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
use tokio::{sync::mpsc, time::interval};

pub struct BatchProcessor {
    batch_size: usize,
    timeout_duration: Duration,
    event_tx: mpsc::UnboundedSender<Box<dyn UnifiedEvent>>,
}

impl BatchProcessor {
    pub fn new(
        batch_size: usize,
        timeout_duration: Duration,
    ) -> (Self, mpsc::UnboundedReceiver<Vec<Box<dyn UnifiedEvent>>>) {
        let (event_tx, event_rx) = mpsc::unbounded_channel::<Box<dyn UnifiedEvent>>();
        let (batch_tx, batch_rx) = mpsc::unbounded_channel::<Vec<Box<dyn UnifiedEvent>>>();

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

    pub fn send_event(&self, event: Box<dyn UnifiedEvent>) {
        let _ = self.event_tx.send(event);
    }

    async fn process_batches(
        mut event_rx: mpsc::UnboundedReceiver<Box<dyn UnifiedEvent>>, // Receive individual events
        batch_tx: mpsc::UnboundedSender<Vec<Box<dyn UnifiedEvent>>>,  // Send batches
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
        batch: Vec<Box<dyn UnifiedEvent>>,
        pool_update_tx: mpsc::UnboundedSender<PoolUpdateEvent>,
    ) {
        log::trace!("Processing batch of {} events", batch.len());

        // Process events concurrently within the batch
        let tasks: Vec<_> = batch
            .into_iter()
            .map(|event| {
                let pool_update_tx_clone = pool_update_tx.clone();
                tokio::spawn(async move {
                    Self::process_single_event(event, pool_update_tx_clone).await;
                })
            })
            .collect();

        // Wait for all events in this batch to be processed
        for task in tasks {
            let _ = task.await;
        }
    }

    async fn process_single_event(
        event: Box<dyn UnifiedEvent>,
        pool_update_tx: mpsc::UnboundedSender<PoolUpdateEvent>,
    ) {
        handle_dex_event(event, pool_update_tx);
    }
}

use std::sync::Arc;

pub struct GrpcService {
    grpc: YellowstoneGrpc,
    batch_processor: Arc<BatchProcessor>,
}

impl GrpcService {
    /// Start the gRPC service with batch processing
    pub async fn start(self: Arc<Self>) -> Result<(), Box<dyn std::error::Error>> {
        // Filter accounts
        let account_include = vec![
            PUMPFUN_PROGRAM_ID.to_string(), // Listen to pumpfun program ID
                                            // PUMPSWAP_PROGRAM_ID.to_string(),       // Listen to pumpswap program ID
                                            // BONK_PROGRAM_ID.to_string(),           // Listen to bonk program ID
                                            // RAYDIUM_CPMM_PROGRAM_ID.to_string(),   // Listen to raydium_cpmm program ID
                                            // RAYDIUM_CLMM_PROGRAM_ID.to_string(),   // Listen to raydium_clmm program ID
                                            // RAYDIUM_AMM_V4_PROGRAM_ID.to_string(), // Listen to raydium_amm_v4 program ID
        ];

        let protocols = vec![
            Protocol::PumpFun,
            Protocol::PumpSwap,
            Protocol::Bonk,
            Protocol::RaydiumCpmm,
            Protocol::RaydiumClmm,
            Protocol::RaydiumAmmV4,
        ];

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
            account: vec![],
            owner: account_include.clone(),
            filters: vec![],
        };

        // Event filtering
        // No event filtering, includes all events
        let event_type_filter = None;

        // Clone Arc for the callback
        let batch_processor = Arc::clone(&self.batch_processor);

        // Create callback that sends events to batch processor
        let callback = move |event: Box<dyn UnifiedEvent>| {
            batch_processor.send_event(event);
        };

        log::info!("Starting gRPC subscription with batch processing...");
        log::info!(
            "Batch size: {}, Timeout: {}ms",
            self.batch_processor.batch_size,
            self.batch_processor.timeout_duration.as_millis()
        );
        log::info!("Monitoring programs: {:?}", account_include);
        self.grpc
            .subscribe_events_immediate(
                protocols,
                None,
                vec![transaction_filter],
                vec![account_filter],
                event_type_filter,
                None,
                callback,
            )
            .await?;

        log::info!("gRPC subscription started. Event handling loop should be run separately.");

        Ok(())
    }
}
pub async fn create_grpc_service(
    batch_size: usize,
    batch_timeout_ms: u64,
) -> Result<
    (
        Arc<GrpcService>,
        mpsc::UnboundedReceiver<Vec<Box<dyn UnifiedEvent>>>,
    ),
    Box<dyn std::error::Error>,
> {
    let agg_config = ConfigLoader::load().unwrap();
    // Create low-latency configuration
    let mut config: ClientConfig = ClientConfig::low_latency();
    // Enable performance monitoring, has performance overhead, disabled by default
    config.enable_metrics = true;
    let grpc = YellowstoneGrpc::new_with_config(agg_config.yellowstone_grpc_url, None, config)?;
    log::info!("GRPC client created successfully");

    // Create batch processor
    let (batch_processor, batch_rx) =
        BatchProcessor::new(batch_size, Duration::from_millis(batch_timeout_ms));

    let batch_processor = Arc::new(batch_processor);

    Ok((
        Arc::new(GrpcService {
            grpc,
            batch_processor,
        }),
        batch_rx,
    ))
}
