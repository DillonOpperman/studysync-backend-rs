//! Object storage and upload validation.
//!
//! The service layer depends on [`Storage`], not on S3/MinIO APIs.
//! Development uses the Compose S3-compatible service when `S3_ENDPOINT` is
//! set; tests without those variables use [`memory::MemoryStorage`].
//!
//! Direct-to-bucket browser uploads can replace `put` later; signed GET URLs
//! already isolate credentials from clients.

mod memory;
mod s3;
mod validate;

pub use memory::MemoryStorage;
pub use s3::S3Storage;
pub use validate::{
    detect_payload, sanitize_file_name, AllowedFile, KIND_FILE, KIND_IMAGE, MAX_ATTACHMENTS,
};

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;

use crate::config::{Config, StorageConfig};
use crate::error::AppError;

#[async_trait]
pub trait Storage: Send + Sync {
    async fn put(&self, key: &str, bytes: &[u8], content_type: &str) -> Result<(), AppError>;
    async fn get(&self, key: &str) -> Result<Vec<u8>, AppError>;
    async fn delete(&self, key: &str) -> Result<(), AppError>;
    async fn exists(&self, key: &str) -> Result<bool, AppError>;
    async fn download_url(&self, key: &str, ttl: Duration) -> Result<String, AppError>;
}

pub fn from_config(config: &Config) -> Result<Arc<dyn Storage>, AppError> {
    match &config.storage {
        Some(storage) => Ok(Arc::new(S3Storage::new(storage)?)),
        None => Ok(Arc::new(MemoryStorage::new(config.jwt_secret.clone()))),
    }
}

pub fn s3_from_config(
    storage: &StorageConfig,
    signing_secret: &str,
) -> Result<S3Storage, AppError> {
    let _ = signing_secret;
    S3Storage::new(storage)
}
