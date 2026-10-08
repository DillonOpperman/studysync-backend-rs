# StudySync embedding bake-off (frozen eval)

Dataset: `eval/studysync_embedding_eval.json` — 24 documents, 20 queries, 108 labeled pairs (target mentioned 105; file is unchanged), relevance 0–3.

Hardware: Windows CPU, Candle F32, `cargo test --release --test model_eval -- --ignored`. Same warm-up (1 cold query, 20 warm singles), same batches, same fixture.

Hashed n-gram numbers below are **historical stand-in only**. They are not neural results.

## Historical stand-in (not neural)

| Model | dim | top-1 | recall@5 | nDCG@5 | pairwise |
| --- | --- | --- | --- | --- | --- |
| MiniLM | 384 | 0.950 | 0.947 | 0.900 | 0.600 |
| Qwen3-Embedding-0.6B | 1024 | 0.950 | 0.934 | 0.913 | 0.600 |
| bge-m3 | 1024 | 0.950 | 0.908 | 0.905 | 0.600 |
| multilingual-e5-large-instruct | 1024 | 0.950 | 0.934 | 0.913 | 0.600 |

## Previous neural Qwen (not re-run in this bake-off)

`Qwen/Qwen3-Embedding-0.6B` @ `c935f2d3fce3e2337b4a66ac3130faa6dada3218`: top-1 0.900, recall@5 0.855, nDCG@5 0.884, pairwise 0.500, warm ~308 ms, ~1386 MB RAM. Too heavy for this CPU server.

## Neural lightweight bake-off

| | MiniLM | BGE-small-en-v1.5 | Nomic-embed-text-v1.5 |
| --- | --- | --- | --- |
| Exact name | sentence-transformers/all-MiniLM-L6-v2 | BAAI/bge-small-en-v1.5 | nomic-ai/nomic-embed-text-v1.5 |
| Revision | `8b3219a92973c328a8e22fadcfa821b5dc75636a` | `5c38ec7c405ec4b44b94cc5a9bb96e735b38267a` | `e9b6763023c676ca8431644204f50c2b100d9aab` |
| License | Apache-2.0 | MIT | Apache-2.0 |
| Dimensions | 384 | 384 | 768 |
| Weights | 86.7 MB | 127.3 MB | 521.6 MB |
| Max length used | 256 | 512 | 512 (arch supports 2048 here; card 8192) |
| Query format | raw | `Represent this sentence for searching relevant passages: ` | `search_query: ` |
| Document format | raw | raw | `search_document: ` |
| Pooling | mean + L2 | mean + L2 | mean + unparameterized LN + L2 |
| Runtime | Candle BertModel CPU F32 | Candle BertModel CPU F32 | Candle NomicBert (RoPE/SwiGLU) CPU F32 |
| Load (cached) | 61.5 ms | 82.3 ms | 274.4 ms |
| Cold | 4.7 ms | 16.1 ms | 42.5 ms |
| Warm / p50 / p95 | 4.5 / 4.8 / 5.1 ms | 14.2 / 15.1 / 15.9 ms | 40.0 / 42.2 / 45.7 ms |
| Batch 10/50/100 | 14.3 / 63.1 / 136.9 ms | 61.3 / 325.2 / 669.9 ms | 142.5 / 732.4 / 1317.0 ms |
| Avg doc encode | 10.6 ms | 25.4 ms | 65.8 ms |
| top-1 | 0.850 | 0.900 | **0.950** |
| recall@5 | **0.921** | 0.877 | 0.882 |
| nDCG@5 | 0.893 | 0.915 | **0.928** |
| pairwise | 0.700 | 0.700 | **0.800** |
| related vs unrelated cosine | 0.554 / 0.133 | 0.654 / 0.460 | 0.584 / 0.418 |
| Approx RAM | ~167 MB | ~207 MB | ~602 MB |
| Windows/Candle | builds; LNK4098 only | builds; LNK4098 only | builds after additive mask (no F32 `where_cond`); LNK4098 only |

## Recommendation

**Production:** `nomic-ai/nomic-embed-text-v1.5` @ `e9b6763023c676ca8431644204f50c2b100d9aab` (`nomic-embed-text-v1.5:v1`, 768-d, Apache-2.0).

It improves nDCG@5 from MiniLM 0.893 to **0.928** (+3.9% relative), pairwise from 0.700 to **0.800**, and top-1 from 0.850 to **0.950**, with p95 **45.7 ms** and ~**602 MB** RAM — far below Qwen’s ~308 ms / ~1.4 GB. Candle CPU inference is reliable after the mask fix.

**Fallback:** MiniLM — best recall@5 (0.921), best related/unrelated separation, ~5 ms p95, ~167 MB, standard BERT.

BGE-small is not selected: better nDCG than MiniLM but worse recall@5, weaker cosine separation, and it does not beat Nomic on ranking.

The 24/20/108 set is small; treat gaps as StudySync-specific signal, not a generic MTEB ranking.

## Official Transformers verification (Nomic)

Pinned revision `e9b6763023c676ca8431644204f50c2b100d9aab`. Hugging Face `AutoModel` + `BertTokenizerFast` vs Candle NomicBert on seven fixed strings (short queries, study-group copy, a longer document, related calculus vs unrelated organic chemistry).

Contract checked: BERT `[CLS]`/`[SEP]`/`[PAD]` (101/102/0), `search_query: ` / `search_document: ` prefixes, attention-mask mean pooling, unparameterized layer-norm (`eps=1e-5`), L2, 768-d.

| Check | Result |
| --- | --- |
| Token ids (unpadded) | Exact match |
| Vector cosine vs HF | 1.000000 |
| Max abs component | 2.35e-7 |
| Pairwise cosine delta | ≤ 2.4e-7 |

Differences are float32 noise. Query/document embeddings share that revision and pooling. Python is verification-only.
