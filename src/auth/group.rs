use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::db::groups::{self, GroupMemberRow};
use crate::error::AppError;

/// True when the user has an active membership (leader or member).
/// Chat/WebSocket layers should call this rather than trusting client roles.
pub async fn is_active_group_member(
    tx: &mut Transaction<'_, Postgres>,
    group_id: Uuid,
    user_id: Uuid,
) -> Result<bool, AppError> {
    Ok(groups::find_active_membership(&mut **tx, group_id, user_id)
        .await?
        .is_some())
}

pub async fn is_group_leader(
    tx: &mut Transaction<'_, Postgres>,
    group_id: Uuid,
    user_id: Uuid,
) -> Result<bool, AppError> {
    Ok(groups::find_active_membership(&mut **tx, group_id, user_id)
        .await?
        .is_some_and(|row| row.role == groups::MEMBER_ROLE_LEADER))
}

pub async fn require_group_member(
    tx: &mut Transaction<'_, Postgres>,
    group_id: Uuid,
    user_id: Uuid,
) -> Result<GroupMemberRow, AppError> {
    groups::find_active_membership(&mut **tx, group_id, user_id)
        .await?
        .ok_or(AppError::NotGroupMember)
}

pub async fn require_group_leader(
    tx: &mut Transaction<'_, Postgres>,
    group_id: Uuid,
    user_id: Uuid,
) -> Result<GroupMemberRow, AppError> {
    let member = require_group_member(tx, group_id, user_id).await?;
    if member.role != groups::MEMBER_ROLE_LEADER {
        return Err(AppError::NotGroupLeader);
    }
    Ok(member)
}
