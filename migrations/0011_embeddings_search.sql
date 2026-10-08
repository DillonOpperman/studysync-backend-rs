-- Phase 7: versioned embeddings (model-driven dimensions) and group full-text search.
-- Production vector width follows the selected model (Qwen3-Embedding-0.6B = 1024).
-- Do not mix vectors from different model_id values in one ANN search.

CREATE TABLE entity_embeddings (
    id            uuid PRIMARY KEY,
    entity_kind   text NOT NULL,
    entity_id     uuid NOT NULL,
    model_id      text NOT NULL,
    dimensions    integer NOT NULL,
    embedding     vector(1024) NOT NULL,
    source_hash   text NOT NULL,
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT entity_embeddings_kind CHECK (entity_kind IN ('group', 'user')),
    CONSTRAINT entity_embeddings_dim CHECK (dimensions = 1024),
    CONSTRAINT entity_embeddings_hash_not_blank CHECK (btrim(source_hash) <> ''),
    CONSTRAINT entity_embeddings_model_not_blank CHECK (btrim(model_id) <> ''),
    CONSTRAINT entity_embeddings_unique UNIQUE (entity_kind, entity_id, model_id)
);

CREATE INDEX entity_embeddings_entity_idx
    ON entity_embeddings (entity_kind, entity_id);
CREATE INDEX entity_embeddings_model_idx
    ON entity_embeddings (model_id);
CREATE INDEX entity_embeddings_hnsw
    ON entity_embeddings
    USING hnsw (embedding vector_cosine_ops);

ALTER TABLE study_groups
    ADD COLUMN search_tsv tsvector
    GENERATED ALWAYS AS (
        setweight(to_tsvector('english', coalesce(title, '')), 'A')
        || setweight(to_tsvector('english', coalesce(subject, '')), 'A')
        || setweight(to_tsvector('english', coalesce(description, '')), 'B')
    ) STORED;

CREATE INDEX study_groups_search_tsv_idx ON study_groups USING gin (search_tsv);
