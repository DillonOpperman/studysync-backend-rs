use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use uuid::Uuid;

use crate::api::dto::{
    DirectMessageDto, DirectMessageListQuery, MessageRequestDto, SendDirectMessageRequest,
    SendDirectMessageResponse, SendDmOutcome,
};
use crate::auth::{require_user, AuthUser};
use crate::error::AppError;
use crate::services;
use crate::AppState;

pub async fn send(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<SendDirectMessageRequest>,
) -> Result<(StatusCode, Json<SendDirectMessageResponse>), AppError> {
    let user_id = require_user(auth);
    let outcome = services::messages::send_direct_message(&state.db, user_id, body).await?;
    let (status, body) = match outcome {
        SendDmOutcome::DirectMessage { message } => (
            StatusCode::CREATED,
            SendDirectMessageResponse::DirectMessage { message },
        ),
        SendDmOutcome::NewMessageRequest { message_request } => (
            StatusCode::ACCEPTED,
            SendDirectMessageResponse::MessageRequest { message_request },
        ),
        SendDmOutcome::ExistingMessageRequest { message_request } => (
            StatusCode::OK,
            SendDirectMessageResponse::MessageRequest { message_request },
        ),
    };
    Ok((status, Json(body)))
}

pub async fn list_thread(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(user_id): Path<Uuid>,
    Query(query): Query<DirectMessageListQuery>,
) -> Result<Json<Vec<DirectMessageDto>>, AppError> {
    let reader_id = require_user(auth);
    Ok(Json(
        services::messages::list_direct_messages(
            &state.db,
            reader_id,
            user_id,
            query.before,
            query.after,
            query.limit,
        )
        .await?,
    ))
}

pub async fn list_message_requests(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Vec<MessageRequestDto>>, AppError> {
    let user_id = require_user(auth);
    Ok(Json(
        services::messages::list_message_requests(&state.db, user_id).await?,
    ))
}

pub async fn accept_message_request(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(request_id): Path<Uuid>,
) -> Result<Json<MessageRequestDto>, AppError> {
    let user_id = require_user(auth);
    Ok(Json(
        services::messages::accept_message_request(&state.db, user_id, request_id).await?,
    ))
}

pub async fn reject_message_request(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(request_id): Path<Uuid>,
) -> Result<Json<MessageRequestDto>, AppError> {
    let user_id = require_user(auth);
    Ok(Json(
        services::messages::reject_message_request(&state.db, user_id, request_id).await?,
    ))
}
