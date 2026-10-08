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
    pub fn try_new(config: Config, db: PgPool) -> Result<Self, String> {
        let storage = crate::media::from_config(&config).unwrap_or_else(|_| {
            Arc::new(crate::media::MemoryStorage::new(config.jwt_secret.clone()))
        });
        let embeddings = EmbeddingService::from_config(&config).map_err(|error| {
            format!(
                "failed to load embedding backend (EMBEDDING_BACKEND=qwen downloads Qwen/Qwen3-Embedding-0.6B@{}; use unavailable for structured-only): {error}",
                crate::embed::catalog::QWEN3_EMBEDDING_06B_REVISION
            )
        })?;
        Ok(Self {
            config,
            db,
            chat: Arc::new(ChatHub::new()),
            storage,
            embeddings,
        })
    }

    pub fn new(config: Config, db: PgPool) -> Self {
        Self::try_new(config, db).unwrap_or_else(|error| panic!("{error}"))
    }
}
