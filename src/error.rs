use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

/// Safe HTTP error. Internal details stay in traces, never in the body.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("an internal error occurred")]
    Internal,
    #[error("service is not ready")]
    NotReady,
    #[error("{0}")]
    Validation(String),
    #[error("invalid credentials")]
    InvalidCredentials,
    #[error("authentication required")]
    Unauthorized,
    #[error("access token has expired")]
    ExpiredToken,
    #[error("invalid or expired refresh token")]
    InvalidRefresh,
    #[error("email is already registered")]
    DuplicateEmail,
    #[error("user not found")]
    UserNotFound,
    #[error("group not found")]
    GroupNotFound,
    #[error("not a group member")]
    NotGroupMember,
    #[error("not the group leader")]
    NotGroupLeader,
    #[error("already a group member")]
    AlreadyMember,
    #[error("a join request is already pending")]
    JoinRequestPending,
    #[error("join request has already been decided")]
    JoinRequestDecided,
    #[error("join request not found")]
    JoinRequestNotFound,
    #[error("group is archived")]
    GroupArchived,
    #[error("group is full")]
    GroupFull,
    #[error("leader cannot leave while other members remain; transfer leadership first")]
    CannotLeaveAsLeader,
    #[error("cannot remove the group leader")]
    CannotRemoveLeader,
    #[error("notification not found")]
    NotificationNotFound,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorBody {
    pub error: ErrorDetail,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorDetail {
    pub code: &'static str,
    pub message: String,
}

impl AppError {
    pub fn validation(message: impl Into<String>) -> Self {
        Self::Validation(message.into())
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::Internal => "internal_error",
            Self::NotReady => "not_ready",
            Self::Validation(_) => "validation_error",
            Self::InvalidCredentials => "invalid_credentials",
            Self::Unauthorized => "unauthorized",
            Self::ExpiredToken => "token_expired",
            Self::InvalidRefresh => "invalid_refresh_token",
            Self::DuplicateEmail => "duplicate_email",
            Self::UserNotFound => "user_not_found",
            Self::GroupNotFound => "group_not_found",
            Self::NotGroupMember => "not_group_member",
            Self::NotGroupLeader => "not_group_leader",
            Self::AlreadyMember => "already_member",
            Self::JoinRequestPending => "join_request_pending",
            Self::JoinRequestDecided => "join_request_already_decided",
            Self::JoinRequestNotFound => "join_request_not_found",
            Self::GroupArchived => "group_archived",
            Self::GroupFull => "group_full",
            Self::CannotLeaveAsLeader => "cannot_leave_as_leader",
            Self::CannotRemoveLeader => "cannot_remove_leader",
            Self::NotificationNotFound => "notification_not_found",
        }
    }

    pub fn status(&self) -> StatusCode {
        match self {
            Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
            Self::NotReady => StatusCode::SERVICE_UNAVAILABLE,
            Self::Validation(_) => StatusCode::BAD_REQUEST,
            Self::InvalidCredentials
            | Self::Unauthorized
            | Self::ExpiredToken
            | Self::InvalidRefresh => StatusCode::UNAUTHORIZED,
            Self::DuplicateEmail
            | Self::AlreadyMember
            | Self::JoinRequestPending
            | Self::JoinRequestDecided
            | Self::GroupArchived
            | Self::GroupFull
            | Self::CannotLeaveAsLeader
            | Self::CannotRemoveLeader => StatusCode::CONFLICT,
            Self::NotGroupMember | Self::NotGroupLeader => StatusCode::FORBIDDEN,
            Self::UserNotFound
            | Self::GroupNotFound
            | Self::JoinRequestNotFound
            | Self::NotificationNotFound => StatusCode::NOT_FOUND,
        }
    }

    pub fn body(&self) -> ErrorBody {
        ErrorBody {
            error: ErrorDetail {
                code: self.code(),
                message: self.to_string(),
            },
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        match self {
            Self::Internal => tracing::error!(code = self.code(), "request failed"),
            Self::Validation(_) => tracing::debug!(code = self.code(), "validation failed"),
            _ => tracing::warn!(code = self.code(), "request rejected"),
        }
        (self.status(), Json(self.body())).into_response()
    }
}

/// Unique-index (`23505`) or serialization (`40001`) failure that callers may
/// recover from by re-selecting the existing row (or retrying once).
pub fn is_unique_or_serialization(error: &sqlx::Error) -> bool {
    let sqlx::Error::Database(db) = error else {
        return false;
    };
    matches!(db.code().as_deref(), Some("40001") | Some("23505"))
}

pub fn unique_constraint(error: &sqlx::Error) -> Option<&str> {
    match error {
        sqlx::Error::Database(db) => db.constraint(),
        _ => None,
    }
}

pub fn map_db_error(error: sqlx::Error) -> AppError {
    if let sqlx::Error::Database(db) = &error {
        if db.constraint() == Some("users_email_key") {
            return AppError::DuplicateEmail;
        }
        if db.constraint() == Some("group_join_requests_one_pending") {
            return AppError::JoinRequestPending;
        }
        if db.constraint() == Some("group_members_user_group_key") {
            return AppError::AlreadyMember;
        }
        tracing::error!(
            constraint = db.constraint(),
            code = db.code().as_deref(),
            "database operation failed"
        );
        return AppError::Internal;
    }
    tracing::error!(kind = "sqlx", "database operation failed");
    AppError::Internal
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_internal_error_without_implementation_details() {
        let json = serde_json::to_value(AppError::Internal.body()).expect("json");
        assert_eq!(
            json,
            serde_json::json!({
                "error": {
                    "code": "internal_error",
                    "message": "an internal error occurred"
                }
            })
        );
    }

    #[test]
    fn serializes_not_ready_error() {
        let json = serde_json::to_value(AppError::NotReady.body()).expect("json");
        assert_eq!(json["error"]["code"], "not_ready");
    }

    #[test]
    fn serializes_validation_and_auth_errors() {
        let validation =
            serde_json::to_value(AppError::validation("year is invalid").body()).unwrap();
        assert_eq!(validation["error"]["code"], "validation_error");
        assert_eq!(validation["error"]["message"], "year is invalid");

        assert_eq!(AppError::DuplicateEmail.status(), StatusCode::CONFLICT);
        assert_eq!(AppError::ExpiredToken.code(), "token_expired");
        assert_eq!(
            AppError::InvalidCredentials.status(),
            StatusCode::UNAUTHORIZED
        );
    }
}
