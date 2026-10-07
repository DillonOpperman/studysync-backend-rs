use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Serialize;
use sqlx::PgPool;

use crate::AppState;

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
}

pub async fn health() -> impl IntoResponse {
    Json(HealthResponse { status: "ok" })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadyResponse {
    pub status: &'static str,
    pub postgres: &'static str,
}

pub async fn ready(State(state): State<AppState>) -> impl IntoResponse {
    if postgres_reachable(&state.db).await {
        (
            StatusCode::OK,
            Json(ReadyResponse {
                status: "ready",
                postgres: "ok",
            }),
        )
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ReadyResponse {
                status: "notReady",
                postgres: "unavailable",
            }),
        )
    }
}

async fn postgres_reachable(pool: &PgPool) -> bool {
    match sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(pool)
        .await
    {
        Ok(_) => true,
        Err(_) => {
            tracing::warn!("readiness postgres check failed");
            false
        }
    }
}
