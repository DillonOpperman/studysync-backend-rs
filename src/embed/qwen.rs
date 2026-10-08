use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Instant;

use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use hf_hub::api::sync::ApiBuilder;
use hf_hub::{Repo, RepoType};
use tokenizers::{PaddingDirection, PaddingParams, PaddingStrategy, Tokenizer, TruncationParams};

use super::catalog::{self, QWEN3_EMBEDDING_06B_REVISION};
use super::qwen3_model::{Config as Qwen3Config, Qwen3Model};
use super::{Embedder, EmbeddingError, ModelMetadata};

const HF_REPO: &str = "Qwen/Qwen3-Embedding-0.6B";
const DEFAULT_TASK: &str = "Given a study-group search query, retrieve relevant study groups";

struct QwenInner {
    model: Qwen3Model,
    tokenizer: Tokenizer,
}

pub struct QwenEmbedder {
    metadata: ModelMetadata,
    inner: Mutex<QwenInner>,
    pub load_ms: f64,
    pub snapshot_dir: PathBuf,
    pub weight_bytes: u64,
}

pub struct QwenLoadOptions<'a> {
    pub cache_dir: &'a Path,
    pub max_length: usize,
}

impl QwenEmbedder {
    pub fn load(options: QwenLoadOptions<'_>) -> Result<Self, EmbeddingError> {
        let started = Instant::now();
        fs::create_dir_all(options.cache_dir).map_err(|error| {
            EmbeddingError::Unavailable(format!("cannot create embedding cache: {error}"))
        })?;

        let api = ApiBuilder::new()
            .with_cache_dir(options.cache_dir.to_path_buf())
            .with_progress(true)
            .build()
            .map_err(|error| EmbeddingError::Unavailable(error.to_string()))?;
        let repo = api.repo(Repo::with_revision(
            HF_REPO.to_string(),
            RepoType::Model,
            QWEN3_EMBEDDING_06B_REVISION.to_string(),
        ));

        let cfg_path = repo
            .get("config.json")
            .map_err(|error| EmbeddingError::Unavailable(error.to_string()))?;
        let snapshot_dir = cfg_path
            .parent()
            .ok_or_else(|| EmbeddingError::Unavailable("missing snapshot directory".into()))?
            .to_path_buf();
        verify_revision(&snapshot_dir)?;

        let tok_path = repo
            .get("tokenizer.json")
            .map_err(|error| EmbeddingError::Unavailable(error.to_string()))?;
        let weight_files = safetensor_files(&repo)?;
        let weight_bytes = weight_files
            .iter()
            .map(|path| fs::metadata(path).map(|meta| meta.len()).unwrap_or(0))
            .sum();

        let device = Device::Cpu;
        let dtype = DType::F32;
        let cfg_bytes =
            fs::read(&cfg_path).map_err(|error| EmbeddingError::Unavailable(error.to_string()))?;
        let cfg: Qwen3Config = serde_json::from_slice(&cfg_bytes)
            .map_err(|error| EmbeddingError::Unavailable(format!("qwen config: {error}")))?;
        let expected_dim = catalog::qwen3_embedding_06b().dimensions;
        if cfg.hidden_size != expected_dim {
            return Err(EmbeddingError::DimensionMismatch {
                expected: expected_dim,
                actual: cfg.hidden_size,
            });
        }

        let vb = unsafe {
            VarBuilder::from_mmaped_safetensors(&weight_files, dtype, &device)
                .map_err(|error| EmbeddingError::Unavailable(error.to_string()))?
        };
        let vb = if vb.contains_tensor("model.embed_tokens.weight") {
            vb.pp("model")
        } else {
            vb
        };
        let model = Qwen3Model::new(cfg, vb)
            .map_err(|error| EmbeddingError::Unavailable(error.to_string()))?;

        let mut tokenizer = Tokenizer::from_file(&tok_path)
            .map_err(|error| EmbeddingError::Unavailable(format!("tokenizer: {error}")))?;
        let pad_id = tokenizer
            .token_to_id("<|endoftext|>")
            .or_else(|| tokenizer.get_padding().map(|padding| padding.pad_id))
            .unwrap_or(0);
        let _ = tokenizer.with_padding(Some(PaddingParams {
            strategy: PaddingStrategy::BatchLongest,
            direction: PaddingDirection::Left,
            pad_id,
            ..Default::default()
        }));
        let _ = tokenizer.with_truncation(Some(TruncationParams {
            max_length: options.max_length,
            ..Default::default()
        }));

        let load_ms = started.elapsed().as_secs_f64() * 1000.0;
        let metadata = catalog::qwen3_embedding_06b();
        tracing::info!(
            model = metadata.exact_name,
            revision = metadata.revision,
            model_id = metadata.model_id,
            dimensions = metadata.dimensions,
            license = metadata.license,
            runtime = "candle Qwen3 last-token pool (CPU F32)",
            load_ms,
            cache = %options.cache_dir.display(),
            snapshot = %snapshot_dir.display(),
            "qwen embedding model loaded"
        );
        Ok(Self {
            metadata,
            inner: Mutex::new(QwenInner { model, tokenizer }),
            load_ms,
            snapshot_dir,
            weight_bytes,
        })
    }

