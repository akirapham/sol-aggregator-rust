use crate::DexAggregatorError;
use crate::Token;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use spl_token_2022::extension::BaseStateWithExtensions;
use spl_token_2022::extension::StateWithExtensions;
use spl_token_2022::state::Mint;
use std::sync::Arc;

pub async fn fetch_token(
    mint: &Pubkey,
    rpc_client: &Arc<RpcClient>,
) -> Result<Token, DexAggregatorError> {
    // Implement your token fetching logic here
    // fetch token decimals from on chain using MPL Token program

    let mint_data = rpc_client.get_account_data(mint).await.map_err(|e| {
        DexAggregatorError::RpcError(format!(
            "Failed to fetch account data for mint {}: {}",
            mint, e
        ))
    })?;

    let mint_extentions = StateWithExtensions::<Mint>::unpack(&mint_data).unwrap();
    Ok(Token {
        address: mint.clone(),
        decimals: mint_extentions.base.decimals,
    })
}
