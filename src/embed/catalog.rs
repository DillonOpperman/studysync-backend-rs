//! Exact candidate metadata. Names are Hugging Face repo ids, not nicknames.

use super::{ModelMetadata, QueryStyle};

/// Production vector width after the lightweight neural bake-off (Nomic 768-d).
/// MiniLM/BGE 384-d and historical Qwen 1024-d rows live in separate tables.
pub const PRODUCTION_EMBEDDING_DIM: usize = 768;

/// Versioned production model id stored beside vectors.
pub const PRODUCTION_MODEL_ID: &str = "nomic-embed-text-v1.5:v1";

pub const QWEN3_EMBEDDING_06B_REVISION: &str = "c935f2d3fce3e2337b4a66ac3130faa6dada3218";
pub const MINILM_REVISION: &str = "8b3219a92973c328a8e22fadcfa821b5dc75636a";
/// Hugging Face snapshot that includes `model.safetensors` and the ONNX export.
pub const BGE_SMALL_EN_V15_REVISION: &str = "5c38ec7c405ec4b44b94cc5a9bb96e735b38267a";
/// Current published tree of nomic-embed-text-v1.5 (v5 Transformers config + weights).
pub const NOMIC_EMBED_TEXT_V15_REVISION: &str = "e9b6763023c676ca8431644204f50c2b100d9aab";

pub fn minilm_baseline() -> ModelMetadata {
    ModelMetadata {
        exact_name: "sentence-transformers/all-MiniLM-L6-v2",
        model_id: "all-minilm-l6-v2:legacy",
        revision: MINILM_REVISION,
        license: "Apache-2.0 (declared on the Hugging Face model card)",
        dimensions: 384,
        context_length: Some(256),
        intended_use: "Legacy StudySync English sentence similarity baseline",
        languages: "English-centric",
        inference_library: "Candle BertModel + official MiniLM safetensors (mean-pool, L2)",
        model_size: "~90 MB safetensors / ~22M parameters",
        runtime_requirements: "Low CPU RAM; typical laptop CPU inference",
        source_url: "https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2",
        commercially_redistributable: true,
        query_style: QueryStyle::Raw,
        selected: false,
    }
}

pub fn bge_small_en_v15() -> ModelMetadata {
    ModelMetadata {
        exact_name: "BAAI/bge-small-en-v1.5",
        model_id: "bge-small-en-v1.5:v1",
        revision: BGE_SMALL_EN_V15_REVISION,
        license: "MIT",
        dimensions: 384,
        context_length: Some(512),
        intended_use: "English retrieval with a query instruction prefix",
        languages: "English",
        inference_library:
            "Candle BertModel + official safetensors (mean-pool, L2); query instruction",
        model_size: "~33 MB safetensors / ~33M parameters",
        runtime_requirements: "Similar to MiniLM; CPU F32 Candle",
        source_url: "https://huggingface.co/BAAI/bge-small-en-v1.5",
        commercially_redistributable: true,
        query_style: QueryStyle::BgeInstruct,
        selected: false,
    }
}

pub fn nomic_embed_text_v15() -> ModelMetadata {
    ModelMetadata {
        exact_name: "nomic-ai/nomic-embed-text-v1.5",
        model_id: "nomic-embed-text-v1.5:v1",
        revision: NOMIC_EMBED_TEXT_V15_REVISION,
        license: "Apache-2.0",
        dimensions: 768,
        context_length: Some(2048),
        intended_use: "English/code retrieval with search_query/search_document prefixes",
        languages: "English-centric (long context)",
        inference_library:
            "Candle NomicBert (RoPE, SwiGLU) + mean-pool, unparameterized layer-norm, L2",
        model_size: "~0.55 GB safetensors / ~137M parameters",
        runtime_requirements: "Higher than MiniLM/BGE-small; CPU F32 Candle; no ONNX Runtime",
        source_url: "https://huggingface.co/nomic-ai/nomic-embed-text-v1.5",
        commercially_redistributable: true,
        query_style: QueryStyle::NomicSearch,
        selected: true,
    }
}

pub fn qwen3_embedding_06b() -> ModelMetadata {
    ModelMetadata {
        exact_name: "Qwen/Qwen3-Embedding-0.6B",
        model_id: "qwen3-embedding-0.6b:v1",
        revision: QWEN3_EMBEDDING_06B_REVISION,
        license: "Apache-2.0",
        dimensions: 1024,
        context_length: Some(32_768),
        intended_use: "Previous heavyweight candidate (not the lightweight bake-off)",
        languages: "100+ languages (model card)",
        inference_library:
            "Candle Qwen3 transformer (CPU F32, last-token pool, L2) + hf-hub pinned revision",
        model_size: "~0.6B parameters / ~1.2 GB weights (not stored in git)",
        runtime_requirements:
            "Hundreds of MB to ~1.5 GB RAM for official weights; spawn_blocking for inference",
        source_url: "https://huggingface.co/Qwen/Qwen3-Embedding-0.6B",
        commercially_redistributable: true,
        query_style: QueryStyle::QwenInstruct,
        selected: false,
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

/// Historical hashed-stand-in catalog (not neural bake-off participants).
pub fn all_candidates() -> [ModelMetadata; 4] {
    [
        minilm_baseline(),
        qwen3_embedding_06b(),
        bge_m3(),
        multilingual_e5_large_instruct(),
    ]
}

pub fn production() -> ModelMetadata {
    nomic_embed_text_v15()
}

pub fn fallback() -> ModelMetadata {
    minilm_baseline()
}
