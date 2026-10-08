use chrono::{Duration, Utc};
use uuid::Uuid;

use crate::api::dto::MediaDto;
use crate::auth::{require_group_member, require_not_blocked};
use crate::db::groups::{self, GROUP_STATUS_ACTIVE};
use crate::db::identity;
use crate::db::media::{self, MediaRow, PURPOSE_DM, PURPOSE_GROUP, PURPOSE_PROFILE};
use crate::error::AppError;
use crate::media::{detect_payload, sanitize_file_name, KIND_IMAGE};
use crate::services::messages::can_send_dm;
use crate::AppState;

pub struct UploadInput {
    pub purpose: String,
    pub group_id: Option<Uuid>,
    pub recipient_id: Option<Uuid>,
    pub file_name: String,
    pub declared_type: Option<String>,
    pub bytes: Vec<u8>,
}

fn storage_key(
    purpose: &str,
    owner_id: Uuid,
    group_id: Option<Uuid>,
    media_id: Uuid,
    ext: &str,
) -> String {
    match purpose {
        PURPOSE_PROFILE => format!("profiles/{owner_id}/{media_id}.{ext}"),
        PURPOSE_GROUP => format!("groups/{}/{media_id}.{ext}", group_id.expect("group id")),
        _ => format!("direct-messages/{owner_id}/{media_id}.{ext}"),
    }
}

pub fn media_dto(state: &AppState, row: &MediaRow, download_url: Option<String>) -> MediaDto {
    let _ = state;
    MediaDto {
        id: row.id,
        file_name: row.file_name.clone(),
        content_type: row.content_type.clone(),
        byte_size: row.byte_size,
        kind: row.kind.clone(),
        status: row.status.clone(),
        download_url,
    }
}

pub async fn sign_row(state: &AppState, row: &MediaRow) -> Result<MediaDto, AppError> {
    if row.status == media::STATUS_DELETED {
        return Ok(media_dto(state, row, None));
    }
    let url = state
        .storage
        .download_url(&row.storage_key, state.config.media_signed_url_ttl)
        .await?;
    Ok(media_dto(state, row, Some(url)))
}

async fn require_active(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user_id: Uuid,
) -> Result<(), AppError> {
    identity::get_user_by_id(&mut **tx, user_id)
        .await?
        .filter(|row| row.deleted_at.is_none())
        .ok_or(AppError::Unauthorized)?;
    Ok(())
}

