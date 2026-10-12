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
    #[error("device registration not found")]
    DeviceNotFound,
    #[error("cannot interact with this user")]
    UserBlocked,
    #[error("already friends")]
    AlreadyFriends,
    #[error("a friend request is already pending")]
    FriendRequestPending,
    #[error("friend request has already been decided")]
    FriendRequestDecided,
    #[error("friend request not found")]
    FriendRequestNotFound,
    #[error("a message request is already pending")]
    MessageRequestPending,
    #[error("message request has already been decided")]
    MessageRequestDecided,
    #[error("message request not found")]
    MessageRequestNotFound,
    #[error("cannot send a direct message to this user")]
    CannotMessage,
    #[error("message not found")]
    MessageNotFound,
    #[error("message has been deleted")]
    MessageDeleted,
    #[error("message can no longer be edited")]
    EditWindowExpired,
    #[error("not subscribed to this group")]
    NotSubscribed,
    #[error("too many requests")]
    RateLimited,
    #[error("not allowed to modify this message")]
    CannotModifyMessage,
    #[error("media not found")]
    MediaNotFound,
    #[error("not allowed to access this media")]
    MediaForbidden,
    #[error("upload is too large")]
    UploadTooLarge,
    #[error("semantic search is unavailable")]
    SemanticUnavailable,
    #[error("embedding dimensions do not match the selected model")]
    EmbeddingDimensionMismatch,
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
            Self::DeviceNotFound => "device_not_found",
            Self::UserBlocked => "user_blocked",
            Self::AlreadyFriends => "already_friends",
            Self::FriendRequestPending => "friend_request_pending",
            Self::FriendRequestDecided => "friend_request_already_decided",
            Self::FriendRequestNotFound => "friend_request_not_found",
            Self::MessageRequestPending => "message_request_pending",
            Self::MessageRequestDecided => "message_request_already_decided",
            Self::MessageRequestNotFound => "message_request_not_found",
            Self::CannotMessage => "cannot_message",
            Self::MessageNotFound => "message_not_found",
            Self::MessageDeleted => "message_deleted",
            Self::EditWindowExpired => "edit_window_expired",
            Self::NotSubscribed => "not_subscribed",
            Self::RateLimited => "rate_limited",
            Self::CannotModifyMessage => "cannot_modify_message",
            Self::MediaNotFound => "media_not_found",
            Self::MediaForbidden => "media_forbidden",
            Self::UploadTooLarge => "upload_too_large",
            Self::SemanticUnavailable => "semantic_unavailable",
            Self::EmbeddingDimensionMismatch => "embedding_dimension_mismatch",
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
            | Self::CannotRemoveLeader
            | Self::AlreadyFriends
            | Self::FriendRequestPending
            | Self::FriendRequestDecided
            | Self::MessageRequestPending
            | Self::MessageRequestDecided => StatusCode::CONFLICT,
            Self::NotGroupMember
            | Self::NotGroupLeader
            | Self::UserBlocked
            | Self::CannotMessage
            | Self::NotSubscribed
            | Self::EditWindowExpired
            | Self::CannotModifyMessage
            | Self::MediaForbidden => StatusCode::FORBIDDEN,
            Self::MessageDeleted => StatusCode::CONFLICT,
            Self::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            Self::UploadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::SemanticUnavailable => StatusCode::SERVICE_UNAVAILABLE,
            Self::EmbeddingDimensionMismatch => StatusCode::BAD_REQUEST,
            Self::UserNotFound
            | Self::GroupNotFound
            | Self::JoinRequestNotFound
            | Self::NotificationNotFound
            | Self::DeviceNotFound
            | Self::FriendRequestNotFound
            | Self::MessageRequestNotFound
            | Self::MessageNotFound
            | Self::MediaNotFound => StatusCode::NOT_FOUND,
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
        if db.constraint() == Some("friend_requests_one_pending") {
            return AppError::FriendRequestPending;
        }
        if db.constraint() == Some("message_requests_one_pending") {
            return AppError::MessageRequestPending;
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
