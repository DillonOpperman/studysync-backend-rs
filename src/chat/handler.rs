use std::time::{Duration, Instant};

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Query, State, WebSocketUpgrade};
use axum::response::Response;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use uuid::Uuid;

use super::hub::ConnectionId;
use super::protocol::{ClientOp, ServerEvent};
use crate::auth::jwt::decode_access_token;
use crate::db::identity;
use crate::error::AppError;
use crate::services::group_chat;
use crate::AppState;

const IDLE_TIMEOUT: Duration = Duration::from_secs(90);
const IDLE_CHECK: Duration = Duration::from_secs(15);
const SEND_LIMIT: u32 = 20;
const OP_LIMIT: u32 = 60;
const RATE_WINDOW: Duration = Duration::from_secs(10);
const MAX_INCOMING: usize = 16 * 1024;

#[derive(Debug, Deserialize)]
pub struct WsQuery {
    access_token: String,
}

struct RateWindow {
    started: Instant,
    sends: u32,
    ops: u32,
}

impl RateWindow {
    fn new() -> Self {
        Self {
            started: Instant::now(),
            sends: 0,
            ops: 0,
        }
    }

    fn tick(&mut self, is_send: bool) -> Result<(), AppError> {
        if self.started.elapsed() >= RATE_WINDOW {
            *self = Self::new();
        }
        self.ops += 1;
        if is_send {
            self.sends += 1;
        }
        if self.ops > OP_LIMIT || self.sends > SEND_LIMIT {
            return Err(AppError::RateLimited);
        }
        Ok(())
    }
}

pub async fn upgrade(
    Query(query): Query<WsQuery>,
    State(state): State<AppState>,
    ws: WebSocketUpgrade,
) -> Result<Response, AppError> {
    let user_id = authenticate_access_token(&state, &query.access_token).await?;
    let conn_id = ConnectionId(Uuid::now_v7());
    tracing::info!(
        connection_id = %conn_id.0,
        user_id = %user_id,
        "websocket connect"
    );
    Ok(ws
        .max_message_size(MAX_INCOMING)
        .on_upgrade(move |socket| handle_socket(state, user_id, conn_id, socket)))
}

async fn authenticate_access_token(state: &AppState, token: &str) -> Result<Uuid, AppError> {
    if token.is_empty() {
        return Err(AppError::Unauthorized);
    }
    let claims = decode_access_token(token, &state.config.jwt_secret)?;
    let id = Uuid::parse_str(&claims.sub).map_err(|_| AppError::Unauthorized)?;
    let user = identity::get_user_by_id(&state.db, id)
        .await?
        .filter(|row| row.deleted_at.is_none())
        .ok_or(AppError::Unauthorized)?;
    Ok(user.id)
}

async fn handle_socket(state: AppState, user_id: Uuid, conn_id: ConnectionId, socket: WebSocket) {
    let (mut sink, mut stream) = socket.split();
    let mut outbound = state.chat.connect(conn_id, user_id).await;
    let mut last_activity = Instant::now();
    let mut rates = RateWindow::new();
    let mut idle = tokio::time::interval(IDLE_CHECK);
    idle.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            _ = idle.tick() => {
                if last_activity.elapsed() > IDLE_TIMEOUT {
                    tracing::info!(
                        connection_id = %conn_id.0,
                        user_id = %user_id,
                        "websocket idle timeout"
                    );
                    break;
                }
            }
            outgoing = outbound.recv() => {
                let Some(payload) = outgoing else { break };
                if sink.send(Message::Text(payload.as_ref().into())).await.is_err() {
                    break;
                }
            }
            incoming = stream.next() => {
                match incoming {
                    None => break,
                    Some(Err(_)) => break,
                    Some(Ok(Message::Close(_))) => break,
                    Some(Ok(Message::Ping(data))) => {
                        last_activity = Instant::now();
                        let _ = sink.send(Message::Pong(data)).await;
                    }
                    Some(Ok(Message::Pong(_))) => {
                        last_activity = Instant::now();
                    }
                    Some(Ok(Message::Text(text))) => {
                        last_activity = Instant::now();
                        let is_send = text.contains("\"op\":\"message.send\"")
                            || text.contains("\"op\": \"message.send\"");
                        if let Err(error) = rates.tick(is_send) {
                            state
                                .chat
                                .send_to(
                                    conn_id,
                                    &ServerEvent::error(error.code(), error.to_string()),
                                )
                                .await;
                            continue;
                        }
                        dispatch(&state, user_id, conn_id, text.as_str()).await;
                    }
                    Some(Ok(Message::Binary(_))) => {
                        state
                            .chat
                            .send_to(
                                conn_id,
                                &ServerEvent::error("validation_error", "binary frames are not supported"),
                            )
                            .await;
                    }
                }
            }
        }
    }

    tracing::info!(
        connection_id = %conn_id.0,
        user_id = %user_id,
        "websocket disconnect"
    );
    state.chat.disconnect(conn_id).await;
    let _ = sink.send(Message::Close(None)).await;
}

