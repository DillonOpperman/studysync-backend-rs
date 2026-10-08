//! Compare Candle Nomic embeddings to the official Transformers fixture.
//!
//! Fixture: `eval/nomic_hf_reference.json` from `eval/nomic_hf_reference.py`
//! (development verification only; Python is not a production dependency).
//!
//! Run: `cargo test --release --test nomic_reference -- --ignored --nocapture --test-threads=1`

use std::path::PathBuf;

use serde::Deserialize;
use studysync_backend::embed::catalog::{self, NOMIC_EMBED_TEXT_V15_REVISION};
use studysync_backend::embed::nomic::NomicEmbedder;
use studysync_backend::embed::{cosine, Embedder};

#[derive(Deserialize)]
struct Fixture {
    revision: String,
    records: Vec<Record>,
    similarities: Vec<Pair>,
    special_tokens: SpecialTokens,
}

#[derive(Deserialize)]
struct SpecialTokens {
    cls: u32,
    sep: u32,
    pad: u32,
}

#[derive(Deserialize)]
struct Record {
    role: String,
    name: String,
    text: String,
    prefixed: String,
    input_ids: Vec<u32>,
    embedding: Vec<f32>,
}

#[derive(Deserialize)]
struct Pair {
    left: String,
    right: String,
    cosine: f32,
}

fn cache_dir() -> PathBuf {
    std::env::var("EMBEDDING_CACHE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(".fastembed_cache"))
}

fn load_fixture() -> Fixture {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("eval/nomic_hf_reference.json");
    let bytes = std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    serde_json::from_slice(&bytes).expect("nomic HF fixture")
}

fn max_abs(a: &[f32], b: &[f32]) -> f32 {
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y).abs())
        .fold(0.0_f32, f32::max)
}

fn mean_abs(a: &[f32], b: &[f32]) -> f32 {
    let n = a.len() as f32;
    a.iter().zip(b).map(|(x, y)| (x - y).abs()).sum::<f32>() / n
}

#[test]
fn hf_fixture_is_768d_pinned_revision() {
    let fixture = load_fixture();
    assert_eq!(fixture.revision, NOMIC_EMBED_TEXT_V15_REVISION);
    assert_eq!(fixture.special_tokens.cls, 101);
    assert_eq!(fixture.special_tokens.sep, 102);
    assert_eq!(fixture.special_tokens.pad, 0);
    for record in &fixture.records {
        assert_eq!(record.embedding.len(), catalog::PRODUCTION_EMBEDDING_DIM);
    }
}

#[test]
#[ignore = "requires cached nomic-embed-text-v1.5 safetensors at the pinned revision"]
fn candle_matches_official_transformers_fixture() {
    let fixture = load_fixture();
    let embedder = NomicEmbedder::load(&cache_dir(), 512)
        .unwrap_or_else(|error| panic!("Nomic neural weights required: {error}"));
    assert_eq!(embedder.metadata().revision, NOMIC_EMBED_TEXT_V15_REVISION);
    assert_eq!(embedder.metadata().dimensions, 768);
    assert_eq!(embedder.metadata().model_id, catalog::PRODUCTION_MODEL_ID);

    let tok_path = cache_dir()
        .join("models--nomic-ai--nomic-embed-text-v1.5")
        .join("snapshots")
        .join(NOMIC_EMBED_TEXT_V15_REVISION)
        .join("tokenizer.json");
    let tokenizer = tokenizers::Tokenizer::from_file(&tok_path).unwrap_or_else(|error| {
        panic!(
            "nomic tokenizer.json required at {}: {error}",
            tok_path.display()
        )
    });

    let mut rust_by_name = std::collections::HashMap::new();
    let mut worst_max = 0.0_f32;
    let mut worst_mean = 0.0_f32;
    let mut worst_cos = 1.0_f32;
    for record in &fixture.records {
        let rust_ids = tokenizer
            .encode(record.prefixed.as_str(), true)
            .expect("tokenize")
            .get_ids()
            .to_vec();
        let hf_ids: Vec<u32> = record
            .input_ids
            .iter()
            .copied()
            .rev()
            .skip_while(|id| *id == 0)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        assert_eq!(rust_ids, hf_ids, "tokenizer mismatch on {}", record.name);
        let vector = if record.role == "query" {
            embedder.encode_query(&record.text).expect("query")
        } else {
            embedder.encode_document(&record.text).expect("document")
        };
        assert_eq!(vector.len(), 768);
        let l2: f32 = vector.iter().map(|v| v * v).sum::<f32>().sqrt();
        assert!((l2 - 1.0).abs() < 1e-3, "{} L2={l2}", record.name);
        let max = max_abs(&vector, &record.embedding);
        let mean = mean_abs(&vector, &record.embedding);
        let sim = cosine(&vector, &record.embedding).expect("cos");
        eprintln!(
            "REF {} max_abs={max:.6e} mean_abs={mean:.6e} cosine={sim:.8}",
            record.name
        );
        worst_max = worst_max.max(max);
        worst_mean = worst_mean.max(mean);
        worst_cos = worst_cos.min(sim);
        rust_by_name.insert(record.name.clone(), vector);
    }

    for pair in &fixture.similarities {
        let left = &rust_by_name[&pair.left];
        let right = &rust_by_name[&pair.right];
        let rust_cos = cosine(left, right).expect("pair");
        let delta = (rust_cos - pair.cosine).abs();
        eprintln!(
            "PAIR {} vs {} hf={:.6} rust={:.6} |d|={delta:.6e}",
            pair.left, pair.right, pair.cosine, rust_cos
        );
        assert!(
            delta < 1e-5,
            "cosine pair {} vs {} diverged by {delta}",
            pair.left,
            pair.right
        );
    }

    eprintln!("WORST max_abs={worst_max:.6e} mean_abs={worst_mean:.6e} min_cosine={worst_cos:.8}");
    assert!(
        worst_cos > 0.99999,
        "Candle vs HF cosine {worst_cos} is too low"
    );
    assert!(
        worst_max < 1e-5,
        "component max abs {worst_max} exceeds float32 noise"
    );
}
