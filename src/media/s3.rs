use std::time::Duration;

use async_trait::async_trait;
use reqwest::Url;
use rusty_s3::actions::{GetObject, PutObject, S3Action};
use rusty_s3::{Bucket, Credentials, UrlStyle};

use super::Storage;
use crate::config::StorageConfig;
use crate::error::AppError;

const SIGN_PUT_SECS: u64 = 120;

pub struct S3Storage {
    bucket: Bucket,
    bucket_name: String,
    endpoint: String,
    credentials: Credentials,
    http: reqwest::Client,
}

impl S3Storage {
    pub fn new(config: &StorageConfig) -> Result<Self, AppError> {
        let endpoint: Url = config.endpoint.parse().map_err(|_| AppError::Internal)?;
        let style = if config.force_path_style {
            UrlStyle::Path
        } else {
            UrlStyle::VirtualHost
        };
        let bucket = Bucket::new(
            endpoint,
            style,
            config.bucket.clone(),
            config.region.clone(),
        )
        .map_err(|_| {
            tracing::error!("invalid object-storage endpoint or bucket");
            AppError::Internal
        })?;
        Ok(Self {
            bucket,
            bucket_name: config.bucket.clone(),
            endpoint: config.endpoint.trim_end_matches('/').to_string(),
            credentials: Credentials::new(config.access_key.clone(), config.secret_key.clone()),
            http: reqwest::Client::new(),
        })
    }

    async fn ensure_bucket(&self) -> Result<(), AppError> {
        let url = format!("{}/{}", self.endpoint, self.bucket_name);
        let response = self.http.put(&url).send().await.map_err(|_| {
            tracing::error!("object storage create-bucket failed");
            AppError::Internal
        })?;
        if response.status().is_success() || response.status().as_u16() == 409 {
            return Ok(());
        }
        tracing::warn!(status = %response.status(), "create-bucket returned unexpected status");
        Ok(())
    }
}

#[async_trait]
impl Storage for S3Storage {
    async fn put(&self, key: &str, bytes: &[u8], content_type: &str) -> Result<(), AppError> {
        let action = PutObject::new(&self.bucket, Some(&self.credentials), key);
        let url = action.sign(Duration::from_secs(SIGN_PUT_SECS));
        let response = self
            .http
            .put(url)
            .header("content-type", content_type)
            .header("content-length", bytes.len())
            .body(bytes.to_vec())
            .send()
            .await
            .map_err(|_| {
                tracing::error!("object storage put failed");
                AppError::Internal
            })?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            self.ensure_bucket().await?;
            let retry = PutObject::new(&self.bucket, Some(&self.credentials), key);
            let url = retry.sign(Duration::from_secs(SIGN_PUT_SECS));
            let response = self
                .http
                .put(url)
                .header("content-type", content_type)
                .header("content-length", bytes.len())
                .body(bytes.to_vec())
                .send()
                .await
                .map_err(|_| AppError::Internal)?;
            if !response.status().is_success() {
                tracing::error!(status = %response.status(), "object storage put rejected");
                return Err(AppError::Internal);
            }
            return Ok(());
        }
        if !response.status().is_success() {
            tracing::error!(status = %response.status(), "object storage put rejected");
            return Err(AppError::Internal);
        }
        Ok(())
    }

    async fn get(&self, key: &str) -> Result<Vec<u8>, AppError> {
        let action = GetObject::new(&self.bucket, Some(&self.credentials), key);
        let url = action.sign(Duration::from_secs(SIGN_PUT_SECS));
        let response = self.http.get(url).send().await.map_err(|_| {
            tracing::error!("object storage get failed");
            AppError::Internal
        })?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(AppError::MediaNotFound);
        }
        if !response.status().is_success() {
            tracing::error!(status = %response.status(), "object storage get rejected");
            return Err(AppError::Internal);
        }
        response
            .bytes()
            .await
            .map(|b| b.to_vec())
            .map_err(|_| AppError::Internal)
    }

    async fn delete(&self, key: &str) -> Result<(), AppError> {
        let action =
            rusty_s3::actions::DeleteObject::new(&self.bucket, Some(&self.credentials), key);
        let url = action.sign(Duration::from_secs(SIGN_PUT_SECS));
        let response = self.http.delete(url).send().await.map_err(|_| {
            tracing::error!("object storage delete failed");
            AppError::Internal
        })?;
        if response.status().is_success() || response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(());
        }
        tracing::error!(status = %response.status(), "object storage delete rejected");
        Err(AppError::Internal)
    }

    async fn exists(&self, key: &str) -> Result<bool, AppError> {
        match self.get(key).await {
            Ok(_) => Ok(true),
            Err(AppError::MediaNotFound) => Ok(false),
            Err(error) => Err(error),
        }
    }

    async fn download_url(&self, key: &str, ttl: Duration) -> Result<String, AppError> {
        let action = GetObject::new(&self.bucket, Some(&self.credentials), key);
        Ok(action.sign(ttl).to_string())
    }
}
