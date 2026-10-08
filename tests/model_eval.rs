//! Real neural embedding bake-off. Not run by default `cargo test`.
//!
//! Required: Hugging Face network on first download, then `EMBEDDING_CACHE_DIR`.
//! Missing weights panic (they do not emit hashed stand-in vectors).
//!
//! Run: `cargo test --release --test model_eval -- --ignored --nocapture --test-threads=1`

mod common;

use std::path::PathBuf;
use std::time::Instant;

use studysync_backend::embed::bge::BgeSmallEmbedder;
use studysync_backend::embed::catalog::{
    self, BGE_SMALL_EN_V15_REVISION, MINILM_REVISION, NOMIC_EMBED_TEXT_V15_REVISION,
};
use studysync_backend::embed::eval::{evaluate_embedder_with_load, load_dataset};
use studysync_backend::embed::minilm::MiniLmEmbedder;
use studysync_backend::embed::nomic::NomicEmbedder;
use studysync_backend::embed::{cosine, Embedder};

fn cache_dir() -> PathBuf {
    std::env::var("EMBEDDING_CACHE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(".fastembed_cache"))
}

struct Perf {
    cold_ms: f64,
    warm_ms: f64,
    p50_ms: f64,
    p95_ms: f64,
    batch10_ms: f64,
    batch50_ms: f64,
    batch100_ms: f64,
    related_cos: f32,
    unrelated_cos: f32,
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((p / 100.0) * (sorted.len() - 1) as f64).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

fn measure_perf(embedder: &dyn Embedder) -> Perf {
    let sample: Vec<&str> = (0..100).map(|_| "computer science study group").collect();
    let cold_start = Instant::now();
    let _ = embedder.encode_query("calculus exam prep").unwrap();
    let cold_ms = cold_start.elapsed().as_secs_f64() * 1000.0;
    let mut warms = Vec::new();
    for _ in 0..20 {
        let started = Instant::now();
        let _ = embedder.encode_query("calculus exam prep").unwrap();
        warms.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    warms.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let batch = |n: usize| {
        let started = Instant::now();
        embedder.encode_queries(&sample[..n]).unwrap();
        started.elapsed().as_secs_f64() * 1000.0
    };
    let q = embedder
        .encode_query("computer networks study group")
        .unwrap();
    let related = embedder
        .encode_document("Networking homework help")
        .unwrap();
    let unrelated = embedder
        .encode_document("Organic Chemistry exam prep")
        .unwrap();
    Perf {
        cold_ms,
        warm_ms: warms[0],
        p50_ms: percentile(&warms, 50.0),
        p95_ms: percentile(&warms, 95.0),
        batch10_ms: batch(10),
        batch50_ms: batch(50),
        batch100_ms: batch(100),
        related_cos: cosine(&q, &related).unwrap(),
        unrelated_cos: cosine(&q, &unrelated).unwrap(),
    }
}

fn assert_contract(embedder: &dyn Embedder, dim: usize) {
    assert_eq!(embedder.metadata().dimensions, dim);
    let a = embedder.encode_document("Computer Networks lab").unwrap();
    let b = embedder.encode_document("Computer Networks lab").unwrap();
    assert_eq!(a.len(), dim);
    let norm: f32 = a.iter().map(|v| v * v).sum::<f32>().sqrt();
    assert!((norm - 1.0).abs() < 1e-3, "expected L2~1, got {norm}");
    assert!(cosine(&a, &b).unwrap() > 0.999);
}

fn print_report(
    label: &str,
    report: &studysync_backend::embed::eval::CandidateReport,
    perf: &Perf,
    weight_bytes: u64,
) {
    let ram = (weight_bytes as f64 / (1024.0 * 1024.0)) + 80.0;
    eprintln!(
        "NEURAL {label} rev={} dim={} license={} load_ms={:.1} top1={:.3} recall5={:.3} ndcg5={:.3} order={:.3} q_ms={:.1} d_ms={:.1} cold_ms={:.1} warm_ms={:.1} p50_ms={:.1} p95_ms={:.1} batch10_ms={:.1} batch50_ms={:.1} batch100_ms={:.1} related={:.3} unrelated={:.3} weights_mb={:.1} approx_ram_mb={:.0}",
        report.metadata.revision,
        report.metadata.dimensions,
        report.metadata.license,
        report.load_ms,
        report.top1_relevance,
        report.top5_recall,
        report.ndcg_at_5,
        report.pairwise_order_accuracy,
        report.avg_query_encode_ms,
        report.avg_doc_encode_ms,
        perf.cold_ms,
        perf.warm_ms,
        perf.p50_ms,
        perf.p95_ms,
        perf.batch10_ms,
        perf.batch50_ms,
        perf.batch100_ms,
        perf.related_cos,
        perf.unrelated_cos,
        weight_bytes as f64 / (1024.0 * 1024.0),
        ram
    );
}

#[test]
#[ignore = "requires cached or downloadable MiniLM, BGE-small, and Nomic safetensors"]
fn neural_lightweight_bakeoff() {
    let cache = cache_dir();
    let data = load_dataset();
    assert_eq!(data.documents.len(), 24);
    assert_eq!(data.queries.len(), 20);

    let minilm = MiniLmEmbedder::load(&cache)
        .unwrap_or_else(|error| panic!("MiniLM neural weights required: {error}"));
    assert_eq!(minilm.metadata().revision, MINILM_REVISION);
    assert_contract(&minilm, 384);
    let minilm_perf = measure_perf(&minilm);
    assert!(minilm_perf.related_cos > minilm_perf.unrelated_cos);
    let minilm_report = evaluate_embedder_with_load(&minilm, &data, Some(minilm.load_ms));
    print_report("MiniLM", &minilm_report, &minilm_perf, minilm.weight_bytes);

    let bge = BgeSmallEmbedder::load(&cache)
        .unwrap_or_else(|error| panic!("BGE-small neural weights required: {error}"));
    assert_eq!(bge.metadata().revision, BGE_SMALL_EN_V15_REVISION);
    assert_contract(&bge, 384);
    let bge_q = bge.encode_query("networks").unwrap();
    let bge_raw = bge.encode_document("networks").unwrap();
    assert!(
        cosine(&bge_q, &bge_raw).unwrap() < 0.999,
        "BGE query instruction must change the vector"
    );
    let bge_perf = measure_perf(&bge);
    assert!(bge_perf.related_cos > bge_perf.unrelated_cos);
    let bge_report = evaluate_embedder_with_load(&bge, &data, Some(bge.load_ms));
    print_report("BGE-small", &bge_report, &bge_perf, bge.weight_bytes);

    let nomic = NomicEmbedder::load(&cache, 512)
        .unwrap_or_else(|error| panic!("Nomic neural weights required: {error}"));
    assert_eq!(nomic.metadata().revision, NOMIC_EMBED_TEXT_V15_REVISION);
    assert_contract(&nomic, 768);
    let nomic_q = nomic.encode_query("networks").unwrap();
    let nomic_d = nomic.encode_document("networks").unwrap();
    assert!(
        cosine(&nomic_q, &nomic_d).unwrap() < 0.999,
        "Nomic query/document prefixes must change the vector"
    );
    let nomic_perf = measure_perf(&nomic);
    assert!(nomic_perf.related_cos > nomic_perf.unrelated_cos);
    let nomic_report = evaluate_embedder_with_load(&nomic, &data, Some(nomic.load_ms));
    print_report("Nomic", &nomic_report, &nomic_perf, nomic.weight_bytes);

    assert_eq!(minilm_report.document_count, 24);
    assert_eq!(bge_report.document_count, 24);
    assert_eq!(nomic_report.document_count, 24);
}

#[tokio::test]
#[ignore = "requires neural weights, PostgreSQL, and dimension-specific embedding tables"]
async fn neural_pgvector_save_reload_cosine() {
    use studysync_backend::db::embeddings;
    use uuid::Uuid;

    let cache = cache_dir();
    let nomic = NomicEmbedder::load(&cache, 512).expect("Nomic weights required");
    let text = "Computer Networks. Subject: Networking. Exam prep. Leader: Alex. Major: CS.";
    let vector = nomic.encode_document(text).expect("encode");
    assert_eq!(vector.len(), 768);

    let state = common::test_state().await;
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
    assert_eq!(loaded.len(), 768);
    assert!(cosine(&vector, &loaded).unwrap() > 0.999);
}
