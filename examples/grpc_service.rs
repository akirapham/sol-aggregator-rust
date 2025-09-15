use env_logger::Env;
use sol_agg_rust::grpc::create_grpc_service;
use sol_agg_rust::pool_manager::PoolStateManager;
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting Yellowstone gRPC Streamer...");
    test_grpc().await?;
    Ok(())
}

async fn test_grpc() -> Result<(), Box<dyn std::error::Error>> {
    println!("Subscribing to Yellowstone gRPC events...");
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();
    let (grpc_service, batch_rx) = create_grpc_service(100, 500).await.unwrap();
    let mut psm = PoolStateManager::new(grpc_service).await;

    PoolStateManager::start_batch_event_processing(batch_rx, psm.get_pool_update_sender().clone());

    psm.start().await;

    println!("Waiting for Ctrl+C to stop...");
    tokio::signal::ctrl_c().await?;

    Ok(())
}
