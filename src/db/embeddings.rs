use chrono::{DateTime, Utc};
use pgvector::Vector;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::embed::catalog;
use crate::error::{map_db_error, AppError};

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct EmbeddingRow {
    pub id: Uuid,
    pub entity_kind: String,
    pub entity_id: Uuid,
    pub model_id: String,
    pub dimensions: i32,
    pub source_hash: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub async fn upsert(
    tx: &mut Transaction<'_, Postgres>,
    entity_kind: &str,
    entity_id: Uuid,
    model_id: &str,
    source_hash: &str,
    values: &[f32],
) -> Result<(), AppError> {
    if values.len() != catalog::PRODUCTION_EMBEDDING_DIM {
        return Err(AppError::EmbeddingDimensionMismatch);
    }
    if model_id.trim().is_empty() {
        return Err(AppError::validation("modelId is required"));
    }
    let vector = Vector::from(values.to_vec());
    sqlx::query(
        "INSERT INTO entity_embeddings (
            id, entity_kind, entity_id, model_id, dimensions, embedding, source_hash
         ) VALUES ($1,$2,$3,$4,$5,$6,$7)
         ON CONFLICT (entity_kind, entity_id, model_id)
         DO UPDATE SET
            embedding = EXCLUDED.embedding,
            dimensions = EXCLUDED.dimensions,
            source_hash = EXCLUDED.source_hash,
            updated_at = now()
         WHERE entity_embeddings.source_hash IS DISTINCT FROM EXCLUDED.source_hash",
    )
    .bind(Uuid::now_v7())
    .bind(entity_kind)
    .bind(entity_id)
    .bind(model_id)
    .bind(catalog::PRODUCTION_EMBEDDING_DIM as i32)
    .bind(vector)
    .bind(source_hash)
    .execute(&mut **tx)
    .await
    .map_err(map_db_error)?;
    Ok(())
}

pub async fn get(
    executor: impl sqlx::Executor<'_, Database = Postgres>,
    entity_kind: &str,
    entity_id: Uuid,
    model_id: &str,
) -> Result<Option<EmbeddingRow>, AppError> {
    sqlx::query_as::<_, EmbeddingRow>(
        "SELECT id, entity_kind, entity_id, model_id, dimensions, source_hash, created_at, updated_at
         FROM entity_embeddings
         WHERE entity_kind = $1 AND entity_id = $2 AND model_id = $3",
    )
    .bind(entity_kind)
    .bind(entity_id)
    .bind(model_id)
    .fetch_optional(executor)
    .await
    .map_err(map_db_error)
}

pub async fn load_vector(
    executor: impl sqlx::Executor<'_, Database = Postgres>,
    entity_kind: &str,
    entity_id: Uuid,
    model_id: &str,
) -> Result<Option<Vec<f32>>, AppError> {
    let row: Option<(Vector,)> = sqlx::query_as(
        "SELECT embedding FROM entity_embeddings
         WHERE entity_kind = $1 AND entity_id = $2 AND model_id = $3",
    )
    .bind(entity_kind)
    .bind(entity_id)
    .bind(model_id)
    .fetch_optional(executor)
    .await
    .map_err(map_db_error)?;
    Ok(row.map(|tuple| tuple.0.to_vec()))
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SemanticHit {
    pub entity_id: Uuid,
    pub similarity: f32,
}

pub async fn search_groups(
    executor: impl sqlx::Executor<'_, Database = Postgres>,
    model_id: &str,
    query: &[f32],
    threshold: f32,
    limit: i64,
    offset: i64,
) -> Result<Vec<SemanticHit>, AppError> {
    if query.len() != catalog::PRODUCTION_EMBEDDING_DIM {
        return Err(AppError::EmbeddingDimensionMismatch);
    }
    let vector = Vector::from(query.to_vec());
    sqlx::query_as::<_, SemanticHit>(
        "SELECT e.entity_id,
                (1 - (e.embedding <=> $1))::real AS similarity
         FROM entity_embeddings e
         JOIN study_groups g ON g.id = e.entity_id AND g.status = 'active'
         WHERE e.entity_kind = 'group'
           AND e.model_id = $2
           AND (1 - (e.embedding <=> $1)) >= $3
         ORDER BY e.embedding <=> $1
         LIMIT $4 OFFSET $5",
    )
    .bind(vector)
    .bind(model_id)
    .bind(threshold)
    .bind(limit)
    .bind(offset)
    .fetch_all(executor)
    .await
    .map_err(map_db_error)
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct FullTextHit {
    pub id: Uuid,
    pub rank: f32,
}

pub async fn search_groups_full_text(
    executor: impl sqlx::Executor<'_, Database = Postgres>,
    query: &str,
    limit: i64,
    offset: i64,
) -> Result<Vec<FullTextHit>, AppError> {
    sqlx::query_as::<_, FullTextHit>(
        "SELECT g.id,
                ts_rank(g.search_tsv, websearch_to_tsquery('english', $1))::real AS rank
         FROM study_groups g
         WHERE g.status = 'active'
           AND g.search_tsv @@ websearch_to_tsquery('english', $1)
         ORDER BY rank DESC, g.created_at DESC
         LIMIT $2 OFFSET $3",
    )
    .bind(query)
    .bind(limit)
    .bind(offset)
    .fetch_all(executor)
    .await
    .map_err(map_db_error)
}
