use anyhow::Result;
use axum::Router;
use log::{error, info};
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tracing_subscriber;

mod api;
mod mexc;
mod types;
use mexc::MexcService;

#[tokio::main]
async fn main() -> Result<()> {
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

    // Start HTTP API server
    let app = Router::new()
        .merge(api::create_router(mexc_service))
        .layer(CorsLayer::permissive());

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3001").await?;
    info!("CEX Pricing API server listening on http://0.0.0.0:3001");

    axum::serve(listener, app).await?;

    Ok(())
}
