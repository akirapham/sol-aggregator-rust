use crate::aggregator::DexAggregator;
use crate::arbitrage_config::ArbitrageConfig;
use crate::pool_manager::ArbitragePoolUpdate;
use crate::types::{ExecutionPriority, SwapParams};
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc};

/// Detected arbitrage opportunity
#[derive(Debug, Clone)]
pub struct ArbitrageOpportunity {
    pub pair_name: String,
    pub token_a: Pubkey,
    pub token_b: Pubkey,
    pub profit_amount: u64,
    pub profit_percent: f64,
    pub input_amount: u64,
    pub forward_output: u64,
    pub reverse_output: u64,
    pub detected_at: u64,
}

/// Price comparison result
#[derive(Debug, Clone)]
struct PriceComparison {
    forward_price: f64,    // Price of token_b in token_a (token_a -> token_b)
    reverse_price: f64,    // Price of token_a in token_b (token_b -> token_a)
    price_difference: f64, // (forward_price * reverse_price - 1) * 100 as percentage
}

/// Active arbitrage monitor that watches pool updates
pub struct ArbitrageMonitor {
    aggregator: Arc<DexAggregator>,
    config: ArbitrageConfig,
    opportunity_tx: mpsc::UnboundedSender<ArbitrageOpportunity>,
}

impl ArbitrageMonitor {
    pub fn new(
        aggregator: Arc<DexAggregator>,
        config: ArbitrageConfig,
    ) -> (Self, mpsc::UnboundedReceiver<ArbitrageOpportunity>) {
        let (opportunity_tx, opportunity_rx) = mpsc::unbounded_channel();

        let monitor = Self {
            aggregator,
            config,
            opportunity_tx,
        };

        (monitor, opportunity_rx)
    }

    /// Subscribe to pool update events from the pool manager
    /// This spawns a task that listens for events and triggers arbitrage checks
    pub fn subscribe_to_pool_updates(
        self: Arc<Self>,
        mut pool_update_rx: broadcast::Receiver<ArbitragePoolUpdate>,
    ) {
        tokio::spawn(async move {
            log::info!("Arbitrage monitor subscribed to broadcast pool update events");

            loop {
                match pool_update_rx.recv().await {
                    Ok(pool_update) => {
                        // Spawn event handler to avoid blocking the loop
                        let self_clone = self.clone();
                        tokio::spawn(async move {
                            self_clone.on_broadcast_pool_update(&pool_update).await;
                        });
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        // Subscriber lagged, skip
                        log::warn!("Arbitrage monitor lagged on broadcast channel");
                        continue;
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        // Channel closed
                        log::info!("Arbitrage monitor broadcast channel closed");
                        break;
                    }
                }
            }
        });
    }

    /// Check arbitrage when a broadcast pool update is received
    /// This is triggered by real-time pool updates from the broadcast channel
    async fn on_broadcast_pool_update(&self, pool_update: &ArbitragePoolUpdate) {
        log::debug!(
            "🔄 Broadcast pool update: {} | {} <-> {} | Forward price: {:.8}, Reverse price: {:.8}",
            pool_update.pool_address,
            pool_update.token_a,
            pool_update.token_b,
            pool_update.forward_price,
            pool_update.reverse_price
        );

        // Quick price check first - round trip should yield profit
        let price_round_trip = pool_update.forward_price * pool_update.reverse_price;
        let price_diff_percent = ((price_round_trip - 1.0) * 100.0).abs();
        let threshold_percent = (self.config.settings.min_profit_bps as f64) / 100.0;

        if price_diff_percent < threshold_percent {
            return; // Not profitable enough
        }

        log::debug!(
            "💹 Price opportunity detected: {:.4}% difference in pool {}",
            price_diff_percent,
            pool_update.pool_address
        );

        // Get base token and monitored tokens
        let base_token = match self.config.get_base_token() {
            Ok(t) => t,
            Err(_) => return,
        };

        let monitored_pubkeys = self.config.get_monitored_token_pubkeys();

        // One token must be base, other must be monitored
        let other_token = if pool_update.token_a == base_token
            && monitored_pubkeys.contains(&pool_update.token_b)
        {
            pool_update.token_b
        } else if pool_update.token_b == base_token
            && monitored_pubkeys.contains(&pool_update.token_a)
        {
            pool_update.token_a
        } else {
            return; // Not a base-to-monitored pair
        };

        log::debug!("✅ Arbitrage pair: base <-> {}", other_token);
        self.check_arbitrage(base_token, other_token).await;
    }

    /// Check arbitrage opportunity for a specific token pair
    /// This performs the actual expensive arbitrage calculation after price checks pass
    async fn check_arbitrage(&self, token_a: Pubkey, token_b: Pubkey) {
        // Get token info from pool manager
        let token_a_info = match self.aggregator.get_pool_manager().get_token(&token_a).await {
            Some(t) => t,
            None => {
                log::debug!("Token A {} not found in pool manager", token_a);
                return;
            }
        };

        let token_b_info = match self.aggregator.get_pool_manager().get_token(&token_b).await {
            Some(t) => t,
            None => {
                log::debug!("Token B {} not found in pool manager", token_b);
                return;
            }
        };

        // STEP: Calculate actual arbitrage profit with full routing
        let swap_params = SwapParams {
            input_token: token_a_info.clone(),
            output_token: token_b_info.clone(),
            input_amount: self.config.settings.base_amount,
            slippage_bps: self.config.settings.slippage_bps,
            user_wallet: Pubkey::default(),
            priority: ExecutionPriority::Medium,
        };

        match self
            .aggregator
            .calculate_arbitrage_profit(&swap_params, &token_b, self.config.settings.slippage_bps)
            .await
        {
            Some((profit, forward_route, reverse_route)) => {
                let profit_percent =
                    (profit as f64 / self.config.settings.base_amount as f64) * 100.0;

                // Check if profit meets minimum threshold
                let profit_bps = (profit_percent * 100.0) as u64;
                if profit_bps >= self.config.settings.min_profit_bps {
                    let opportunity = ArbitrageOpportunity {
                        pair_name: format!("{}-{}", token_a, token_b),
                        token_a,
                        token_b,
                        profit_amount: profit,
                        profit_percent,
                        input_amount: self.config.settings.base_amount,
                        forward_output: forward_route.output_amount,
                        reverse_output: reverse_route.output_amount,
                        detected_at: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_secs(),
                    };

                    log::info!(
                        "🎯 ARBITRAGE: {} <-> {} | Profit: {:.4}% ({} lamports)",
                        token_a,
                        token_b,
                        profit_percent,
                        profit
                    );

                    // Send opportunity to channel
                    if let Err(e) = self.opportunity_tx.send(opportunity) {
                        log::error!("Failed to send arbitrage opportunity: {}", e);
                    }
                }
            }
            None => {
                // No profitable arbitrage
                log::debug!("No profitable arbitrage for {} -> {}", token_a, token_b);
            }
        }
    }
}
