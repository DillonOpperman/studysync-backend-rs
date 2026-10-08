use std::path::Path;
use std::sync::Mutex;
use std::time::Instant;

use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use tokenizers::{PaddingDirection, PaddingParams, PaddingStrategy, Tokenizer, TruncationParams};

use super::catalog;
use super::hf;
use super::nomic_bert::{Config, NomicBertModel};
use super::pool::mean_pool_layer_norm_l2;
use super::{Embedder, EmbeddingError, ModelMetadata};

const HF_REPO: &str = "nomic-ai/nomic-embed-text-v1.5";
const QUERY_PREFIX: &str = "search_query: ";
const DOCUMENT_PREFIX: &str = "search_document: ";

struct Inner {
    model: NomicBertModel,
    tokenizer: Tokenizer,
    device: Device,
}

pub struct NomicEmbedder {
    metadata: ModelMetadata,
    inner: Mutex<Inner>,
    pub load_ms: f64,
    pub weight_bytes: u64,
}

impl NomicEmbedder {
    pub fn load(cache_dir: &Path, max_length: usize) -> Result<Self, EmbeddingError> {
        let started = Instant::now();
        let revision = catalog::nomic_embed_text_v15().revision;
        let cfg_path = hf::snapshot_file(cache_dir, HF_REPO, revision, "config.json")?;
        let tok_path = hf::snapshot_file(cache_dir, HF_REPO, revision, "tokenizer.json")?;
        let weights = hf::snapshot_file(cache_dir, HF_REPO, revision, "model.safetensors")?;
        let weight_bytes = hf::weight_bytes(&weights);
        let max_length = max_length.clamp(32, 2048);

        let device = Device::Cpu;
        let cfg: Config = serde_json::from_slice(
            &std::fs::read(&cfg_path)
                .map_err(|error| EmbeddingError::Unavailable(error.to_string()))?,
        )
        .map_err(|error| EmbeddingError::Unavailable(format!("nomic config: {error}")))?;
        if cfg.n_embd != 768 {
            return Err(EmbeddingError::DimensionMismatch {
                expected: 768,
                actual: cfg.n_embd,
            });
        }
        let vb = unsafe {
            VarBuilder::from_mmaped_safetensors(&[weights], DType::F32, &device)
                .map_err(|error| EmbeddingError::Unavailable(error.to_string()))?
        };
        let model = NomicBertModel::load(vb, &cfg)
            .map_err(|error| EmbeddingError::Unavailable(error.to_string()))?;
        let mut tokenizer = Tokenizer::from_file(&tok_path)
            .map_err(|error| EmbeddingError::Unavailable(format!("nomic tokenizer: {error}")))?;
        let _ = tokenizer.with_padding(Some(PaddingParams {
            strategy: PaddingStrategy::BatchLongest,
            direction: PaddingDirection::Right,
            pad_id: 0,
            ..Default::default()
        }));
        let _ = tokenizer.with_truncation(Some(TruncationParams {
            max_length,
            ..Default::default()
        }));

        Ok(Self {
            metadata: catalog::nomic_embed_text_v15(),
            inner: Mutex::new(Inner {
                model,
                tokenizer,
                device,
            }),
            load_ms: started.elapsed().as_secs_f64() * 1000.0,
            weight_bytes,
        })
    }

    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        if texts.iter().any(|text| text.trim().is_empty()) {
            return Err(EmbeddingError::EmptyInput);
        }
        let guard = self
            .inner
            .lock()
            .map_err(|_| EmbeddingError::Unavailable("nomic lock poisoned".into()))?;
        let encodings = guard
            .tokenizer
            .encode_batch(texts.iter().map(String::as_str).collect::<Vec<_>>(), true)
            .map_err(|error| EmbeddingError::Unavailable(error.to_string()))?;
        let batch = encodings.len();
        let seq = encodings[0].len();
        let mut ids = Vec::with_capacity(batch * seq);
        let mut mask = Vec::with_capacity(batch * seq);
        let mut types = Vec::with_capacity(batch * seq);
        for enc in &encodings {
            ids.extend(enc.get_ids().iter().copied());
            mask.extend(enc.get_attention_mask().iter().map(|m| *m as f32));
            types.extend(enc.get_type_ids().iter().copied());
        }
        let input_ids = Tensor::from_vec(ids, (batch, seq), &guard.device)
            .map_err(|error| EmbeddingError::Unavailable(error.to_string()))?;
        let token_type_ids = Tensor::from_vec(types, (batch, seq), &guard.device)
            .map_err(|error| EmbeddingError::Unavailable(error.to_string()))?;
        let attention = Tensor::from_vec(mask, (batch, seq), &guard.device)
            .map_err(|error| EmbeddingError::Unavailable(error.to_string()))?;
        let hidden = guard
            .model
            .forward(&input_ids, Some(&token_type_ids), Some(&attention))
            .map_err(|error| EmbeddingError::Unavailable(error.to_string()))?;
        let vectors = mean_pool_layer_norm_l2(&hidden, &attention)?;
        for vector in &vectors {
            if vector.len() != 768 {
                return Err(EmbeddingError::DimensionMismatch {
                    expected: 768,
                    actual: vector.len(),
                });
            }
        }
        Ok(vectors)
    }
}

impl Embedder for NomicEmbedder {
    fn metadata(&self) -> &ModelMetadata {
        &self.metadata
    }

    fn encode_query(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        if text.trim().is_empty() {
            return Err(EmbeddingError::EmptyInput);
        }
        Ok(self
            .embed(&[format!("{QUERY_PREFIX}{}", text.trim())])?
            .remove(0))
    }

    fn encode_document(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        if text.trim().is_empty() {
            return Err(EmbeddingError::EmptyInput);
        }
        Ok(self
            .embed(&[format!("{DOCUMENT_PREFIX}{}", text.trim())])?
            .remove(0))
    }

    fn encode_queries(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        let owned: Vec<String> = texts
            .iter()
            .map(|text| {
                if text.trim().is_empty() {
                    Err(EmbeddingError::EmptyInput)
                } else {
                    Ok(format!("{QUERY_PREFIX}{}", text.trim()))
                }
            })
            .collect::<Result<_, _>>()?;
        self.embed(&owned)
    }

    fn encode_documents(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        let owned: Vec<String> = texts
            .iter()
            .map(|text| {
                if text.trim().is_empty() {
                    Err(EmbeddingError::EmptyInput)
                } else {
                    Ok(format!("{DOCUMENT_PREFIX}{}", text.trim()))
                }
            })
            .collect::<Result<_, _>>()?;
        self.embed(&owned)
    }
}
