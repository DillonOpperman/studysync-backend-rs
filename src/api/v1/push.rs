use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use uuid::Uuid;

use crate::api::dto::{
    PushDeviceDto, PushPreferencesDto, RegisterPushDeviceRequest, UpdatePushPreferencesRequest,
};
use crate::auth::{require_user, AuthUser};
use crate::error::AppError;
use crate::services;
use crate::AppState;

pub async fn register_device(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<RegisterPushDeviceRequest>,
) -> Result<(StatusCode, Json<PushDeviceDto>), AppError> {
    let user_id = require_user(auth);
    Ok((
        StatusCode::CREATED,
        Json(services::push::register_device(&state.db, user_id, body).await?),
    ))
}

pub async fn unregister_device(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(device_id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    let user_id = require_user(auth);
    services::push::unregister_device(&state.db, user_id, device_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn get_preferences(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<PushPreferencesDto>, AppError> {
    let user_id = require_user(auth);
    Ok(Json(
        services::push::get_preferences(&state.db, user_id).await?,
    ))
}

pub async fn update_preferences(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<UpdatePushPreferencesRequest>,
) -> Result<Json<PushPreferencesDto>, AppError> {
    let user_id = require_user(auth);
    Ok(Json(
        services::push::update_preferences(&state.db, user_id, body).await?,
    ))
}
