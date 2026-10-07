use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use uuid::Uuid;

use crate::api::dto::{CreateGroupRequest, GroupDto, GroupMemberDto, JoinRequestDto};
use crate::auth::{require_user, AuthUser};
use crate::error::AppError;
use crate::services;
use crate::AppState;

pub async fn create(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<CreateGroupRequest>,
) -> Result<(StatusCode, Json<GroupDto>), AppError> {
    let user_id = require_user(auth);
    let group = services::groups::create_group(&state.db, user_id, body).await?;
    Ok((StatusCode::CREATED, Json(group)))
}

pub async fn mine(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Vec<GroupDto>>, AppError> {
    let user_id = require_user(auth);
    Ok(Json(services::groups::list_mine(&state.db, user_id).await?))
}

pub async fn detail(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(group_id): Path<Uuid>,
) -> Result<Json<GroupDto>, AppError> {
    let user_id = require_user(auth);
    Ok(Json(
        services::groups::get_group(&state.db, user_id, group_id).await?,
    ))
}

pub async fn archive(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(group_id): Path<Uuid>,
) -> Result<Json<GroupDto>, AppError> {
    let user_id = require_user(auth);
    Ok(Json(
        services::groups::archive_group(&state.db, user_id, group_id).await?,
    ))
}

pub async fn create_join_request(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(group_id): Path<Uuid>,
) -> Result<(StatusCode, Json<JoinRequestDto>), AppError> {
    let user_id = require_user(auth);
    let request = services::groups::create_join_request(&state.db, user_id, group_id).await?;
    Ok((StatusCode::CREATED, Json(request)))
}

pub async fn list_join_requests(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(group_id): Path<Uuid>,
) -> Result<Json<Vec<JoinRequestDto>>, AppError> {
    let user_id = require_user(auth);
    Ok(Json(
        services::groups::list_join_requests(&state.db, user_id, group_id).await?,
    ))
}

pub async fn approve_join_request(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((group_id, request_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<JoinRequestDto>, AppError> {
    let user_id = require_user(auth);
    Ok(Json(
        services::groups::approve_join_request(&state.db, user_id, group_id, request_id).await?,
    ))
}

pub async fn reject_join_request(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((group_id, request_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<JoinRequestDto>, AppError> {
    let user_id = require_user(auth);
    Ok(Json(
        services::groups::reject_join_request(&state.db, user_id, group_id, request_id).await?,
    ))
}

pub async fn members(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(group_id): Path<Uuid>,
) -> Result<Json<Vec<GroupMemberDto>>, AppError> {
    let user_id = require_user(auth);
    Ok(Json(
        services::groups::list_members(&state.db, user_id, group_id).await?,
    ))
}

pub async fn leave(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(group_id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    let user_id = require_user(auth);
    services::groups::leave_group(&state.db, user_id, group_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn remove_member(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((group_id, target_user_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    let user_id = require_user(auth);
    services::groups::remove_member(&state.db, user_id, group_id, target_user_id).await?;
    Ok(StatusCode::NO_CONTENT)
}
