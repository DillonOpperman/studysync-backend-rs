use std::collections::HashMap;

use http_body_util::BodyExt;
use studysync_backend::config::Config;
use studysync_backend::{api, AppState};
use tower::ServiceExt;
use uuid::Uuid;

pub fn test_config(database_url: &str) -> Config {
    let vars = HashMap::from([
        ("DATABASE_URL".into(), database_url.to_string()),
        (
            "JWT_SECRET".into(),
            "abcdefghijklmnopqrstuvwxyz012345".into(),
        ),
        ("PUBLIC_BASE_URL".into(), "http://127.0.0.1:8080".into()),
    ]);
    Config::from_vars(&vars).expect("test config")
}

/// PostgreSQL is required. Missing URL or an unreachable server fails the test
/// process; it must not skip and count as passing.
pub async fn test_state() -> AppState {
    let database_url = std::env::var("DATABASE_URL")
        .ok()
        .filter(|url| !url.is_empty());
    let Some(database_url) = database_url else {
        panic!(
            "DATABASE_URL is required for integration tests. Start docker compose, then set \
             DATABASE_URL=postgres://studysync:studysync_dev_only_change_me@127.0.0.1:5432/studysync?sslmode=disable"
        );
    };
    let config = test_config(&database_url);
    let db = match studysync_backend::db::create_pool(&config).await {
        Ok(pool) => pool,
        Err(_) => panic!(
            "PostgreSQL is required for integration tests but is unreachable. \
             Start docker compose and confirm DATABASE_URL (do not print the URL; it contains a password)."
        ),
    };
    AppState::new(config, db)
}

pub fn unique_email() -> String {
    format!("user-{}@campus.edu", Uuid::now_v7())
}

pub fn register_body(email: &str, password: &str) -> serde_json::Value {
    serde_json::json!({
        "name": "Ada Lovelace",
        "email": email,
        "password": password,
        "institutionName": format!("Example University {email}"),
        "major": "Computer Science",
        "year": "Junior",
        "learningStyle": "I learn by doing",
        "studyEnvironments": ["Library"],
        "studyMethods": ["Practice problems"],
        "subjects": ["Algorithms"],
        "availability": [{
            "dayOfWeek": 0,
            "startTime": "17:00",
            "endTime": "21:00",
            "timezone": "America/Chicago"
        }],
        "performanceLevel": 4,
        "groupPreferences": {
            "preferredGroupSizeMin": 3,
            "preferredGroupSizeMax": 5,
            "sessionDurationMinutes": 90,
            "studyGoals": ["Exam preparation"]
        }
    })
}

#[allow(dead_code)]
pub async fn register_user(state: AppState) -> (String, Uuid) {
    let email = unique_email();
    let (status, json) = send(
        state,
        axum::http::Request::builder()
            .method("POST")
            .uri("/api/v1/auth/register")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(
                register_body(&email, "password12").to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::CREATED, "{json}");
    let access = json["accessToken"].as_str().unwrap().to_string();
    let id = Uuid::parse_str(json["user"]["id"].as_str().unwrap()).unwrap();
    (access, id)
}

pub async fn send(
    state: AppState,
    request: axum::http::Request<axum::body::Body>,
) -> (axum::http::StatusCode, serde_json::Value) {
    let response = api::router(state).oneshot(request).await.expect("response");
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    if bytes.is_empty() {
        return (status, serde_json::Value::Null);
    }
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}

#[allow(dead_code)]
pub async fn spawn_app(state: AppState) -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let app = api::router(state);
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    port
}
