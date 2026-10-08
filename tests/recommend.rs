mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{register_body, send, test_state, unique_email};
use studysync_backend::AppState;
use uuid::Uuid;

async fn register_named(
    state: AppState,
    institution: &str,
    major: &str,
    subjects: &[&str],
) -> (String, Uuid) {
    let email = unique_email();
    let mut body = register_body(&email, "password12");
    body["institutionName"] = serde_json::json!(institution);
    body["major"] = serde_json::json!(major);
    body["subjects"] = serde_json::json!(subjects);
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
    assert_eq!(status, StatusCode::CREATED, "{json}");
    let access = json["accessToken"].as_str().unwrap().to_string();
    let id = Uuid::parse_str(json["user"]["id"].as_str().unwrap()).unwrap();
    (access, id)
}

async fn create_group(
    state: AppState,
    access: &str,
    title: &str,
    subject: &str,
    start: &str,
    end: &str,
) -> serde_json::Value {
    let (status, json) = send(
        state,
        Request::builder()
            .method("POST")
            .uri("/api/v1/groups")
            .header("authorization", format!("Bearer {access}"))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "title": title,
                    "subject": subject,
                    "description": format!("{subject} study"),
                    "location": "Library",
                    "maxMembers": 6,
                    "recurringSlots": [{
                        "dayOfWeek": 0,
                        "startTime": start,
                        "endTime": end,
                        "timezone": "America/Chicago"
                    }]
                })
                .to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{json}");
    json
}

#[tokio::test]
async fn recommend_matches_exact_subject_and_schedule() {
    let state = test_state().await;
    let campus = format!("Campus {}", Uuid::now_v7());
    let (leader_access, _) = register_named(
        state.clone(),
        &campus,
        "Computer Science",
        &["Computer Networks"],
    )
    .await;
    let group = create_group(
        state.clone(),
        &leader_access,
        "Networks Lab",
        "Computer Networks",
        "17:00",
        "21:00",
    )
    .await;

    let (seeker_access, _) = register_named(
        state.clone(),
        &campus,
        "Computer Science",
        &["Computer Networks"],
    )
    .await;
    let (status, json) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/v1/recommend")
            .header("authorization", format!("Bearer {seeker_access}"))
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["createNew"], false);
    assert_eq!(json["groups"].as_array().unwrap().len(), 1);
    assert_eq!(json["groups"][0]["group"]["id"], group["id"]);
    assert!(json["groups"][0]["score"].as_f64().unwrap() >= 0.25);
    assert_eq!(
        json["formula"],
        "0.50*group_subject + 0.20*leader_subject_similarity + 0.20*schedule_overlap + 0.10*size_match"
    );
}

#[tokio::test]
async fn recommend_excludes_leader_member_and_pending() {
    let state = test_state().await;
    let campus = format!("Campus {}", Uuid::now_v7());
    let (leader_access, _) = register_named(
        state.clone(),
        &campus,
        "Computer Science",
        &["Computer Networks"],
    )
    .await;
    let group = create_group(
        state.clone(),
        &leader_access,
        "Networks Lab",
        "Computer Networks",
        "17:00",
        "21:00",
    )
    .await;
    let group_id = group["id"].as_str().unwrap();

    let (status, mine) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/v1/recommend")
            .header("authorization", format!("Bearer {leader_access}"))
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{mine}");
    assert_eq!(mine["createNew"], true);
    assert!(mine["groups"].as_array().unwrap().is_empty());

    let (seeker_access, _) = register_named(
        state.clone(),
        &campus,
        "Computer Science",
        &["Computer Networks"],
    )
    .await;
    let (status, _) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri(format!("/api/v1/groups/{group_id}/join-requests"))
            .header("authorization", format!("Bearer {seeker_access}"))
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, json) = send(
        state,
        Request::builder()
            .method("POST")
            .uri("/api/v1/recommend")
            .header("authorization", format!("Bearer {seeker_access}"))
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["createNew"], true);
}

#[tokio::test]
async fn recommend_create_new_below_threshold() {
    let state = test_state().await;
    let campus = format!("Campus {}", Uuid::now_v7());
    let (leader_access, _) =
        register_named(state.clone(), &campus, "Chemistry", &["Organic Chemistry"]).await;
    create_group(
        state.clone(),
        &leader_access,
        "Orgo night",
        "Organic Chemistry",
        "08:00",
        "09:00",
    )
    .await;
    let (seeker_access, _) =
        register_named(state.clone(), &campus, "History", &["World History"]).await;
    let (status, json) = send(
        state,
        Request::builder()
            .method("POST")
            .uri("/api/v1/recommend")
            .header("authorization", format!("Bearer {seeker_access}"))
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["createNew"], true);
}

#[tokio::test]
async fn recommend_still_works_when_embeddings_unavailable() {
    let mut state = test_state().await;
    state.embeddings = studysync_backend::embed::EmbeddingService::unavailable();
    let campus = format!("Campus {}", Uuid::now_v7());
    let (leader_access, _) = register_named(
        state.clone(),
        &campus,
        "Computer Science",
        &["Computer Networks"],
    )
    .await;
    create_group(
        state.clone(),
        &leader_access,
        "Networks Lab",
        "Computer Networks",
        "17:00",
        "21:00",
    )
    .await;
    let (seeker_access, _) = register_named(
        state.clone(),
        &campus,
        "Computer Science",
        &["Computer Networks"],
    )
    .await;
    let (status, json) = send(
        state,
        Request::builder()
            .method("POST")
            .uri("/api/v1/recommend")
            .header("authorization", format!("Bearer {seeker_access}"))
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["semanticAvailable"], false);
    assert_eq!(json["createNew"], false);
}
