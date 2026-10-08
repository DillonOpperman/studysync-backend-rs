pub mod catalog;
pub mod eval;
pub mod hashing;

use std::sync::Arc;
use std::time::Instant;

use sha2::{Digest, Sha256};

use crate::config::EmbeddingBackend;
use crate::error::AppError;

use self::hashing::HashingEmbedder;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryStyle {
    Raw,
    QwenInstruct,
    E5Instruct,
}

#[derive(Debug, Clone, Copy)]
pub struct ModelMetadata {
    pub exact_name: &'static str,
    pub model_id: &'static str,
    pub revision: &'static str,
    pub license: &'static str,
    pub dimensions: usize,
    pub context_length: Option<u32>,
    pub intended_use: &'static str,
    pub languages: &'static str,
    pub inference_library: &'static str,
    pub model_size: &'static str,
    pub runtime_requirements: &'static str,
    pub source_url: &'static str,
    pub commercially_redistributable: bool,
    pub query_style: QueryStyle,
    pub selected: bool,
}

impl ModelMetadata {
    pub fn identity(&self) -> String {
        format!("{}@{}", self.model_id, self.revision)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EmbeddingError {
    #[error("embedding input must not be blank")]
    EmptyInput,
    #[error("embedding model is unavailable")]
    Unavailable(String),
    #[error("embedding dimension mismatch: expected {expected}, got {actual}")]
    DimensionMismatch { expected: usize, actual: usize },
}

pub trait Embedder: Send + Sync {
    fn metadata(&self) -> &ModelMetadata;
    fn encode_query(&self, text: &str) -> Result<Vec<f32>, EmbeddingError>;
    fn encode_document(&self, text: &str) -> Result<Vec<f32>, EmbeddingError>;
    fn dimensions(&self) -> usize {
        self.metadata().dimensions
    }
}

/// Canonical corpus text used for group embeddings (legacy StudySync fields).
pub fn canonical_group_text(
    title: &str,
    subject: &str,
    description: &str,
    leader_name: &str,
    leader_major: &str,
) -> String {
    format!(
        "{title}. Subject: {subject}. {description}. Leader: {leader_name}. Major: {leader_major}."
    )
}

pub fn canonical_user_text(name: &str, major: &str, courses: &[String]) -> String {
    format!(
        "Student {name}. Major: {major}. Courses: {}.",
        courses.join(", ")
    )
}

pub fn source_hash(model_id: &str, text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(model_id.as_bytes());
    hasher.update([0]);
    hasher.update(text.as_bytes());
    hex::encode(hasher.finalize())
}

pub fn cosine(a: &[f32], b: &[f32]) -> Result<f32, EmbeddingError> {
    if a.len() != b.len() {
        return Err(EmbeddingError::DimensionMismatch {
            expected: a.len(),
            actual: b.len(),
        });
    }
    let mut dot = 0.0f64;
    let mut na = 0.0f64;
    let mut nb = 0.0f64;
    for (x, y) in a.iter().zip(b.iter()) {
        let xf = f64::from(*x);
        let yf = f64::from(*y);
        dot += xf * yf;
        na += xf * xf;
        nb += yf * yf;
    }
    let denom = na.sqrt() * nb.sqrt();
    if denom == 0.0 {
        Ok(0.0)
    } else {
        Ok((dot / denom) as f32)
    }
}

struct UnavailableEmbedder {
    metadata: ModelMetadata,
}

impl Embedder for UnavailableEmbedder {
    fn metadata(&self) -> &ModelMetadata {
        &self.metadata
    }

    fn encode_query(&self, _text: &str) -> Result<Vec<f32>, EmbeddingError> {
        Err(EmbeddingError::Unavailable(
            "embedding backend is unavailable".into(),
        ))
    }

    fn encode_document(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        self.encode_query(text)
    }
}

#[derive(Clone)]
pub struct EmbeddingService {
    inner: Arc<dyn Embedder>,
    semantic_available: bool,
}

impl EmbeddingService {
    pub fn from_backend(backend: EmbeddingBackend) -> Self {
        let production = catalog::production();
        match backend {
            EmbeddingBackend::Hash => Self {
                inner: Arc::new(HashingEmbedder::from_metadata(production)),
                semantic_available: true,
            },
            EmbeddingBackend::Unavailable => Self {
                inner: Arc::new(UnavailableEmbedder {
                    metadata: production,
                }),
                semantic_available: false,
            },
        }
    }

    pub fn hashing() -> Self {
        Self::from_backend(EmbeddingBackend::Hash)
    }

    pub fn unavailable() -> Self {
        Self::from_backend(EmbeddingBackend::Unavailable)
    }

    pub fn semantic_available(&self) -> bool {
        self.semantic_available
    }

    pub fn metadata(&self) -> &ModelMetadata {
        self.inner.metadata()
    }

    pub fn model_id(&self) -> &str {
        self.metadata().model_id
    }

    pub fn dimensions(&self) -> usize {
        self.metadata().dimensions
    }

    pub async fn encode_query(&self, text: &str) -> Result<Vec<f32>, AppError> {
        self.encode(true, text).await
    }

    pub async fn encode_document(&self, text: &str) -> Result<Vec<f32>, AppError> {
        self.encode(false, text).await
    }

    async fn encode(&self, query: bool, text: &str) -> Result<Vec<f32>, AppError> {
        if !self.semantic_available {
            return Err(AppError::SemanticUnavailable);
        }
        let inner = self.inner.clone();
        let owned = text.to_string();
        let started = Instant::now();
        let result = tokio::task::spawn_blocking(move || {
            if query {
                inner.encode_query(&owned)
            } else {
                inner.encode_document(&owned)
            }
        })
        .await
        .map_err(|_| AppError::Internal)?;
        let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
        match result {
            Ok(vector) => {
                if vector.len() != self.dimensions() {
                    return Err(AppError::EmbeddingDimensionMismatch);
                }
                tracing::info!(
                    model_id = self.model_id(),
                    elapsed_ms,
                    kind = if query { "query" } else { "document" },
                    "embedding encoded"
                );
                Ok(vector)
            }
            Err(EmbeddingError::EmptyInput) => {
                Err(AppError::validation("embedding text must not be blank"))
            }
            Err(EmbeddingError::Unavailable(_)) => Err(AppError::SemanticUnavailable),
            Err(EmbeddingError::DimensionMismatch { .. }) => {
                Err(AppError::EmbeddingDimensionMismatch)
            }
        }
    }
}

impl From<EmbeddingError> for AppError {
    fn from(value: EmbeddingError) -> Self {
        match value {
            EmbeddingError::EmptyInput => AppError::validation("embedding text must not be blank"),
            EmbeddingError::Unavailable(_) => AppError::SemanticUnavailable,
            EmbeddingError::DimensionMismatch { .. } => AppError::EmbeddingDimensionMismatch,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_metadata_is_exact_qwen_not_nickname() {
        let meta = catalog::production();
        assert_eq!(meta.exact_name, "Qwen/Qwen3-Embedding-0.6B");
        assert_eq!(meta.dimensions, 1024);
        assert_eq!(meta.model_id, catalog::PRODUCTION_MODEL_ID);
        assert!(!meta.revision.is_empty());
        assert_eq!(meta.license, "Apache-2.0");
        assert!(meta.commercially_redistributable);
        assert!(meta.selected);
    }

    #[test]
    fn hashing_is_deterministic_and_normalized() {
        let embedder = HashingEmbedder::from_metadata(catalog::production());
        let a = embedder.encode_document("Computer Networks lab").unwrap();
        let b = embedder.encode_document("Computer Networks lab").unwrap();
        assert_eq!(a, b);
        assert_eq!(a.len(), 1024);
        let norm: f32 = a.iter().map(|v| v * v).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-4);
    }

    #[test]
    fn blank_input_errors_instead_of_random_vectors() {
        let embedder = HashingEmbedder::from_metadata(catalog::production());
        assert!(matches!(
            embedder.encode_query("   "),
            Err(EmbeddingError::EmptyInput)
        ));
    }

    #[test]
    fn related_courses_rank_above_unrelated() {
        let embedder = HashingEmbedder::from_metadata(catalog::production());
        let q = embedder.encode_query("Computer Networks").unwrap();
        let related = embedder
            .encode_document("Networking homework help")
            .unwrap();
        let unrelated = embedder
            .encode_document("Organic Chemistry exam prep")
            .unwrap();
        assert!(cosine(&q, &related).unwrap() > cosine(&q, &unrelated).unwrap());
    }

    #[test]
    fn unavailable_embedder_does_not_invent_vectors() {
        let service = EmbeddingService::unavailable();
        assert!(!service.semantic_available());
        let embedder = UnavailableEmbedder {
            metadata: catalog::production(),
        };
        assert!(matches!(
            embedder.encode_query("calculus"),
            Err(EmbeddingError::Unavailable(_))
        ));
    }

    #[test]
    fn canonical_group_text_keeps_legacy_fields() {
        let text = canonical_group_text(
            "Networks Lab",
            "Computer Networks",
            "Exam prep",
            "Alex Rivera",
            "Computer Science",
        );
        assert!(text.contains("Networks Lab"));
        assert!(text.contains("Computer Networks"));
        assert!(text.contains("Exam prep"));
        assert!(text.contains("Alex Rivera"));
        assert!(text.contains("Computer Science"));
    }
}
