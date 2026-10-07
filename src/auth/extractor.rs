use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use uuid::Uuid;

use crate::auth::jwt::decode_access_token;
use crate::error::AppError;
use crate::AppState;

/// Authenticated user id extracted from a Bearer access token.
#[derive(Debug, Clone, Copy)]
pub struct AuthUser {
    pub id: Uuid,
}

pub fn require_user(auth: AuthUser) -> Uuid {
    auth.id
}

fn bearer_token(parts: &Parts) -> Result<&str, AppError> {
    let header = parts
        .headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or(AppError::Unauthorized)?;

    header
        .strip_prefix("Bearer ")
        .or_else(|| header.strip_prefix("bearer "))
        .filter(|token| !token.is_empty())
        .ok_or(AppError::Unauthorized)
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = bearer_token(parts)?;
        let claims = decode_access_token(token, &state.config.jwt_secret)?;
        let id = Uuid::parse_str(&claims.sub).map_err(|_| AppError::Unauthorized)?;
        Ok(Self { id })
    }
}
