use std::sync::Arc;
use std::time::Duration;

use sqlx::PgPool;

use super::{
    category_for_type, preference_allows, public_preview, FcmSendError, FcmSender, PushRuntime,
    SendOutcome,
};
use crate::db::push::{self, OutboxRow};

pub const MAX_ATTEMPTS: i32 = 8;
const BATCH_SIZE: i64 = 8;
const TICK: Duration = Duration::from_secs(2);
const SEND_GAP: Duration = Duration::from_millis(80);

pub fn spawn_worker(pool: PgPool, runtime: Arc<PushRuntime>) {
    if !runtime.delivery_enabled {
        tracing::info!("push worker not started; delivery is disabled");
        return;
    }
    tokio::spawn(async move {
        tracing::info!("push outbox worker started");
        loop {
            if let Err(error) = process_due_batch(&pool, runtime.sender.as_ref(), BATCH_SIZE).await
            {
                tracing::error!(code = error.code(), "push outbox tick failed");
            }
            tokio::time::sleep(TICK).await;
        }
    });
}

pub async fn process_due_batch(
    pool: &PgPool,
    sender: &dyn FcmSender,
    limit: i64,
) -> Result<usize, crate::error::AppError> {
    push::requeue_stale_sending(pool).await?;
    let claimed = push::claim_due(pool, limit).await?;
    let mut processed = 0;
    for row in claimed {
        deliver_one(pool, sender, row).await?;
        processed += 1;
        tokio::time::sleep(SEND_GAP).await;
    }
    Ok(processed)
}

async fn deliver_one(
    pool: &PgPool,
    sender: &dyn FcmSender,
    row: OutboxRow,
) -> Result<(), crate::error::AppError> {
    let Some(note) =
        crate::db::notifications::get_owned(pool, row.user_id, row.notification_id).await?
    else {
        push::mark_status(pool, row.id, "skipped", Some("missing_notification")).await?;
        return Ok(());
    };
    let prefs = push::get_or_default_preferences(pool, row.user_id).await?;
    let Some(category) = category_for_type(&note.ntype) else {
        push::mark_status(pool, row.id, "skipped", Some("no_category")).await?;
        return Ok(());
    };
    if !prefs.push_enabled
        || !preference_allows(
            category,
            prefs.friends,
            prefs.groups,
            prefs.messages,
            prefs.announcements,
        )
    {
        push::mark_status(pool, row.id, "skipped", Some("preferences")).await?;
        return Ok(());
    }
    let Some((title, body)) = public_preview(&note.ntype) else {
        push::mark_status(pool, row.id, "skipped", Some("no_preview")).await?;
        return Ok(());
    };
    let devices = push::list_active_tokens(pool, row.user_id).await?;
    if devices.is_empty() {
        push::mark_status(pool, row.id, "skipped", Some("no_devices")).await?;
        return Ok(());
    }

    let mut transient = false;
    let mut delivered_any = false;
    for device in devices {
        match sender
            .send(&device.fcm_token, row.notification_id, title, body)
            .await
        {
            Ok(SendOutcome::Delivered) => {
                delivered_any = true;
            }
            Ok(SendOutcome::Unregistered) => {
                push::revoke_device(pool, device.id).await?;
            }
            Err(FcmSendError::Transient) => {
                transient = true;
            }
            Err(FcmSendError::Permanent) => {
                tracing::warn!(outbox_id = %row.id, "fcm permanent failure for a device");
            }
        }
    }

    if transient && !delivered_any {
        schedule_retry(pool, &row, "transient").await?;
        return Ok(());
    }
    if delivered_any {
        push::mark_delivered(pool, row.id).await?;
    } else {
        push::mark_status(pool, row.id, "skipped", Some("no_valid_devices")).await?;
    }
    Ok(())
}

async fn schedule_retry(
    pool: &PgPool,
    row: &OutboxRow,
    class: &str,
) -> Result<(), crate::error::AppError> {
    if row.attempts >= MAX_ATTEMPTS {
        push::mark_status(pool, row.id, "failed", Some(class)).await?;
        return Ok(());
    }
    let secs = backoff_secs(row.attempts);
    push::requeue_pending(pool, row.id, secs, class).await?;
    Ok(())
}

fn backoff_secs(attempts: i32) -> i64 {
    let exp = attempts.saturating_sub(1).clamp(0, 10);
    (30_i64.saturating_mul(1_i64 << exp)).min(3600)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_is_bounded() {
        assert_eq!(backoff_secs(1), 30);
        assert_eq!(backoff_secs(2), 60);
        assert!(backoff_secs(20) <= 3600);
    }
}
