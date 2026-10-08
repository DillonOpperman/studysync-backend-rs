use std::collections::HashMap;
use std::sync::OnceLock;

use super::{Embedder, EmbeddingError, ModelMetadata, QueryStyle};

/// Deterministic hashed n-gram encoder with a StudySync synonym lexicon.
///
/// This is the in-process encoder used for tests and local development. It is
/// **not** the official Qwen/BGE/E5/MiniLM neural weights. Candidate evaluation
/// still uses each model's documented query/document formatting and dimensions
/// so StudySync ranking can be compared without shipping gigabytes of weights.
#[derive(Clone)]
pub struct HashingEmbedder {
    metadata: ModelMetadata,
}

impl HashingEmbedder {
    pub fn from_metadata(metadata: ModelMetadata) -> Self {
        Self { metadata }
    }

    fn vectorize(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        let dim = self.metadata.dimensions;
        if dim == 0 {
            return Err(EmbeddingError::Unavailable(
                "embedding dimension must be positive".into(),
            ));
        }
        let tokens = expand_tokens(&tokenize(text));
        if tokens.is_empty() {
            return Err(EmbeddingError::EmptyInput);
        }
        let mut vec = vec![0.0f32; dim];
        for token in &tokens {
            if let Some(axis) = concept_axis(token) {
                let idx = axis % dim;
                vec[idx] += 3.0;
            }
            let h = fnv1a64(token.as_bytes());
            let idx = (h as usize) % dim;
            let sign = if h & 1 == 0 { 1.0 } else { -1.0 };
            vec[idx] += sign;
            if token.len() > 3 {
                let bigram = format!("{token}#");
                let h2 = fnv1a64(bigram.as_bytes());
                vec[(h2 as usize) % dim] += 0.5;
            }
        }
        l2_normalize(&mut vec);
        Ok(vec)
    }
}

impl Embedder for HashingEmbedder {
    fn metadata(&self) -> &ModelMetadata {
        &self.metadata
    }

    fn encode_query(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        if text.trim().is_empty() {
            return Err(EmbeddingError::EmptyInput);
        }
        let formatted = self.metadata.query_style.format_query(text);
        self.vectorize(&formatted)
    }

    fn encode_document(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        if text.trim().is_empty() {
            return Err(EmbeddingError::EmptyInput);
        }
        let formatted = self.metadata.query_style.format_document(text);
        self.vectorize(&formatted)
    }
}

pub fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|tok| tok.len() > 1)
        .map(ToOwned::to_owned)
        .collect()
}

fn expand_tokens(tokens: &[String]) -> Vec<String> {
    let map = synonyms();
    let mut out = Vec::with_capacity(tokens.len() * 2);
    for token in tokens {
        out.push(token.clone());
        if let Some(extra) = map.get(token.as_str()) {
            out.extend(extra.iter().map(|s| (*s).to_string()));
        }
    }
    out
}

fn synonyms() -> &'static HashMap<&'static str, Vec<&'static str>> {
    static MAP: OnceLock<HashMap<&'static str, Vec<&'static str>>> = OnceLock::new();
    MAP.get_or_init(|| {
        HashMap::from([
            ("networks", vec!["networking", "network", "tcp"]),
            ("networking", vec!["networks", "network"]),
            ("network", vec!["networks", "networking"]),
            ("security", vec!["cybersecurity", "firewall"]),
            ("python", vec!["programming", "coding"]),
            ("java", vec!["programming", "coding"]),
            ("programming", vec!["coding", "code"]),
            ("coding", vec!["programming"]),
            (
                "calculus",
                vec!["calc", "math", "mathematics", "integration"],
            ),
            ("calc", vec!["calculus"]),
            ("discrete", vec!["combinatorics", "proofs", "algorithms"]),
            ("organic", vec!["orgo", "chemistry"]),
            ("orgo", vec!["organic", "chemistry"]),
            ("chemistry", vec!["chem"]),
            ("chem", vec!["chemistry"]),
            ("machine", vec!["ml"]),
            ("learning", vec!["ml"]),
            ("ml", vec!["machine", "learning"]),
            ("exam", vec!["exams", "test", "midterm"]),
            ("homework", vec!["assignment", "problem"]),
            ("project", vec!["capstone", "partners"]),
            ("writing", vec!["essay", "report"]),
            ("essay", vec!["writing"]),
            ("biology", vec!["science", "bio"]),
            ("physics", vec!["science"]),
            ("statistics", vec!["stats", "probability"]),
            ("stats", vec!["statistics"]),
            ("algebra", vec!["matrices", "mathematics"]),
            ("algorithms", vec!["discrete", "complexity"]),
            ("redes", vec!["networks", "networking"]),
            ("computadoras", vec!["computer", "computers"]),
            ("grupo", vec!["group", "study"]),
            ("spanish", vec!["espanol"]),
            ("history", vec!["essay", "writing"]),
            ("structures", vec!["trees", "graphs", "programming"]),
            ("tuesday", vec!["weekday"]),
            ("evening", vec!["night"]),
        ])
    })
}

fn concept_axis(token: &str) -> Option<usize> {
    Some(match token {
        "computer" | "computers" | "cs" => 0,
        "networks" | "networking" | "network" | "redes" => 1,
        "security" | "cybersecurity" => 2,
        "python" | "programming" | "coding" | "java" => 3,
        "calculus" | "calc" | "integration" => 4,
        "discrete" | "proofs" | "combinatorics" => 5,
        "organic" | "orgo" | "chemistry" | "chem" => 6,
        "machine" | "learning" | "ml" => 7,
        "exam" | "exams" | "midterm" => 8,
        "homework" | "assignment" => 9,
        "project" | "capstone" | "partners" => 10,
        "writing" | "essay" | "report" => 11,
        "biology" | "bio" => 12,
        "physics" => 13,
        "statistics" | "stats" => 14,
        "algebra" | "matrices" => 15,
        "history" => 16,
        "spanish" | "espanol" => 17,
        "tuesday" => 18,
        "evening" | "night" => 19,
        "science" => 20,
        "mathematics" | "math" => 21,
        "algorithms" => 22,
        "structures" | "trees" | "graphs" => 23,
        _ => return None,
    })
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    hash
}

fn l2_normalize(values: &mut [f32]) {
    let norm = values
        .iter()
        .map(|v| (*v as f64) * (*v as f64))
        .sum::<f64>()
        .sqrt();
    if norm > 0.0 {
        for value in values {
            *value = (*value as f64 / norm) as f32;
        }
    }
}

impl QueryStyle {
    pub fn format_query(self, text: &str) -> String {
        let trimmed = text.trim();
        match self {
            Self::Raw => trimmed.to_string(),
            Self::QwenInstruct => format!(
                "Instruct: Given a study-group search query, retrieve relevant study groups\nQuery:{trimmed}"
            ),
            Self::E5Instruct => format!(
                "Instruct: Given a study-group search query, retrieve relevant study groups\nQuery: {trimmed}"
            ),
        }
    }

    pub fn format_document(self, text: &str) -> String {
        let trimmed = text.trim();
        match self {
            Self::Raw | Self::QwenInstruct => trimmed.to_string(),
            Self::E5Instruct => format!("passage: {trimmed}"),
        }
    }
}
