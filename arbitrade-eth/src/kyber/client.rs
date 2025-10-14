use anyhow::{Context, Result};
use ethers::prelude::*;
use ethers::providers::{Http, Provider};
use ethers::types::transaction::eip2718::TypedTransaction;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

const KYBER_API_BASE: &str = "https://aggregator-api.kyberswap.com";
const ETHEREUM_CHAIN_ID: &str = "ethereum";
const MAX_RETRIES: u32 = 2;
const RETRY_DELAY_MS: u64 = 1000;

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
    eth_provider: Option<Arc<Provider<Http>>>,
    private_key: Option<String>,
}

impl KyberClient {
    pub fn new() -> Self {
        // Try to load ETH provider and private key from environment
        let eth_provider = std::env::var("ETH_RPC_URL")
            .ok()
            .and_then(|url| Provider::<Http>::try_from(url).ok())
            .map(Arc::new);

        let private_key = std::env::var("ETH_PRIVATE_KEY").ok();

        // Build client with browser-like headers to avoid bot detection
        let client = Client::builder()
            .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("Failed to build HTTP client");

        Self {
            client,
            base_url: KYBER_API_BASE.to_string(),
            eth_provider,
            private_key,
        }
    }

    /// Helper function to check if error is Cloudflare-related
    fn is_cloudflare_block(body: &str) -> bool {
        body.contains("Just a moment")
            || body.contains("challenge-platform")
            || body.contains("cf_chl_opt")
            || body.contains("Cloudflare")
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
        let client_id =
            std::env::var("KYBER_CLIENT_ID").unwrap_or_else(|_| "my-trade-eth".to_string());

        let response = self
            .client
            .get(&url)
            .header("x-client-id", client_id)
            .header("Accept", "application/json")
            .send()
            .await
            .context("Failed to send request to KyberSwap API")?;

        let status = response.status();

        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();

            // If we get a Cloudflare challenge, log it clearly
            if Self::is_cloudflare_block(&body) {
                log::warn!("KyberSwap returned Cloudflare challenge - bot protection triggered");
                return Err(anyhow::anyhow!(
                    "KyberSwap API blocked by Cloudflare (status: {})",
                    status
                ));
            }

            return Err(anyhow::anyhow!(
                "KyberSwap API error: {} - {}",
                status,
                body
            ));
        }

        // Get response text first for debugging
        let response_text = response
            .text()
            .await
            .context("Failed to read KyberSwap response body")?;

