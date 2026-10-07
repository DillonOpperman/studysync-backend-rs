use chrono::Utc;
use sqlx::PgPool;
use uuid::Uuid;

use crate::api::dto::{
    DirectMessageDto, MessageRequestDto, SendDirectMessageRequest, SendDmOutcome, UserSummaryDto,
};
use crate::auth::social::{are_friends, require_not_blocked};
use crate::db::groups;
use crate::db::identity;
use crate::db::notifications;
use crate::db::social::{self, DirectMessageRow, MessageRequestRow};
use crate::error::AppError;

const MAX_CONTENT_LEN: usize = 4000;

/// Send a DM when authorized, otherwise create a message request if the
/// recipient is an active study-group leader. WebSocket handlers should call
/// this same function rather than writing SQL.
pub async fn send_direct_message(
    pool: &PgPool,
    sender_id: Uuid,
    request: SendDirectMessageRequest,
) -> Result<SendDmOutcome, AppError> {
    if sender_id == request.recipient_id {
        return Err(AppError::validation("cannot message yourself"));
    }
    let content = request.content.trim();
    if content.is_empty() {
        return Err(AppError::validation("content is required"));
    }
    if content.len() > MAX_CONTENT_LEN {
        return Err(AppError::validation("content is too long"));
    }
    let message_type = request.message_type.as_deref().unwrap_or("text");
    if message_type != "text" {
        return Err(AppError::validation(
            "only text messages are supported until the media phase",
        ));
    }

    let now = Utc::now();
    let mut tx = pool.begin().await.map_err(crate::error::map_db_error)?;
    require_active(&mut tx, sender_id).await?;
    require_active(&mut tx, request.recipient_id).await?;
    require_not_blocked(&mut tx, sender_id, request.recipient_id).await?;

    if can_send_dm(&mut tx, sender_id, request.recipient_id).await? {
        let message_id = Uuid::now_v7();
        let row = DirectMessageRow {
            id: message_id,
            sender_id,
            receiver_id: request.recipient_id,
            content: content.to_string(),
            message_type: "text".to_string(),
            created_at: now,
            read_at: None,
        };
        social::insert_direct_message(&mut tx, &row).await?;
        notifications::insert_notification(
            &mut tx,
            request.recipient_id,
            "direct_message",
            "New message",
            "You received a direct message.",
            serde_json::json!({ "messageId": message_id, "senderId": sender_id }),
        )
        .await?;
        tx.commit().await.map_err(crate::error::map_db_error)?;
        return Ok(SendDmOutcome::DirectMessage {
            message: message_dto(&row),
        });
    }

    if !groups::user_is_active_group_leader(&mut *tx, request.recipient_id).await? {
        return Err(AppError::CannotMessage);
    }

    if let Some(existing) =
        social::find_pending_message_request(&mut *tx, sender_id, request.recipient_id).await?
    {
        tx.commit().await.map_err(crate::error::map_db_error)?;
        return Ok(SendDmOutcome::ExistingMessageRequest {
            message_request: message_request_dto(&existing),
        });
    }

    let request_id = Uuid::now_v7();
    match social::insert_message_request(&mut tx, request_id, sender_id, request.recipient_id, now)
        .await
    {
        Ok(()) => {}
        Err(AppError::MessageRequestPending) => {
            let existing =
                social::find_pending_message_request(&mut *tx, sender_id, request.recipient_id)
                    .await?
                    .ok_or(AppError::MessageRequestPending)?;
            tx.commit().await.map_err(crate::error::map_db_error)?;
            return Ok(SendDmOutcome::ExistingMessageRequest {
                message_request: message_request_dto(&existing),
            });
        }
        Err(error) => return Err(error),
    }
    notifications::insert_notification(
        &mut tx,
        request.recipient_id,
        "message_request",
        "New message request",
        "Someone wants to send you a message.",
        serde_json::json!({ "requestId": request_id, "senderId": sender_id }),
    )
    .await?;
    let row = social::find_pending_message_request(&mut *tx, sender_id, request.recipient_id)
        .await?
        .ok_or(AppError::Internal)?;
    tx.commit().await.map_err(crate::error::map_db_error)?;
    Ok(SendDmOutcome::NewMessageRequest {
        message_request: message_request_dto(&row),
    })
}

