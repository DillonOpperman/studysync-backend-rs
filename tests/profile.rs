mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{register_body, send, test_state, unique_email};

#[tokio::test]
async fn patch_me_updates_postgres_and_validates_availability() {
    let Some(state) = test_state().await else {
        eprintln!("skipping: DATABASE_URL not reachable");
        return;
    };
    let email = unique_email();
    let (_, created) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/v1/auth/register")
            .header("content-type", "application/json")
            .body(Body::from(register_body(&email, "password12").to_string()))
            .unwrap(),
    )
    .await;
    let access = created["accessToken"].as_str().unwrap();

    let (status, json) = send(
        state.clone(),
        Request::builder()
            .method("PATCH")
            .uri("/api/v1/me")
            .header("authorization", format!("Bearer {access}"))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "major": "Mathematics",
                    "subjects": ["Linear Algebra", "Statistics"],
                    "groupPreferences": {
                        "preferredGroupSizeMin": 2,
                        "preferredGroupSizeMax": 4,
                        "sessionDurationMinutes": 60
                    },
                    "availability": [{
                        "dayOfWeek": 2,
                        "startTime": "09:00",
                        "endTime": "11:00",
                        "timezone": "America/New_York"
                    }]
                })
                .to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["major"], "Mathematics");
    assert_eq!(json["subjects"].as_array().unwrap().len(), 2);
    assert_eq!(json["groupPreferences"]["sessionDurationMinutes"], 60);
    assert_eq!(json["availability"][0]["timezone"], "America/New_York");

    let (status, json) = send(
        state,
        Request::builder()
            .method("PATCH")
            .uri("/api/v1/me")
            .header("authorization", format!("Bearer {access}"))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "availability": [{
                        "dayOfWeek": 1,
                        "startTime": "15:00",
                        "endTime": "14:00",
                        "timezone": "America/Chicago"
                    }]
                })
                .to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json["error"]["code"], "validation_error");
}

#[tokio::test]
async fn register_rejects_invalid_year_and_group_size() {
    let Some(state) = test_state().await else {
        eprintln!("skipping: DATABASE_URL not reachable");
        return;
    };
    let mut body = register_body(&unique_email(), "password12");
    body["year"] = serde_json::json!("Alumni");
    let (status, json) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/v1/auth/register")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json["error"]["code"], "validation_error");

    let mut body = register_body(&unique_email(), "password12");
    body["groupPreferences"] = serde_json::json!({
        "preferredGroupSizeMin": 8,
        "preferredGroupSizeMax": 3,
        "sessionDurationMinutes": 90
    });
    let (status, json) = send(
        state,
        Request::builder()
            .method("POST")
            .uri("/api/v1/auth/register")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json["error"]["code"], "validation_error");
}
