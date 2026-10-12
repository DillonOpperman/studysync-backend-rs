use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use uuid::Uuid;

use crate::api::dto::{NotificationDto, NotificationListQuery};
use crate::auth::{require_user, AuthUser};
use crate::error::AppError;
use crate::services;
use crate::AppState;

pub async fn get_one(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(notification_id): Path<Uuid>,
) -> Result<Json<NotificationDto>, AppError> {
    let user_id = require_user(auth);
    Ok(Json(
        services::notifications::get_one(&state.db, user_id, notification_id).await?,
    ))
}

pub async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<NotificationListQuery>,
) -> Result<Json<Vec<NotificationDto>>, AppError> {
    let user_id = require_user(auth);
    Ok(Json(
        services::notifications::list(&state.db, user_id, query.limit, query.offset).await?,
    ))
}

pub async fn mark_read(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(notification_id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    let user_id = require_user(auth);
    services::notifications::mark_read(&state.db, user_id, notification_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn mark_all_read(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<StatusCode, AppError> {
    let user_id = require_user(auth);
    services::notifications::mark_all_read(&state.db, user_id).await?;
    Ok(StatusCode::NO_CONTENT)
}
