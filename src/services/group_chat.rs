use std::collections::{HashMap, HashSet};

use chrono::{Duration, Utc};
use uuid::Uuid;

use crate::api::dto::{
    CursorPageDto, GroupMessageDto, GroupMessagePageDto, MediaDto, ReactionCountDto,
    ReactionStateDto,
};
use crate::auth::{is_blocked, require_group_member};
use crate::chat::ServerEvent;
use crate::db::group_chat::{self, GroupMessageRow, MAX_CONTENT_CHARS, MAX_EMOJI_CHARS};
use crate::db::groups::{self, GROUP_STATUS_ACTIVE};
use crate::db::identity;
use crate::db::notifications;
use crate::error::AppError;
use crate::AppState;

/// Sender may edit for 15 minutes after `created_at` (UTC).
pub const EDIT_WINDOW: Duration = Duration::minutes(15);

pub fn validate_content(content: &str) -> Result<String, AppError> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Err(AppError::validation("content is required"));
    }
    if trimmed.chars().count() > MAX_CONTENT_CHARS {
        return Err(AppError::validation(format!(
            "content must be at most {MAX_CONTENT_CHARS} characters"
        )));
    }
    Ok(trimmed.to_string())
}

pub fn validate_message_type(message_type: Option<&str>) -> Result<String, AppError> {
    let value = message_type.unwrap_or(group_chat::TYPE_TEXT).trim();
    match value {
        "text" | "announcement" | "image" | "file" => Ok(value.to_string()),
        _ => Err(AppError::validation(
            "messageType must be text, announcement, image, or file",
        )),
    }
}

pub fn validate_emoji(emoji: &str) -> Result<String, AppError> {
    let trimmed = emoji.trim();
    if trimmed.is_empty() || trimmed.chars().count() > MAX_EMOJI_CHARS {
        return Err(AppError::validation(format!(
            "emoji must be 1 to {MAX_EMOJI_CHARS} characters"
        )));
    }
    Ok(trimmed.to_string())
}

async fn require_active_user(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user_id: Uuid,
) -> Result<(), AppError> {
    identity::get_user_by_id(&mut **tx, user_id)
        .await?
        .filter(|row| row.deleted_at.is_none())
        .ok_or(AppError::Unauthorized)?;
    Ok(())
}

async fn lock_active_group(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    group_id: Uuid,
) -> Result<groups::StudyGroupRow, AppError> {
    let group = groups::lock_group(tx, group_id)
        .await?
        .ok_or(AppError::GroupNotFound)?;
    if group.status != GROUP_STATUS_ACTIVE {
        return Err(AppError::GroupArchived);
    }
    Ok(group)
}

fn message_dto(
    row: &GroupMessageRow,
    reactions: Vec<ReactionCountDto>,
    attachments: Vec<MediaDto>,
) -> GroupMessageDto {
    GroupMessageDto {
        id: row.id,
        group_id: row.group_id,
        sender_id: row.sender_id,
        client_msg_id: row.client_msg_id,
        content: row.content.clone(),
        message_type: row.message_type.clone(),
        created_at: row.created_at,
        edited_at: row.edited_at,
        deleted_at: row.deleted_at,
        reactions,
        attachments,
    }
}

async fn reactions_map(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ids: &[Uuid],
    viewer_id: Uuid,
) -> Result<HashMap<Uuid, Vec<ReactionCountDto>>, AppError> {
    let rows = group_chat::reactions_for_messages(&mut **tx, ids, viewer_id).await?;
    let mut map: HashMap<Uuid, Vec<ReactionCountDto>> = HashMap::new();
    for row in rows {
        map.entry(row.message_id)
            .or_default()
            .push(ReactionCountDto {
                emoji: row.emoji,
                count: row.count,
                me: row.me,
            });
    }
    Ok(map)
}

async fn dto_with_reactions(
    state: &AppState,
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    row: &GroupMessageRow,
    viewer_id: Uuid,
) -> Result<GroupMessageDto, AppError> {
    let mut map = reactions_map(tx, &[row.id], viewer_id).await?;
    let mut attachments =
        crate::services::media::dtos_for_group_messages(state, tx, &[row.id]).await?;
    Ok(message_dto(
        row,
        map.remove(&row.id).unwrap_or_default(),
        attachments.remove(&row.id).unwrap_or_default(),
    ))
}

async fn skip_blocked(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user_id: Uuid,
) -> Result<HashSet<Uuid>, AppError> {
    Ok(group_chat::blocked_user_ids(&mut **tx, user_id)
        .await?
        .into_iter()
        .collect())
}

