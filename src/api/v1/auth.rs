use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;

use crate::api::dto::{AuthResponse, LoginRequest, RefreshRequest, RegisterRequest};
use crate::error::AppError;
use crate::services;
use crate::AppState;

pub async fn register(
    State(state): State<AppState>,
    Json(body): Json<RegisterRequest>,
) -> Result<(StatusCode, Json<AuthResponse>), AppError> {
    let response = services::auth::register(&state.db, &state.config, body).await?;
    Ok((StatusCode::CREATED, Json(response)))
}

pub async fn login(
    State(state): State<AppState>,
    Json(body): Json<LoginRequest>,
) -> Result<Json<AuthResponse>, AppError> {
    Ok(Json(
        services::auth::login(&state.db, &state.config, body).await?,
    ))
}

pub async fn refresh(
    State(state): State<AppState>,
    Json(body): Json<RefreshRequest>,
) -> Result<Json<AuthResponse>, AppError> {
    Ok(Json(
        services::auth::refresh(&state.db, &state.config, &body.refresh_token).await?,
    ))
}

pub async fn logout(
    State(state): State<AppState>,
    Json(body): Json<RefreshRequest>,
) -> Result<StatusCode, AppError> {
    services::auth::logout(&state.db, &body.refresh_token).await?;
    Ok(StatusCode::NO_CONTENT)
}
