# Phase 7 embedding evaluation

Frozen dataset: `eval/studysync_embedding_eval.json` (synthetic, de-identified).

Corpus text for groups (legacy fields, unchanged while comparing models):

`{title}. Subject: {subject}. {description}. Leader: {leader_name}. Major: {leader_major}.`

Matching formula is unchanged:

`0.50 * group_subject + 0.20 * leader_subject_similarity + 0.20 * schedule_overlap + 0.10 * size_match`

with leader / active-member / pending-request exclusion, 25% threshold, top 10, and `createNew` when nothing passes.

Schedule overlap is structured (including overnight wrap and IANA timezones). Embedding similarity never decides availability.

## Failure behavior

- Recommendation: if the embedding backend is down, structured scores still run; `group_subject` falls back to exact/substring only (`semanticAvailable: false`).
- Semantic search: HTTP 503 `semantic_unavailable`. No empty or random vectors.

## Embedding persistence

Groups are re-encoded when title, subject, description, or included leader name/major change (`source_hash` of canonical text + `model_id`). Users are re-encoded when name, major, or courses change. Unrelated profile/media updates do not embed.

Vectors live in `entity_embeddings` keyed by `(entity_kind, entity_id, model_id)` so old and new models can coexist. Production width is 1024 (`vector(1024)`), driven by Qwen3-Embedding-0.6B, not MiniLM’s 384.

## Real Qwen weights

Production inference is a Candle Qwen3 transformer (CPU F32) loading `Qwen/Qwen3-Embedding-0.6B` at revision `c935f2d3fce3e2337b4a66ac3130faa6dada3218`. fastembed/ONNX Runtime is not used (MSVC 2019 cannot link `ort` on this builder).

- First process start with `EMBEDDING_BACKEND=qwen` downloads tokenizer + safetensors into `EMBEDDING_CACHE_DIR` (default `.fastembed_cache`). Later starts reuse the cache; weights are not downloaded again if the snapshot is present.
- Production can pre-provision the same Hugging Face snapshot under that cache (hf-hub layout: `models--Qwen--Qwen3-Embedding-0.6B/snapshots/<revision>/`). Verify revision by confirming the snapshot directory name is exactly `c935f2d3fce3e2337b4a66ac3130faa6dada3218`; the loader refuses any other folder name (no silent `main`).
- `EMBEDDING_BACKEND=hash` is rejected. There is no production path that emits hashed stand-in vectors.
- `EMBEDDING_BACKEND=unavailable` keeps structured recommendation and returns 503 for semantic search. Startup with `qwen` fails clearly if weights cannot load.
- Default `cargo test` does not download weights. Neural eval (fails, does not fake-pass, if weights are missing): `cargo test --release --test model_eval -- --ignored --nocapture`.
