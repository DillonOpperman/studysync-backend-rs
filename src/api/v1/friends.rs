use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use uuid::Uuid;

use crate::api::dto::{
    BlockUserRequest, FriendListDto, FriendRequestCreate, FriendRequestDto, FriendRequestListDto,
};
use crate::auth::{require_user, AuthUser};
use crate::error::AppError;
use crate::services;
use crate::AppState;

pub async fn send_request(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<FriendRequestCreate>,
) -> Result<(StatusCode, Json<FriendRequestDto>), AppError> {
    let user_id = require_user(auth);
    let dto = services::friends::send_request(&state.db, user_id, body.user_id).await?;
    Ok((StatusCode::CREATED, Json(dto)))
}

pub async fn list_requests(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<FriendRequestListDto>, AppError> {
    let user_id = require_user(auth);
    Ok(Json(
        services::friends::list_requests(&state.db, user_id).await?,
    ))
}

pub async fn list_friends(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Vec<FriendListDto>>, AppError> {
    let user_id = require_user(auth);
    Ok(Json(
        services::friends::list_friends(&state.db, user_id).await?,
    ))
}

pub async fn accept(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(request_id): Path<Uuid>,
) -> Result<Json<FriendRequestDto>, AppError> {
    let user_id = require_user(auth);
    Ok(Json(
        services::friends::accept_request(&state.db, user_id, request_id).await?,
    ))
}

pub async fn reject(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(request_id): Path<Uuid>,
) -> Result<Json<FriendRequestDto>, AppError> {
    let user_id = require_user(auth);
    Ok(Json(
        services::friends::reject_request(&state.db, user_id, request_id).await?,
    ))
}

pub async fn block(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<BlockUserRequest>,
) -> Result<StatusCode, AppError> {
    let user_id = require_user(auth);
    services::friends::block_user(&state.db, user_id, body).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn unblock(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(blocked_id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    let user_id = require_user(auth);
    services::friends::unblock_user(&state.db, user_id, blocked_id).await?;
    Ok(StatusCode::NO_CONTENT)
}
