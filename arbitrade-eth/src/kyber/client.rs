use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};

const KYBER_API_BASE: &str = "https://aggregator-api.kyberswap.com";
const ETHEREUM_CHAIN_ID: &str = "ethereum";

#[derive(Debug, Serialize, Deserialize)]
pub struct RouteResponse {
    pub code: i32,
    pub message: String,
    pub data: RouteData,
    #[serde(rename = "requestId")]
    pub request_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RouteData {
    #[serde(rename = "routeSummary")]
    pub route_summary: RouteSummary,
    #[serde(rename = "routerAddress")]
    pub router_address: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RouteSummary {
    #[serde(rename = "tokenIn")]
    pub token_in: String,
    #[serde(rename = "amountIn")]
    pub amount_in: String,
    #[serde(rename = "amountInUsd")]
    pub amount_in_usd: String,
    #[serde(rename = "tokenOut")]
    pub token_out: String,
    #[serde(rename = "amountOut")]
    pub amount_out: String,
    #[serde(rename = "amountOutUsd")]
    pub amount_out_usd: String,
    pub gas: String,
    #[serde(rename = "gasPrice")]
    pub gas_price: String,
    #[serde(rename = "gasUsd")]
    pub gas_usd: String,
    #[serde(rename = "l1FeeUsd")]
    pub l1_fee_usd: String,
    #[serde(rename = "extraFee")]
    pub extra_fee: ExtraFee,
    pub route: Vec<Vec<RouteStep>>,
    #[serde(rename = "routeID")]
    pub route_id: String,
    pub checksum: String,
    pub timestamp: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ExtraFee {
    #[serde(rename = "feeAmount")]
    pub fee_amount: String,
    #[serde(rename = "chargeFeeBy")]
    pub charge_fee_by: String,
    #[serde(rename = "isInBps")]
    pub is_in_bps: bool,
    #[serde(rename = "feeReceiver")]
    pub fee_receiver: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RouteStep {
    pub pool: String,
    #[serde(rename = "tokenIn")]
    pub token_in: String,
    #[serde(rename = "tokenOut")]
    pub token_out: String,
    #[serde(rename = "swapAmount")]
    pub swap_amount: String,
    #[serde(rename = "amountOut")]
    pub amount_out: String,
    pub exchange: String,
    #[serde(rename = "poolType")]
    pub pool_type: String,
    #[serde(rename = "poolExtra")]
    pub pool_extra: Option<serde_json::Value>,
    pub extra: Option<serde_json::Value>,
}

pub struct KyberClient {
    client: Client,
    base_url: String,
}

impl KyberClient {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
            base_url: KYBER_API_BASE.to_string(),
        }
    }

    /// Get swap route from KyberSwap Aggregator
    /// Returns the estimated output amount for a given input
    pub async fn get_swap_route(
        &self,
        token_in: &str,
        token_out: &str,
        amount_in: &str,
    ) -> Result<RouteResponse> {
        let url = format!(
            "{}/{}/api/v1/routes?tokenIn={}&tokenOut={}&amountIn={}",
            self.base_url, ETHEREUM_CHAIN_ID, token_in, token_out, amount_in
        );

        // Get client ID from environment variable
        let client_id = std::env::var("KYBER_CLIENT_ID")
            .unwrap_or_else(|_| "my-trade-eth".to_string());

        let response = self
            .client
            .get(&url)
            .header("x-client-id", client_id)
            .header("Accept", "application/json")
            .header("User-Agent", "curl/7.68.0")
            .send()
            .await
            .context("Failed to send request to KyberSwap API")?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "KyberSwap API error: {} - {}",
                status,
                body
            ));
        }

        let route_response: RouteResponse = response.json().await.unwrap();

        Ok(route_response)
    }

    /// Estimate output amount for a swap
    /// Returns the amount of tokenOut you would receive for the given amountIn of tokenIn + gas fees
    pub async fn estimate_swap_output(
        &self,
        token_in: &str,
        token_out: &str,
        amount_in_wei: &str,
    ) -> Result<(String, String)> {
        let route = self.get_swap_route(token_in, token_out, amount_in_wei).await?;
        Ok((route.data.route_summary.amount_out, route.data.route_summary.gas_usd))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[tokio::test]
    async fn test_get_swap_route_logs() {
        // Set up environment variable for client ID
        env::set_var("KYBER_CLIENT_ID", "test-client");

        // Create client
        let client = KyberClient::new();

        // This will make a real HTTP call and print the log, then fail
        // We just want to see the log output
        let result = client.get_swap_route(
            "0xdAC17F958D2ee523a2206206994597C13D831ec7",
            "0xa0ef786bf476fe0810408caba05e536ac800ff86",
            "1000000000"
        ).await;
        assert!(result.is_ok());
    }
}
