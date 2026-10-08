-- Split embeddings by vector width so MiniLM/BGE (384), Nomic (768), and
-- historical Qwen (1024) never share an incompatible pgvector column.
-- Existing Qwen rows are preserved in entity_embeddings_1024.

ALTER TABLE entity_embeddings RENAME TO entity_embeddings_1024;

ALTER INDEX entity_embeddings_entity_idx RENAME TO entity_embeddings_1024_entity_idx;
ALTER INDEX entity_embeddings_model_idx RENAME TO entity_embeddings_1024_model_idx;
ALTER INDEX entity_embeddings_hnsw RENAME TO entity_embeddings_1024_hnsw;

CREATE TABLE entity_embeddings_384 (
    id            uuid PRIMARY KEY,
    entity_kind   text NOT NULL,
    entity_id     uuid NOT NULL,
    model_id      text NOT NULL,
    dimensions    integer NOT NULL,
    embedding     vector(384) NOT NULL,
    source_hash   text NOT NULL,
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT entity_embeddings_384_kind CHECK (entity_kind IN ('group', 'user')),
    CONSTRAINT entity_embeddings_384_dim CHECK (dimensions = 384),
    CONSTRAINT entity_embeddings_384_hash_not_blank CHECK (btrim(source_hash) <> ''),
    CONSTRAINT entity_embeddings_384_model_not_blank CHECK (btrim(model_id) <> ''),
    CONSTRAINT entity_embeddings_384_unique UNIQUE (entity_kind, entity_id, model_id)
);

CREATE INDEX entity_embeddings_384_entity_idx
    ON entity_embeddings_384 (entity_kind, entity_id);
CREATE INDEX entity_embeddings_384_model_idx
    ON entity_embeddings_384 (model_id);
CREATE INDEX entity_embeddings_384_hnsw
    ON entity_embeddings_384
    USING hnsw (embedding vector_cosine_ops);

CREATE TABLE entity_embeddings_768 (
    id            uuid PRIMARY KEY,
    entity_kind   text NOT NULL,
    entity_id     uuid NOT NULL,
    model_id      text NOT NULL,
    dimensions    integer NOT NULL,
    embedding     vector(768) NOT NULL,
    source_hash   text NOT NULL,
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT entity_embeddings_768_kind CHECK (entity_kind IN ('group', 'user')),
    CONSTRAINT entity_embeddings_768_dim CHECK (dimensions = 768),
    CONSTRAINT entity_embeddings_768_hash_not_blank CHECK (btrim(source_hash) <> ''),
    CONSTRAINT entity_embeddings_768_model_not_blank CHECK (btrim(model_id) <> ''),
    CONSTRAINT entity_embeddings_768_unique UNIQUE (entity_kind, entity_id, model_id)
);

CREATE INDEX entity_embeddings_768_entity_idx
    ON entity_embeddings_768 (entity_kind, entity_id);
CREATE INDEX entity_embeddings_768_model_idx
    ON entity_embeddings_768 (model_id);
CREATE INDEX entity_embeddings_768_hnsw
    ON entity_embeddings_768
    USING hnsw (embedding vector_cosine_ops);
