use std::collections::HashMap;

use http_body_util::BodyExt;
use studysync_backend::config::Config;
use studysync_backend::db;
use studysync_backend::{api, AppState};
use tower::ServiceExt;

fn test_config() -> Config {
    let vars = HashMap::from([
        (
            "DATABASE_URL".into(),
            "postgres://studysync:dev@127.0.0.1:1/studysync".into(),
        ),
        (
            "JWT_SECRET".into(),
            "abcdefghijklmnopqrstuvwxyz012345".into(),
        ),
        ("PUBLIC_BASE_URL".into(), "http://127.0.0.1:8080".into()),
        ("EMBEDDING_BACKEND".into(), "unavailable".into()),
    ]);
    Config::from_vars(&vars).expect("test config")
}

fn unreachable_state() -> AppState {
    let config = test_config();
    let db = db::lazy_pool(&config.database_url).expect("lazy pool");
    AppState::new(config, db)
}

#[tokio::test]
async fn health_returns_ok_without_postgres() {
    let app = api::router(unreachable_state());
    let response = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/v1/health")
                .body(axum::body::Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(json, serde_json::json!({ "status": "ok" }));
}

#[tokio::test]
async fn ready_reports_not_ready_when_postgres_is_unavailable() {
    let app = api::router(unreachable_state());
    let response = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/v1/ready")
                .body(axum::body::Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(
        response.status(),
        axum::http::StatusCode::SERVICE_UNAVAILABLE
    );
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(json["status"], "notReady");
    assert_eq!(json["postgres"], "unavailable");
    assert!(json.get("databaseUrl").is_none());
}

#[tokio::test]
async fn request_ids_are_uuid_v7_strings() {
    let id = api::new_request_id();
    assert_eq!(id.len(), 36);
    let version_nibble = id.as_bytes().get(14).copied();
    assert_eq!(version_nibble, Some(b'7'));
}

#[tokio::test]
async fn ready_returns_ready_when_postgres_is_available() {
    let database_url = std::env::var("DATABASE_URL")
        .ok()
        .filter(|url| !url.is_empty());
    let Some(database_url) = database_url else {
        panic!(
            "DATABASE_URL is required for integration tests. Start docker compose and export DATABASE_URL."
        );
    };

    let mut vars = HashMap::new();
    vars.insert("DATABASE_URL".into(), database_url);
    vars.insert(
        "JWT_SECRET".into(),
        "abcdefghijklmnopqrstuvwxyz012345".into(),
    );
    vars.insert("PUBLIC_BASE_URL".into(), "http://127.0.0.1:8080".into());
    vars.insert("EMBEDDING_BACKEND".into(), "unavailable".into());
    let config = Config::from_vars(&vars).expect("config");

    let db = match studysync_backend::db::create_pool(&config).await {
        Ok(pool) => pool,
        Err(_) => panic!(
            "PostgreSQL is required for integration tests but is unreachable (URL not logged)."
        ),
    };

    let app = api::router(AppState::new(config, db));
    let response = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/v1/ready")
                .body(axum::body::Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(json["status"], "ready");
    assert_eq!(json["postgres"], "ok");
}
