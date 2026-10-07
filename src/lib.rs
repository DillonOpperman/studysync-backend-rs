pub mod api;
pub mod auth;
pub mod chat;
pub mod config;
pub mod db;
pub mod domain;
pub mod error;
pub mod services;
pub mod telemetry;

pub use config::Config;
pub use error::AppError;

use std::sync::Arc;

use sqlx::PgPool;

use crate::chat::ChatHub;

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub db: PgPool,
    pub chat: Arc<ChatHub>,
}

impl AppState {
    pub fn new(config: Config, db: PgPool) -> Self {
        Self {
            config,
            db,
            chat: Arc::new(ChatHub::new()),
        }
    }
}
