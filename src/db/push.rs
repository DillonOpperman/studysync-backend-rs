use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::error::{map_db_error, AppError};

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct DeviceRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub platform: String,
    pub fcm_token: String,
    pub token_fingerprint: String,
    pub installation_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct PreferenceRow {
    pub user_id: Uuid,
    pub push_enabled: bool,
    pub friends: bool,
    pub groups: bool,
    pub messages: bool,
    pub announcements: bool,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct OutboxRow {
    pub id: Uuid,
    pub notification_id: Uuid,
    pub user_id: Uuid,
    pub status: String,
    pub attempts: i32,
    pub next_attempt_at: DateTime<Utc>,
    pub last_error_class: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub delivered_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct DeviceTokenRow {
    pub id: Uuid,
    pub fcm_token: String,
}

pub async fn enqueue_outbox_tx(
    tx: &mut Transaction<'_, Postgres>,
    notification_id: Uuid,
    user_id: Uuid,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO push_outbox (id, notification_id, user_id, status)
         VALUES ($1, $2, $3, 'pending')
         ON CONFLICT (notification_id) DO NOTHING",
    )
    .bind(Uuid::now_v7())
    .bind(notification_id)
    .bind(user_id)
    .execute(&mut **tx)
    .await
    .map_err(map_db_error)?;
    Ok(())
}

pub async fn enqueue_outbox(
    pool: &PgPool,
    notification_id: Uuid,
    user_id: Uuid,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO push_outbox (id, notification_id, user_id, status)
         VALUES ($1, $2, $3, 'pending')
         ON CONFLICT (notification_id) DO NOTHING",
    )
    .bind(Uuid::now_v7())
    .bind(notification_id)
    .bind(user_id)
    .execute(pool)
    .await
    .map_err(map_db_error)?;
    Ok(())
}

pub async fn find_active_by_fingerprint(
    pool: &PgPool,
    fingerprint: &str,
) -> Result<Option<DeviceRow>, AppError> {
    sqlx::query_as::<_, DeviceRow>(
        "SELECT id, user_id, platform, fcm_token, token_fingerprint, installation_id,
                created_at, updated_at, last_seen_at, revoked_at
         FROM push_devices
         WHERE token_fingerprint = $1 AND revoked_at IS NULL",
    )
    .bind(fingerprint)
    .fetch_optional(pool)
    .await
    .map_err(map_db_error)
}

pub async fn find_active_by_installation(
    pool: &PgPool,
    user_id: Uuid,
    installation_id: &str,
) -> Result<Option<DeviceRow>, AppError> {
    sqlx::query_as::<_, DeviceRow>(
        "SELECT id, user_id, platform, fcm_token, token_fingerprint, installation_id,
                created_at, updated_at, last_seen_at, revoked_at
         FROM push_devices
         WHERE user_id = $1 AND installation_id = $2 AND revoked_at IS NULL",
    )
    .bind(user_id)
    .bind(installation_id)
    .fetch_optional(pool)
    .await
    .map_err(map_db_error)
}

pub async fn insert_device(
    pool: &PgPool,
    id: Uuid,
    user_id: Uuid,
    platform: &str,
    token: &str,
    fingerprint: &str,
    installation_id: Option<&str>,
) -> Result<DeviceRow, AppError> {
    sqlx::query_as::<_, DeviceRow>(
        "INSERT INTO push_devices
            (id, user_id, platform, fcm_token, token_fingerprint, installation_id)
         VALUES ($1,$2,$3,$4,$5,$6)
         RETURNING id, user_id, platform, fcm_token, token_fingerprint, installation_id,
                   created_at, updated_at, last_seen_at, revoked_at",
    )
    .bind(id)
    .bind(user_id)
    .bind(platform)
    .bind(token)
    .bind(fingerprint)
    .bind(installation_id)
    .fetch_one(pool)
    .await
    .map_err(map_db_error)
}

