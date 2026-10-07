pub mod api;
pub mod config;
pub mod db;
pub mod error;
pub mod telemetry;

pub use config::Config;
pub use error::AppError;

use sqlx::PgPool;

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub db: PgPool,
}

impl AppState {
    pub fn new(config: Config, db: PgPool) -> Self {
        Self { config, db }
    }
}