        // Try to parse JSON response
        let route_response: RouteResponse = serde_json::from_str(&response_text)
            .map_err(|e| {
                log::error!("Failed to parse KyberSwap response. Error: {}. Response body (first 500 chars): {}",
                    e,
                    &response_text.chars().take(500).collect::<String>()
                );
                anyhow::anyhow!("Failed to parse KyberSwap route response JSON: {}", e)
            })?;

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
        let route = self
            .get_swap_route(token_in, token_out, amount_in_wei)
            .await?;
        Ok((
            route.data.route_summary.amount_out,
            route.data.route_summary.gas_usd,
        ))
    }

    /// Build transaction data for executing a swap
    /// Returns the transaction data that can be used to simulate or execute the swap
    pub async fn build_swap_transaction(
        &self,
        route_summary: &RouteSummary,
        sender: &str,
        recipient: &str,
        slippage_tolerance: u32, // in basis points (e.g., 50 = 0.5%)
    ) -> Result<serde_json::Value> {
        let url = format!("{}/{}/api/v1/route/build", self.base_url, ETHEREUM_CHAIN_ID);

        let body = serde_json::json!({
            "routeSummary": route_summary,
            "sender": sender,
            "recipient": recipient,
            "slippageTolerance": slippage_tolerance,
            "skipSimulateTx": false,
        });

        // Get client ID from environment variable
        let client_id =
            std::env::var("KYBER_CLIENT_ID").unwrap_or_else(|_| "my-trade-eth".to_string());

        // Retry logic for Cloudflare blocks
        let mut last_error = None;
        for attempt in 0..=MAX_RETRIES {
            if attempt > 0 {
                let delay = RETRY_DELAY_MS * (1 << (attempt - 1)); // Exponential backoff: 1s, 2s, 4s
                log::debug!(
                    "Retrying KyberSwap build request (attempt {}/{}) after {}ms delay",
                    attempt + 1,
                    MAX_RETRIES + 1,
                    delay
                );
                sleep(Duration::from_millis(delay)).await;
            }

            let response = self
                .client
                .post(&url)
                .header("x-client-id", &client_id)
                .header("Accept", "application/json")
                .header("Content-Type", "application/json")
                .json(&body)
                .send()
                .await
                .context("Failed to send build request to KyberSwap API")?;

            if response.status().is_success() {
                let build_response: serde_json::Value = response.json().await?;
                return Ok(build_response);
            }

            let status = response.status();
            let response_body = response.text().await.unwrap_or_default();

            // Check if Cloudflare block - retry if we have attempts left
            if Self::is_cloudflare_block(&response_body) {
                log::warn!(
                    "KyberSwap build endpoint blocked by Cloudflare (attempt {}/{})",
                    attempt + 1,
                    MAX_RETRIES + 1
                );
                last_error = Some(anyhow::anyhow!(
                    "KyberSwap API blocked by Cloudflare (status: {})",
                    status
                ));

                if attempt < MAX_RETRIES {
                    continue; // Retry
                }
            } else {
                // Non-Cloudflare error, don't retry
                return Err(anyhow::anyhow!(
                    "KyberSwap build API error: {} - {}",
                    status,
                    response_body
                ));
            }
        }

        // All retries exhausted
        Err(last_error
            .unwrap_or_else(|| anyhow::anyhow!("Max retries exceeded for KyberSwap build request")))
    }

    /// Simulate a swap transaction on-chain using eth_estimateGas
    /// This verifies the transaction would succeed before actually executing it
    /// Returns Ok(gas_estimate) if simulation succeeds, Err if it would fail
    pub async fn simulate_swap_transaction(&self, route_summary: &RouteSummary) -> Result<u64> {
        // Check if we have the required components
        let provider = self
            .eth_provider
            .as_ref()
            .context("ETH_RPC_URL not configured - cannot simulate transaction")?;

        let private_key = self
            .private_key
            .as_ref()
            .context("ETH_PRIVATE_KEY not configured - cannot simulate transaction")?;

        // Parse private key and get wallet address
        let wallet: LocalWallet = private_key
            .parse()
            .context("Failed to parse ETH_PRIVATE_KEY")?;
        let sender = format!("{:?}", wallet.address());

        log::info!("Simulating transaction from sender: {}", sender);

        // Build the transaction data
        let build_response = self
            .build_swap_transaction(
                route_summary,
                &sender,
                &sender, // recipient same as sender
                50,      // 0.5% slippage tolerance
            )
            .await?;

        // Extract transaction data from build response
        let tx_data = build_response
            .get("data")
            .context("Failed to extract transaction data from build response")?;

        let to_address = tx_data
            .get("routerAddress")
            .and_then(|v| v.as_str())
            .context("Failed to extract 'routerAddress' address")?;

        let data = tx_data
            .get("data")
            .and_then(|v| v.as_str())
            .context("Failed to extract transaction data")?;

        let value = tx_data
            .get("value")
            .and_then(|v| v.as_str())
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0);

        // Parse addresses
        let from: Address = sender.parse().context("Failed to parse sender address")?;
        let to: Address = to_address.parse().context("Failed to parse 'to' address")?;

        // Build transaction for gas estimation
        let tx = TransactionRequest::new()
            .from(from)
            .to(to)
            .data(Bytes::from(
                hex::decode(&data[2..]).context("Failed to decode hex data")?,
            ))
            .value(U256::from(value));

        // Convert TransactionRequest to TypedTransaction
        let typed_tx: TypedTransaction = tx.into();

        // Estimate gas - this will fail if the transaction would revert
        log::debug!(
            "Calling eth_estimateGas for token {} -> {}",
            route_summary.token_in,
            route_summary.token_out
        );
        log::debug!(
            "  Amount in: {}, Expected out: {}",
            route_summary.amount_in,
            route_summary.amount_out
        );
        log::debug!("  Router: {}, Value: {}", to_address, value);

        let gas_estimate = match provider.estimate_gas(&typed_tx, None).await {
            Ok(gas) => gas,
            Err(e) => {
                // Try to extract revert reason from error
                let error_msg = format!("{:?}", e);
                log::error!("eth_estimateGas failed: {}", error_msg);

                // Check for common revert reasons
                if error_msg.contains("insufficient funds") {
                    return Err(anyhow::anyhow!("Insufficient ETH balance for gas"));
                } else if error_msg.contains("execution reverted") {
                    // Try to extract the revert reason if available
                    return Err(anyhow::anyhow!("Transaction would revert: {}", error_msg));
                } else {
                    return Err(anyhow::anyhow!("Gas estimation failed: {}", e));
                }
            }
        };

        log::info!(
            "✅ Transaction simulation successful! Estimated gas: {}",
            gas_estimate
        );
        Ok(gas_estimate.as_u64())
    }

    /// Execute a real swap transaction on-chain
    /// Returns the transaction hash if successful
    pub async fn execute_swap(
        &self,
        route_summary: &RouteSummary,
        recipient: &str,
        slippage_tolerance: u32, // in basis points (e.g., 50 = 0.5%)
    ) -> Result<String> {
        // Check if we have the required components
        let provider = self
            .eth_provider
            .as_ref()
            .context("ETH_RPC_URL not configured - cannot execute transaction")?;

        let private_key = self
            .private_key
            .as_ref()
            .context("ETH_PRIVATE_KEY not configured - cannot execute transaction")?;

        // Parse private key and create wallet
        let wallet: LocalWallet = private_key
            .parse()
            .context("Failed to parse ETH_PRIVATE_KEY")?;
        let sender = format!("{:?}", wallet.address());

        log::info!(
            "Executing swap from sender: {} to recipient: {}",
            sender,
            recipient
        );
        log::info!(
            "  Swap: {} {} -> {} {}",
            route_summary.amount_in,
            route_summary.token_in,
            route_summary.amount_out,
            route_summary.token_out
        );

        // Build the transaction data
        let build_response = self
            .build_swap_transaction(route_summary, &sender, recipient, slippage_tolerance)
            .await?;

        // CRITICAL: Simulate transaction BEFORE executing to prevent failed transactions that cost gas
        log::info!("🔍 Simulating transaction to verify it won't revert...");
        let tx_data = build_response
            .get("data")
            .context("Failed to extract transaction data from build response")?;

        let to_address = tx_data
            .get("routerAddress")
            .and_then(|v| v.as_str())
            .context("Failed to extract 'routerAddress' address")?;

        let data_hex = tx_data
            .get("data")
            .and_then(|v| v.as_str())
            .context("Failed to extract transaction data")?;

        let value = tx_data
            .get("value")
            .and_then(|v| v.as_str())
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0);

        // Parse addresses for simulation
        let from: Address = sender.parse().context("Failed to parse sender address")?;
        let to: Address = to_address.parse().context("Failed to parse 'to' address")?;

        // Build transaction for simulation
        let sim_tx = TransactionRequest::new()
            .from(from)
            .to(to)
            .data(Bytes::from(
                hex::decode(&data_hex[2..]).context("Failed to decode hex data")?,
            ))
            .value(U256::from(value));

        // Convert to TypedTransaction for estimation
        let typed_tx: TypedTransaction = sim_tx.into();

        // Estimate gas - this will FAIL if the transaction would revert
        log::debug!("Calling eth_estimateGas to simulate execution...");
        let gas_estimate = match provider.estimate_gas(&typed_tx, None).await {
            Ok(gas) => {
                log::info!(
                    "✅ Transaction simulation PASSED! Estimated gas: {} ({:.4} ETH at 50 gwei)",
                    gas,
                    gas.as_u64() as f64 * 50e9 / 1e18
                );
                gas
            }
            Err(e) => {
                // Extract and log detailed error information
                let error_msg = format!("{:?}", e);
                log::error!("❌ Transaction simulation FAILED - transaction would revert!");
                log::error!("   Error: {}", error_msg);

                // Check for common failure reasons
                if error_msg.contains("insufficient funds") {
                    log::error!("   Reason: Insufficient ETH balance for gas or token transfer");
                    return Err(anyhow::anyhow!(
                        "Transaction would fail: Insufficient funds for gas or transfer"
                    ));
                } else if error_msg.contains("transfer amount exceeds balance") {
                    log::error!("   Reason: Insufficient token balance");
                    return Err(anyhow::anyhow!(
                        "Transaction would fail: Insufficient token balance"
                    ));
                } else if error_msg.contains("execution reverted") {
                    log::error!("   Reason: Smart contract execution would revert");
                    if error_msg.contains("STF") {
                        log::error!(
                            "   Possible issue: Slippage tolerance too tight or price moved"
                        );
                    }
                    return Err(anyhow::anyhow!(
                        "Transaction would fail: Execution reverted - {}",
                        error_msg
                    ));
                } else if error_msg.contains("allowance") {
                    log::error!("   Reason: Token allowance not set or insufficient");
                    return Err(anyhow::anyhow!(
                        "Transaction would fail: Insufficient token allowance"
                    ));
                } else {
                    log::error!("   Reason: Unknown - {}", error_msg);
                    return Err(anyhow::anyhow!("Transaction would fail: {}", e));
                }
            }
        };

        log::info!("✅ Pre-flight simulation complete. Proceeding with real transaction...");

        // Parse gas parameters from route summary (data already extracted during simulation)
        let gas_limit = route_summary
            .gas
            .parse::<u64>()
            .context("Failed to parse gas limit")?;
        let base_gas_price = route_summary
            .gas_price
            .parse::<u64>()
            .context("Failed to parse gas price")?;

        // Use simulated gas estimate if it's higher (add 10% buffer for safety)
        let final_gas_limit = std::cmp::max(gas_limit, (gas_estimate.as_u64() as f64 * 1.1) as u64);

        // Increase gas price by 20% for faster execution
        let gas_price = (base_gas_price as f64 * 1.2) as u64;

        log::info!("Gas parameters for execution:");
        log::info!("  KyberSwap Gas Limit: {}", gas_limit);
        log::info!(
            "  Simulated Gas Estimate: {} (+10% buffer = {})",
            gas_estimate,
            (gas_estimate.as_u64() as f64 * 1.1) as u64
        );
        log::info!("  Final Gas Limit: {} (max of both)", final_gas_limit);
        log::info!(
            "  Base Gas Price: {} wei ({:.2} gwei)",
            base_gas_price,
            base_gas_price as f64 / 1e9
        );
        log::info!(
            "  Boosted Gas Price (+20%): {} wei ({:.2} gwei)",
            gas_price,
            gas_price as f64 / 1e9
        );
        log::info!("  Estimated Gas Cost: ${} (base)", route_summary.gas_usd);

        // Create a signer from the wallet and provider
        let chain_id = provider.get_chainid().await?.as_u64();
        let signer = wallet.with_chain_id(chain_id);
        let client = SignerMiddleware::new(provider.clone(), signer);

        // Build transaction for execution with gas parameters from KyberSwap
        let tx = TransactionRequest::new()
            .from(from)
            .to(to)
            .data(Bytes::from(
                hex::decode(&data_hex[2..]).context("Failed to decode hex data")?,
            ))
            .value(U256::from(value))
            .gas(U256::from(final_gas_limit))
            .gas_price(U256::from(gas_price));

        log::debug!(
            "Sending transaction to router: {}, value: {}, gas: {}, gasPrice: {} gwei",
            to_address,
            value,
            gas_limit,
            gas_price as f64 / 1e9
        );

        // Send the transaction
        let pending_tx = client
            .send_transaction(tx, None)
            .await
            .context("Failed to send transaction")?;

        let tx_hash = format!("{:?}", pending_tx.tx_hash());
        log::info!("✅ Transaction sent! Hash: {}", tx_hash);
        log::info!("   View on Etherscan: https://etherscan.io/tx/{}", tx_hash);

        // Wait for confirmation (optional - can be removed if you want fire-and-forget)
        log::info!("Waiting for transaction confirmation...");
        let receipt = pending_tx
            .await?
            .context("Transaction failed - no receipt")?;

        if receipt.status == Some(U64::from(1)) {
            log::info!("✅ Transaction confirmed successfully!");
            log::info!("   Gas used: {}", receipt.gas_used.unwrap_or_default());
            log::info!(
                "   Block number: {}",
                receipt.block_number.unwrap_or_default()
            );
        } else {
            log::error!("❌ Transaction failed on-chain");
            return Err(anyhow::anyhow!("Transaction reverted on-chain"));
        }

        Ok(tx_hash)
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
        let result = client
            .get_swap_route(
                "0xdAC17F958D2ee523a2206206994597C13D831ec7",
                "0xa0ef786bf476fe0810408caba05e536ac800ff86",
                "1000000000",
            )
            .await;
        assert!(result.is_ok());
    }
}
