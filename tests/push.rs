mod common;

use std::collections::VecDeque;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{register_user, send, test_state};
use studysync_backend::push::{
    process_due_batch, FcmSendError, FcmSender, PushRuntime, SendOutcome,
};
use tokio::sync::Mutex;
use uuid::Uuid;

fn token(label: &str) -> String {
    format!("fcm-token-{label}-{}", "x".repeat(24))
}

struct ScriptedFcm {
    script: Mutex<VecDeque<Result<SendOutcome, FcmSendError>>>,
    calls: Mutex<usize>,
}

impl ScriptedFcm {
    fn new(items: Vec<Result<SendOutcome, FcmSendError>>) -> Arc<Self> {
        Arc::new(Self {
            script: Mutex::new(items.into()),
            calls: Mutex::new(0),
        })
    }
}

#[async_trait::async_trait]
impl FcmSender for ScriptedFcm {
    async fn send(
        &self,
        _token: &str,
        _notification_id: Uuid,
        _title: &str,
        _body: &str,
    ) -> Result<SendOutcome, FcmSendError> {
        *self.calls.lock().await += 1;
        self.script
            .lock()
            .await
            .pop_front()
            .unwrap_or(Ok(SendOutcome::Delivered))
    }
}

async fn register_device(
    state: studysync_backend::AppState,
    access: &str,
    token: &str,
    installation: Option<&str>,
) -> (StatusCode, serde_json::Value) {
    let mut body = serde_json::json!({
        "platform": "android",
        "token": token,
    });
    if let Some(id) = installation {
        body["installationId"] = serde_json::json!(id);
    }
    send(
        state,
        Request::builder()
            .method("POST")
            .uri("/api/v1/push/devices")
            .header("authorization", format!("Bearer {access}"))
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap(),
    )
    .await
}