pub async fn update_device_token(
    pool: &PgPool,
    id: Uuid,
    user_id: Uuid,
    platform: &str,
    token: &str,
    fingerprint: &str,
    installation_id: Option<&str>,
) -> Result<Option<DeviceRow>, AppError> {
    sqlx::query_as::<_, DeviceRow>(
        "UPDATE push_devices
         SET platform = $3,
             fcm_token = $4,
             token_fingerprint = $5,
             installation_id = COALESCE($6, installation_id),
             updated_at = now(),
             last_seen_at = now()
         WHERE id = $1 AND user_id = $2 AND revoked_at IS NULL
         RETURNING id, user_id, platform, fcm_token, token_fingerprint, installation_id,
                   created_at, updated_at, last_seen_at, revoked_at",
    )
    .bind(id)
    .bind(user_id)
    .bind(platform)
    .bind(token)
    .bind(fingerprint)
    .bind(installation_id)
    .fetch_optional(pool)
    .await
    .map_err(map_db_error)
}

pub async fn touch_device(
    pool: &PgPool,
    id: Uuid,
    user_id: Uuid,
) -> Result<Option<DeviceRow>, AppError> {
    sqlx::query_as::<_, DeviceRow>(
        "UPDATE push_devices SET last_seen_at = now(), updated_at = now()
         WHERE id = $1 AND user_id = $2 AND revoked_at IS NULL
         RETURNING id, user_id, platform, fcm_token, token_fingerprint, installation_id,
                   created_at, updated_at, last_seen_at, revoked_at",
    )
    .bind(id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(map_db_error)
}

pub async fn get_owned_device(
    pool: &PgPool,
    user_id: Uuid,
    device_id: Uuid,
) -> Result<Option<DeviceRow>, AppError> {
    sqlx::query_as::<_, DeviceRow>(
        "SELECT id, user_id, platform, fcm_token, token_fingerprint, installation_id,
                created_at, updated_at, last_seen_at, revoked_at
         FROM push_devices
         WHERE id = $1 AND user_id = $2 AND revoked_at IS NULL",
    )
    .bind(device_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(map_db_error)
}

pub async fn revoke_device(pool: &PgPool, device_id: Uuid) -> Result<(), AppError> {
    sqlx::query("UPDATE push_devices SET revoked_at = now(), updated_at = now() WHERE id = $1 AND revoked_at IS NULL")
        .bind(device_id)
        .execute(pool)
        .await
        .map_err(map_db_error)?;
    Ok(())
}

pub async fn revoke_owned_device(
    pool: &PgPool,
    user_id: Uuid,
    device_id: Uuid,
) -> Result<u64, AppError> {
    let result = sqlx::query(
        "UPDATE push_devices SET revoked_at = now(), updated_at = now()
         WHERE id = $1 AND user_id = $2 AND revoked_at IS NULL",
    )
    .bind(device_id)
    .bind(user_id)
    .execute(pool)
    .await
    .map_err(map_db_error)?;
    Ok(result.rows_affected())
}

pub async fn list_active_tokens(
    pool: &PgPool,
    user_id: Uuid,
) -> Result<Vec<DeviceTokenRow>, AppError> {
    sqlx::query_as::<_, DeviceTokenRow>(
        "SELECT id, fcm_token FROM push_devices WHERE user_id = $1 AND revoked_at IS NULL",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
    .map_err(map_db_error)
}

pub async fn count_active_devices(pool: &PgPool, user_id: Uuid) -> Result<i64, AppError> {
    sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM push_devices WHERE user_id = $1 AND revoked_at IS NULL",
    )
    .bind(user_id)
    .fetch_one(pool)
    .await
    .map_err(map_db_error)
}

pub async fn get_or_default_preferences(
    pool: &PgPool,
    user_id: Uuid,
) -> Result<PreferenceRow, AppError> {
    if let Some(row) = sqlx::query_as::<_, PreferenceRow>(
        "SELECT user_id, push_enabled, friends, groups, messages, announcements, updated_at
         FROM push_preferences WHERE user_id = $1",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(map_db_error)?
    {
        return Ok(row);
    }
    sqlx::query_as::<_, PreferenceRow>(
        "INSERT INTO push_preferences (user_id)
         VALUES ($1)
         ON CONFLICT (user_id) DO UPDATE SET user_id = EXCLUDED.user_id
         RETURNING user_id, push_enabled, friends, groups, messages, announcements, updated_at",
    )
    .bind(user_id)
    .fetch_one(pool)
    .await
    .map_err(map_db_error)
}

pub async fn upsert_preferences(
    pool: &PgPool,
    user_id: Uuid,
    push_enabled: bool,
    friends: bool,
    groups: bool,
    messages: bool,
    announcements: bool,
) -> Result<PreferenceRow, AppError> {
    sqlx::query_as::<_, PreferenceRow>(
        "INSERT INTO push_preferences
            (user_id, push_enabled, friends, groups, messages, announcements, updated_at)
         VALUES ($1,$2,$3,$4,$5,$6, now())
         ON CONFLICT (user_id) DO UPDATE SET
            push_enabled = EXCLUDED.push_enabled,
            friends = EXCLUDED.friends,
            groups = EXCLUDED.groups,
            messages = EXCLUDED.messages,
            announcements = EXCLUDED.announcements,
            updated_at = now()
         RETURNING user_id, push_enabled, friends, groups, messages, announcements, updated_at",
    )
    .bind(user_id)
    .bind(push_enabled)
    .bind(friends)
    .bind(groups)
    .bind(messages)
    .bind(announcements)
    .fetch_one(pool)
    .await
    .map_err(map_db_error)
}

pub async fn claim_due(pool: &PgPool, limit: i64) -> Result<Vec<OutboxRow>, AppError> {
    sqlx::query_as::<_, OutboxRow>(
        "WITH next AS (
            SELECT id FROM push_outbox
            WHERE status = 'pending' AND next_attempt_at <= now()
            ORDER BY created_at
            LIMIT $1
            FOR UPDATE SKIP LOCKED
         )
         UPDATE push_outbox o
         SET status = 'sending', attempts = attempts + 1, updated_at = now()
         FROM next
         WHERE o.id = next.id
         RETURNING o.id, o.notification_id, o.user_id, o.status, o.attempts, o.next_attempt_at,
                   o.last_error_class, o.created_at, o.updated_at, o.delivered_at",
    )
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(map_db_error)
}

pub async fn requeue_stale_sending(pool: &PgPool) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE push_outbox
         SET status = 'pending', updated_at = now()
         WHERE status = 'sending' AND updated_at < now() - interval '5 minutes'",
    )
    .execute(pool)
    .await
    .map_err(map_db_error)?;
    Ok(())
}

