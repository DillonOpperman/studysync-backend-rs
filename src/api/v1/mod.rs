use axum::routing::{delete, get, post};
use axum::Router;

use crate::AppState;

mod auth;
mod groups;
mod health;
mod notifications;
mod users;

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
}