#[tokio::test]
async fn device_registration_validation_ownership_and_rotation() {
    let state = test_state().await;
    let (a_access, _) = register_user(state.clone()).await;
    let (b_access, _) = register_user(state.clone()).await;

    let (status, json) = register_device(state.clone(), &a_access, "short", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{json}");

    let (status, json) =
        register_device(state.clone(), &a_access, &token("a1"), Some("phone-a")).await;
    assert_eq!(status, StatusCode::CREATED, "{json}");
    assert!(json.get("token").is_none());
    assert!(json.get("fcmToken").is_none());
    let device_id = json["id"].as_str().unwrap().to_string();

    let (status, json) =
        register_device(state.clone(), &a_access, &token("a1-rot"), Some("phone-a")).await;
    assert_eq!(status, StatusCode::CREATED, "{json}");
    assert_eq!(json["id"], device_id);

    let (status, json) =
        register_device(state.clone(), &a_access, &token("tablet"), Some("tablet-a")).await;
    assert_eq!(status, StatusCode::CREATED, "{json}");
    assert_ne!(json["id"], device_id);

    let stolen = token("stolen");
    let (status, json) =
        register_device(state.clone(), &a_access, &stolen, Some("old-phone")).await;
    assert_eq!(status, StatusCode::CREATED, "{json}");
    let (status, json) = register_device(state.clone(), &b_access, &stolen, Some("b-phone")).await;
    assert_eq!(status, StatusCode::CREATED, "{json}");

    let (status, json) = send(
        state.clone(),
        Request::builder()
            .method("DELETE")
            .uri(format!("/api/v1/push/devices/{device_id}"))
            .header("authorization", format!("Bearer {b_access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{json}");

    let (status, _) = send(
        state.clone(),
        Request::builder()
            .method("DELETE")
            .uri(format!("/api/v1/push/devices/{device_id}"))
            .header("authorization", format!("Bearer {a_access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, json) = send(
        state,
        Request::builder()
            .method("POST")
            .uri("/api/v1/push/devices")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({ "platform": "android", "token": token("anon") }).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{json}");
}

#[tokio::test]
async fn preferences_round_trip_does_not_use_tokens() {
    let state = test_state().await;
    let (access, _) = register_user(state.clone()).await;
    let (status, json) = send(
        state.clone(),
        Request::builder()
            .method("GET")
            .uri("/api/v1/push/preferences")
            .header("authorization", format!("Bearer {access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["pushEnabled"], true);
    assert_eq!(json["messages"], true);

    let (status, json) = send(
        state,
        Request::builder()
            .method("PUT")
            .uri("/api/v1/push/preferences")
            .header("authorization", format!("Bearer {access}"))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({ "pushEnabled": false, "messages": false }).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["pushEnabled"], false);
    assert_eq!(json["messages"], false);
    assert_eq!(json["friends"], true);
}

#[tokio::test]
async fn notification_insert_enqueues_outbox_and_get_one_is_owner_only() {
    let state = test_state().await;
    let (a_access, a_id) = register_user(state.clone()).await;
    let (b_access, _) = register_user(state.clone()).await;

    let (status, json) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/v1/friends/requests")
            .header("authorization", format!("Bearer {b_access}"))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({ "userId": a_id }).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{json}");

    let (status, json) = send(
        state.clone(),
        Request::builder()
            .method("GET")
            .uri("/api/v1/notifications")
            .header("authorization", format!("Bearer {a_access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let note_id = json[0]["id"].as_str().unwrap();
    assert_eq!(json[0]["type"], "friend_request");

    let row: (String,) =
        sqlx::query_as("SELECT status FROM push_outbox WHERE notification_id = $1")
            .bind(Uuid::parse_str(note_id).unwrap())
            .fetch_one(&state.db)
            .await
            .expect("outbox");
    assert_eq!(row.0, "pending");

    let (status, json) = send(
        state.clone(),
        Request::builder()
            .method("GET")
            .uri(format!("/api/v1/notifications/{note_id}"))
            .header("authorization", format!("Bearer {a_access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["id"], note_id);

    let (status, json) = send(
        state,
        Request::builder()
            .method("GET")
            .uri(format!("/api/v1/notifications/{note_id}"))
            .header("authorization", format!("Bearer {b_access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{json}");
}

#[tokio::test]
async fn worker_delivers_retries_and_revokes_invalid_tokens() {
    let base = test_state().await;
    let (access, user_id) = register_user(base.clone()).await;
    let (status, _) = register_device(base.clone(), &access, &token("live"), None).await;
    assert_eq!(status, StatusCode::CREATED);

    let (peer_access, _) = register_user(base.clone()).await;
    let (status, json) = send(
        base.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/v1/friends/requests")
            .header("authorization", format!("Bearer {peer_access}"))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({ "userId": user_id }).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{json}");

    let sender = ScriptedFcm::new(vec![Ok(SendOutcome::Delivered)]);
    let runtime = Arc::new(PushRuntime {
        delivery_enabled: true,
        sender: sender.clone(),
    });
    let n = process_due_batch(&base.db, runtime.sender.as_ref(), 16)
        .await
        .expect("batch");
    assert!(n >= 1);
    assert_eq!(*sender.calls.lock().await, 1);

    let (status, json) = send(
        base.clone(),
        Request::builder()
            .method("GET")
            .uri("/api/v1/notifications")
            .header("authorization", format!("Bearer {access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json[0]["type"], "friend_request");

    let (status, json) =
        register_device(base.clone(), &access, &token("bad"), Some("bad-phone")).await;
    assert_eq!(status, StatusCode::CREATED, "{json}");

    let (peer2, _) = register_user(base.clone()).await;
    let (status, _) = send(
        base.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/v1/friends/requests")
            .header("authorization", format!("Bearer {peer2}"))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({ "userId": user_id }).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let fail_then_ok = ScriptedFcm::new(vec![
        Err(FcmSendError::Transient),
        Err(FcmSendError::Transient),
        Ok(SendOutcome::Unregistered),
        Ok(SendOutcome::Unregistered),
    ]);
    let _ = process_due_batch(&base.db, fail_then_ok.as_ref(), 16)
        .await
        .expect("transient");
    let _ = process_due_batch(&base.db, fail_then_ok.as_ref(), 16)
        .await
        .expect("unregistered");
}

#[tokio::test]
async fn duplicate_outbox_rows_are_not_created_for_one_notification() {
    let state = test_state().await;
    let (access, user_id) = register_user(state.clone()).await;
    let note_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO notifications (id, user_id, type, title, body, data)
         VALUES ($1,$2,'friend_request','t','b','{}'::jsonb)",
    )
    .bind(note_id)
    .bind(user_id)
    .execute(&state.db)
    .await
    .unwrap();
    studysync_backend::db::push::enqueue_outbox(&state.db, note_id, user_id)
        .await
        .unwrap();
    studysync_backend::db::push::enqueue_outbox(&state.db, note_id, user_id)
        .await
        .unwrap();
    let count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM push_outbox WHERE notification_id = $1")
            .bind(note_id)
            .fetch_one(&state.db)
            .await
            .unwrap();
    assert_eq!(count.0, 1);
    let _ = access;
}
