# StudySync embedding candidate results (frozen eval)

Dataset: `eval/studysync_embedding_eval.json` — 24 documents, 20 queries, 108 labeled pairs, relevance 0–3.

Neural runs used official tokenizer + safetensors (not Git). Command:

`cargo test --release --test model_eval -- --ignored --nocapture`

## Historical stand-in (not neural)

Encoder: deterministic hashed n-gram stand-in with each candidate’s documented dimensions and query/document formatting. **Do not treat these as Qwen or MiniLM model quality.**

| Model | License | Dimensions | Avg query encode (ms, stand-in) | top-1 (label≥2) | top-5 recall | nDCG@5 | pairwise order |
| --- | --- | --- | --- | --- | --- | --- | --- |
| sentence-transformers/all-MiniLM-L6-v2 | Apache-2.0 | 384 | 0.019 | 0.950 | 0.947 | 0.900 | 0.600 |
| Qwen/Qwen3-Embedding-0.6B | Apache-2.0 | 1024 | 0.027 | 0.950 | 0.934 | 0.913 | 0.600 |
| BAAI/bge-m3 | MIT | 1024 | 0.014 | 0.950 | 0.908 | 0.905 | 0.600 |
| intfloat/multilingual-e5-large-instruct | MIT | 1024 | 0.027 | 0.950 | 0.934 | 0.913 | 0.600 |

## Neural MiniLM vs neural Qwen (this machine, Candle CPU F32, release)

| Field | MiniLM | Qwen3-Embedding-0.6B |
| --- | --- | --- |
| Exact name | `sentence-transformers/all-MiniLM-L6-v2` | `Qwen/Qwen3-Embedding-0.6B` |
| Revision | `8b3219a92973c328a8e22fadcfa821b5dc75636a` | `c935f2d3fce3e2337b4a66ac3130faa6dada3218` |
| Runtime | Candle `BertModel` mean-pool + L2 | Candle Qwen3 last-token pool + L2, instruct queries |
| Dimensions | 384 | 1024 |
| License | Apache-2.0 (model card) | Apache-2.0 |
| Load time | 6407 ms | 66227 ms |
| Avg query encode | 4.9 ms | 300.1 ms |
| Avg document encode | 11.0 ms | 365.6 ms |
| Cold single query | (included in MiniLM query avg) | 302.3 ms |
| Warm single query | (included in MiniLM query avg) | 307.7 ms |
| Batch 10 | — | 1796.1 ms |
| Batch 50 | — | 8053.3 ms |
| Batch 100 | — | 15124.2 ms |
| Weight files | MiniLM safetensors | 1136 MB |
| Approx RAM | low hundreds of MB | ~1386 MB (weights + ~250 MB working set estimate) |
| top-1 (label≥2) | 0.850 | **0.900** |
| top-5 recall | **0.921** | 0.855 |
| nDCG@5 | **0.893** | 0.884 |
| pairwise order | **0.700** | 0.500 |

Normalized cosine: Qwen same-text document cosine > 0.999, L2 norm ≈ 1, related networking document ranked above organic chemistry. pgvector save→reload cosine > 0.999 at 1024-d.

## StudySync production choice

Matching formula unchanged: `0.50 * group_subject + 0.20 * leader_subject_similarity + 0.20 * schedule_overlap + 0.10 * size_match`.

On this frozen **English campus** set, MiniLM is the stronger ranker (recall@5, nDCG@5, pairwise). Qwen is stronger on top-1 and is already the 1024-d production schema (`qwen3-embedding-0.6b:v1`).

**Production model:** `Qwen/Qwen3-Embedding-0.6B` @ `c935f2d3fce3e2337b4a66ac3130faa6dada3218` — kept for 1024-d pgvector compatibility, multilingual headroom, and top-1. Not justified by the hashed stand-in; justified by the neural run plus operational schema.

**Fallback:** MiniLM (faster, better nDCG/recall/pairwise on this set). Not a silent hashed fallback.

**Legacy baseline:** MiniLM (`all-minilm-l6-v2:legacy`).

BGE-M3 and E5 were not re-run as neural models in this pass (still evaluation-only in the catalog).
