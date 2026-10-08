use chrono::{DateTime, Utc};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::error::{map_db_error, AppError};

pub const STATUS_UPLOADED: &str = "uploaded";
pub const STATUS_ATTACHED: &str = "attached";
pub const STATUS_DELETED: &str = "deleted";
pub const PURPOSE_PROFILE: &str = "profile";
pub const PURPOSE_GROUP: &str = "group_message";
pub const PURPOSE_DM: &str = "direct_message";

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct MediaRow {
    pub id: Uuid,
    pub owner_id: Uuid,
    pub purpose: String,
    pub kind: String,
    pub status: String,
    pub storage_key: String,
    pub file_name: String,
    pub content_type: String,
    pub byte_size: i32,
    pub group_id: Option<Uuid>,
    pub peer_id: Option<Uuid>,
    pub message_id: Option<Uuid>,
    pub direct_message_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub attached_at: Option<DateTime<Utc>>,
    pub deleted_at: Option<DateTime<Utc>>,
}

const SELECT: &str = "SELECT id, owner_id, purpose, kind, status, storage_key, file_name,
        content_type, byte_size, group_id, peer_id, message_id, direct_message_id,
        created_at, attached_at, deleted_at
     FROM media_objects";

pub async fn insert(tx: &mut Transaction<'_, Postgres>, row: &MediaRow) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO media_objects (
            id, owner_id, purpose, kind, status, storage_key, file_name, content_type,
            byte_size, group_id, peer_id, message_id, direct_message_id, created_at,
            attached_at, deleted_at
         ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)",
    )
    .bind(row.id)
    .bind(row.owner_id)
    .bind(&row.purpose)
    .bind(&row.kind)
    .bind(&row.status)
    .bind(&row.storage_key)
    .bind(&row.file_name)
    .bind(&row.content_type)
    .bind(row.byte_size)
    .bind(row.group_id)
    .bind(row.peer_id)
    .bind(row.message_id)
    .bind(row.direct_message_id)
    .bind(row.created_at)
    .bind(row.attached_at)
    .bind(row.deleted_at)
    .execute(&mut **tx)
    .await
    .map_err(map_db_error)?;
    Ok(())
}

pub async fn get_by_id(
    executor: impl sqlx::Executor<'_, Database = Postgres>,
    id: Uuid,
) -> Result<Option<MediaRow>, AppError> {
    sqlx::query_as::<_, MediaRow>(&format!("{SELECT} WHERE id = $1"))
        .bind(id)
        .fetch_optional(executor)
        .await
        .map_err(map_db_error)
}

pub async fn lock_by_id(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
) -> Result<Option<MediaRow>, AppError> {
    sqlx::query_as::<_, MediaRow>(&format!("{SELECT} WHERE id = $1 FOR UPDATE"))
        .bind(id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(map_db_error)
}

pub async fn list_for_group_messages(
    executor: impl sqlx::Executor<'_, Database = Postgres>,
    message_ids: &[Uuid],
) -> Result<Vec<MediaRow>, AppError> {
    sqlx::query_as::<_, MediaRow>(&format!(
        "{SELECT} WHERE message_id = ANY($1) AND status = 'attached' ORDER BY created_at"
    ))
    .bind(message_ids)
    .fetch_all(executor)
    .await
    .map_err(map_db_error)
}

pub async fn list_for_direct_messages(
    executor: impl sqlx::Executor<'_, Database = Postgres>,
    message_ids: &[Uuid],
) -> Result<Vec<MediaRow>, AppError> {
    sqlx::query_as::<_, MediaRow>(&format!(
        "{SELECT} WHERE direct_message_id = ANY($1) AND status = 'attached' ORDER BY created_at"
    ))
    .bind(message_ids)
    .fetch_all(executor)
    .await
    .map_err(map_db_error)
}

pub async fn attach_to_group_message(
    tx: &mut Transaction<'_, Postgres>,
    ids: &[Uuid],
    message_id: Uuid,
    now: DateTime<Utc>,
) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE media_objects
         SET status = 'attached', message_id = $2, attached_at = $3
         WHERE id = ANY($1) AND status = 'uploaded'",
    )
    .bind(ids)
    .bind(message_id)
    .bind(now)
    .execute(&mut **tx)
    .await
    .map_err(map_db_error)?;
    Ok(())
}

pub async fn attach_to_direct_message(
    tx: &mut Transaction<'_, Postgres>,
    ids: &[Uuid],
    message_id: Uuid,
    now: DateTime<Utc>,
) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE media_objects
         SET status = 'attached', direct_message_id = $2, attached_at = $3
         WHERE id = ANY($1) AND status = 'uploaded'",
    )
    .bind(ids)
    .bind(message_id)
    .bind(now)
    .execute(&mut **tx)
    .await
    .map_err(map_db_error)?;
    Ok(())
}

pub async fn mark_deleted(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    now: DateTime<Utc>,
) -> Result<(), AppError> {
    sqlx::query("UPDATE media_objects SET status = 'deleted', deleted_at = $2 WHERE id = $1")
        .bind(id)
        .bind(now)
        .execute(&mut **tx)
        .await
        .map_err(map_db_error)?;
    Ok(())
}

pub async fn mark_deleted_for_group_message(
    tx: &mut Transaction<'_, Postgres>,
    message_id: Uuid,
    now: DateTime<Utc>,
) -> Result<Vec<MediaRow>, AppError> {
    sqlx::query_as::<_, MediaRow>(
        "UPDATE media_objects
         SET status = 'deleted', deleted_at = $2
         WHERE message_id = $1 AND status = 'attached'
         RETURNING id, owner_id, purpose, kind, status, storage_key, file_name,
                   content_type, byte_size, group_id, peer_id, message_id, direct_message_id,
                   created_at, attached_at, deleted_at",
    )
    .bind(message_id)
    .bind(now)
    .fetch_all(&mut **tx)
    .await
    .map_err(map_db_error)
}

pub async fn list_orphans(
    executor: impl sqlx::Executor<'_, Database = Postgres>,
    older_than: DateTime<Utc>,
) -> Result<Vec<MediaRow>, AppError> {
    sqlx::query_as::<_, MediaRow>(&format!(
        "{SELECT} WHERE status = 'uploaded' AND created_at < $1"
    ))
    .bind(older_than)
    .fetch_all(executor)
    .await
    .map_err(map_db_error)
}

pub async fn find_attached_profile(
    executor: impl sqlx::Executor<'_, Database = Postgres>,
    owner_id: Uuid,
    storage_key: &str,
) -> Result<Option<MediaRow>, AppError> {
    sqlx::query_as::<_, MediaRow>(&format!(
        "{SELECT} WHERE owner_id = $1 AND purpose = 'profile' AND status = 'attached'
         AND storage_key = $2"
    ))
    .bind(owner_id)
    .bind(storage_key)
    .fetch_optional(executor)
    .await
    .map_err(map_db_error)
}
