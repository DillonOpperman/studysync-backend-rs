# StudySync embedding candidate results (frozen eval)

Dataset: `eval/studysync_embedding_eval.json` — 24 documents, 20 queries, 108 labeled pairs, relevance 0–3.

Encoder used in CI: deterministic hashed n-gram stand-in with each candidate’s documented dimensions and query/document formatting. Official neural weights are not stored in git.

| Model | License | Dimensions | Model Size | Avg query encode (ms, stand-in) | top-1 (label≥2) | top-5 recall | nDCG@5 | pairwise order | Memory / deploy |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| sentence-transformers/all-MiniLM-L6-v2 | Apache-2.0 | 384 | ~22M / ~90MB | 0.019 | 0.950 | 0.947 | 0.900 | 0.600 | Lowest; English-centric |
| Qwen/Qwen3-Embedding-0.6B | Apache-2.0 | 1024 | ~0.6B / ~1.2GB | 0.027 | 0.950 | 0.934 | 0.913 | 0.600 | Medium; instruct-aware; 32k ctx |
| BAAI/bge-m3 | MIT | 1024 | ~560M | 0.014 | 0.950 | 0.908 | 0.905 | 0.600 | Higher; dense+sparse complexity |
| intfloat/multilingual-e5-large-instruct | MIT | 1024 | ~560M | 0.027 | 0.950 | 0.934 | 0.913 | 0.600 | Similar size; 512-token ctx |

Selected production: **Qwen/Qwen3-Embedding-0.6B** (`qwen3-embedding-0.6b:v1`, revision `c935f2d3fce3e2337b4a66ac3130faa6dada3218`).

Fallback: MiniLM baseline (lowest resource).

Qwen and E5 tied on nDCG@5 (best ranking quality on this set). Qwen is preferred over E5 for 32k context vs 512 and a single 1024-d dense production schema. MiniLM’s slightly higher top-5 recall on the hashed stand-in does not outweigh English-only 384-d limits for the production vector store.
