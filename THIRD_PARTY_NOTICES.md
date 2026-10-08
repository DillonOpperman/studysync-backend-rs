# Third-party notices (embedding models)

StudySync does not vendor embedding weight files in this git repository. Operators must download pinned revisions from the upstream source at deploy time.

This product is intended for commercial/public release. Licenses below were read from the Hugging Face model cards (not from a copy of each full upstream repository). Confirm the license file at the pinned revision before redistribution.

## Selected production model

| Field | Value |
| --- | --- |
| Exact name | `nomic-ai/nomic-embed-text-v1.5` |
| Version id | `nomic-embed-text-v1.5:v1` |
| Revision | `e9b6763023c676ca8431644204f50c2b100d9aab` |
| License | Apache License 2.0 |
| Source | https://huggingface.co/nomic-ai/nomic-embed-text-v1.5 |
| Dimensions | 768 (Matryoshka capable; StudySync stores full 768) |
| Context | 8192 on the model card; this runtime truncates at 512 (RoPE tables sized to 2048) |
| Pooling | mean-pool, unparameterized layer-norm, L2 |
| Query / document | `search_query: ` / `search_document: ` |
| Runtime | Candle NomicBert CPU F32 (RoPE, fused QKV, SwiGLU) |
| Notice | Redistribution of Apache-2.0 materials requires retaining copyright, license, and NOTICE files from the upstream package. This repository does not copy those files; include them beside downloaded weights at deploy time. |

## Fallback model (not the production default)

| Field | Value |
| --- | --- |
| Exact name | `sentence-transformers/all-MiniLM-L6-v2` |
| Version id | `all-minilm-l6-v2:legacy` |
| Revision | `8b3219a92973c328a8e22fadcfa821b5dc75636a` |
| License | Apache-2.0 (declared on the Hugging Face model card) |
| Source | https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2 |
| Dimensions | 384 |
| Pooling | mean-pool, L2 |
| Query / document | raw / raw |
| Role | Lightweight fallback; best recall@5 and lowest latency on the frozen StudySync set |

## Evaluated in the lightweight bake-off but not selected

### `BAAI/bge-small-en-v1.5`

- Revision: `5c38ec7c405ec4b44b94cc5a9bb96e735b38267a`
- License: MIT (FlagEmbedding / model card: commercial use permitted)
- Dimensions: 384
- Query instruction: `Represent this sentence for searching relevant passages: `
- Documents: no instruction
- Pooling: mean-pool, L2
- Source: https://huggingface.co/BAAI/bge-small-en-v1.5
- Not selected: nDCG@5 below Nomic; recall@5 below MiniLM; weaker related/unrelated cosine separation

### `Qwen/Qwen3-Embedding-0.6B` (previous heavyweight candidate)

- Revision: `c935f2d3fce3e2337b4a66ac3130faa6dada3218`
- License: Apache-2.0
- Dimensions: 1024
- Not selected for production: ~300 ms/query and ~1.4 GB RAM on this Windows CPU host

## Runtime

Weights download into `EMBEDDING_CACHE_DIR` (default `.fastembed_cache`) via Hugging Face Hub at the pinned revision. The loader refuses a snapshot directory that is not that commit. Pre-provision production by copying `models--nomic-ai--nomic-embed-text-v1.5/snapshots/e9b6763023c676ca8431644204f50c2b100d9aab/` into the cache. ONNX Runtime is not used.

The hashed n-gram encoder is unit-test-only. `EMBEDDING_BACKEND=hash` is rejected.
