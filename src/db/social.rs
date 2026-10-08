use chrono::{DateTime, Utc};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::error::{is_unique_or_serialization, map_db_error, AppError};

pub const REQ_PENDING: &str = "pending";
pub const REQ_ACCEPTED: &str = "accepted";
pub const REQ_REJECTED: &str = "rejected";

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct FriendRequestRow {
    pub id: Uuid,
    pub requester_id: Uuid,
    pub recipient_id: Uuid,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub decided_at: Option<DateTime<Utc>>,
    pub requester_name: Option<String>,
    pub requester_picture: Option<String>,
    pub recipient_name: Option<String>,
    pub recipient_picture: Option<String>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct FriendRow {
    pub user_id: Uuid,
    pub name: String,
    pub profile_picture_key: Option<String>,
    pub friends_since: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct MessageRequestRow {
    pub id: Uuid,
    pub sender_id: Uuid,
    pub receiver_id: Uuid,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub sender_name: Option<String>,
    pub sender_picture: Option<String>,
    pub receiver_name: Option<String>,
    pub receiver_picture: Option<String>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct DirectMessageRow {
    pub id: Uuid,
    pub sender_id: Uuid,
    pub receiver_id: Uuid,
    pub content: String,
    pub message_type: String,
    pub created_at: DateTime<Utc>,
    pub read_at: Option<DateTime<Utc>>,
}

pub fn ordered_pair(a: Uuid, b: Uuid) -> (Uuid, Uuid) {
    if a < b {
        (a, b)
    } else {
        (b, a)
    }
}

pub async fn is_blocked(
    executor: impl sqlx::Executor<'_, Database = Postgres>,
    user_a: Uuid,
    user_b: Uuid,
) -> Result<bool, AppError> {
    sqlx::query_scalar(
        "SELECT EXISTS (
            SELECT 1 FROM user_blocks
            WHERE (blocker_id = $1 AND blocked_id = $2)
               OR (blocker_id = $2 AND blocked_id = $1)
         )",
    )
    .bind(user_a)
    .bind(user_b)
    .fetch_one(executor)
    .await
    .map_err(map_db_error)
}

pub async fn insert_block(
    tx: &mut Transaction<'_, Postgres>,
    blocker_id: Uuid,
    blocked_id: Uuid,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO user_blocks (blocker_id, blocked_id) VALUES ($1, $2)
         ON CONFLICT DO NOTHING",
    )
    .bind(blocker_id)
    .bind(blocked_id)
    .execute(&mut **tx)
    .await
    .map_err(map_db_error)?;
    Ok(())
}

pub async fn delete_block(
    executor: impl sqlx::Executor<'_, Database = Postgres>,
    blocker_id: Uuid,
    blocked_id: Uuid,
) -> Result<u64, AppError> {
    let result = sqlx::query("DELETE FROM user_blocks WHERE blocker_id = $1 AND blocked_id = $2")
        .bind(blocker_id)
        .bind(blocked_id)
        .execute(executor)
        .await
        .map_err(map_db_error)?;
    Ok(result.rows_affected())
}

pub async fn are_friends(
    executor: impl sqlx::Executor<'_, Database = Postgres>,
    user_a: Uuid,
    user_b: Uuid,
) -> Result<bool, AppError> {
    let (low, high) = ordered_pair(user_a, user_b);
    sqlx::query_scalar(
        "SELECT EXISTS (
            SELECT 1 FROM friendships WHERE user_low = $1 AND user_high = $2
         )",
    )
    .bind(low)
    .bind(high)
    .fetch_one(executor)
    .await
    .map_err(map_db_error)
}

pub async fn insert_friendship(
    tx: &mut Transaction<'_, Postgres>,
    user_a: Uuid,
    user_b: Uuid,
    now: DateTime<Utc>,
) -> Result<(), AppError> {
    let (low, high) = ordered_pair(user_a, user_b);
    sqlx::query(
        "INSERT INTO friendships (user_low, user_high, created_at)
         VALUES ($1, $2, $3)
         ON CONFLICT DO NOTHING",
    )
    .bind(low)
    .bind(high)
    .bind(now)
    .execute(&mut **tx)
    .await
    .map_err(map_db_error)?;
    Ok(())
}

