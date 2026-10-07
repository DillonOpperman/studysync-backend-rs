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
    pub fn code(&self) -> &'static str {
        match self {
            Self::Internal => "internal_error",
            Self::NotReady => "not_ready",
        }
    }

    pub fn status(&self) -> StatusCode {
        match self {
            Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
            Self::NotReady => StatusCode::SERVICE_UNAVAILABLE,
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
        tracing::error!(code = self.code(), "request failed");
        (self.status(), Json(self.body())).into_response()
    }
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
}
