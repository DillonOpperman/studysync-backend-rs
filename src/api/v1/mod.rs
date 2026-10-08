use axum::routing::{delete, get, patch, post};
use axum::Router;

use crate::AppState;

mod auth;
mod friends;
mod group_chat;
mod groups;
mod health;
mod media;
mod messages;
mod notifications;
mod users;

pub fn media_routes(max_bytes: usize) -> Router<AppState> {
    media::routes(max_bytes)
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/health", get(health::health))
        .route("/ready", get(health::ready))
        .route("/auth/register", post(auth::register))
        .route("/auth/login", post(auth::login))
        .route("/auth/refresh", post(auth::refresh))
        .route("/auth/logout", post(auth::logout))
        .route("/me", get(users::me).patch(users::patch_me))
        .route("/institutions", get(users::list_institutions))
        .route("/groups", post(groups::create))
        .route("/groups/mine", get(groups::mine))
        .route("/groups/{id}", get(groups::detail))
        .route("/groups/{id}/archive", post(groups::archive))
        .route("/groups/{id}/leave", post(groups::leave))
        .route(
            "/groups/{id}/join-requests",
            post(groups::create_join_request).get(groups::list_join_requests),
        )
        .route(
            "/groups/{group_id}/join-requests/{request_id}/approve",
            post(groups::approve_join_request),
        )
        .route(
            "/groups/{group_id}/join-requests/{request_id}/reject",
            post(groups::reject_join_request),
        )
        .route("/groups/{id}/members", get(groups::members))
        .route(
            "/groups/{group_id}/members/{user_id}",
            delete(groups::remove_member),
        )
        .route("/notifications", get(notifications::list))
        .route(
            "/notifications/read-all",
            post(notifications::mark_all_read),
        )
        .route("/notifications/{id}/read", post(notifications::mark_read))
        .route(
            "/friends/requests",
            post(friends::send_request).get(friends::list_requests),
        )
        .route("/friends/requests/{id}/accept", post(friends::accept))
        .route("/friends/requests/{id}/reject", post(friends::reject))
        .route("/friends", get(friends::list_friends))
        .route("/blocks", post(friends::block))
        .route("/blocks/{user_id}", delete(friends::unblock))
        .route("/direct-messages", post(messages::send))
        .route("/direct-messages/{user_id}", get(messages::list_thread))
        .route("/message-requests", get(messages::list_message_requests))
        .route(
            "/message-requests/{id}/accept",
            post(messages::accept_message_request),
        )
        .route(
            "/message-requests/{id}/reject",
            post(messages::reject_message_request),
        )
        .route(
            "/groups/{group_id}/messages",
            get(group_chat::list_messages).post(group_chat::post_message),
        )
        .route(
            "/messages/{id}",
            patch(group_chat::edit_message).delete(group_chat::delete_message),
        )
        .route(
            "/messages/{id}/reactions",
            post(group_chat::toggle_reaction),
        )
}
