use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

use crate::config::Config;

pub mod embeddings;
pub mod group_chat;
pub mod groups;
pub mod identity;
pub mod media;
pub mod notifications;
pub mod profile;
pub mod social;

/// Create a PostgreSQL pool. Failures must not include credentials in user-facing output.
pub async fn create_pool(config: &Config) -> Result<PgPool, sqlx::Error> {
    let pool_cfg = &config.database_pool;

    PgPoolOptions::new()
        .max_connections(pool_cfg.max_connections)
        .min_connections(pool_cfg.min_connections)
        .acquire_timeout(pool_cfg.acquire_timeout)
        .idle_timeout(pool_cfg.idle_timeout)
        .max_lifetime(pool_cfg.max_lifetime)
        .connect(&config.database_url)
        .await
}

/// Build a pool that does not connect until a query is issued.
/// Used by tests that must not require a running database for HTTP unit tests.
pub fn lazy_pool(database_url: &str) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(std::time::Duration::from_millis(200))
        .connect_lazy(database_url)
}