async fn dispatch(state: &AppState, user_id: Uuid, conn_id: ConnectionId, text: &str) {
    let op = match serde_json::from_str::<ClientOp>(text) {
        Ok(op) => op,
        Err(_) => {
            state
                .chat
                .send_to(
                    conn_id,
                    &ServerEvent::error("unknown_op", "unknown or invalid websocket operation"),
                )
                .await;
            return;
        }
    };

    let result = match op {
        ClientOp::Ping { ts } => {
            state.chat.send_to(conn_id, &ServerEvent::Pong { ts }).await;
            return;
        }
        ClientOp::Subscribe { group_id } => subscribe(state, user_id, conn_id, group_id).await,
        ClientOp::Unsubscribe { group_id } => {
            state.chat.unsubscribe(conn_id, group_id).await;
            tracing::info!(
                connection_id = %conn_id.0,
                user_id = %user_id,
                group_id = %group_id,
                "websocket unsubscribe"
            );
            Ok(())
        }
        ClientOp::MessageSend {
            group_id,
            client_msg_id,
            content,
            message_type,
        } => {
            if !state.chat.is_subscribed(conn_id, group_id).await {
                Err(AppError::NotSubscribed)
            } else {
                group_chat::send_group_message(
                    state,
                    user_id,
                    group_id,
                    client_msg_id,
                    content,
                    message_type.as_deref(),
                )
                .await
                .map(|_| ())
            }
        }
        ClientOp::ReactionToggle { message_id, emoji } => {
            group_chat::toggle_reaction(state, user_id, message_id, emoji)
                .await
                .map(|_| ())
        }
        ClientOp::MessageEdit {
            message_id,
            content,
        } => group_chat::edit_group_message(state, user_id, message_id, content)
            .await
            .map(|_| ()),
        ClientOp::MessageDelete { message_id } => {
            group_chat::delete_group_message(state, user_id, message_id)
                .await
                .map(|_| ())
        }
    };

    if let Err(error) = result {
        match error {
            AppError::NotGroupMember | AppError::GroupArchived | AppError::GroupNotFound => {
                tracing::warn!(
                    connection_id = %conn_id.0,
                    user_id = %user_id,
                    code = error.code(),
                    "websocket authorization failed"
                );
            }
            AppError::Validation(_) => {}
            _ => {
                tracing::warn!(
                    connection_id = %conn_id.0,
                    user_id = %user_id,
                    code = error.code(),
                    "websocket message failed"
                );
            }
        }
        state
            .chat
            .send_to(
                conn_id,
                &ServerEvent::error(error.code(), error.to_string()),
            )
            .await;
    }
}

async fn subscribe(
    state: &AppState,
    user_id: Uuid,
    conn_id: ConnectionId,
    group_id: Uuid,
) -> Result<(), AppError> {
    let mut tx = state.db.begin().await.map_err(crate::error::map_db_error)?;
    crate::db::identity::get_user_by_id(&mut *tx, user_id)
        .await?
        .filter(|row| row.deleted_at.is_none())
        .ok_or(AppError::Unauthorized)?;
    let group = crate::db::groups::get_group(&mut *tx, group_id)
        .await?
        .ok_or(AppError::GroupNotFound)?;
    if group.status != crate::db::groups::GROUP_STATUS_ACTIVE {
        return Err(AppError::GroupArchived);
    }
    crate::auth::require_group_member(&mut tx, group_id, user_id).await?;
    tx.commit().await.map_err(crate::error::map_db_error)?;

    state.chat.subscribe(conn_id, group_id).await;
    tracing::info!(
        connection_id = %conn_id.0,
        user_id = %user_id,
        group_id = %group_id,
        "websocket subscribe"
    );
    state
        .chat
        .send_to(conn_id, &ServerEvent::Subscribed { group_id })
        .await;
    Ok(())
}
