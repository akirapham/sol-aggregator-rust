use anyhow::Result;
use axum::Router;
use dotenv::dotenv;
use log::{error, info};
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tracing_subscriber;

mod api;
mod dex_price;
mod mexc;
mod types;

use dex_price::{DexPriceClient, DexPriceConfig};
use mexc::MexcService;

use crate::types::PriceProvider;

#[tokio::main]
async fn main() -> Result<()> {
    // Load environment variables from .env file
    dotenv().ok();

    // Initialize logging
    tracing_subscriber::fmt::init();

    info!("Starting CEX Pricing Service");

    let mexc_service = Arc::new(MexcService::new().await?);

    // Start the WebSocket service in background
    let mexc_service_clone = mexc_service.clone();
    tokio::spawn(async move {
        if let Err(e) = mexc_service_clone.start().await {
            error!("MEXC service error: {}", e);
        }
    });

    // Example: Start DEX price client (if DEX_PRICE_STREAM environment variable is set)
    if std::env::var("DEX_PRICE_STREAM").is_ok() {
        let dex_config = DexPriceConfig::from_env();

        info!(
            "Starting DEX price client with URL: {}",
            dex_config.websocket_url
        );

        let (dex_client, mut dex_receiver) = DexPriceClient::new(dex_config);

        // Start DEX client in background
        tokio::spawn(async move {
            if let Err(e) = dex_client.start().await {
                error!("DEX price client error: {}", e);
            }
        });

        // Handle DEX price updates in background
        let mexc_service_clone = mexc_service.clone();
        tokio::spawn(async move {
            while let Some(price_updates) = dex_receiver.recv().await {
                info!("Received {} DEX price updates", price_updates.len());

                for update in &price_updates {
                    // read token price from cex
                    let cex_price = mexc_service_clone
                        .get_price(&update.token_address.to_lowercase())
                        .await;
                    match cex_price {
                        Some(price) => {
                            let price_diff_percent =
                                ((update.price_in_usd - price.price).abs() / price.price) * 100.0;
                            log::info!(
                                "Token: {}, Symbol {}, DEX Price: {}, CEX Price: {}, Diff: {:.2}%",
                                update.token_address,
                                price.symbol,
                                update.price_in_usd,
                                price.price,
                                price_diff_percent
                            );
                        }
                        None => {
                            // do nothing
                        }
                    }
                }

                // Here you can process the price updates:
                // - Store in database
                // - Update internal caches
                // - Forward to other services
                // - Calculate arbitrage opportunities, etc.
            }
        });

        info!("DEX price client started");
    } else {
        info!("DEX_PRICE_STREAM not set, skipping DEX price client");
    }

    // Start HTTP API server
    let app = Router::new()
        .merge(api::create_router(mexc_service))
        .layer(CorsLayer::permissive());

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3001").await?;
    info!("CEX Pricing API server listening on http://0.0.0.0:3001");

    axum::serve(listener, app).await?;

    Ok(())
}
