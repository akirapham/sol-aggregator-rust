use crate::constants::get_base_token_symbol;
use crate::constants::is_base_token;
use crate::error::DexAggregatorError;
use crate::types::Token;
use crate::utils::get_sol_mint;
use crate::utils::tokens_equal;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use spl_token::solana_program::program_pack::Pack;
use spl_token_2022::extension::StateWithExtensions;
use spl_token_2022::state::Mint;
use std::sync::Arc;
pub async fn fetch_token(
    mint: &Pubkey,
    rpc_client: &Arc<RpcClient>,
) -> Result<Token, DexAggregatorError> {
    if tokens_equal(mint, &get_sol_mint()) {
        return Ok(Token {
            address: *mint,
            decimals: 9,
            is_token_2022: false,
            symbol: Some("SOL".to_string()),
            name: Some("Solana".to_string()),
            logo_uri: None,
        });
    }

    if is_base_token(&mint.to_string()) {
        return Ok(Token {
            address: *mint,
            decimals: 6,
            is_token_2022: false,
            symbol: Some(get_base_token_symbol(*mint)),
            name: None,
            logo_uri: None,
        });
    }

    const MAX_RETRIES: u32 = 5;
    const INITIAL_BACKOFF: u64 = 200;

    for attempt in 0..MAX_RETRIES {
        let mint_data = rpc_client.get_account_data(mint).await;
        match mint_data {
            Ok(data) => {
                let len = data.len();
                let is_token_2022 = len > Mint::LEN;
                return match StateWithExtensions::<Mint>::unpack(&data) {
                    Ok(mint_extentions) => Ok(Token {
                        address: *mint,
                        decimals: mint_extentions.base.decimals,
                        is_token_2022,
                        symbol: None,
                        name: None,
                        logo_uri: None,
                    }),
                    Err(_) => Err(DexAggregatorError::RpcError(
                        "Failed to unpack mint account data".to_string(),
                    )),
                };
            }
            Err(e) => {
                if attempt == MAX_RETRIES - 1 {
                    return Err(DexAggregatorError::RpcError(format!(
                        "Failed to fetch mint account data after {} retries: {}",
                        MAX_RETRIES, e
                    )));
                }
                log::warn!(
                    "Failed to fetch token {}, retrying... ({}/{}): {}",
                    mint,
                    attempt + 1,
                    MAX_RETRIES,
                    e
                );
                tokio::time::sleep(std::time::Duration::from_millis(
                    INITIAL_BACKOFF * 2u64.pow(attempt),
                ))
                .await;
            }
        }
    }

    // Should be unreachable if the loop logic is correct, but effectively covered by the Err return in loop
    Err(DexAggregatorError::RpcError(
        "Failed to fetch mint account data".to_string(),
    ))
}

pub async fn fetch_account_data(
    rpc_client: &Arc<RpcClient>,
    account: &Pubkey,
) -> Result<Vec<u8>, DexAggregatorError> {
    const MAX_RETRIES: u32 = 3;
    const INITIAL_BACKOFF: u64 = 200;

    for attempt in 0..MAX_RETRIES {
        let account_data = rpc_client.get_account_data(account).await;
        match account_data {
            Ok(data) => return Ok(data),
            Err(e) => {
                if attempt == MAX_RETRIES - 1 {
                    return Err(DexAggregatorError::RpcError(format!(
                        "Failed to fetch account data after {} retries: {}",
                        MAX_RETRIES, e
                    )));
                }
                log::warn!(
                    "Failed to fetch account {}, retrying... ({}/{}): {}",
                    account,
                    attempt + 1,
                    MAX_RETRIES,
                    e
                );
                tokio::time::sleep(std::time::Duration::from_millis(
                    INITIAL_BACKOFF * 2u64.pow(attempt),
                ))
                .await;
            }
        }
    }

    // Should be unreachable if the loop logic is correct
    Err(DexAggregatorError::RpcError(
        "Failed to fetch account data".to_string(),
    ))
}

pub async fn fetch_multiple_accounts(
    rpc_client: &Arc<RpcClient>,
    accounts: &[Pubkey],
) -> Result<Vec<Option<Vec<u8>>>, DexAggregatorError> {
    const CHUNK_SIZE: usize = 50; // Solana limit is often 100, we stick to safe 50
    const MAX_RETRIES: u32 = 3;
    const INITIAL_BACKOFF: u64 = 200;

    let mut all_results = Vec::with_capacity(accounts.len());

    for chunk in accounts.chunks(CHUNK_SIZE) {
        let mut fetched = false;
        for attempt in 0..MAX_RETRIES {
            match rpc_client.get_multiple_accounts(chunk).await {
                Ok(accounts_data) => {
                    // map Account to Option<Vec<u8>>
                    let data_only: Vec<Option<Vec<u8>>> = accounts_data
                        .into_iter()
                        .map(|acc| acc.map(|a| a.data))
                        .collect();
                    all_results.extend(data_only);
                    fetched = true;
                    break;
                }
                Err(e) => {
                    if attempt == MAX_RETRIES - 1 {
                        return Err(DexAggregatorError::RpcError(format!(
                            "Failed to fetch multiple accounts after {} retries: {}",
                            MAX_RETRIES, e
                        )));
                    }
                    log::warn!(
                        "Failed to fetch batch of {} accounts, retrying... ({}/{}): {}",
                        chunk.len(),
                        attempt + 1,
                        MAX_RETRIES,
                        e
                    );
                    tokio::time::sleep(std::time::Duration::from_millis(
                        INITIAL_BACKOFF * 2u64.pow(attempt),
                    ))
                    .await;
                }
            }
        }
        if !fetched {
            return Err(DexAggregatorError::RpcError(
                "Failed to fetch batch of accounts".to_string(),
            ));
        }
    }

    Ok(all_results)
}
