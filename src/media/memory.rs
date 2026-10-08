use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use hmac::{Hmac, Mac};
use sha2::Sha256;

use super::Storage;
use crate::error::AppError;

type HmacSha256 = Hmac<Sha256>;

pub struct MemoryStorage {
    objects: Mutex<HashMap<String, (String, Vec<u8>)>>,
    signing_secret: String,
}

impl MemoryStorage {
    pub fn new(signing_secret: impl Into<String>) -> Self {
        Self {
            objects: Mutex::new(HashMap::new()),
            signing_secret: signing_secret.into(),
        }
    }

    pub fn sign(secret: &str, key: &str, exp: i64) -> String {
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).expect("hmac");
        mac.update(key.as_bytes());
        mac.update(b"|");
        mac.update(exp.to_string().as_bytes());
        hex::encode(mac.finalize().into_bytes())
    }

    pub fn verify(secret: &str, key: &str, exp: i64, sig: &str) -> bool {
        let expected = Self::sign(secret, key, exp);
        expected == sig
    }
}

#[async_trait]
impl Storage for MemoryStorage {
    async fn put(&self, key: &str, bytes: &[u8], content_type: &str) -> Result<(), AppError> {
        self.objects
            .lock()
            .map_err(|_| AppError::Internal)?
            .insert(key.to_string(), (content_type.to_string(), bytes.to_vec()));
        Ok(())
    }

    async fn get(&self, key: &str) -> Result<Vec<u8>, AppError> {
        self.objects
            .lock()
            .map_err(|_| AppError::Internal)?
            .get(key)
            .map(|(_, bytes)| bytes.clone())
            .ok_or(AppError::MediaNotFound)
    }

    async fn delete(&self, key: &str) -> Result<(), AppError> {
        self.objects
            .lock()
            .map_err(|_| AppError::Internal)?
            .remove(key);
        Ok(())
    }

    async fn exists(&self, key: &str) -> Result<bool, AppError> {
        Ok(self
            .objects
            .lock()
            .map_err(|_| AppError::Internal)?
            .contains_key(key))
    }

    async fn download_url(&self, key: &str, ttl: Duration) -> Result<String, AppError> {
        let exp = chrono::Utc::now().timestamp() + ttl.as_secs() as i64;
        let sig = Self::sign(&self.signing_secret, key, exp);
        Ok(format!("memory://{key}?exp={exp}&sig={sig}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn put_get_delete_and_signed_url() {
        let store = MemoryStorage::new("secret-secret-secret-secret-32ch");
        store.put("a/b.png", b"hello", "image/png").await.unwrap();
        assert!(store.exists("a/b.png").await.unwrap());
        assert_eq!(store.get("a/b.png").await.unwrap(), b"hello");
        let url = store
            .download_url("a/b.png", Duration::from_secs(60))
            .await
            .unwrap();
        assert!(url.starts_with("memory://a/b.png?"));
        store.delete("a/b.png").await.unwrap();
        assert!(!store.exists("a/b.png").await.unwrap());
    }
}
