use chrono::Utc;
use sqlx::PgPool;
use uuid::Uuid;

use crate::api::dto::{
    BlockUserRequest, FriendListDto, FriendRequestDto, FriendRequestListDto, UserSummaryDto,
};
use crate::auth::social::{are_friends, require_not_blocked};
use crate::db::identity;
use crate::db::notifications;
use crate::db::social::{self, FriendRequestRow};
use crate::error::AppError;

pub async fn send_request(
    pool: &PgPool,
    requester_id: Uuid,
    recipient_id: Uuid,
) -> Result<FriendRequestDto, AppError> {
    if requester_id == recipient_id {
        return Err(AppError::validation(
            "cannot send a friend request to yourself",
        ));
    }
    let now = Utc::now();
    let request_id = Uuid::now_v7();
    let mut tx = pool.begin().await.map_err(crate::error::map_db_error)?;
    require_active(&mut tx, requester_id).await?;
    require_active(&mut tx, recipient_id).await?;
    require_not_blocked(&mut tx, requester_id, recipient_id).await?;
    if are_friends(&mut tx, requester_id, recipient_id).await? {
        return Err(AppError::AlreadyFriends);
    }

    social::insert_friend_request(&mut tx, request_id, requester_id, recipient_id, now).await?;
    notifications::insert_notification(
        &mut tx,
        recipient_id,
        "friend_request",
        "New friend request",
        "Someone sent you a friend request.",
        serde_json::json!({ "requestId": request_id, "requesterId": requester_id }),
    )
    .await?;
    let row = social::get_friend_request(&mut *tx, request_id)
        .await?
        .ok_or(AppError::Internal)?;
    tx.commit().await.map_err(crate::error::map_db_error)?;
    Ok(friend_request_dto(&row))
}

pub async fn list_requests(pool: &PgPool, user_id: Uuid) -> Result<FriendRequestListDto, AppError> {
    let incoming = social::list_incoming_friend_requests(pool, user_id).await?;
    let outgoing = social::list_outgoing_friend_requests(pool, user_id).await?;
    Ok(FriendRequestListDto {
        incoming: incoming.iter().map(friend_request_dto).collect(),
        outgoing: outgoing.iter().map(friend_request_dto).collect(),
    })
}

pub async fn list_friends(pool: &PgPool, user_id: Uuid) -> Result<Vec<FriendListDto>, AppError> {
    let rows = social::list_friends(pool, user_id).await?;
    Ok(rows
        .into_iter()
        .map(|row| FriendListDto {
            user: UserSummaryDto {
                id: row.user_id,
                name: row.name,
                profile_picture_key: row.profile_picture_key,
            },
            friends_since: row.friends_since,
        })
        .collect())
}

pub async fn accept_request(
    pool: &PgPool,
    user_id: Uuid,
    request_id: Uuid,
) -> Result<FriendRequestDto, AppError> {
    let now = Utc::now();
    let mut tx = pool.begin().await.map_err(crate::error::map_db_error)?;
    let request = social::lock_friend_request(&mut tx, request_id)
        .await?
        .ok_or(AppError::FriendRequestNotFound)?;
    if request.recipient_id != user_id {
        return Err(AppError::FriendRequestNotFound);
    }
    if request.status != social::REQ_PENDING {
        return Err(AppError::FriendRequestDecided);
    }
    require_active(&mut tx, request.requester_id).await?;
    require_not_blocked(&mut tx, user_id, request.requester_id).await?;

    social::insert_friendship(&mut tx, request.requester_id, request.recipient_id, now).await?;
    social::decide_friend_request(&mut tx, request_id, social::REQ_ACCEPTED, now).await?;
    notifications::insert_notification(
        &mut tx,
        request.requester_id,
        "friend_request_accepted",
        "Friend request accepted",
        "Your friend request was accepted.",
        serde_json::json!({ "requestId": request_id, "userId": user_id }),
    )
    .await?;
    let row = social::get_friend_request(&mut *tx, request_id)
        .await?
        .ok_or(AppError::Internal)?;
    tx.commit().await.map_err(crate::error::map_db_error)?;
    Ok(friend_request_dto(&row))
}

pub async fn reject_request(
    pool: &PgPool,
    user_id: Uuid,
    request_id: Uuid,
) -> Result<FriendRequestDto, AppError> {
    let now = Utc::now();
    let mut tx = pool.begin().await.map_err(crate::error::map_db_error)?;
    let request = social::lock_friend_request(&mut tx, request_id)
        .await?
        .ok_or(AppError::FriendRequestNotFound)?;
    if request.recipient_id != user_id {
        return Err(AppError::FriendRequestNotFound);
    }
    if request.status != social::REQ_PENDING {
        return Err(AppError::FriendRequestDecided);
    }
    social::decide_friend_request(&mut tx, request_id, social::REQ_REJECTED, now).await?;
    notifications::insert_notification(
        &mut tx,
        request.requester_id,
        "friend_request_rejected",
        "Friend request declined",
        "Your friend request was declined.",
        serde_json::json!({ "requestId": request_id }),
    )
    .await?;
    let row = social::get_friend_request(&mut *tx, request_id)
        .await?
        .ok_or(AppError::Internal)?;
    tx.commit().await.map_err(crate::error::map_db_error)?;
    Ok(friend_request_dto(&row))
}

pub async fn block_user(
    pool: &PgPool,
    blocker_id: Uuid,
    request: BlockUserRequest,
) -> Result<(), AppError> {
    if blocker_id == request.user_id {
        return Err(AppError::validation("cannot block yourself"));
    }
    let now = Utc::now();
    let mut tx = pool.begin().await.map_err(crate::error::map_db_error)?;
    require_active(&mut tx, request.user_id).await?;
    social::insert_block(&mut tx, blocker_id, request.user_id).await?;
    social::reject_pending_friend_requests_between(&mut tx, blocker_id, request.user_id, now)
        .await?;
    social::reject_pending_message_requests_between(&mut tx, blocker_id, request.user_id, now)
        .await?;
    tx.commit().await.map_err(crate::error::map_db_error)?;
    Ok(())
}

pub async fn unblock_user(
    pool: &PgPool,
    blocker_id: Uuid,
    blocked_id: Uuid,
) -> Result<(), AppError> {
    social::delete_block(pool, blocker_id, blocked_id).await?;
    Ok(())
}

async fn require_active(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user_id: Uuid,
) -> Result<(), AppError> {
    identity::get_user_by_id(&mut **tx, user_id)
        .await?
        .filter(|row| row.deleted_at.is_none())
        .ok_or(AppError::UserNotFound)?;
    Ok(())
}

fn friend_request_dto(row: &FriendRequestRow) -> FriendRequestDto {
    FriendRequestDto {
        id: row.id,
        status: row.status.clone(),
        created_at: row.created_at,
        decided_at: row.decided_at,
        requester: UserSummaryDto {
            id: row.requester_id,
            name: row.requester_name.clone().unwrap_or_default(),
            profile_picture_key: row.requester_picture.clone(),
        },
        recipient: UserSummaryDto {
            id: row.recipient_id,
            name: row.recipient_name.clone().unwrap_or_default(),
            profile_picture_key: row.recipient_picture.clone(),
        },
    }
}
