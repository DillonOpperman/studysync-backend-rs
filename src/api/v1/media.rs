use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Multipart, Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::Response;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde::Deserialize;
use uuid::Uuid;

use crate::api::dto::MediaDto;
use crate::auth::{require_user, AuthUser};
use crate::error::AppError;
use crate::services::media::{self, UploadInput};
use crate::AppState;

const MULTIPART_OVERHEAD: usize = 1024 * 1024;

pub fn routes(max_bytes: usize) -> Router<AppState> {
    Router::new()
        .route("/media", post(upload_media))
        .route("/media/{id}", get(get_media).delete(delete_media))
        .route("/media/{id}/content", get(download_content))
        .route("/me/profile-picture", put(put_profile_picture))
        .layer(DefaultBodyLimit::max(
            max_bytes.saturating_add(MULTIPART_OVERHEAD),
        ))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DownloadQuery {
    exp: Option<i64>,
    sig: Option<String>,
}

async fn read_upload(multipart: &mut Multipart, max_bytes: usize) -> Result<UploadInput, AppError> {
    let mut purpose = None;
    let mut group_id = None;
    let mut recipient_id = None;
    let mut file_name = None;
    let mut declared_type = None;
    let mut bytes = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| AppError::validation("invalid multipart body"))?
    {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "purpose" => {
                purpose = Some(
                    field
                        .text()
                        .await
                        .map_err(|_| AppError::validation("invalid purpose"))?,
                );
            }
            "groupId" => {
                let raw = field
                    .text()
                    .await
                    .map_err(|_| AppError::validation("invalid groupId"))?;
                group_id = Some(
                    Uuid::parse_str(raw.trim())
                        .map_err(|_| AppError::validation("groupId is invalid"))?,
                );
            }
            "recipientId" => {
                let raw = field
                    .text()
                    .await
                    .map_err(|_| AppError::validation("invalid recipientId"))?;
                recipient_id = Some(
                    Uuid::parse_str(raw.trim())
                        .map_err(|_| AppError::validation("recipientId is invalid"))?,
                );
            }
            "file" => {
                file_name = Some(field.file_name().unwrap_or("upload").to_string());
                declared_type = field.content_type().map(ToOwned::to_owned);
                let data = field
                    .bytes()
                    .await
                    .map_err(|_| AppError::validation("failed to read upload"))?;
                if data.len() > max_bytes {
                    return Err(AppError::UploadTooLarge);
                }
                bytes = Some(data.to_vec());
            }
            _ => {}
        }
    }

    let bytes = bytes.ok_or_else(|| AppError::validation("file is required"))?;
    if bytes.len() > max_bytes {
        return Err(AppError::UploadTooLarge);
    }
    Ok(UploadInput {
        purpose: purpose.unwrap_or_default(),
        group_id,
        recipient_id,
        file_name: file_name.unwrap_or_else(|| "upload".into()),
        declared_type,
        bytes,
    })
}

async fn upload_media(
    State(state): State<AppState>,
    auth: AuthUser,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<MediaDto>), AppError> {
    let user_id = require_user(auth);
    let max = state.config.media_max_bytes as usize;
    let input = read_upload(&mut multipart, max).await?;
    let dto = media::upload(&state, user_id, input).await?;
    Ok((StatusCode::CREATED, Json(dto)))
}

async fn put_profile_picture(
    State(state): State<AppState>,
    auth: AuthUser,
    mut multipart: Multipart,
) -> Result<Json<MediaDto>, AppError> {
    let user_id = require_user(auth);
    let max = state.config.media_max_bytes as usize;
    let mut input = read_upload(&mut multipart, max).await?;
    input.purpose = "profile".into();
    Ok(Json(media::upload(&state, user_id, input).await?))
}

async fn get_media(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<MediaDto>, AppError> {
    let user_id = require_user(auth);
    let row = media::get_authorized(&state, user_id, id).await?;
    Ok(Json(media::sign_row(&state, &row).await?))
}

async fn delete_media(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    let user_id = require_user(auth);
    media::delete_uploaded(&state, user_id, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn download_content(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<DownloadQuery>,
    Path(id): Path<Uuid>,
) -> Result<Response, AppError> {
    let row = if let (Some(exp), Some(sig)) = (query.exp, query.sig.as_deref()) {
        let row = crate::db::media::get_by_id(&state.db, id)
            .await?
            .ok_or(AppError::MediaNotFound)?;
        if row.status == crate::db::media::STATUS_DELETED {
            return Err(AppError::MediaNotFound);
        }
        if !media::verify_memory_signature(&state, &row.storage_key, exp, sig) {
            return Err(AppError::Unauthorized);
        }
        row
    } else {
        let auth = bearer_user(&state, &headers).await?;
        media::get_authorized(&state, auth, id).await?
    };
    let bytes = media::load_bytes(&state, &row.storage_key).await?;
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, row.content_type)
        .header(
            header::CONTENT_DISPOSITION,
            format!("inline; filename=\"{}\"", row.file_name.replace('"', "")),
        )
        .body(Body::from(bytes))
        .map_err(|_| AppError::Internal)
}

async fn bearer_user(state: &AppState, headers: &HeaderMap) -> Result<Uuid, AppError> {
    let header = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or(AppError::Unauthorized)?;
    let token = header
        .strip_prefix("Bearer ")
        .or_else(|| header.strip_prefix("bearer "))
        .ok_or(AppError::Unauthorized)?;
    let claims = crate::auth::jwt::decode_access_token(token, &state.config.jwt_secret)?;
    let id = Uuid::parse_str(&claims.sub).map_err(|_| AppError::Unauthorized)?;
    crate::db::identity::get_user_by_id(&state.db, id)
        .await?
        .filter(|row| row.deleted_at.is_none())
        .map(|row| row.id)
        .ok_or(AppError::Unauthorized)
}