pub async fn mark_status(
    pool: &PgPool,
    id: Uuid,
    status: &str,
    last_error_class: Option<&str>,
) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE push_outbox
         SET status = $2, last_error_class = $3, updated_at = now()
         WHERE id = $1",
    )
    .bind(id)
    .bind(status)
    .bind(last_error_class)
    .execute(pool)
    .await
    .map_err(map_db_error)?;
    Ok(())
}

pub async fn mark_delivered(pool: &PgPool, id: Uuid) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE push_outbox
         SET status = 'delivered', delivered_at = now(), updated_at = now(), last_error_class = NULL
         WHERE id = $1",
    )
    .bind(id)
    .execute(pool)
    .await
    .map_err(map_db_error)?;
    Ok(())
}

pub async fn requeue_pending(
    pool: &PgPool,
    id: Uuid,
    delay_secs: i64,
    class: &str,
) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE push_outbox
         SET status = 'pending',
             next_attempt_at = now() + ($2 * interval '1 second'),
             last_error_class = $3,
             updated_at = now()
         WHERE id = $1",
    )
    .bind(id)
    .bind(delay_secs)
    .bind(class)
    .execute(pool)
    .await
    .map_err(map_db_error)?;
    Ok(())
}

pub async fn get_outbox_for_notification(
    pool: &PgPool,
    notification_id: Uuid,
) -> Result<Option<OutboxRow>, AppError> {
    sqlx::query_as::<_, OutboxRow>(
        "SELECT id, notification_id, user_id, status, attempts, next_attempt_at,
                last_error_class, created_at, updated_at, delivered_at
         FROM push_outbox WHERE notification_id = $1",
    )
    .bind(notification_id)
    .fetch_optional(pool)
    .await
    .map_err(map_db_error)
}
