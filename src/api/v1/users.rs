use axum::extract::State;
use axum::Json;

use crate::api::dto::{InstitutionDto, PatchMeRequest, UserProfileDto};
use crate::auth::{require_user, AuthUser};
use crate::error::AppError;
use crate::services;
use crate::AppState;

pub async fn me(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<UserProfileDto>, AppError> {
    let user_id = require_user(auth);
    Ok(Json(services::users::get_me(&state.db, user_id).await?))
}

pub async fn patch_me(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<PatchMeRequest>,
) -> Result<Json<UserProfileDto>, AppError> {
    let user_id = require_user(auth);
    let reembed = body.name.is_some() || body.major.is_some() || body.subjects.is_some();
    let profile = services::users::patch_me(&state.db, user_id, body).await?;
    if reembed {
        if let Err(error) =
            services::embeddings::upsert_user(&state.db, &state.embeddings, user_id).await
        {
            tracing::error!(
                code = error.code(),
                "user embedding persist failed after profile update"
            );
        }
    }
    Ok(Json(profile))
}

pub async fn list_institutions(
    State(state): State<AppState>,
) -> Result<Json<Vec<InstitutionDto>>, AppError> {
    Ok(Json(services::users::list_institutions(&state.db).await?))
}
