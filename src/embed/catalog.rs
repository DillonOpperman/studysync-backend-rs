//! Exact candidate metadata. Names are Hugging Face repo ids, not nicknames.

use super::{ModelMetadata, QueryStyle};

/// Production vector width after model selection (`Qwen/Qwen3-Embedding-0.6B`).
pub const PRODUCTION_EMBEDDING_DIM: usize = 1024;

/// Versioned production model id stored beside vectors.
pub const PRODUCTION_MODEL_ID: &str = "qwen3-embedding-0.6b:v1";

/// Hugging Face weights revision pinned for the selected production model.
pub const QWEN3_EMBEDDING_06B_REVISION: &str = "c935f2d3fce3e2337b4a66ac3130faa6dada3218";

pub fn minilm_baseline() -> ModelMetadata {
    ModelMetadata {
        exact_name: "sentence-transformers/all-MiniLM-L6-v2",
        model_id: "all-minilm-l6-v2:legacy",
        revision: "8b3219a92973c328a8e22fadcfa821b5dc75636a",
        license: "Apache-2.0 (declared on the Hugging Face model card)",
        dimensions: 384,
        context_length: Some(256),
        intended_use: "Legacy StudySync English sentence similarity baseline",
        languages: "English-centric",
        inference_library: "Candle BertModel + official MiniLM safetensors (mean-pool, L2)",
        model_size: "~80–90 MB ONNX / ~22M parameters",
        runtime_requirements: "Low CPU RAM; typical laptop CPU inference",
        source_url: "https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2",
        commercially_redistributable: true,
        query_style: QueryStyle::Raw,
        selected: false,
    }
}

pub fn qwen3_embedding_06b() -> ModelMetadata {
    ModelMetadata {
        exact_name: "Qwen/Qwen3-Embedding-0.6B",
        model_id: PRODUCTION_MODEL_ID,
        revision: QWEN3_EMBEDDING_06B_REVISION,
        license: "Apache-2.0",
        dimensions: 1024,
        context_length: Some(32_768),
        intended_use: "Multilingual retrieval with instruction-aware queries",
        languages: "100+ languages (model card)",
        inference_library: "Candle Qwen3 transformer (CPU F32, last-token pool, L2) + hf-hub pinned revision",
        model_size: "~0.6B parameters / ~1.2 GB weights (not stored in git)",
        runtime_requirements:
            "Hundreds of MB to ~1.5 GB RAM for official weights; spawn_blocking for inference",
        source_url: "https://huggingface.co/Qwen/Qwen3-Embedding-0.6B",
        commercially_redistributable: true,
        query_style: QueryStyle::QwenInstruct,
        selected: true,
    }
}

pub fn bge_m3() -> ModelMetadata {
    ModelMetadata {
        exact_name: "BAAI/bge-m3",
        model_id: "bge-m3:eval",
        revision: "6892b95fed65c899a30896eb40d619ae284d0455",
        license: "MIT",
        dimensions: 1024,
        context_length: Some(8192),
        intended_use: "Multilingual dense (and optional sparse/ColBERT) retrieval",
        languages: "Multilingual",
        inference_library: "FlagEmbedding / sentence-transformers (evaluation only)",
        model_size: "~560M parameters",
        runtime_requirements: "Higher than MiniLM; dense+sparse path adds deployment complexity",
        source_url: "https://huggingface.co/BAAI/bge-m3",
        commercially_redistributable: true,
        query_style: QueryStyle::Raw,
        selected: false,
    }
}

pub fn multilingual_e5_large_instruct() -> ModelMetadata {
    ModelMetadata {
        exact_name: "intfloat/multilingual-e5-large-instruct",
        model_id: "multilingual-e5-large-instruct:eval",
        revision: "b9fbfc1b8c70709e5371d98135d214b005562b7a",
        license: "MIT",
        dimensions: 1024,
        context_length: Some(512),
        intended_use: "Instruct-style multilingual embedding (query vs passage prefixes)",
        languages: "100 languages (XLM-R family; model card)",
        inference_library: "sentence-transformers / transformers (evaluation only)",
        model_size: "~560M parameters",
        runtime_requirements: "Similar to BGE-M3; 512-token context is tighter than Qwen/BGE",
        source_url: "https://huggingface.co/intfloat/multilingual-e5-large-instruct",
        commercially_redistributable: true,
        query_style: QueryStyle::E5Instruct,
        selected: false,
    }
}

pub fn all_candidates() -> [ModelMetadata; 4] {
    [
        minilm_baseline(),
        qwen3_embedding_06b(),
        bge_m3(),
        multilingual_e5_large_instruct(),
    ]
}

pub fn production() -> ModelMetadata {
    qwen3_embedding_06b()
}

pub fn fallback() -> ModelMetadata {
    minilm_baseline()
}
