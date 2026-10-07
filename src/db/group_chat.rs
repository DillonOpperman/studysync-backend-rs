use chrono::{DateTime, Utc};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::error::{map_db_error, unique_constraint, AppError};

pub const TYPE_TEXT: &str = "text";
pub const TYPE_ANNOUNCEMENT: &str = "announcement";
pub const MAX_CONTENT_CHARS: usize = 4000;
pub const MAX_EMOJI_CHARS: usize = 32;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct GroupMessageRow {
    pub id: Uuid,
    pub group_id: Uuid,
    pub sender_id: Uuid,
    pub client_msg_id: Uuid,
    pub content: String,
    pub message_type: String,
    pub created_at: DateTime<Utc>,
    pub edited_at: Option<DateTime<Utc>>,
    pub deleted_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ReactionAggRow {
    pub message_id: Uuid,
    pub emoji: String,
    pub count: i32,
    pub me: bool,
}

pub async fn insert_message(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    group_id: Uuid,
    sender_id: Uuid,
    client_msg_id: Uuid,
    content: &str,
    message_type: &str,
) -> Result<Result<GroupMessageRow, Uuid>, AppError> {
    sqlx::query("SAVEPOINT message_insert")
        .execute(&mut **tx)
        .await
        .map_err(map_db_error)?;

    let result = sqlx::query_as::<_, GroupMessageRow>(
        "INSERT INTO messages (
            id, group_id, sender_id, client_msg_id, content, message_type
         ) VALUES ($1,$2,$3,$4,$5,$6)
         RETURNING id, group_id, sender_id, client_msg_id, content, message_type,
                   created_at, edited_at, deleted_at",
    )
    .bind(id)
    .bind(group_id)
    .bind(sender_id)
    .bind(client_msg_id)
    .bind(content)
    .bind(message_type)
    .fetch_one(&mut **tx)
    .await;

    match result {
        Ok(row) => {
            sqlx::query("RELEASE SAVEPOINT message_insert")
                .execute(&mut **tx)
                .await
                .map_err(map_db_error)?;
            Ok(Ok(row))
        }
        Err(error) if unique_constraint(&error) == Some("messages_idempotency_key") => {
            sqlx::query("ROLLBACK TO SAVEPOINT message_insert")
                .execute(&mut **tx)
                .await
                .map_err(map_db_error)?;
            sqlx::query("RELEASE SAVEPOINT message_insert")
                .execute(&mut **tx)
                .await
                .map_err(map_db_error)?;
            Ok(Err(client_msg_id))
        }
        Err(error) => {
            let _ = sqlx::query("ROLLBACK TO SAVEPOINT message_insert")
                .execute(&mut **tx)
                .await;
            Err(map_db_error(error))
        }
    }
}