pub async fn send_group_message(
    state: &AppState,
    sender_id: Uuid,
    group_id: Uuid,
    client_msg_id: Uuid,
    content: String,
    message_type: Option<&str>,
    attachment_ids: Vec<Uuid>,
) -> Result<GroupMessageDto, AppError> {
    let message_type = validate_message_type(message_type)?;
    let content = if matches!(message_type.as_str(), "image" | "file") {
        let trimmed = content.trim();
        if trimmed.chars().count() > MAX_CONTENT_CHARS {
            return Err(AppError::validation(format!(
                "content must be at most {MAX_CONTENT_CHARS} characters"
            )));
        }
        trimmed.to_string()
    } else {
        validate_content(&content)?
    };
    if matches!(message_type.as_str(), "image" | "file") && attachment_ids.is_empty() {
        return Err(AppError::validation(
            "image and file messages require attachments",
        ));
    }
    if !attachment_ids.is_empty() && !matches!(message_type.as_str(), "image" | "file") {
        return Err(AppError::validation(
            "attachments are only allowed on image or file messages",
        ));
    }
    let mut tx = state.db.begin().await.map_err(crate::error::map_db_error)?;
    require_active_user(&mut tx, sender_id).await?;
    lock_active_group(&mut tx, group_id).await?;
    let member = require_group_member(&mut tx, group_id, sender_id).await?;
    if message_type == group_chat::TYPE_ANNOUNCEMENT && member.role != groups::MEMBER_ROLE_LEADER {
        return Err(AppError::NotGroupLeader);
    }
    if !attachment_ids.is_empty() {
        crate::services::media::lock_and_validate_group_attachments(
            &mut tx,
            sender_id,
            group_id,
            &attachment_ids,
            &message_type,
        )
        .await?;
    }

    let id = Uuid::now_v7();
    let inserted = group_chat::insert_message(
        &mut tx,
        id,
        group_id,
        sender_id,
        client_msg_id,
        &content,
        &message_type,
    )
    .await?;
    let (row, created) = match inserted {
        Ok(row) => (row, true),
        Err(_) => {
            let existing =
                group_chat::get_by_idempotency(&mut tx, group_id, sender_id, client_msg_id)
                    .await?
                    .ok_or(AppError::Internal)?;
            (existing, false)
        }
    };

    if created && !attachment_ids.is_empty() {
        crate::db::media::attach_to_group_message(&mut tx, &attachment_ids, row.id, Utc::now())
            .await?;
    }

    if created && message_type == group_chat::TYPE_ANNOUNCEMENT {
        let members = groups::list_active_members(&mut *tx, group_id).await?;
        for other in members.into_iter().filter(|m| m.user_id != sender_id) {
            notifications::insert_notification(
                &mut tx,
                other.user_id,
                "group_announcement",
                "Group announcement",
                "A group announcement was posted.",
                serde_json::json!({
                    "groupId": group_id,
                    "messageId": row.id
                }),
            )
            .await?;
        }
    }

    let dto = dto_with_reactions(state, &mut tx, &row, sender_id).await?;
    let skip = skip_blocked(&mut tx, sender_id).await?;
    tx.commit().await.map_err(crate::error::map_db_error)?;

    if created {
        state
            .chat
            .broadcast(
                group_id,
                &ServerEvent::MessageCreated {
                    payload: dto.clone(),
                },
                &skip,
            )
            .await;
    }
    Ok(dto)
}

pub async fn list_group_messages(
    state: &AppState,
    user_id: Uuid,
    group_id: Uuid,
    before: Option<Uuid>,
    after: Option<Uuid>,
    limit: Option<i64>,
) -> Result<GroupMessagePageDto, AppError> {
    if before.is_some() && after.is_some() {
        return Err(AppError::validation("provide only one of before or after"));
    }
    let limit = limit.unwrap_or(50).clamp(1, 100);
    let mut tx = state.db.begin().await.map_err(crate::error::map_db_error)?;
    require_active_user(&mut tx, user_id).await?;
    let group = groups::get_group(&mut *tx, group_id)
        .await?
        .ok_or(AppError::GroupNotFound)?;
    if group.status != GROUP_STATUS_ACTIVE {
        return Err(AppError::GroupArchived);
    }
    require_group_member(&mut tx, group_id, user_id).await?;

    if let Some(cursor) = before.or(after) {
        let cursor_row = group_chat::get_by_id(&mut *tx, cursor)
            .await?
            .ok_or(AppError::validation("cursor message was not found"))?;
        if cursor_row.group_id != group_id {
            return Err(AppError::validation(
                "cursor message does not belong to this group",
            ));
        }
    }

    let mut rows = group_chat::list_page(&mut *tx, group_id, before, after, limit + 1).await?;
    let has_more = rows.len() as i64 > limit;
    if has_more {
        rows.pop();
    }
    rows.reverse();
    let ids: Vec<Uuid> = rows.iter().map(|row| row.id).collect();
    let mut map = reactions_map(&mut tx, &ids, user_id).await?;
    let mut attachments =
        crate::services::media::dtos_for_group_messages(state, &mut tx, &ids).await?;
    tx.commit().await.map_err(crate::error::map_db_error)?;

    let messages: Vec<GroupMessageDto> = rows
        .iter()
        .map(|row| {
            message_dto(
                row,
                map.remove(&row.id).unwrap_or_default(),
                attachments.remove(&row.id).unwrap_or_default(),
            )
        })
        .collect();
    let next_before = messages.first().map(|m| m.id);
    let next_after = messages.last().map(|m| m.id);
    Ok(GroupMessagePageDto {
        messages,
        pagination: CursorPageDto {
            limit,
            has_more,
            next_before,
            next_after,
        },
    })
}

