use std::time::Duration;

use axum::extract::Request;
use axum::http::HeaderName;
use axum::http::HeaderValue;
use axum::Router;
use tower_http::request_id::{
    MakeRequestId, PropagateRequestIdLayer, RequestId, SetRequestIdLayer,
};
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;
use uuid::Uuid;

use crate::AppState;

pub mod dto;
pub mod v1;

const REQUEST_ID_HEADER: HeaderName = HeaderName::from_static("x-request-id");

#[derive(Clone, Default)]
struct MakeRequestUuidV7;

impl MakeRequestId for MakeRequestUuidV7 {
    fn make_request_id<B>(&mut self, _request: &Request<B>) -> Option<RequestId> {
        let id = Uuid::now_v7().to_string();
        HeaderValue::from_str(&id).ok().map(RequestId::from)
    }
}

pub fn router(state: AppState) -> Router {
    let trace = TraceLayer::new_for_http().make_span_with(|request: &Request| {
        let request_id = request
            .headers()
            .get(REQUEST_ID_HEADER)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("-");
        tracing::info_span!(
            "http",
            method = %request.method(),
            path = %request.uri().path(),
            request_id = %request_id,
        )
    });

    // Timeouts apply to REST only. Long-lived WebSockets are idle by design.
    let http = Router::new()
        .nest("/api/v1", v1::router())
        .layer(TimeoutLayer::with_status_code(
            axum::http::StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(30),
        ));

    let ws = Router::new().route("/api/v1/ws", axum::routing::get(crate::chat::upgrade));

    http.merge(ws)
        .layer(trace)
        .layer(PropagateRequestIdLayer::new(REQUEST_ID_HEADER))
        .layer(SetRequestIdLayer::new(REQUEST_ID_HEADER, MakeRequestUuidV7))
        .with_state(state)
}

/// Correlation id generation used by tests (UUID v7, string form).
pub fn new_request_id() -> String {
    Uuid::now_v7().to_string()
}
