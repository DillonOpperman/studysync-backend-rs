use axum::routing::{get, post};
use axum::Router;

use crate::AppState;

mod auth;
mod health;
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
}