pub async fn get_by_idempotency(
    tx: &mut Transaction<'_, Postgres>,
    group_id: Uuid,
    sender_id: Uuid,
    client_msg_id: Uuid,
) -> Result<Option<GroupMessageRow>, AppError> {
    sqlx::query_as::<_, GroupMessageRow>(
        "SELECT id, group_id, sender_id, client_msg_id, content, message_type,
                created_at, edited_at, deleted_at
         FROM messages
         WHERE group_id = $1 AND sender_id = $2 AND client_msg_id = $3",
    )
    .bind(group_id)
    .bind(sender_id)
    .bind(client_msg_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(map_db_error)
}

pub async fn get_by_id(
    executor: impl sqlx::Executor<'_, Database = Postgres>,
    message_id: Uuid,
) -> Result<Option<GroupMessageRow>, AppError> {
    sqlx::query_as::<_, GroupMessageRow>(
        "SELECT id, group_id, sender_id, client_msg_id, content, message_type,
                created_at, edited_at, deleted_at
         FROM messages WHERE id = $1",
    )
    .bind(message_id)
    .fetch_optional(executor)
    .await
    .map_err(map_db_error)
}

pub async fn lock_by_id(
    tx: &mut Transaction<'_, Postgres>,
    message_id: Uuid,
) -> Result<Option<GroupMessageRow>, AppError> {
    sqlx::query_as::<_, GroupMessageRow>(
        "SELECT id, group_id, sender_id, client_msg_id, content, message_type,
                created_at, edited_at, deleted_at
         FROM messages WHERE id = $1 FOR UPDATE",
    )
    .bind(message_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(map_db_error)
}

pub async fn list_page(
    executor: impl sqlx::Executor<'_, Database = Postgres>,
    group_id: Uuid,
    before: Option<Uuid>,
    after: Option<Uuid>,
    limit: i64,
) -> Result<Vec<GroupMessageRow>, AppError> {
    sqlx::query_as::<_, GroupMessageRow>(
        "SELECT id, group_id, sender_id, client_msg_id, content, message_type,
                created_at, edited_at, deleted_at
         FROM messages
         WHERE group_id = $1
           AND ($2::uuid IS NULL OR id < $2)
           AND ($3::uuid IS NULL OR id > $3)
         ORDER BY created_at DESC, id DESC
         LIMIT $4",
    )
    .bind(group_id)
    .bind(before)
    .bind(after)
    .bind(limit)
    .fetch_all(executor)
    .await
    .map_err(map_db_error)
}

pub async fn update_content(
    tx: &mut Transaction<'_, Postgres>,
    message_id: Uuid,
    content: &str,
    edited_at: DateTime<Utc>,
) -> Result<GroupMessageRow, AppError> {
    sqlx::query_as::<_, GroupMessageRow>(
        "UPDATE messages
         SET content = $2, edited_at = $3
         WHERE id = $1
         RETURNING id, group_id, sender_id, client_msg_id, content, message_type,
                   created_at, edited_at, deleted_at",
    )
    .bind(message_id)
    .bind(content)
    .bind(edited_at)
    .fetch_one(&mut **tx)
    .await
    .map_err(map_db_error)
}

pub async fn soft_delete(
    tx: &mut Transaction<'_, Postgres>,
    message_id: Uuid,
    deleted_at: DateTime<Utc>,
) -> Result<GroupMessageRow, AppError> {
    sqlx::query_as::<_, GroupMessageRow>(
        "UPDATE messages
         SET deleted_at = $2, content = ''
         WHERE id = $1
         RETURNING id, group_id, sender_id, client_msg_id, content, message_type,
                   created_at, edited_at, deleted_at",
    )
    .bind(message_id)
    .bind(deleted_at)
    .fetch_one(&mut **tx)
    .await
    .map_err(map_db_error)
}

pub async fn toggle_reaction(
    tx: &mut Transaction<'_, Postgres>,
    message_id: Uuid,
    user_id: Uuid,
    emoji: &str,
) -> Result<(), AppError> {
    let inserted = sqlx::query(
        "INSERT INTO message_reactions (message_id, user_id, emoji)
         VALUES ($1, $2, $3)
         ON CONFLICT (message_id, user_id, emoji) DO NOTHING",
    )
    .bind(message_id)
    .bind(user_id)
    .bind(emoji)
    .execute(&mut **tx)
    .await
    .map_err(map_db_error)?;

    if inserted.rows_affected() == 0 {
        sqlx::query(
            "DELETE FROM message_reactions
             WHERE message_id = $1 AND user_id = $2 AND emoji = $3",
        )
        .bind(message_id)
        .bind(user_id)
        .bind(emoji)
        .execute(&mut **tx)
        .await
        .map_err(map_db_error)?;
    }
    Ok(())
}

pub async fn reactions_for_messages(
    executor: impl sqlx::Executor<'_, Database = Postgres>,
    message_ids: &[Uuid],
    viewer_id: Uuid,
) -> Result<Vec<ReactionAggRow>, AppError> {
    sqlx::query_as::<_, ReactionAggRow>(
        "SELECT message_id, emoji, COUNT(*)::int AS count,
                bool_or(user_id = $2) AS me
         FROM message_reactions
         WHERE message_id = ANY($1)
         GROUP BY message_id, emoji
         ORDER BY message_id, emoji",
    )
    .bind(message_ids)
    .bind(viewer_id)
    .fetch_all(executor)
    .await
    .map_err(map_db_error)
}

pub async fn blocked_user_ids(
    executor: impl sqlx::Executor<'_, Database = Postgres>,
    user_id: Uuid,
) -> Result<Vec<Uuid>, AppError> {
    sqlx::query_scalar(
        "SELECT CASE WHEN blocker_id = $1 THEN blocked_id ELSE blocker_id END
         FROM user_blocks
         WHERE blocker_id = $1 OR blocked_id = $1",
    )
    .bind(user_id)
    .fetch_all(executor)
    .await
    .map_err(map_db_error)
}
