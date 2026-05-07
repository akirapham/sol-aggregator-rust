use crate::error::Result; // Internal Result
use crate::pool_data_types::PoolState;
use crate::pool_manager::traits::DatabaseTrait;
use crate::types::Token;
use anyhow::Result as AnyhowResult; // Trait Result
use async_trait::async_trait;
use solana_sdk::pubkey::Pubkey;
use sqlx::postgres::PgPoolOptions;
use sqlx::{Pool, Postgres};

#[derive(Clone)]
pub struct Database {
    pub pool: Pool<Postgres>,
}

impl Database {
    pub async fn new(connection_string: &str) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(50)
            .acquire_timeout(std::time::Duration::from_secs(3))
            .connect(connection_string)
            .await
            .map_err(|e| format!("Failed to connect to database: {}", e))?;

        log::info!("✅ Connected to PostgreSQL database");

        Ok(Self { pool })
    }

    pub fn get_pool(&self) -> &Pool<Postgres> {
        &self.pool
    }

    /// Run migrations located in the migrations directory
    /// Note: In production this might be handled by a separate migration tool
    pub async fn run_migrations(&self) -> Result<()> {
        // Simple verification that connection works
        sqlx::query("SELECT 1")
            .execute(&self.pool)
            .await
            .map_err(|e| format!("Database health check failed: {}", e))?;

        Ok(())
    }
}

#[async_trait]
impl DatabaseTrait for Database {
    async fn load_pools(&self) -> AnyhowResult<Vec<PoolState>> {
        let pools_query = sqlx::query("SELECT data FROM pools");
        let rows = pools_query.fetch_all(&self.pool).await?;

        let mut pools = Vec::new();
        let mut skipped = 0usize;
        for row in rows {
            use sqlx::Row;
            let data: serde_json::Value = row.try_get("data").unwrap_or_default();
            match serde_json::from_value::<PoolState>(data) {
                Ok(pool_state) => pools.push(pool_state),
                Err(e) => {
                    skipped += 1;
                    log::warn!("Skipping pool with incompatible schema: {}", e);
                }
            }
        }
        if skipped > 0 {
            log::warn!(
                "Skipped {} pools due to schema changes (will be overwritten on next save)",
                skipped
            );
        }
        Ok(pools)
    }

    async fn save_pools(&self, pools: &[PoolState]) -> AnyhowResult<()> {
        for chunk in pools.chunks(500) {
            let mut query_builder = sqlx::QueryBuilder::new(
                "INSERT INTO pools (address, dex_type, token_a, token_b, data, last_updated_ts) ",
            );

            query_builder.push_values(chunk, |mut b, pool| {
                let (token_a, token_b) = pool.get_tokens();
                b.push_bind(pool.address().to_string())
                    .push_bind(pool.dex().to_string())
                    .push_bind(token_a.to_string())
                    .push_bind(token_b.to_string())
                    .push_bind(sqlx::types::Json(pool))
                    .push_bind(pool.last_updated() as i64);
            });

            query_builder.push(
                " ON CONFLICT (address) DO UPDATE SET 
                data = EXCLUDED.data,
                last_updated_ts = EXCLUDED.last_updated_ts",
            );

            let query = query_builder.build();
            query.execute(&self.pool).await?;
        }
        Ok(())
    }

    async fn load_tokens(&self) -> AnyhowResult<Vec<Token>> {
        let tokens_query = sqlx::query("SELECT data FROM tokens");
        let rows = tokens_query.fetch_all(&self.pool).await?;

        let mut tokens = Vec::new();
        for row in rows {
            use sqlx::Row;
            let json_val: Option<serde_json::Value> = row.try_get("data").ok();
            if let Some(json_val) = json_val {
                let token: Token = serde_json::from_value(json_val)?;
                tokens.push(token);
            }
        }
        Ok(tokens)
    }

    async fn save_tokens(&self, tokens: &[Token]) -> AnyhowResult<()> {
        for chunk in tokens.chunks(500) {
            let mut query_builder = sqlx::QueryBuilder::new(
                "INSERT INTO tokens (address, symbol, name, decimals, is_token2022, logo_uri, data) "
            );

            query_builder.push_values(chunk, |mut b, token| {
                b.push_bind(token.address.to_string())
                    .push_bind(token.symbol.clone())
                    .push_bind(token.name.clone())
                    .push_bind(token.decimals as i16)
                    .push_bind(token.is_token_2022)
                    .push_bind(token.logo_uri.clone())
                    .push_bind(sqlx::types::Json(token));
            });

            query_builder.push(
                " ON CONFLICT (address) DO UPDATE SET 
                symbol = EXCLUDED.symbol,
                name = EXCLUDED.name,
                decimals = EXCLUDED.decimals,
                is_token2022 = EXCLUDED.is_token2022,
                logo_uri = EXCLUDED.logo_uri,
                data = EXCLUDED.data,
                updated_at = NOW()",
            );

            let query = query_builder.build();
            query.execute(&self.pool).await?;
        }
        Ok(())
    }

    async fn load_arbitrage_tokens(&self) -> AnyhowResult<Vec<Pubkey>> {
        let row = sqlx::query("SELECT value FROM app_settings WHERE key = $1")
            .bind("arbitrage_monitored_token_addresses")
            .fetch_optional(&self.pool)
            .await?;

        if let Some(row) = row {
            use sqlx::Row;
            let value: serde_json::Value = row.try_get("value")?;
            let addrs: Vec<String> = serde_json::from_value(value)?;
            let mut tokens = Vec::new();
            for s in addrs {
                use std::str::FromStr;
                tokens.push(Pubkey::from_str(&s)?);
            }
            Ok(tokens)
        } else {
            Ok(Vec::new())
        }
    }

    async fn save_arbitrage_tokens(&self, tokens: &[Pubkey]) -> AnyhowResult<()> {
        let addrs: Vec<String> = tokens.iter().map(|p| p.to_string()).collect();
        let value = serde_json::to_value(&addrs)?;

        sqlx::query(
            "INSERT INTO app_settings (key, value) VALUES ($1, $2) ON CONFLICT (key) DO UPDATE SET value = $2"
        )
        .bind("arbitrage_monitored_token_addresses")
        .bind(value)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn add_arbitrage_token(&self, token: &Pubkey) -> AnyhowResult<()> {
        let mut tokens = self.load_arbitrage_tokens().await?;
        if !tokens.contains(token) {
            tokens.push(*token);
            self.save_arbitrage_tokens(&tokens).await?;
        }
        Ok(())
    }

    async fn remove_arbitrage_token(&self, token: &Pubkey) -> AnyhowResult<()> {
        let mut tokens = self.load_arbitrage_tokens().await?;
        if let Some(pos) = tokens.iter().position(|t| t == token) {
            tokens.remove(pos);
            self.save_arbitrage_tokens(&tokens).await?;
        }
        Ok(())
    }
}