pub async fn upload(
    state: &AppState,
    owner_id: Uuid,
    input: UploadInput,
) -> Result<MediaDto, AppError> {
    let max = state.config.media_max_bytes as usize;
    if input.bytes.len() > max {
        return Err(AppError::UploadTooLarge);
    }
    let purpose = input.purpose.trim();
    let image_only = purpose == PURPOSE_PROFILE;
    let detected = detect_payload(
        &input.file_name,
        input.declared_type.as_deref(),
        &input.bytes,
        image_only,
    )?;
    let file_name = sanitize_file_name(&input.file_name)?;
    let media_id = Uuid::now_v7();
    let now = Utc::now();

    let mut tx = state.db.begin().await.map_err(crate::error::map_db_error)?;
    require_active(&mut tx, owner_id).await?;
    let (group_id, peer_id, status) = match purpose {
        PURPOSE_PROFILE => {
            if detected.kind != KIND_IMAGE {
                return Err(AppError::validation("profile pictures must be images"));
            }
            (None, None, media::STATUS_ATTACHED)
        }
        PURPOSE_GROUP => {
            let group_id = input.group_id.ok_or_else(|| {
                AppError::validation("groupId is required for group message uploads")
            })?;
            let group = groups::lock_group(&mut tx, group_id)
                .await?
                .ok_or(AppError::GroupNotFound)?;
            if group.status != GROUP_STATUS_ACTIVE {
                return Err(AppError::GroupArchived);
            }
            require_group_member(&mut tx, group_id, owner_id).await?;
            (Some(group_id), None, media::STATUS_UPLOADED)
        }
        PURPOSE_DM => {
            let peer = input.recipient_id.ok_or_else(|| {
                AppError::validation("recipientId is required for direct message uploads")
            })?;
            if peer == owner_id {
                return Err(AppError::validation("cannot message yourself"));
            }
            require_active(&mut tx, peer)
                .await
                .map_err(|_| AppError::UserNotFound)?;
            require_not_blocked(&mut tx, owner_id, peer).await?;
            if !can_send_dm(&mut tx, owner_id, peer).await? {
                return Err(AppError::CannotMessage);
            }
            (None, Some(peer), media::STATUS_UPLOADED)
        }
        _ => {
            return Err(AppError::validation(
                "purpose must be profile, group_message, or direct_message",
            ))
        }
    };

    let key = storage_key(purpose, owner_id, group_id, media_id, detected.extension);
    let row = MediaRow {
        id: media_id,
        owner_id,
        purpose: purpose.to_string(),
        kind: detected.kind.to_string(),
        status: status.to_string(),
        storage_key: key.clone(),
        file_name,
        content_type: detected.content_type.to_string(),
        byte_size: i32::try_from(input.bytes.len()).unwrap_or(i32::MAX),
        group_id,
        peer_id,
        message_id: None,
        direct_message_id: None,
        created_at: now,
        attached_at: if status == media::STATUS_ATTACHED {
            Some(now)
        } else {
            None
        },
        deleted_at: None,
    };

    if purpose == PURPOSE_PROFILE {
        let user = identity::get_user_by_id(&mut *tx, owner_id)
            .await?
            .ok_or(AppError::Unauthorized)?;
        let old_key = user.profile_picture_key.clone();
        drop(user);
        state
            .storage
            .put(&key, &input.bytes, detected.content_type)
            .await?;
        if let Err(error) = media::insert(&mut tx, &row).await {
            let _ = state.storage.delete(&key).await;
            return Err(error);
        }
        if let Err(error) = sqlx::query(
            "UPDATE users SET profile_picture_key = $2, updated_at = $3
             WHERE id = $1 AND deleted_at IS NULL",
        )
        .bind(owner_id)
        .bind(&key)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(crate::error::map_db_error)
        {
            let _ = state.storage.delete(&key).await;
            return Err(error);
        }
        if let Some(old_key) = old_key.filter(|existing| existing != &key) {
            if let Ok(Some(old)) = media::find_attached_profile(&mut *tx, owner_id, &old_key).await
            {
                media::mark_deleted(&mut tx, old.id, now).await?;
                tx.commit().await.map_err(crate::error::map_db_error)?;
                let _ = state.storage.delete(&old_key).await;
            } else {
                tx.commit().await.map_err(crate::error::map_db_error)?;
                let _ = state.storage.delete(&old_key).await;
            }
        } else {
            tx.commit().await.map_err(crate::error::map_db_error)?;
        }
        tracing::info!(
            media_id = %media_id,
            user_id = %owner_id,
            content_type = detected.content_type,
            byte_size = row.byte_size,
            "profile picture stored"
        );
        return sign_row(state, &row).await;
    }

    state
        .storage
        .put(&key, &input.bytes, detected.content_type)
        .await?;
    if let Err(error) = media::insert(&mut tx, &row).await {
        let _ = state.storage.delete(&key).await;
        return Err(error);
    }
    tx.commit().await.map_err(crate::error::map_db_error)?;
    tracing::info!(
        media_id = %media_id,
        user_id = %owner_id,
        content_type = detected.content_type,
        byte_size = row.byte_size,
        purpose,
        "media uploaded"
    );
    sign_row(state, &row).await
}

pub async fn get_authorized(
    state: &AppState,
    user_id: Uuid,
    media_id: Uuid,
) -> Result<MediaRow, AppError> {
    let mut tx = state.db.begin().await.map_err(crate::error::map_db_error)?;
    require_active(&mut tx, user_id).await?;
    let row = media::get_by_id(&mut *tx, media_id)
        .await?
        .ok_or(AppError::MediaNotFound)?;
    if row.status == media::STATUS_DELETED {
        return Err(AppError::MediaNotFound);
    }
    authorize(&mut tx, user_id, &row).await?;
    tx.commit().await.map_err(crate::error::map_db_error)?;
    Ok(row)
}

async fn authorize(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user_id: Uuid,
    row: &MediaRow,
) -> Result<(), AppError> {
    match row.purpose.as_str() {
        PURPOSE_PROFILE => Ok(()),
        PURPOSE_GROUP => {
            let group_id = row.group_id.ok_or(AppError::MediaForbidden)?;
            require_group_member(tx, group_id, user_id)
                .await
                .map(|_| ())
                .map_err(|_| AppError::MediaForbidden)
        }
        PURPOSE_DM => {
            if row.owner_id == user_id || row.peer_id == Some(user_id) {
                require_not_blocked(tx, user_id, row.owner_id).await?;
                Ok(())
            } else if let Some(message_id) = row.direct_message_id {
                let dm = crate::db::social::get_direct_message(&mut **tx, message_id).await?;
                match dm {
                    Some(dm) if dm.sender_id == user_id || dm.receiver_id == user_id => Ok(()),
                    _ => Err(AppError::MediaForbidden),
                }
            } else {
                Err(AppError::MediaForbidden)
            }
        }
        _ => Err(AppError::MediaForbidden),
    }
}