    fn embed_formatted(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        if texts.iter().any(|text| text.trim().is_empty()) {
            return Err(EmbeddingError::EmptyInput);
        }
        let guard = self
            .inner
            .lock()
            .map_err(|_| EmbeddingError::Unavailable("qwen embedder lock poisoned".into()))?;
        let encodings = guard
            .tokenizer
            .encode_batch(texts.to_vec(), true)
            .map_err(|error| EmbeddingError::Unavailable(format!("tokenize: {error}")))?;
        if encodings.is_empty() {
            return Ok(Vec::new());
        }
        let seq_len = encodings[0].len();
        let batch = encodings.len();
        let mut ids = Vec::with_capacity(batch * seq_len);
        let mut mask = Vec::with_capacity(batch * seq_len);
        for encoding in &encodings {
            if encoding.len() != seq_len {
                return Err(EmbeddingError::Unavailable(
                    "tokenizer did not pad the batch to a uniform length".into(),
                ));
            }
            ids.extend(encoding.get_ids().iter().copied());
            mask.extend(
                encoding
                    .get_attention_mask()
                    .iter()
                    .map(|&value| value as f32),
            );
        }
        let device = guard.model.device().clone();
        let input_ids = Tensor::from_vec(ids, (batch, seq_len), &device)
            .map_err(|error| EmbeddingError::Unavailable(error.to_string()))?;
        let attention_mask = Tensor::from_vec(mask, (batch, seq_len), &device)
            .map_err(|error| EmbeddingError::Unavailable(error.to_string()))?;
        let vectors = guard
            .model
            .embed_normalized(&input_ids, &attention_mask)
            .map_err(|error| EmbeddingError::Unavailable(error.to_string()))?;
        for vector in &vectors {
            if vector.len() != catalog::qwen3_embedding_06b().dimensions {
                return Err(EmbeddingError::DimensionMismatch {
                    expected: catalog::qwen3_embedding_06b().dimensions,
                    actual: vector.len(),
                });
            }
        }
        Ok(vectors)
    }
}

impl Embedder for QwenEmbedder {
    fn metadata(&self) -> &ModelMetadata {
        &self.metadata
    }

    fn encode_query(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        if text.trim().is_empty() {
            return Err(EmbeddingError::EmptyInput);
        }
        let formatted = format!("Instruct: {DEFAULT_TASK}\nQuery:{}", text.trim());
        Ok(self.embed_formatted(&[formatted])?.remove(0))
    }

    fn encode_document(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        if text.trim().is_empty() {
            return Err(EmbeddingError::EmptyInput);
        }
        Ok(self.embed_formatted(&[text.trim().to_string()])?.remove(0))
    }

    fn encode_queries(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        let formatted: Vec<String> = texts
            .iter()
            .map(|text| {
                if text.trim().is_empty() {
                    Err(EmbeddingError::EmptyInput)
                } else {
                    Ok(format!("Instruct: {DEFAULT_TASK}\nQuery:{}", text.trim()))
                }
            })
            .collect::<Result<_, _>>()?;
        self.embed_formatted(&formatted)
    }

    fn encode_documents(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        let formatted: Vec<String> = texts
            .iter()
            .map(|text| {
                if text.trim().is_empty() {
                    Err(EmbeddingError::EmptyInput)
                } else {
                    Ok(text.trim().to_string())
                }
            })
            .collect::<Result<_, _>>()?;
        self.embed_formatted(&formatted)
    }
}

fn verify_revision(snapshot_dir: &Path) -> Result<(), EmbeddingError> {
    let name = snapshot_dir
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if name != QWEN3_EMBEDDING_06B_REVISION {
        return Err(EmbeddingError::Unavailable(format!(
            "refusing to load unpinned Qwen snapshot {name}; expected {QWEN3_EMBEDDING_06B_REVISION}"
        )));
    }
    Ok(())
}

fn safetensor_files(repo: &hf_hub::api::sync::ApiRepo) -> Result<Vec<PathBuf>, EmbeddingError> {
    if let Ok(path) = repo.get("model.safetensors") {
        return Ok(vec![path]);
    }
    let index_path = repo
        .get("model.safetensors.index.json")
        .map_err(|error| EmbeddingError::Unavailable(format!("qwen weights: {error}")))?;
    let index: serde_json::Value = serde_json::from_slice(
        &fs::read(&index_path).map_err(|error| EmbeddingError::Unavailable(error.to_string()))?,
    )
    .map_err(|error| EmbeddingError::Unavailable(error.to_string()))?;
    let map = index
        .get("weight_map")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| EmbeddingError::Unavailable("qwen weight_map missing".into()))?;
    let names: BTreeSet<String> = map
        .values()
        .filter_map(|value| value.as_str().map(ToOwned::to_owned))
        .collect();
    names
        .into_iter()
        .map(|name| {
            repo.get(&name)
                .map_err(|error| EmbeddingError::Unavailable(error.to_string()))
        })
        .collect()
}
