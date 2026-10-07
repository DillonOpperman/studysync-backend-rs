use chrono::Utc;
use sqlx::PgPool;
use uuid::Uuid;

use crate::api::dto::NotificationDto;
use crate::db::notifications;
use crate::error::AppError;

pub async fn list(
    pool: &PgPool,
    user_id: Uuid,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<Vec<NotificationDto>, AppError> {
    let limit = limit.unwrap_or(20).clamp(1, 50);
    let offset = offset.unwrap_or(0).max(0);
    let rows = notifications::list_for_user(pool, user_id, limit, offset).await?;
    Ok(rows
        .into_iter()
        .map(|row| NotificationDto {
            id: row.id,
            ntype: row.ntype,
            title: row.title,
            body: row.body,
            data: row.data.0,
            read_at: row.read_at,
            created_at: row.created_at,
        })
        .collect())
}

pub async fn mark_read(
    pool: &PgPool,
    user_id: Uuid,
    notification_id: Uuid,
) -> Result<(), AppError> {
    notifications::get_owned(pool, user_id, notification_id)
        .await?
        .ok_or(AppError::NotificationNotFound)?;
    notifications::mark_read(pool, user_id, notification_id, Utc::now()).await?;
    Ok(())
}

pub async fn mark_all_read(pool: &PgPool, user_id: Uuid) -> Result<(), AppError> {
    notifications::mark_all_read(pool, user_id, Utc::now()).await?;
    Ok(())
}