pub async fn delete_uploaded(
    state: &AppState,
    user_id: Uuid,
    media_id: Uuid,
) -> Result<(), AppError> {
    let mut tx = state.db.begin().await.map_err(crate::error::map_db_error)?;
    require_active(&mut tx, user_id).await?;
    let row = media::lock_by_id(&mut tx, media_id)
        .await?
        .ok_or(AppError::MediaNotFound)?;
    if row.owner_id != user_id {
        return Err(AppError::MediaForbidden);
    }
    if row.status != media::STATUS_UPLOADED {
        return Err(AppError::validation(
            "only unattached uploads can be deleted directly",
        ));
    }
    media::mark_deleted(&mut tx, media_id, Utc::now()).await?;
    tx.commit().await.map_err(crate::error::map_db_error)?;
    let _ = state.storage.delete(&row.storage_key).await;
    Ok(())
}

pub async fn lock_and_validate_group_attachments(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    owner_id: Uuid,
    group_id: Uuid,
    ids: &[Uuid],
    expected_kind: &str,
) -> Result<(), AppError> {
    if ids.len() > crate::media::MAX_ATTACHMENTS {
        return Err(AppError::validation("too many attachments"));
    }
    for id in ids {
        let row = media::lock_by_id(tx, *id)
            .await?
            .ok_or(AppError::MediaNotFound)?;
        if row.owner_id != owner_id
            || row.purpose != PURPOSE_GROUP
            || row.group_id != Some(group_id)
            || row.status != media::STATUS_UPLOADED
        {
            return Err(AppError::MediaForbidden);
        }
        if row.kind != expected_kind {
            return Err(AppError::validation(
                "attachment kind does not match messageType",
            ));
        }
    }
    Ok(())
}

pub async fn lock_and_validate_dm_attachments(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    owner_id: Uuid,
    peer_id: Uuid,
    ids: &[Uuid],
    expected_kind: &str,
) -> Result<(), AppError> {
    if ids.len() > crate::media::MAX_ATTACHMENTS {
        return Err(AppError::validation("too many attachments"));
    }
    for id in ids {
        let row = media::lock_by_id(tx, *id)
            .await?
            .ok_or(AppError::MediaNotFound)?;
        if row.owner_id != owner_id
            || row.purpose != PURPOSE_DM
            || row.peer_id != Some(peer_id)
            || row.status != media::STATUS_UPLOADED
        {
            return Err(AppError::MediaForbidden);
        }
        if row.kind != expected_kind {
            return Err(AppError::validation(
                "attachment kind does not match messageType",
            ));
        }
    }
    Ok(())
}

pub async fn dtos_for_group_messages(
    state: &AppState,
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    message_ids: &[Uuid],
) -> Result<std::collections::HashMap<Uuid, Vec<MediaDto>>, AppError> {
    let rows = media::list_for_group_messages(&mut **tx, message_ids).await?;
    let mut map = std::collections::HashMap::<Uuid, Vec<MediaDto>>::new();
    for row in rows {
        if let Some(message_id) = row.message_id {
            map.entry(message_id)
                .or_default()
                .push(sign_row(state, &row).await?);
        }
    }
    Ok(map)
}

pub async fn dtos_for_direct_messages(
    state: &AppState,
    executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    message_ids: &[Uuid],
) -> Result<std::collections::HashMap<Uuid, Vec<MediaDto>>, AppError> {
    let rows = media::list_for_direct_messages(executor, message_ids).await?;
    let mut map = std::collections::HashMap::<Uuid, Vec<MediaDto>>::new();
    for row in rows {
        if let Some(message_id) = row.direct_message_id {
            map.entry(message_id)
                .or_default()
                .push(sign_row(state, &row).await?);
        }
    }
    Ok(map)
}

/// Removes abandoned `uploaded` objects older than 24 hours.
pub async fn cleanup_orphaned_media(state: &AppState) -> Result<u64, AppError> {
    let cutoff = Utc::now() - Duration::hours(24);
    let orphans = media::list_orphans(&state.db, cutoff).await?;
    let mut removed = 0u64;
    for row in orphans {
        let mut tx = state.db.begin().await.map_err(crate::error::map_db_error)?;
        media::mark_deleted(&mut tx, row.id, Utc::now()).await?;
        tx.commit().await.map_err(crate::error::map_db_error)?;
        let _ = state.storage.delete(&row.storage_key).await;
        removed += 1;
    }
    tracing::info!(removed, "orphaned media cleaned");
    Ok(removed)
}

pub fn verify_memory_signature(state: &AppState, key: &str, exp: i64, sig: &str) -> bool {
    if exp < Utc::now().timestamp() {
        return false;
    }
    crate::media::MemoryStorage::verify(&state.config.jwt_secret, key, exp, sig)
}

pub async fn load_bytes(state: &AppState, key: &str) -> Result<Vec<u8>, AppError> {
    state.storage.get(key).await
}
