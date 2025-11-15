use crate::types::{PriceProvider, TokenPrice};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
    routing::get,
    Router,
};
use serde_json::{json, Value};
use std::sync::Arc;

pub fn create_router<T: PriceProvider + Send + Sync + 'static>(price_provider: Arc<T>) -> Router {
    Router::new()
        .route("/health", get(health_check))
        .route("/prices", get(get_all_prices))
        .route("/price/:symbol", get(get_price))
        .with_state(price_provider)
}

async fn health_check() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

async fn get_all_prices<T: PriceProvider + Send + Sync>(
    State(provider): State<Arc<T>>,
) -> Result<Json<Vec<TokenPrice>>, StatusCode> {
    let prices = provider.get_all_prices().await;
    Ok(Json(prices))
}

async fn get_price<T: PriceProvider + Send + Sync>(
    Path(symbol): Path<String>,
    State(provider): State<Arc<T>>,
) -> Result<Json<TokenPrice>, StatusCode> {
    match provider.get_price(&symbol.to_uppercase()).await {
        Some(price) => Ok(Json(price)),
        None => Err(StatusCode::NOT_FOUND),
    }
}
