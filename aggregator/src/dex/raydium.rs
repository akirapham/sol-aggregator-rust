// use async_trait::async_trait;
// use reqwest::Client;
// use rust_decimal::Decimal;
// use solana_sdk::pubkey::Pubkey;

// use crate::dex::traits::DexInterface;
// use crate::error::{DexAggregatorError, Result};
// use crate::types::{DexType, PoolInfo, PriceInfo, SwapParams, SwapRoute, Token};
// use crate::utils::*;

// /// Raydium DEX implementation
// pub struct RaydiumDex {
//     client: Client,
//     rpc_url: String,
//     program_id: Pubkey,
//     amm_program_id: Pubkey,
// }

// impl RaydiumDex {
//     pub fn new(rpc_url: String) -> Self {
//         Self {
//             client: Client::new(),
//             rpc_url,
//             program_id: parse_pubkey("675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8").unwrap(), // Raydium AMM program
//             amm_program_id: parse_pubkey("675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8").unwrap(),
//         }
//     }

//     /// Get pool address for a token pair
//     async fn get_pool_address(&self, token_a: &Pubkey, token_b: &Pubkey) -> Result<Pubkey> {
//         // Raydium uses a deterministic pool address generation
//         let (pool_address, _) = Pubkey::find_program_address(
//             &[b"amm_associated_seed", token_a.as_ref(), token_b.as_ref()],
//             &self.amm_program_id,
//         );
//         Ok(pool_address)
//     }

//     /// Get pool state from on-chain data
//     async fn get_pool_state(&self, _pool_address: &Pubkey) -> Result<PoolState> {
//         // In a real implementation, you'd fetch this from the on-chain pool account
//         // For now, return placeholder values
//         Ok(PoolState {
//             base_reserve: 1000000000,  // 1B base tokens
//             quote_reserve: 2000000000, // 2B quote tokens
//             base_decimals: 6,
//             quote_decimals: 6,
//             fee_rate: Decimal::new(25, 4), // 0.25% fee
//             lp_supply: 1000000000,
//         })
//     }

//     /// Calculate output amount using Raydium's AMM formula
//     fn calculate_output_amount(
//         &self,
//         input_amount: u64,
//         input_reserve: u64,
//         output_reserve: u64,
//         fee_rate: Decimal,
//     ) -> Result<u64> {
//         if input_reserve == 0 || output_reserve == 0 {
//             return Err(DexAggregatorError::InsufficientLiquidity);
//         }

//         // Apply fee to input amount
//         let fee = calculate_fee(input_amount, fee_rate)?;
//         let input_amount_after_fee = input_amount - fee;

//         // Constant product formula with fee
//         let numerator = output_reserve * input_amount_after_fee;
//         let denominator = input_reserve + input_amount_after_fee;

//         if denominator == 0 {
//             return Err(DexAggregatorError::InsufficientLiquidity);
//         }

//         Ok(numerator / denominator)
//     }

//     /// Get token metadata
//     async fn get_token_metadata(&self, token_address: &Pubkey) -> Result<Token> {
//         // In a real implementation, you'd fetch this from token metadata
//         Ok(Token {
//             address: *token_address,
//             decimals: 6,
//         })
//     }

//     /// Check if pool exists and has liquidity
//     async fn pool_has_liquidity(&self, pool_address: &Pubkey) -> Result<bool> {
//         let pool_state = self.get_pool_state(pool_address).await?;
//         Ok(pool_state.base_reserve > 0 && pool_state.quote_reserve > 0)
//     }
// }


// #[async_trait]
// impl DexInterface for RaydiumDex {
//     fn get_dex_type(&self) -> DexType {
//         DexType::Raydium
//     }

//     async fn get_pools(&self, token_a: &Pubkey, token_b: &Pubkey) -> Result<Vec<PoolInfo>> {
//         if tokens_equal(token_a, token_b) {
//             return Ok(vec![]);
//         }

//         let pool_address = self.get_pool_address(token_a, token_b).await?;

//         // Check if pool has liquidity
//         if !self.pool_has_liquidity(&pool_address).await? {
//             return Ok(vec![]);
//         }

//         let pool_state = self.get_pool_state(&pool_address).await?;
//         let token_a_info = self.get_token_metadata(token_a).await?;
//         let token_b_info = self.get_token_metadata(token_b).await?;

//         let pool = PoolInfo {
//             address: pool_address,
//             // dex: DexType::Raydium,
//             token_a: token_a_info,
//             token_b: token_b_info,
//             reserve_a: pool_state.base_reserve,
//             reserve_b: pool_state.quote_reserve,
//             fee_rate: pool_state.fee_rate,
//         };

//         Ok(vec![pool])
//     }