pub async fn list_friends(
    executor: impl sqlx::Executor<'_, Database = Postgres>,
    user_id: Uuid,
) -> Result<Vec<FriendRow>, AppError> {
    sqlx::query_as::<_, FriendRow>(
        "SELECT u.id AS user_id, u.name, u.profile_picture_key, f.created_at AS friends_since
         FROM friendships f
         JOIN users u ON u.id = CASE WHEN f.user_low = $1 THEN f.user_high ELSE f.user_low END
         WHERE (f.user_low = $1 OR f.user_high = $1)
           AND u.deleted_at IS NULL
           AND NOT EXISTS (
                SELECT 1 FROM user_blocks b
                WHERE (b.blocker_id = $1 AND b.blocked_id = u.id)
                   OR (b.blocker_id = u.id AND b.blocked_id = $1)
           )
         ORDER BY u.name",
    )
    .bind(user_id)
    .fetch_all(executor)
    .await
    .map_err(map_db_error)
}

const FRIEND_REQUEST_SELECT: &str = "SELECT r.id, r.requester_id, r.recipient_id, r.status,
            r.created_at, r.decided_at,
            a.name AS requester_name, a.profile_picture_key AS requester_picture,
            b.name AS recipient_name, b.profile_picture_key AS recipient_picture
     FROM friend_requests r
     JOIN users a ON a.id = r.requester_id
     JOIN users b ON b.id = r.recipient_id";

pub async fn insert_friend_request(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    requester_id: Uuid,
    recipient_id: Uuid,
    now: DateTime<Utc>,
) -> Result<(), AppError> {
    sqlx::query("SAVEPOINT friend_request_insert")
        .execute(&mut **tx)
        .await
        .map_err(map_db_error)?;
    let inserted = sqlx::query(
        "INSERT INTO friend_requests (
            id, requester_id, recipient_id, status, created_at, decided_at
         ) VALUES ($1,$2,$3,'pending',$4,NULL)",
    )
    .bind(id)
    .bind(requester_id)
    .bind(recipient_id)
    .bind(now)
    .execute(&mut **tx)
    .await;
    match inserted {
        Ok(_) => {
            sqlx::query("RELEASE SAVEPOINT friend_request_insert")
                .execute(&mut **tx)
                .await
                .map_err(map_db_error)?;
            Ok(())
        }
        Err(error) if is_unique_or_serialization(&error) => {
            sqlx::query("ROLLBACK TO SAVEPOINT friend_request_insert")
                .execute(&mut **tx)
                .await
                .map_err(map_db_error)?;
            sqlx::query("RELEASE SAVEPOINT friend_request_insert")
                .execute(&mut **tx)
                .await
                .map_err(map_db_error)?;
            Err(AppError::FriendRequestPending)
        }
        Err(error) => {
            let _ = sqlx::query("ROLLBACK TO SAVEPOINT friend_request_insert")
                .execute(&mut **tx)
                .await;
            Err(map_db_error(error))
        }
    }
}

pub async fn get_friend_request(
    executor: impl sqlx::Executor<'_, Database = Postgres>,
    request_id: Uuid,
) -> Result<Option<FriendRequestRow>, AppError> {
    sqlx::query_as::<_, FriendRequestRow>(&format!("{FRIEND_REQUEST_SELECT} WHERE r.id = $1"))
        .bind(request_id)
        .fetch_optional(executor)
        .await
        .map_err(map_db_error)
}

pub async fn lock_friend_request(
    tx: &mut Transaction<'_, Postgres>,
    request_id: Uuid,
) -> Result<Option<FriendRequestRow>, AppError> {
    sqlx::query_as::<_, FriendRequestRow>(&format!(
        "{FRIEND_REQUEST_SELECT} WHERE r.id = $1 FOR UPDATE OF r"
    ))
    .bind(request_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(map_db_error)
}

pub async fn list_incoming_friend_requests(
    executor: impl sqlx::Executor<'_, Database = Postgres>,
    user_id: Uuid,
) -> Result<Vec<FriendRequestRow>, AppError> {
    sqlx::query_as::<_, FriendRequestRow>(&format!(
        "{FRIEND_REQUEST_SELECT}
         WHERE r.recipient_id = $1 AND r.status = 'pending'
         ORDER BY r.created_at"
    ))
    .bind(user_id)
    .fetch_all(executor)
    .await
    .map_err(map_db_error)
}

pub async fn list_outgoing_friend_requests(
    executor: impl sqlx::Executor<'_, Database = Postgres>,
    user_id: Uuid,
) -> Result<Vec<FriendRequestRow>, AppError> {
    sqlx::query_as::<_, FriendRequestRow>(&format!(
        "{FRIEND_REQUEST_SELECT}
         WHERE r.requester_id = $1 AND r.status = 'pending'
         ORDER BY r.created_at"
    ))
    .bind(user_id)
    .fetch_all(executor)
    .await
    .map_err(map_db_error)
}

pub async fn decide_friend_request(
    tx: &mut Transaction<'_, Postgres>,
    request_id: Uuid,
    status: &str,
    now: DateTime<Utc>,
) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE friend_requests SET status = $2, decided_at = $3
         WHERE id = $1 AND status = 'pending'",
    )
    .bind(request_id)
    .bind(status)
    .bind(now)
    .execute(&mut **tx)
    .await
    .map_err(map_db_error)?;
    Ok(())
}

