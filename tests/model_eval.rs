//! Real neural embedding evaluation. Not run by default `cargo test`.
//!
//! Required:
//! - Hugging Face network on first download, then `EMBEDDING_CACHE_DIR` (default `.fastembed_cache`)
//! - pinned Qwen revision `c935f2d3fce3e2337b4a66ac3130faa6dada3218`
//!
//! Run: `cargo test --test model_eval -- --ignored --nocapture`

mod common;

use std::path::PathBuf;
use std::time::Instant;

use studysync_backend::embed::catalog::{self, QWEN3_EMBEDDING_06B_REVISION};
use studysync_backend::embed::eval::{evaluate_embedder_with_load, load_dataset};
use studysync_backend::embed::minilm::MiniLmEmbedder;
use studysync_backend::embed::qwen::{QwenEmbedder, QwenLoadOptions};
use studysync_backend::embed::{cosine, Embedder};

fn cache_dir() -> PathBuf {
    std::env::var("EMBEDDING_CACHE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(".fastembed_cache"))
}

#[test]
#[ignore = "requires cached or downloadable MiniLM safetensors and Qwen3-Embedding-0.6B weights"]
fn neural_minilm_and_qwen_studysync_eval() {
    let cache = cache_dir();
    let data = load_dataset();

    let minilm = MiniLmEmbedder::load(&cache).unwrap_or_else(|error| {
        panic!("MiniLM neural weights are required for this test: {error}");
    });
    assert_eq!(minilm.metadata().dimensions, 384);
    let minilm_load = minilm.load_ms;
    let minilm_report = evaluate_embedder_with_load(&minilm, &data, Some(minilm_load));

    let qwen = QwenEmbedder::load(QwenLoadOptions {
        cache_dir: &cache,
        max_length: 512,
    })
    .unwrap_or_else(|error| {
        panic!(
            "Qwen/Qwen3-Embedding-0.6B@{QWEN3_EMBEDDING_06B_REVISION} is required for this test: {error}"
        );
    });
    assert_eq!(qwen.metadata().dimensions, 1024);
    assert_eq!(qwen.metadata().revision, QWEN3_EMBEDDING_06B_REVISION);
    assert!(qwen.snapshot_dir.ends_with(QWEN3_EMBEDDING_06B_REVISION));

    let same_a = qwen.encode_document("Computer Networks lab").unwrap();
    let same_b = qwen.encode_document("Computer Networks lab").unwrap();
    assert_eq!(same_a.len(), 1024);
    let norm: f32 = same_a.iter().map(|v| v * v).sum::<f32>().sqrt();
    assert!((norm - 1.0).abs() < 1e-3);
    assert!(cosine(&same_a, &same_b).unwrap() > 0.999);

    let query = "computer networks study group";
    let related = qwen.encode_document("Networking homework help").unwrap();
    let unrelated = qwen.encode_document("Organic Chemistry exam prep").unwrap();
    let qvec = qwen.encode_query(query).unwrap();
    assert!(cosine(&qvec, &related).unwrap() > cosine(&qvec, &unrelated).unwrap());

    let cold_start = Instant::now();
    let _ = qwen.encode_query("calculus exam prep").unwrap();
    let cold_ms = cold_start.elapsed().as_secs_f64() * 1000.0;
    let warm_start = Instant::now();
    let _ = qwen.encode_query("calculus exam prep").unwrap();
    let warm_ms = warm_start.elapsed().as_secs_f64() * 1000.0;

    let sample: Vec<&str> = (0..100).map(|_| "computer science study group").collect();
    let batch = |n: usize| {
        let started = Instant::now();
        qwen.encode_queries(&sample[..n]).unwrap();
        started.elapsed().as_secs_f64() * 1000.0
    };
    let batch10 = batch(10);
    let batch50 = batch(50);
    let batch100 = batch(100);

    let qwen_report = evaluate_embedder_with_load(&qwen, &data, Some(qwen.load_ms));
    let approx_ram_mb = (qwen.weight_bytes as f64 / (1024.0 * 1024.0)) + 250.0;

    eprintln!("NEURAL MiniLM load_ms={:.1} top1={:.3} recall5={:.3} ndcg5={:.3} order={:.3} q_ms={:.1} d_ms={:.1}",
        minilm_report.load_ms, minilm_report.top1_relevance, minilm_report.top5_recall,
        minilm_report.ndcg_at_5, minilm_report.pairwise_order_accuracy,
        minilm_report.avg_query_encode_ms, minilm_report.avg_doc_encode_ms);
    eprintln!(
        "NEURAL Qwen {}@{} dim={} load_ms={:.1} top1={:.3} recall5={:.3} ndcg5={:.3} order={:.3} q_ms={:.1} d_ms={:.1} cold_ms={:.1} warm_ms={:.1} batch10_ms={:.1} batch50_ms={:.1} batch100_ms={:.1} weights_mb={:.0} approx_ram_mb={:.0}",
        catalog::qwen3_embedding_06b().exact_name,
        QWEN3_EMBEDDING_06B_REVISION,
        qwen_report.metadata.dimensions,
        qwen_report.load_ms,
        qwen_report.top1_relevance,
        qwen_report.top5_recall,
        qwen_report.ndcg_at_5,
        qwen_report.pairwise_order_accuracy,
        qwen_report.avg_query_encode_ms,
        qwen_report.avg_doc_encode_ms,
        cold_ms,
        warm_ms,
        batch10,
        batch50,
        batch100,
        qwen.weight_bytes as f64 / (1024.0 * 1024.0),
        approx_ram_mb
    );

    assert!(qwen_report.top1_relevance >= 0.0);
    assert_eq!(qwen_report.document_count, 24);
    let _ = minilm;
}

#[tokio::test]
#[ignore = "requires Qwen weights, PostgreSQL, and EMBEDDING_BACKEND=qwen"]
async fn qwen_pgvector_save_reload_cosine() {
    use studysync_backend::db::embeddings;
    use uuid::Uuid;

    let cache = cache_dir();
    let qwen = QwenEmbedder::load(QwenLoadOptions {
        cache_dir: &cache,
        max_length: 512,
    })
    .expect("Qwen weights required");
    let text = "Computer Networks. Subject: Networking. Exam prep. Leader: Alex. Major: CS.";
    let vector = qwen.encode_document(text).expect("encode");
    assert_eq!(vector.len(), 1024);

    let mut state = common::test_state().await;
    state.embeddings = studysync_backend::embed::EmbeddingService::unavailable();
    let mut tx = state.db.begin().await.expect("tx");
    let entity = Uuid::now_v7();
    embeddings::upsert(
        &mut tx,
        "group",
        entity,
        catalog::PRODUCTION_MODEL_ID,
        "parity-hash",
        &vector,
    )
    .await
    .expect("insert");
    let loaded = embeddings::load_vector(&mut *tx, "group", entity, catalog::PRODUCTION_MODEL_ID)
        .await
        .expect("load")
        .expect("row");
    tx.commit().await.expect("commit");
    assert_eq!(loaded.len(), 1024);
    assert!(cosine(&vector, &loaded).unwrap() > 0.999);
}