//     async fn get_best_route(&self, params: &SwapParams) -> Result<Option<SwapRoute>> {
//         let pool_address = self
//             .get_pool_address(&params.input_token, &params.output_token)
//             .await?;

//         // Check if pool has liquidity
//         if !self.pool_has_liquidity(&pool_address).await? {
//             return Ok(None);
//         }

//         let pool_state = self.get_pool_state(&pool_address).await?;
//         let output_amount = self.calculate_output_amount(
//             params.input_amount,
//             pool_state.base_reserve,
//             pool_state.quote_reserve,
//             pool_state.fee_rate,
//         )?;

//         let input_token = self.get_token_metadata(&params.input_token).await?;
//         let output_token = self.get_token_metadata(&params.output_token).await?;

//         // Calculate price impact
//         let market_price =
//             Decimal::from(pool_state.quote_reserve) / Decimal::from(pool_state.base_reserve);
//         let price_impact =
//             calculate_price_impact(params.input_amount, output_amount, market_price)?;

//         let fee = calculate_fee(params.input_amount, pool_state.fee_rate)?;

//         let route = SwapRoute {
//             dex: DexType::Raydium,
//             input_token,
//             output_token,
//             input_amount: params.input_amount,
//             output_amount,
//             price_impact,
//             fee,
//             route_path: vec![pool_address],
//             gas_cost: 10000,
//             execution_time_ms: 1000,
//             mev_risk: crate::types::MevRisk::Low,
//             liquidity_depth: pool_state.base_reserve + pool_state.quote_reserve,
//         };

//         Ok(Some(route))
//     }

//     async fn get_price(
//         &self,
//         input_token: &Pubkey,
//         output_token: &Pubkey,
//         amount: u64,
//     ) -> Result<PriceInfo> {
//         let pool_address = self.get_pool_address(input_token, output_token).await?;

//         // Check if pool has liquidity
//         if !self.pool_has_liquidity(&pool_address).await? {
//             return Err(DexAggregatorError::InsufficientLiquidity);
//         }

//         let pool_state = self.get_pool_state(&pool_address).await?;
//         let output_amount = self.calculate_output_amount(
//             amount,
//             pool_state.base_reserve,
//             pool_state.quote_reserve,
//             pool_state.fee_rate,
//         )?;

//         let price = Decimal::from(output_amount) / Decimal::from(amount);

//         Ok(PriceInfo {
//             dex: DexType::Raydium,
//             input_token: *input_token,
//             output_token: *output_token,
//             price,
//             liquidity: pool_state.base_reserve + pool_state.quote_reserve,
//             last_updated: std::time::SystemTime::now()
//                 .duration_since(std::time::UNIX_EPOCH)
//                 .unwrap()
//                 .as_secs(),
//         })
//     }

//     async fn get_token_info(&self, token_address: &Pubkey) -> Result<Token> {
//         self.get_token_metadata(token_address).await
//     }

//     async fn supports_token_pair(&self, token_a: &Pubkey, token_b: &Pubkey) -> Result<bool> {
//         if tokens_equal(token_a, token_b) {
//             return Ok(false);
//         }

//         let pool_address = self.get_pool_address(token_a, token_b).await?;
//         self.pool_has_liquidity(&pool_address).await
//     }

//     async fn get_supported_tokens(&self) -> Result<Vec<Token>> {
//         // In a real implementation, you'd fetch all tokens from Raydium
//         // For now, return empty vector
//         Ok(vec![])
//     }

//     async fn estimate_swap_fee(&self, params: &SwapParams) -> Result<u64> {
//         let pool_address = self
//             .get_pool_address(&params.input_token, &params.output_token)
//             .await?;
//         let pool_state = self.get_pool_state(&pool_address).await?;
//         Ok(calculate_fee(params.input_amount, pool_state.fee_rate))
//     }

//     async fn get_pool_info(&self, pool_address: &Pubkey) -> Result<PoolInfo> {
//         let pool_state = self.get_pool_state(pool_address).await?;

//         // In a real implementation, you'd need to determine which token is base/quote
//         // For now, we'll use placeholder tokens
//         let token_a = Token {
//             address: Pubkey::new_unique(),
//             decimals: pool_state.base_decimals,
//         };

//         let token_b = Token {
//             address: Pubkey::new_unique(),
//             decimals: pool_state.quote_decimals,
//         };

//         Ok(PoolInfo {
//             address: *pool_address,
//             dex: DexType::Raydium,
//             token_a,
//             token_b,
//             reserve_a: pool_state.base_reserve,
//             reserve_b: pool_state.quote_reserve,
//             fee_rate: pool_state.fee_rate,
//         })
//     }

//     async fn pool_exists(&self, token_a: &Pubkey, token_b: &Pubkey) -> Result<bool> {
//         self.supports_token_pair(token_a, token_b).await
//     }
// }