pub async fn reject_pending_friend_requests_between(
    tx: &mut Transaction<'_, Postgres>,
    user_a: Uuid,
    user_b: Uuid,
    now: DateTime<Utc>,
) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE friend_requests
         SET status = 'rejected', decided_at = $3
         WHERE status = 'pending'
           AND ((requester_id = $1 AND recipient_id = $2)
             OR (requester_id = $2 AND recipient_id = $1))",
    )
    .bind(user_a)
    .bind(user_b)
    .bind(now)
    .execute(&mut **tx)
    .await
    .map_err(map_db_error)?;
    Ok(())
}

const MESSAGE_REQUEST_SELECT: &str = "SELECT r.id, r.sender_id, r.receiver_id, r.status,
            r.created_at, r.updated_at,
            a.name AS sender_name, a.profile_picture_key AS sender_picture,
            b.name AS receiver_name, b.profile_picture_key AS receiver_picture
     FROM message_requests r
     JOIN users a ON a.id = r.sender_id
     JOIN users b ON b.id = r.receiver_id";

pub async fn insert_message_request(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    sender_id: Uuid,
    receiver_id: Uuid,
    now: DateTime<Utc>,
) -> Result<(), AppError> {
    sqlx::query("SAVEPOINT message_request_insert")
        .execute(&mut **tx)
        .await
        .map_err(map_db_error)?;
    let inserted = sqlx::query(
        "INSERT INTO message_requests (
            id, sender_id, receiver_id, status, created_at, updated_at
         ) VALUES ($1,$2,$3,'pending',$4,$4)",
    )
    .bind(id)
    .bind(sender_id)
    .bind(receiver_id)
    .bind(now)
    .execute(&mut **tx)
    .await;
    match inserted {
        Ok(_) => {
            sqlx::query("RELEASE SAVEPOINT message_request_insert")
                .execute(&mut **tx)
                .await
                .map_err(map_db_error)?;
            Ok(())
        }
        Err(error) if is_unique_or_serialization(&error) => {
            sqlx::query("ROLLBACK TO SAVEPOINT message_request_insert")
                .execute(&mut **tx)
                .await
                .map_err(map_db_error)?;
            sqlx::query("RELEASE SAVEPOINT message_request_insert")
                .execute(&mut **tx)
                .await
                .map_err(map_db_error)?;
            Err(AppError::MessageRequestPending)
        }
        Err(error) => {
            let _ = sqlx::query("ROLLBACK TO SAVEPOINT message_request_insert")
                .execute(&mut **tx)
                .await;
            Err(map_db_error(error))
        }
    }
}

pub async fn find_pending_message_request(
    executor: impl sqlx::Executor<'_, Database = Postgres>,
    sender_id: Uuid,
    receiver_id: Uuid,
) -> Result<Option<MessageRequestRow>, AppError> {
    sqlx::query_as::<_, MessageRequestRow>(&format!(
        "{MESSAGE_REQUEST_SELECT}
         WHERE r.sender_id = $1 AND r.receiver_id = $2 AND r.status = 'pending'"
    ))
    .bind(sender_id)
    .bind(receiver_id)
    .fetch_optional(executor)
    .await
    .map_err(map_db_error)
}

pub async fn message_request_accepted(
    executor: impl sqlx::Executor<'_, Database = Postgres>,
    sender_id: Uuid,
    receiver_id: Uuid,
) -> Result<bool, AppError> {
    sqlx::query_scalar(
        "SELECT EXISTS (
            SELECT 1 FROM message_requests
            WHERE status = 'accepted'
              AND ((sender_id = $1 AND receiver_id = $2)
                OR (sender_id = $2 AND receiver_id = $1))
         )",
    )
    .bind(sender_id)
    .bind(receiver_id)
    .fetch_one(executor)
    .await
    .map_err(map_db_error)
}

