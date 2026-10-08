use axum::extract::State;
use axum::Json;

use crate::api::dto::RecommendResponse;
use crate::auth::{require_user, AuthUser};
use crate::error::AppError;
use crate::services;
use crate::AppState;

pub async fn recommend(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<RecommendResponse>, AppError> {
    let user_id = require_user(auth);
    Ok(Json(
        services::recommend::recommend_groups(&state.db, &state.embeddings, user_id).await?,
    ))
}
