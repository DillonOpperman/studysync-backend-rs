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
pub mod push;
pub mod services;
pub mod telemetry;

pub use config::Config;
pub use error::AppError;

use std::sync::Arc;

use sqlx::PgPool;

use crate::chat::ChatHub;
use crate::embed::EmbeddingService;
use crate::media::Storage;
use crate::push::PushRuntime;

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub db: PgPool,
    pub chat: Arc<ChatHub>,
    pub storage: Arc<dyn Storage>,
    pub embeddings: EmbeddingService,
    pub push: Arc<PushRuntime>,
}

impl AppState {
    pub fn try_new(config: Config, db: PgPool) -> Result<Self, String> {
        let storage = crate::media::from_config(&config).unwrap_or_else(|_| {
            Arc::new(crate::media::MemoryStorage::new(config.jwt_secret.clone()))
        });
        let embeddings = EmbeddingService::from_config(&config).map_err(|error| {
            format!(
                "failed to load embedding backend (EMBEDDING_BACKEND=minilm|bge|nomic|qwen downloads pinned Hugging Face weights; use unavailable for structured-only): {error}"
            )
        })?;
        let push = Arc::new(build_push_runtime(&config));
        Ok(Self {
            config,
            db,
            chat: Arc::new(ChatHub::new()),
            storage,
            embeddings,
            push,
        })
    }

    pub fn new(config: Config, db: PgPool) -> Self {
        Self::try_new(config, db).unwrap_or_else(|error| panic!("{error}"))
    }

    pub fn with_push_runtime(mut self, push: Arc<PushRuntime>) -> Self {
        self.push = push;
        self
    }
}

fn build_push_runtime(config: &Config) -> PushRuntime {
    if !config.push.delivery_enabled {
        if config.push.requested {
            tracing::warn!("push delivery requested but credentials are missing; FCM is disabled");
        } else {
            tracing::info!("push delivery disabled");
        }
        return PushRuntime::disabled();
    }
    let path = match &config.push.credentials_path {
        Some(path) => path,
        None => return PushRuntime::disabled(),
    };
    let json = match std::fs::read_to_string(path) {
        Ok(value) => value,
        Err(_) => {
            tracing::warn!("GOOGLE_APPLICATION_CREDENTIALS is unreadable; FCM is disabled");
            return PushRuntime::disabled();
        }
    };
    match crate::push::FcmHttpSender::from_service_account_json(
        &json,
        config.push.project_id.as_deref().unwrap_or(""),
        config.push.send_url_override.clone(),
    ) {
        Ok(sender) => PushRuntime {
            delivery_enabled: true,
            sender: Arc::new(sender),
        },
        Err(_) => {
            tracing::warn!("FCM service account could not be loaded; push delivery is disabled");
            PushRuntime::disabled()
        }
    }
}
