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
- Startup with `EMBEDDING_BACKEND=nomic` (or minilm/bge/qwen) fails clearly if weights cannot load. There is no hashed stand-in path.

## Embedding persistence

Groups are re-encoded when title, subject, description, or included leader name/major change (`source_hash` of canonical text + `model_id`). Users are re-encoded when name, major, or courses change.

Vectors are stored in dimension-specific tables so incompatible widths never share a pgvector column:

- `entity_embeddings_384` — MiniLM / BGE-small
- `entity_embeddings_768` — production Nomic
- `entity_embeddings_1024` — historical Qwen rows (migration 0012 renamed the original table)

ANN search always filters by `model_id` on the table that matches the query vector width.

## Production model

`nomic-ai/nomic-embed-text-v1.5` revision `e9b6763023c676ca8431644204f50c2b100d9aab`, Apache-2.0, 768-d.

- Query: `search_query: {text}`
- Document: `search_document: {text}`
- Pooling: mean-pool, unparameterized layer-norm, L2
- Runtime: Candle NomicBert CPU F32
- Default `EMBEDDING_BACKEND=nomic`. Cache: `EMBEDDING_CACHE_DIR` (default `.fastembed_cache`).
- First download only; later starts reuse the snapshot folder named for that revision. The loader refuses any other folder name (no silent `main`).
- Provision production by placing the Hub snapshot at `models--nomic-ai--nomic-embed-text-v1.5/snapshots/e9b6763023c676ca8431644204f50c2b100d9aab/`.

Fallback: MiniLM (`EMBEDDING_BACKEND=minilm`). Full-text search remains PostgreSQL-only and does not load embeddings.

Neural eval: `cargo test --release --test model_eval -- --ignored --nocapture --test-threads=1`. Missing weights panic; they do not fake-pass.

See `eval/RESULTS.md` for the MiniLM / BGE-small / Nomic numbers and the explicit selection rationale.
