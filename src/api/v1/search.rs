use axum::extract::State;
use axum::Json;

use crate::api::dto::{GroupSearchRequest, GroupSearchResponse};
use crate::auth::{require_user, AuthUser};
use crate::error::AppError;
use crate::services;
use crate::AppState;

pub async fn search_groups(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<GroupSearchRequest>,
) -> Result<Json<GroupSearchResponse>, AppError> {
    let user_id = require_user(auth);
    Ok(Json(
        services::search::search_groups(&state.db, &state.embeddings, user_id, body).await?,
    ))
}