pub async fn lock_message_request(
    tx: &mut Transaction<'_, Postgres>,
    request_id: Uuid,
) -> Result<Option<MessageRequestRow>, AppError> {
    sqlx::query_as::<_, MessageRequestRow>(&format!(
        "{MESSAGE_REQUEST_SELECT} WHERE r.id = $1 FOR UPDATE OF r"
    ))
    .bind(request_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(map_db_error)
}

pub async fn list_incoming_message_requests(
    executor: impl sqlx::Executor<'_, Database = Postgres>,
    user_id: Uuid,
) -> Result<Vec<MessageRequestRow>, AppError> {
    sqlx::query_as::<_, MessageRequestRow>(&format!(
        "{MESSAGE_REQUEST_SELECT}
         WHERE r.receiver_id = $1 AND r.status = 'pending'
         ORDER BY r.created_at"
    ))
    .bind(user_id)
    .fetch_all(executor)
    .await
    .map_err(map_db_error)
}

pub async fn decide_message_request(
    tx: &mut Transaction<'_, Postgres>,
    request_id: Uuid,
    status: &str,
    now: DateTime<Utc>,
) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE message_requests SET status = $2, updated_at = $3
         WHERE id = $1 AND status = 'pending'",
    )
    .bind(request_id)
    .bind(status)
    .bind(now)
    .execute(&mut **tx)
    .await
    .map_err(map_db_error)?;
    Ok(())
}

pub async fn reject_pending_message_requests_between(
    tx: &mut Transaction<'_, Postgres>,
    user_a: Uuid,
    user_b: Uuid,
    now: DateTime<Utc>,
) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE message_requests
         SET status = 'rejected', updated_at = $3
         WHERE status = 'pending'
           AND ((sender_id = $1 AND receiver_id = $2)
             OR (sender_id = $2 AND receiver_id = $1))",
    )
    .bind(user_a)
    .bind(user_b)
    .bind(now)
    .execute(&mut **tx)
    .await
    .map_err(map_db_error)?;
    Ok(())
}

pub async fn get_direct_message(
    executor: impl sqlx::Executor<'_, Database = Postgres>,
    message_id: Uuid,
) -> Result<Option<DirectMessageRow>, AppError> {
    sqlx::query_as::<_, DirectMessageRow>(
        "SELECT id, sender_id, receiver_id, content, message_type, created_at, read_at
         FROM direct_messages WHERE id = $1",
    )
    .bind(message_id)
    .fetch_optional(executor)
    .await
    .map_err(map_db_error)
}

pub async fn insert_direct_message(
    tx: &mut Transaction<'_, Postgres>,
    row: &DirectMessageRow,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO direct_messages (
            id, sender_id, receiver_id, content, message_type, created_at, read_at
         ) VALUES ($1,$2,$3,$4,$5,$6,$7)",
    )
    .bind(row.id)
    .bind(row.sender_id)
    .bind(row.receiver_id)
    .bind(&row.content)
    .bind(&row.message_type)
    .bind(row.created_at)
    .bind(row.read_at)
    .execute(&mut **tx)
    .await
    .map_err(map_db_error)?;
    Ok(())
}

pub async fn list_direct_messages(
    executor: impl sqlx::Executor<'_, Database = Postgres>,
    user_a: Uuid,
    user_b: Uuid,
    before: Option<Uuid>,
    after: Option<Uuid>,
    limit: i64,
) -> Result<Vec<DirectMessageRow>, AppError> {
    sqlx::query_as::<_, DirectMessageRow>(
        "SELECT id, sender_id, receiver_id, content, message_type, created_at, read_at
         FROM direct_messages
         WHERE ((sender_id = $1 AND receiver_id = $2) OR (sender_id = $2 AND receiver_id = $1))
           AND ($3::uuid IS NULL OR id < $3)
           AND ($4::uuid IS NULL OR id > $4)
         ORDER BY created_at DESC, id DESC
         LIMIT $5",
    )
    .bind(user_a)
    .bind(user_b)
    .bind(before)
    .bind(after)
    .bind(limit)
    .fetch_all(executor)
    .await
    .map_err(map_db_error)
}

pub async fn mark_incoming_read(
    executor: impl sqlx::Executor<'_, Database = Postgres>,
    reader_id: Uuid,
    other_id: Uuid,
    now: DateTime<Utc>,
) -> Result<u64, AppError> {
    let result = sqlx::query(
        "UPDATE direct_messages SET read_at = $3
         WHERE receiver_id = $1 AND sender_id = $2 AND read_at IS NULL",
    )
    .bind(reader_id)
    .bind(other_id)
    .bind(now)
    .execute(executor)
    .await
    .map_err(map_db_error)?;
    Ok(result.rows_affected())
}
