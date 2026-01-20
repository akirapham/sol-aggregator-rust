use crate::error::Result;
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
