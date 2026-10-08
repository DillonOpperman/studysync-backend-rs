use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use uuid::Uuid;

use crate::api::dto::{
    EditGroupMessageRequest, GroupMessageDto, GroupMessageListQuery, GroupMessagePageDto,
    ReactionStateDto, SendGroupMessageRequest, ToggleReactionRequest,
};
use crate::auth::{require_user, AuthUser};
use crate::error::AppError;
use crate::services::group_chat;
use crate::AppState;

pub async fn list_messages(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(group_id): Path<Uuid>,
    Query(query): Query<GroupMessageListQuery>,
) -> Result<Json<GroupMessagePageDto>, AppError> {
    let user_id = require_user(auth);
    Ok(Json(
        group_chat::list_group_messages(
            &state,
            user_id,
            group_id,
            query.before,
            query.after,
            query.limit,
        )
        .await?,
    ))
}

pub async fn post_message(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(group_id): Path<Uuid>,
    Json(body): Json<SendGroupMessageRequest>,
) -> Result<(StatusCode, Json<GroupMessageDto>), AppError> {
    let user_id = require_user(auth);
    let dto = group_chat::send_group_message(
        &state,
        user_id,
        group_id,
        body.client_msg_id,
        body.content,
        body.message_type.as_deref(),
        body.attachment_ids,
    )
    .await?;
    Ok((StatusCode::CREATED, Json(dto)))
}

pub async fn edit_message(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(message_id): Path<Uuid>,
    Json(body): Json<EditGroupMessageRequest>,
) -> Result<Json<GroupMessageDto>, AppError> {
    let user_id = require_user(auth);
    Ok(Json(
        group_chat::edit_group_message(&state, user_id, message_id, body.content).await?,
    ))
}

pub async fn delete_message(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(message_id): Path<Uuid>,
) -> Result<Json<GroupMessageDto>, AppError> {
    let user_id = require_user(auth);
    Ok(Json(
        group_chat::delete_group_message(&state, user_id, message_id).await?,
    ))
}

pub async fn toggle_reaction(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(message_id): Path<Uuid>,
    Json(body): Json<ToggleReactionRequest>,
) -> Result<Json<ReactionStateDto>, AppError> {
    let user_id = require_user(auth);
    Ok(Json(
        group_chat::toggle_reaction(&state, user_id, message_id, body.emoji).await?,
    ))
}
