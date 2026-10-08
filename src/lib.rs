pub mod api;
pub mod auth;
pub mod chat;
pub mod config;
pub mod db;
pub mod domain;
pub mod embed;
pub mod error;
pub mod matching;
pub mod media;
pub mod services;
pub mod telemetry;

pub use config::Config;
pub use error::AppError;

use std::sync::Arc;

use sqlx::PgPool;

use crate::chat::ChatHub;
use crate::embed::EmbeddingService;
use crate::media::Storage;

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub db: PgPool,
    pub chat: Arc<ChatHub>,
    pub storage: Arc<dyn Storage>,
    pub embeddings: EmbeddingService,
}

impl AppState {
    pub fn new(config: Config, db: PgPool) -> Self {
        let storage = crate::media::from_config(&config).unwrap_or_else(|_| {
            Arc::new(crate::media::MemoryStorage::new(config.jwt_secret.clone()))
        });
        let embeddings = EmbeddingService::from_backend(config.embedding_backend);
        Self {
            config,
            db,
            chat: Arc::new(ChatHub::new()),
            storage,
            embeddings,
        }
    }
}