pub async fn edit_group_message(
    state: &AppState,
    user_id: Uuid,
    message_id: Uuid,
    content: String,
) -> Result<GroupMessageDto, AppError> {
    let content = validate_content(&content)?;
    let mut tx = state.db.begin().await.map_err(crate::error::map_db_error)?;
    require_active_user(&mut tx, user_id).await?;
    let row = group_chat::lock_by_id(&mut tx, message_id)
        .await?
        .ok_or(AppError::MessageNotFound)?;
    lock_active_group(&mut tx, row.group_id).await?;
    require_group_member(&mut tx, row.group_id, user_id).await?;
    if row.sender_id != user_id {
        return Err(AppError::CannotModifyMessage);
    }
    if row.deleted_at.is_some() {
        return Err(AppError::MessageDeleted);
    }
    if Utc::now() - row.created_at > EDIT_WINDOW {
        return Err(AppError::EditWindowExpired);
    }
    let updated = group_chat::update_content(&mut tx, message_id, &content, Utc::now()).await?;
    let dto = dto_with_reactions(state, &mut tx, &updated, user_id).await?;
    let skip = skip_blocked(&mut tx, user_id).await?;
    let group_id = dto.group_id;
    tx.commit().await.map_err(crate::error::map_db_error)?;
    state
        .chat
        .broadcast(
            group_id,
            &ServerEvent::MessageEdited {
                payload: dto.clone(),
            },
            &skip,
        )
        .await;
    Ok(dto)
}

pub async fn delete_group_message(
    state: &AppState,
    user_id: Uuid,
    message_id: Uuid,
) -> Result<GroupMessageDto, AppError> {
    let mut tx = state.db.begin().await.map_err(crate::error::map_db_error)?;
    require_active_user(&mut tx, user_id).await?;
    let row = group_chat::lock_by_id(&mut tx, message_id)
        .await?
        .ok_or(AppError::MessageNotFound)?;
    lock_active_group(&mut tx, row.group_id).await?;
    let member = require_group_member(&mut tx, row.group_id, user_id).await?;
    let is_sender = row.sender_id == user_id;
    let is_leader = member.role == groups::MEMBER_ROLE_LEADER;
    if !is_sender && !is_leader {
        return Err(AppError::CannotModifyMessage);
    }
    let already = row.deleted_at.is_some();
    let updated = if already {
        row
    } else {
        group_chat::soft_delete(&mut tx, message_id, Utc::now()).await?
    };
    let removed = if already {
        Vec::new()
    } else {
        crate::db::media::mark_deleted_for_group_message(&mut tx, message_id, Utc::now()).await?
    };
    let dto = dto_with_reactions(state, &mut tx, &updated, user_id).await?;
    let skip = skip_blocked(&mut tx, user_id).await?;
    let group_id = dto.group_id;
    tx.commit().await.map_err(crate::error::map_db_error)?;
    for media in removed {
        let _ = state.storage.delete(&media.storage_key).await;
    }
    if !already {
        state
            .chat
            .broadcast(
                group_id,
                &ServerEvent::MessageDeleted {
                    payload: dto.clone(),
                },
                &skip,
            )
            .await;
    }
    Ok(dto)
}

pub async fn toggle_reaction(
    state: &AppState,
    user_id: Uuid,
    message_id: Uuid,
    emoji: String,
) -> Result<ReactionStateDto, AppError> {
    let emoji = validate_emoji(&emoji)?;
    let mut tx = state.db.begin().await.map_err(crate::error::map_db_error)?;
    require_active_user(&mut tx, user_id).await?;
    let row = group_chat::lock_by_id(&mut tx, message_id)
        .await?
        .ok_or(AppError::MessageNotFound)?;
    lock_active_group(&mut tx, row.group_id).await?;
    require_group_member(&mut tx, row.group_id, user_id).await?;
    if row.deleted_at.is_some() {
        return Err(AppError::MessageDeleted);
    }
    if is_blocked(&mut tx, user_id, row.sender_id).await? {
        return Err(AppError::UserBlocked);
    }
    group_chat::toggle_reaction(&mut tx, message_id, user_id, &emoji).await?;
    let mut map = reactions_map(&mut tx, &[message_id], user_id).await?;
    let reactions = map.remove(&message_id).unwrap_or_default();
    let skip = skip_blocked(&mut tx, user_id).await?;
    let group_id = row.group_id;
    tx.commit().await.map_err(crate::error::map_db_error)?;

    let payload = ReactionStateDto {
        message_id,
        group_id,
        reactions,
    };
    state
        .chat
        .broadcast(
            group_id,
            &ServerEvent::ReactionUpdated {
                payload: payload.clone(),
            },
            &skip,
        )
        .await;
    Ok(payload)
}
