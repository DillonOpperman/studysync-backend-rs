use sqlx::PgPool;
use uuid::Uuid;

use crate::api::dto::{
    PushDeviceDto, PushPreferencesDto, RegisterPushDeviceRequest, UpdatePushPreferencesRequest,
};
use crate::db::push::{self, DeviceRow, PreferenceRow};
use crate::error::AppError;
use crate::push::token_fingerprint;

const TOKEN_MIN: usize = 32;
const TOKEN_MAX: usize = 4096;
const INSTALL_MAX: usize = 128;

pub async fn register_device(
    pool: &PgPool,
    user_id: Uuid,
    body: RegisterPushDeviceRequest,
) -> Result<PushDeviceDto, AppError> {
    let platform = normalize_platform(&body.platform)?;
    let token = validate_token(&body.token)?;
    let installation_id = validate_installation(body.installation_id.as_deref())?;
    let fingerprint = token_fingerprint(token);

    if let Some(existing) = push::find_active_by_fingerprint(pool, &fingerprint).await? {
        if existing.user_id != user_id {
            push::revoke_device(pool, existing.id).await?;
        } else {
            let row = push::update_device_token(
                pool,
                existing.id,
                user_id,
                platform,
                token,
                &fingerprint,
                installation_id,
            )
            .await?
            .ok_or(AppError::DeviceNotFound)?;
            return Ok(to_dto(&row));
        }
    }

    if let Some(install) = installation_id {
        if let Some(existing) = push::find_active_by_installation(pool, user_id, install).await? {
            let row = push::update_device_token(
                pool,
                existing.id,
                user_id,
                platform,
                token,
                &fingerprint,
                Some(install),
            )
            .await?
            .ok_or(AppError::DeviceNotFound)?;
            return Ok(to_dto(&row));
        }
    }

    let row = push::insert_device(
        pool,
        Uuid::now_v7(),
        user_id,
        platform,
        token,
        &fingerprint,
        installation_id,
    )
    .await?;
    tracing::info!(device_id = %row.id, platform = %row.platform, "push device registered");
    Ok(to_dto(&row))
}

pub async fn unregister_device(
    pool: &PgPool,
    user_id: Uuid,
    device_id: Uuid,
) -> Result<(), AppError> {
    let n = push::revoke_owned_device(pool, user_id, device_id).await?;
    if n == 0 {
        return Err(AppError::DeviceNotFound);
    }
    tracing::info!(device_id = %device_id, "push device revoked");
    Ok(())
}

pub async fn get_preferences(pool: &PgPool, user_id: Uuid) -> Result<PushPreferencesDto, AppError> {
    Ok(prefs_dto(
        &push::get_or_default_preferences(pool, user_id).await?,
    ))
}

pub async fn update_preferences(
    pool: &PgPool,
    user_id: Uuid,
    body: UpdatePushPreferencesRequest,
) -> Result<PushPreferencesDto, AppError> {
    let current = push::get_or_default_preferences(pool, user_id).await?;
    let row = push::upsert_preferences(
        pool,
        user_id,
        body.push_enabled.unwrap_or(current.push_enabled),
        body.friends.unwrap_or(current.friends),
        body.groups.unwrap_or(current.groups),
        body.messages.unwrap_or(current.messages),
        body.announcements.unwrap_or(current.announcements),
    )
    .await?;
    Ok(prefs_dto(&row))
}

fn normalize_platform(platform: &str) -> Result<&str, AppError> {
    match platform.trim().to_ascii_lowercase().as_str() {
        "android" => Ok("android"),
        "ios" => Ok("ios"),
        _ => Err(AppError::validation("platform must be android or ios")),
    }
}

fn validate_token(token: &str) -> Result<&str, AppError> {
    let trimmed = token.trim();
    if trimmed.len() < TOKEN_MIN || trimmed.len() > TOKEN_MAX {
        return Err(AppError::validation("token length is invalid"));
    }
    if trimmed.chars().any(char::is_whitespace) {
        return Err(AppError::validation("token must not contain whitespace"));
    }
    Ok(trimmed)
}

fn validate_installation(value: Option<&str>) -> Result<Option<&str>, AppError> {
    let Some(raw) = value else {
        return Ok(None);
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.len() > INSTALL_MAX {
        return Err(AppError::validation("installationId is too long"));
    }
    if !trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(AppError::validation("installationId format is invalid"));
    }
    Ok(Some(trimmed))
}

fn to_dto(row: &DeviceRow) -> PushDeviceDto {
    PushDeviceDto {
        id: row.id,
        platform: row.platform.clone(),
        created_at: row.created_at,
        updated_at: row.updated_at,
        last_seen_at: row.last_seen_at,
    }
}

fn prefs_dto(row: &PreferenceRow) -> PushPreferencesDto {
    PushPreferencesDto {
        push_enabled: row.push_enabled,
        friends: row.friends,
        groups: row.groups,
        messages: row.messages,
        announcements: row.announcements,
        updated_at: row.updated_at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_short_token() {
        let err = validate_token("short").unwrap_err();
        assert!(matches!(err, AppError::Validation(_)));
    }

    #[test]
    fn rejects_unknown_platform() {
        let err = normalize_platform("web").unwrap_err();
        assert!(matches!(err, AppError::Validation(_)));
    }
}
