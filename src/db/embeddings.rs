use chrono::{DateTime, Utc};
use pgvector::Vector;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

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

fn table_for_dim(dim: usize) -> Result<&'static str, AppError> {
    match dim {
        384 => Ok("entity_embeddings_384"),
        768 => Ok("entity_embeddings_768"),
        1024 => Ok("entity_embeddings_1024"),
        _ => Err(AppError::EmbeddingDimensionMismatch),
    }
}

pub async fn upsert(
    tx: &mut Transaction<'_, Postgres>,
    entity_kind: &str,
    entity_id: Uuid,
    model_id: &str,
    source_hash: &str,
    values: &[f32],
) -> Result<(), AppError> {
    let table = table_for_dim(values.len())?;
    if model_id.trim().is_empty() {
        return Err(AppError::validation("modelId is required"));
    }
    let vector = Vector::from(values.to_vec());
    let sql = format!(
        "INSERT INTO {table} (
            id, entity_kind, entity_id, model_id, dimensions, embedding, source_hash
         ) VALUES ($1,$2,$3,$4,$5,$6,$7)
         ON CONFLICT (entity_kind, entity_id, model_id)
         DO UPDATE SET
            embedding = EXCLUDED.embedding,
            dimensions = EXCLUDED.dimensions,
            source_hash = EXCLUDED.source_hash,
            updated_at = now()
         WHERE {table}.source_hash IS DISTINCT FROM EXCLUDED.source_hash"
    );
    sqlx::query(&sql)
        .bind(Uuid::now_v7())
        .bind(entity_kind)
        .bind(entity_id)
        .bind(model_id)
        .bind(values.len() as i32)
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
         FROM entity_embeddings_384
         WHERE entity_kind = $1 AND entity_id = $2 AND model_id = $3
         UNION ALL
         SELECT id, entity_kind, entity_id, model_id, dimensions, source_hash, created_at, updated_at
         FROM entity_embeddings_768
         WHERE entity_kind = $1 AND entity_id = $2 AND model_id = $3
         UNION ALL
         SELECT id, entity_kind, entity_id, model_id, dimensions, source_hash, created_at, updated_at
         FROM entity_embeddings_1024
         WHERE entity_kind = $1 AND entity_id = $2 AND model_id = $3
         LIMIT 1",
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
    let row: Option<(Vec<f32>,)> = sqlx::query_as(
        "SELECT embedding FROM (
            SELECT embedding::real[] AS embedding
            FROM entity_embeddings_384
            WHERE entity_kind = $1 AND entity_id = $2 AND model_id = $3
            UNION ALL
            SELECT embedding::real[]
            FROM entity_embeddings_768
            WHERE entity_kind = $1 AND entity_id = $2 AND model_id = $3
            UNION ALL
            SELECT embedding::real[]
            FROM entity_embeddings_1024
            WHERE entity_kind = $1 AND entity_id = $2 AND model_id = $3
         ) s
         LIMIT 1",
    )
    .bind(entity_kind)
    .bind(entity_id)
    .bind(model_id)
    .fetch_optional(executor)
    .await
    .map_err(map_db_error)?;
    Ok(row.map(|tuple| tuple.0))
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
    let table = table_for_dim(query.len())?;
    let vector = Vector::from(query.to_vec());
    let sql = format!(
        "SELECT e.entity_id,
                (1 - (e.embedding <=> $1))::real AS similarity
         FROM {table} e
         JOIN study_groups g ON g.id = e.entity_id AND g.status = 'active'
         WHERE e.entity_kind = 'group'
           AND e.model_id = $2
           AND (1 - (e.embedding <=> $1)) >= $3
         ORDER BY e.embedding <=> $1
         LIMIT $4 OFFSET $5"
    );
    sqlx::query_as::<_, SemanticHit>(&sql)
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
