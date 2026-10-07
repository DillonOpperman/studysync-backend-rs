use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::db::social;
use crate::error::AppError;

/// True if either user has blocked the other. Callers must use this instead of
/// ad-hoc SQL in handlers. Future WebSocket code should reuse this helper.
pub async fn is_blocked(
    tx: &mut Transaction<'_, Postgres>,
    user_a: Uuid,
    user_b: Uuid,
) -> Result<bool, AppError> {
    social::is_blocked(&mut **tx, user_a, user_b).await
}

pub async fn are_friends(
    tx: &mut Transaction<'_, Postgres>,
    user_a: Uuid,
    user_b: Uuid,
) -> Result<bool, AppError> {
    social::are_friends(&mut **tx, user_a, user_b).await
}

pub async fn require_not_blocked(
    tx: &mut Transaction<'_, Postgres>,
    user_a: Uuid,
    user_b: Uuid,
) -> Result<(), AppError> {
    if is_blocked(tx, user_a, user_b).await? {
        return Err(AppError::UserBlocked);
    }
    Ok(())
}
