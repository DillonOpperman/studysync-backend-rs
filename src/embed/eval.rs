use std::collections::HashMap;
use std::time::Instant;

use serde::Deserialize;

use super::hashing::HashingEmbedder;
use super::{canonical_group_text, catalog, cosine, Embedder, ModelMetadata};

pub const EVAL_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/eval/studysync_embedding_eval.json"
));

#[derive(Debug, Deserialize)]
pub struct EvalDataset {
    pub version: u32,
    pub documents: Vec<EvalDocument>,
    pub queries: Vec<EvalQuery>,
    pub relevance: Vec<EvalLabel>,
    pub preferred_order: Vec<PreferredOrder>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct EvalDocument {
    pub id: String,
    pub title: String,
    pub subject: String,
    pub description: String,
    pub leader_name: String,
    pub leader_major: String,
}

#[derive(Debug, Deserialize)]
pub struct EvalQuery {
    pub id: String,
    pub text: String,
}

#[derive(Debug, Deserialize)]
pub struct EvalLabel {
    pub query_id: String,
    pub doc_id: String,
    pub label: u8,
}

#[derive(Debug, Deserialize)]
pub struct PreferredOrder {
    pub query_id: String,
    pub doc_ids: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct CandidateReport {
    pub metadata: ModelMetadata,
    pub document_count: usize,
    pub query_count: usize,
    pub labeled_pairs: usize,
    pub top1_relevance: f64,
    pub top5_recall: f64,
    pub ndcg_at_5: f64,
    pub pairwise_order_accuracy: f64,
    pub avg_query_encode_ms: f64,
    pub avg_doc_encode_ms: f64,
    pub load_ms: f64,
}

pub fn load_dataset() -> EvalDataset {
    serde_json::from_str(EVAL_JSON).expect("frozen StudySync eval JSON")
}

pub fn evaluate_embedder(embedder: &dyn Embedder, data: &EvalDataset) -> CandidateReport {
    evaluate_embedder_with_load(embedder, data, None)
}

pub fn evaluate_embedder_with_load(
    embedder: &dyn Embedder,
    data: &EvalDataset,
    load_ms: Option<f64>,
) -> CandidateReport {
    let load_start = Instant::now();
    let _ = embedder.metadata();
    let load_ms = load_ms.unwrap_or_else(|| load_start.elapsed().as_secs_f64() * 1000.0);

    let mut doc_ms = 0.0;
    let mut docs: HashMap<String, Vec<f32>> = HashMap::new();
    for doc in &data.documents {
        let text = canonical_group_text(
            &doc.title,
            &doc.subject,
            &doc.description,
            &doc.leader_name,
            &doc.leader_major,
        );
        let started = Instant::now();
        let vec = embedder.encode_document(&text).expect("document embedding");
        doc_ms += started.elapsed().as_secs_f64() * 1000.0;
        docs.insert(doc.id.clone(), vec);
    }

    let labels = labels_by_query(data);
    let mut top1_hits = 0.0;
    let mut top1_n = 0.0;
    let mut recall_sum = 0.0;
    let mut recall_n = 0.0;
    let mut ndcg_sum = 0.0;
    let mut ndcg_n = 0.0;
    let mut query_ms = 0.0;

    for query in &data.queries {
        let started = Instant::now();
        let qvec = embedder.encode_query(&query.text).expect("query embedding");
        query_ms += started.elapsed().as_secs_f64() * 1000.0;

        let mut ranked: Vec<(&str, f32)> = docs
            .iter()
            .map(|(id, vec)| (id.as_str(), cosine(&qvec, vec).unwrap_or(0.0)))
            .collect();
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let relevant = labels.get(query.id.as_str());
        let Some(rel) = relevant else {
            continue;
        };
        if let Some((top_id, _)) = ranked.first() {
            let label = *rel.get(*top_id).unwrap_or(&0);
            if label >= 2 {
                top1_hits += 1.0;
            }
            top1_n += 1.0;
        }
        let gold: Vec<&str> = rel
            .iter()
            .filter(|(_, grade)| **grade >= 2)
            .map(|(id, _)| id.as_str())
            .collect();
        if !gold.is_empty() {
            let hit = ranked
                .iter()
                .take(5)
                .filter(|(id, _)| gold.iter().any(|gold_id| gold_id == id))
                .count();
            recall_sum += hit as f64 / gold.len() as f64;
            recall_n += 1.0;
        }
        let gains: Vec<u8> = ranked
            .iter()
            .take(5)
            .map(|(id, _)| *rel.get(*id).unwrap_or(&0))
            .collect();
        ndcg_sum += ndcg(&gains, 5);
        ndcg_n += 1.0;
    }

    let pairwise = preferred_order_accuracy(embedder, data, &docs);

    CandidateReport {
        metadata: *embedder.metadata(),
        document_count: data.documents.len(),
        query_count: data.queries.len(),
        labeled_pairs: data.relevance.len(),
        top1_relevance: if top1_n == 0.0 {
            0.0
        } else {
            top1_hits / top1_n
        },
        top5_recall: if recall_n == 0.0 {
            0.0
        } else {
            recall_sum / recall_n
        },
        ndcg_at_5: if ndcg_n == 0.0 {
            0.0
        } else {
            ndcg_sum / ndcg_n
        },
        pairwise_order_accuracy: pairwise,
        avg_query_encode_ms: query_ms / data.queries.len().max(1) as f64,
        avg_doc_encode_ms: doc_ms / data.documents.len().max(1) as f64,
        load_ms,
    }
}

pub fn evaluate_all_candidates() -> Vec<CandidateReport> {
    let data = load_dataset();
    catalog::all_candidates()
        .into_iter()
        .map(|meta| {
            let embedder = HashingEmbedder::from_metadata(meta);
            evaluate_embedder(&embedder, &data)
        })
        .collect()
}

fn labels_by_query(data: &EvalDataset) -> HashMap<String, HashMap<String, u8>> {
    let mut out: HashMap<String, HashMap<String, u8>> = HashMap::new();
    for row in &data.relevance {
        out.entry(row.query_id.clone())
            .or_default()
            .insert(row.doc_id.clone(), row.label);
    }
    out
}

fn preferred_order_accuracy(
    embedder: &dyn Embedder,
    data: &EvalDataset,
    docs: &HashMap<String, Vec<f32>>,
) -> f64 {
    let queries: HashMap<&str, &str> = data
        .queries
        .iter()
        .map(|q| (q.id.as_str(), q.text.as_str()))
        .collect();
    let mut ok = 0.0;
    let mut n = 0.0;
    for spec in &data.preferred_order {
        let Some(text) = queries.get(spec.query_id.as_str()) else {
            continue;
        };
        let qvec = embedder.encode_query(text).expect("query");
        let scores: Vec<f32> = spec
            .doc_ids
            .iter()
            .filter_map(|id| docs.get(id).and_then(|v| cosine(&qvec, v).ok()))
            .collect();
        for pair in scores.windows(2) {
            n += 1.0;
            if pair[0] + 1e-6 >= pair[1] {
                ok += 1.0;
            }
        }
    }
    if n == 0.0 {
        0.0
    } else {
        ok / n
    }
}

fn ndcg(gains: &[u8], k: usize) -> f64 {
    let dcg = dcg_at(gains, k);
    let mut ideal = gains.to_vec();
    ideal.sort_unstable_by(|a, b| b.cmp(a));
    let idcg = dcg_at(&ideal, k);
    if idcg == 0.0 {
        0.0
    } else {
        dcg / idcg
    }
}

fn dcg_at(gains: &[u8], k: usize) -> f64 {
    gains
        .iter()
        .take(k)
        .enumerate()
        .map(|(i, gain)| {
            let rel = f64::from(*gain);
            (2.0_f64.powf(rel) - 1.0) / (i as f64 + 2.0).log2()
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embed::catalog;

    #[test]
    fn dataset_is_within_target_size() {
        let data = load_dataset();
        assert_eq!(data.version, 1);
        assert_eq!(data.documents.len(), 24);
        assert_eq!(data.queries.len(), 20);
        assert!(data.relevance.len() >= 50);
        assert!(data.relevance.len() <= 200);
        assert!(!data.preferred_order.is_empty());
        assert!(data.relevance.iter().all(|row| row.label <= 3));
    }

    #[test]
    fn historical_standin_reports_are_not_neural_results() {
        let reports = evaluate_all_candidates();
        assert_eq!(reports.len(), 4);
        assert_eq!(
            reports[0].metadata.exact_name,
            catalog::minilm_baseline().exact_name
        );
        assert_eq!(
            reports[1].metadata.exact_name,
            catalog::qwen3_embedding_06b().exact_name
        );
        assert_eq!(reports[2].metadata.exact_name, catalog::bge_m3().exact_name);
        assert_eq!(
            reports[3].metadata.exact_name,
            catalog::multilingual_e5_large_instruct().exact_name
        );
        for report in &reports {
            assert!(report.top1_relevance >= 0.0 && report.top1_relevance <= 1.0);
            assert!(report.top5_recall >= 0.0 && report.top5_recall <= 1.0);
            assert!(report.ndcg_at_5 >= 0.0 && report.ndcg_at_5 <= 1.0);
            assert_eq!(report.document_count, 24);
            assert_eq!(report.labeled_pairs, load_dataset().relevance.len());
        }
        assert!(
            reports.iter().all(|row| !row.metadata.selected),
            "historical stand-in catalog is not the production selection"
        );
        assert_eq!(catalog::production().model_id, catalog::PRODUCTION_MODEL_ID);
        assert!(catalog::production().selected);
        for report in &reports {
            eprintln!(
                "STAND-IN (not neural) {} dim={} top1={:.3} recall5={:.3} ndcg5={:.3} order={:.3} q_ms={:.3} d_ms={:.3} load_ms={:.3}",
                report.metadata.exact_name,
                report.metadata.dimensions,
                report.top1_relevance,
                report.top5_recall,
                report.ndcg_at_5,
                report.pairwise_order_accuracy,
                report.avg_query_encode_ms,
                report.avg_doc_encode_ms,
                report.load_ms
            );
        }
    }
}
