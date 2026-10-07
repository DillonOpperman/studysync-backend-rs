use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::error::{map_db_error, AppError};

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct NotificationRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub ntype: String,
    pub title: String,
    pub body: String,
    pub data: sqlx::types::Json<Value>,
    pub read_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

pub async fn insert_notification(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    ntype: &str,
    title: &str,
    body: &str,
    data: Value,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO notifications (id, user_id, type, title, body, data)
         VALUES ($1,$2,$3,$4,$5,$6)",
    )
    .bind(Uuid::now_v7())
    .bind(user_id)
    .bind(ntype)
    .bind(title)
    .bind(body)
    .bind(sqlx::types::Json(data))
    .execute(&mut **tx)
    .await
    .map_err(map_db_error)?;
    Ok(())
}

pub async fn list_for_user(
    executor: impl sqlx::Executor<'_, Database = Postgres>,
    user_id: Uuid,
    limit: i64,
    offset: i64,
) -> Result<Vec<NotificationRow>, AppError> {
    sqlx::query_as::<_, NotificationRow>(
        "SELECT id, user_id, type AS ntype, title, body, data, read_at, created_at
         FROM notifications
         WHERE user_id = $1
         ORDER BY created_at DESC
         LIMIT $2 OFFSET $3",
    )
    .bind(user_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(executor)
    .await
    .map_err(map_db_error)
}

pub async fn mark_read(
    executor: impl sqlx::Executor<'_, Database = Postgres>,
    user_id: Uuid,
    notification_id: Uuid,
    now: DateTime<Utc>,
) -> Result<u64, AppError> {
    let result = sqlx::query(
        "UPDATE notifications SET read_at = $3
         WHERE id = $1 AND user_id = $2 AND read_at IS NULL",
    )
    .bind(notification_id)
    .bind(user_id)
    .bind(now)
    .execute(executor)
    .await
    .map_err(map_db_error)?;
    Ok(result.rows_affected())
}

pub async fn mark_all_read(
    executor: impl sqlx::Executor<'_, Database = Postgres>,
    user_id: Uuid,
    now: DateTime<Utc>,
) -> Result<u64, AppError> {
    let result =
        sqlx::query("UPDATE notifications SET read_at = $2 WHERE user_id = $1 AND read_at IS NULL")
            .bind(user_id)
            .bind(now)
            .execute(executor)
            .await
            .map_err(map_db_error)?;
    Ok(result.rows_affected())
}

pub async fn get_owned(
    executor: impl sqlx::Executor<'_, Database = Postgres>,
    user_id: Uuid,
    notification_id: Uuid,
) -> Result<Option<NotificationRow>, AppError> {
    sqlx::query_as::<_, NotificationRow>(
        "SELECT id, user_id, type AS ntype, title, body, data, read_at, created_at
         FROM notifications WHERE id = $1 AND user_id = $2",
    )
    .bind(notification_id)
    .bind(user_id)
    .fetch_optional(executor)
    .await
    .map_err(map_db_error)
}