pub async fn list_direct_messages(
    pool: &PgPool,
    reader_id: Uuid,
    other_id: Uuid,
    before: Option<Uuid>,
    after: Option<Uuid>,
    limit: Option<i64>,
) -> Result<Vec<DirectMessageDto>, AppError> {
    if reader_id == other_id {
        return Err(AppError::validation(
            "cannot list a conversation with yourself",
        ));
    }
    let limit = limit.unwrap_or(50).clamp(1, 100);
    let mut tx = pool.begin().await.map_err(crate::error::map_db_error)?;
    require_active(&mut tx, other_id).await?;
    require_not_blocked(&mut tx, reader_id, other_id).await?;
    social::mark_incoming_read(&mut *tx, reader_id, other_id, Utc::now()).await?;
    let mut rows =
        social::list_direct_messages(&mut *tx, reader_id, other_id, before, after, limit).await?;
    tx.commit().await.map_err(crate::error::map_db_error)?;
    rows.reverse();
    Ok(rows.iter().map(message_dto).collect())
}

pub async fn list_message_requests(
    pool: &PgPool,
    user_id: Uuid,
) -> Result<Vec<MessageRequestDto>, AppError> {
    let rows = social::list_incoming_message_requests(pool, user_id).await?;
    Ok(rows.iter().map(message_request_dto).collect())
}

pub async fn accept_message_request(
    pool: &PgPool,
    user_id: Uuid,
    request_id: Uuid,
) -> Result<MessageRequestDto, AppError> {
    let now = Utc::now();
    let mut tx = pool.begin().await.map_err(crate::error::map_db_error)?;
    let request = social::lock_message_request(&mut tx, request_id)
        .await?
        .ok_or(AppError::MessageRequestNotFound)?;
    if request.receiver_id != user_id {
        return Err(AppError::MessageRequestNotFound);
    }
    if request.status != social::REQ_PENDING {
        return Err(AppError::MessageRequestDecided);
    }
    require_active(&mut tx, request.sender_id).await?;
    require_not_blocked(&mut tx, user_id, request.sender_id).await?;
    social::decide_message_request(&mut tx, request_id, social::REQ_ACCEPTED, now).await?;
    notifications::insert_notification(
        &mut tx,
        request.sender_id,
        "message_request_accepted",
        "Message request accepted",
        "You can now send direct messages.",
        serde_json::json!({ "requestId": request_id, "userId": user_id }),
    )
    .await?;
    let row = social::lock_message_request(&mut tx, request_id)
        .await?
        .ok_or(AppError::Internal)?;
    tx.commit().await.map_err(crate::error::map_db_error)?;
    Ok(message_request_dto(&row))
}

pub async fn reject_message_request(
    pool: &PgPool,
    user_id: Uuid,
    request_id: Uuid,
) -> Result<MessageRequestDto, AppError> {
    let now = Utc::now();
    let mut tx = pool.begin().await.map_err(crate::error::map_db_error)?;
    let request = social::lock_message_request(&mut tx, request_id)
        .await?
        .ok_or(AppError::MessageRequestNotFound)?;
    if request.receiver_id != user_id {
        return Err(AppError::MessageRequestNotFound);
    }
    if request.status != social::REQ_PENDING {
        return Err(AppError::MessageRequestDecided);
    }
    social::decide_message_request(&mut tx, request_id, social::REQ_REJECTED, now).await?;
    let row = social::lock_message_request(&mut tx, request_id)
        .await?
        .ok_or(AppError::Internal)?;
    tx.commit().await.map_err(crate::error::map_db_error)?;
    Ok(message_request_dto(&row))
}

async fn can_send_dm(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    sender_id: Uuid,
    recipient_id: Uuid,
) -> Result<bool, AppError> {
    if are_friends(tx, sender_id, recipient_id).await? {
        return Ok(true);
    }
    if groups::share_active_group(&mut **tx, sender_id, recipient_id).await? {
        return Ok(true);
    }
    social::message_request_accepted(&mut **tx, sender_id, recipient_id).await
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

fn message_dto(row: &DirectMessageRow) -> DirectMessageDto {
    DirectMessageDto {
        id: row.id,
        sender_id: row.sender_id,
        receiver_id: row.receiver_id,
        content: row.content.clone(),
        message_type: row.message_type.clone(),
        created_at: row.created_at,
        read_at: row.read_at,
    }
}

fn message_request_dto(row: &MessageRequestRow) -> MessageRequestDto {
    MessageRequestDto {
        id: row.id,
        status: row.status.clone(),
        created_at: row.created_at,
        updated_at: row.updated_at,
        sender: UserSummaryDto {
            id: row.sender_id,
            name: row.sender_name.clone().unwrap_or_default(),
            profile_picture_key: row.sender_picture.clone(),
        },
        receiver: UserSummaryDto {
            id: row.receiver_id,
            name: row.receiver_name.clone().unwrap_or_default(),
            profile_picture_key: row.receiver_picture.clone(),
        },
    }
}
