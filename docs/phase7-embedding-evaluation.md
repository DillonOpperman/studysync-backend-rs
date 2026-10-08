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
