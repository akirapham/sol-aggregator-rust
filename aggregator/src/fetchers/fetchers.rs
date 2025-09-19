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
    if tokens_equal(mint, &&get_sol_mint()) {
        return Ok(Token {
            address: mint.clone(),
            decimals: 9,
            is_token_2022: false,
        });
    }
    // Implement your token fetching logic here
    // fetch token decimals from on chain using MPL Token program

    let mint_data = rpc_client.get_account_data(mint).await;
    match mint_data {
        Err(_) => {
            return Err(DexAggregatorError::RpcError(
                "Failed to fetch mint account data".to_string(),
            ))
        }
        Ok(data) => {
            let len = data.len();
            let is_token_2022 = len > Mint::LEN;
            let mint_extentions = StateWithExtensions::<Mint>::unpack(&data).unwrap();
            Ok(Token {
                address: mint.clone(),
                decimals: mint_extentions.base.decimals,
                is_token_2022,
            })
        }
    }
}
