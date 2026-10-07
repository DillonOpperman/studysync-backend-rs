mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{register_user, send, test_state};
use uuid::Uuid;

fn create_group_body(max_members: i16) -> serde_json::Value {
    serde_json::json!({
        "title": "Algorithms crew",
        "subject": "Algorithms",
        "description": "Weekly problem set",
        "location": "Library",
        "maxMembers": max_members,
        "recurringSlots": [{
            "dayOfWeek": 0,
            "startTime": "23:00",
            "endTime": "01:00",
            "timezone": "America/Chicago"
        }]
    })
}

async fn create_group(
    state: studysync_backend::AppState,
    access: &str,
    max_members: i16,
) -> serde_json::Value {
    let (status, json) = send(
        state,
        Request::builder()
            .method("POST")
            .uri("/api/v1/groups")
            .header("authorization", format!("Bearer {access}"))
            .header("content-type", "application/json")
            .body(Body::from(create_group_body(max_members).to_string()))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{json}");
    json
}

#[tokio::test]
async fn create_group_makes_leader_and_returns_schedule() {
    let state = test_state().await;
    let (access, user_id) = register_user(state.clone()).await;
    let json = create_group(state.clone(), &access, 5).await;
    assert_eq!(json["membershipStatus"], "leader");
    assert_eq!(json["isLeader"], true);
    assert_eq!(json["memberCount"], 1);
    assert_eq!(json["maxMembers"], 5);
    assert_eq!(json["leader"]["id"], user_id.to_string());
    assert_eq!(json["recurringSlots"][0]["startTime"], "23:00:00");
    assert_eq!(json["recurringSlots"][0]["endTime"], "01:00:00");
    assert!(
        json["recurringSlots"][0]["endTime"].as_str().unwrap()
            < json["recurringSlots"][0]["startTime"].as_str().unwrap()
            || json["recurringSlots"][0]["endTime"] == "01:00:00"
    );

    let group_id = json["id"].as_str().unwrap();
    let (status, mine) = send(
        state.clone(),
        Request::builder()
            .method("GET")
            .uri("/api/v1/groups/mine")
            .header("authorization", format!("Bearer {access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(mine[0]["id"], group_id);

    let (status, detail) = send(
        state.clone(),
        Request::builder()
            .method("GET")
            .uri(format!("/api/v1/groups/{group_id}"))
            .header("authorization", format!("Bearer {access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    assert_eq!(detail["isLeader"], true);

    let (status, members) = send(
        state,
        Request::builder()
            .method("GET")
            .uri(format!("/api/v1/groups/{group_id}/members"))
            .header("authorization", format!("Bearer {access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(members.as_array().unwrap().len(), 1);
    assert_eq!(members[0]["role"], "leader");
    assert!(members[0].get("passwordHash").is_none());
}

#[tokio::test]
async fn invalid_max_members_rejected() {
    let state = test_state().await;
    let (access, _) = register_user(state.clone()).await;
    let (status, json) = send(
        state,
        Request::builder()
            .method("POST")
            .uri("/api/v1/groups")
            .header("authorization", format!("Bearer {access}"))
            .header("content-type", "application/json")
            .body(Body::from(create_group_body(1).to_string()))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json["error"]["code"], "validation_error");
}

#[tokio::test]
async fn join_request_approval_rejection_and_notifications() {
    let state = test_state().await;
    let (leader_access, leader_id) = register_user(state.clone()).await;
    let (member_access, member_id) = register_user(state.clone()).await;
    let (other_access, _) = register_user(state.clone()).await;
    let group = create_group(state.clone(), &leader_access, 5).await;
    let group_id = group["id"].as_str().unwrap();

    let (status, json) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri(format!("/api/v1/groups/{group_id}/join-requests"))
            .header("authorization", format!("Bearer {leader_access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{json}");
    assert_eq!(json["error"]["code"], "already_member");

    let (status, created) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri(format!("/api/v1/groups/{group_id}/join-requests"))
            .header("authorization", format!("Bearer {member_access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let request_id = created["id"].as_str().unwrap();

    let (status, dup) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri(format!("/api/v1/groups/{group_id}/join-requests"))
            .header("authorization", format!("Bearer {member_access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(dup["error"]["code"], "join_request_pending");

    let (status, notes) = send(
        state.clone(),
        Request::builder()
            .method("GET")
            .uri("/api/v1/notifications")
            .header("authorization", format!("Bearer {leader_access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(notes[0]["type"], "join_request");
    assert!(notes[0]["readAt"].is_null());

    let (status, member_notes) = send(
        state.clone(),
        Request::builder()
            .method("GET")
            .uri("/api/v1/notifications")
            .header("authorization", format!("Bearer {member_access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(member_notes.as_array().unwrap().len(), 0);

    let (status, json) = send(
        state.clone(),
        Request::builder()
            .method("GET")
            .uri(format!("/api/v1/groups/{group_id}/join-requests"))
            .header("authorization", format!("Bearer {other_access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(json["error"]["code"], "not_group_member");

    let (status, json) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri(format!(
                "/api/v1/groups/{group_id}/join-requests/{request_id}/approve"
            ))
            .header("authorization", format!("Bearer {member_access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(json["error"]["code"], "not_group_member");

    let (status, json) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri(format!(
                "/api/v1/groups/{group_id}/join-requests/{request_id}/reject"
            ))
            .header("authorization", format!("Bearer {leader_access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["status"], "rejected");

    let (status, notes) = send(
        state.clone(),
        Request::builder()
            .method("GET")
            .uri("/api/v1/notifications")
            .header("authorization", format!("Bearer {member_access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(notes[0]["type"], "request_rejected");

    let (status, again) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri(format!("/api/v1/groups/{group_id}/join-requests"))
            .header("authorization", format!("Bearer {member_access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{again}");
    let request_id = again["id"].as_str().unwrap();

    let (status, json) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri(format!(
                "/api/v1/groups/{group_id}/join-requests/{request_id}/approve"
            ))
            .header("authorization", format!("Bearer {leader_access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["status"], "approved");

    let (status, decided) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri(format!(
                "/api/v1/groups/{group_id}/join-requests/{request_id}/approve"
            ))
            .header("authorization", format!("Bearer {leader_access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(decided["error"]["code"], "join_request_already_decided");

    let (status, mine) = send(
        state.clone(),
        Request::builder()
            .method("GET")
            .uri("/api/v1/groups/mine")
            .header("authorization", format!("Bearer {member_access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(mine.as_array().unwrap().len(), 1);

    let (status, members) = send(
        state.clone(),
        Request::builder()
            .method("GET")
            .uri(format!("/api/v1/groups/{group_id}/members"))
            .header("authorization", format!("Bearer {member_access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(members.as_array().unwrap().len(), 2);

    let (status, json) = send(
        state.clone(),
        Request::builder()
            .method("GET")
            .uri(format!("/api/v1/groups/{group_id}/join-requests"))
            .header("authorization", format!("Bearer {member_access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(json["error"]["code"], "not_group_leader");

    let (status, notes) = send(
        state.clone(),
        Request::builder()
            .method("GET")
            .uri("/api/v1/notifications")
            .header("authorization", format!("Bearer {member_access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(notes[0]["type"], "request_approved");
    let note_id = notes[0]["id"].as_str().unwrap();

    let (status, _) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri(format!("/api/v1/notifications/{note_id}/read"))
            .header("authorization", format!("Bearer {other_access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri(format!("/api/v1/notifications/{note_id}/read"))
            .header("authorization", format!("Bearer {member_access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/v1/notifications/read-all")
            .header("authorization", format!("Bearer {leader_access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, json) = send(
        state.clone(),
        Request::builder()
            .method("DELETE")
            .uri(format!("/api/v1/groups/{group_id}/members/{leader_id}"))
            .header("authorization", format!("Bearer {leader_access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(json["error"]["code"], "cannot_remove_leader");

    let (status, json) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri(format!("/api/v1/groups/{group_id}/leave"))
            .header("authorization", format!("Bearer {leader_access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(json["error"]["code"], "cannot_leave_as_leader");

    let (status, _) = send(
        state.clone(),
        Request::builder()
            .method("DELETE")
            .uri(format!("/api/v1/groups/{group_id}/members/{member_id}"))
            .header("authorization", format!("Bearer {leader_access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, json) = send(
        state.clone(),
        Request::builder()
            .method("GET")
            .uri(format!("/api/v1/groups/{group_id}/members"))
            .header("authorization", format!("Bearer {member_access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(json["error"]["code"], "not_group_member");

    let (status, mine) = send(
        state,
        Request::builder()
            .method("GET")
            .uri("/api/v1/groups/mine")
            .header("authorization", format!("Bearer {member_access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(mine.as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn archived_group_rejects_join_and_user_b_cannot_manage_user_a() {
    let state = test_state().await;
    let (leader_a, _) = register_user(state.clone()).await;
    let (leader_b, _) = register_user(state.clone()).await;
    let (joiner, _) = register_user(state.clone()).await;
    let group_a = create_group(state.clone(), &leader_a, 5).await;
    let group_b = create_group(state.clone(), &leader_b, 5).await;
    let id_a = group_a["id"].as_str().unwrap();
    let id_b = group_b["id"].as_str().unwrap();

    let (status, json) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri(format!("/api/v1/groups/{id_a}/archive"))
            .header("authorization", format!("Bearer {leader_b}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(json["error"]["code"], "not_group_member");

    let (status, json) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri(format!("/api/v1/groups/{id_a}/archive"))
            .header("authorization", format!("Bearer {leader_a}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["status"], "archived");

    let (status, json) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri(format!("/api/v1/groups/{id_a}/join-requests"))
            .header("authorization", format!("Bearer {joiner}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(json["error"]["code"], "group_archived");

    let (status, json) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri(format!("/api/v1/groups/{id_b}/join-requests"))
            .header("authorization", format!("Bearer {joiner}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{json}");
    let request_id = json["id"].as_str().unwrap();

    let (status, json) = send(
        state,
        Request::builder()
            .method("POST")
            .uri(format!(
                "/api/v1/groups/{id_b}/join-requests/{request_id}/approve"
            ))
            .header("authorization", format!("Bearer {leader_a}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(json["error"]["code"], "not_group_member");
}

#[tokio::test]
async fn sole_leader_leave_archives_group() {
    let state = test_state().await;
    let (access, _) = register_user(state.clone()).await;
    let group = create_group(state.clone(), &access, 4).await;
    let group_id = group["id"].as_str().unwrap();
    let (status, _) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri(format!("/api/v1/groups/{group_id}/leave"))
            .header("authorization", format!("Bearer {access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, detail) = send(
        state,
        Request::builder()
            .method("GET")
            .uri(format!("/api/v1/groups/{group_id}"))
            .header("authorization", format!("Bearer {access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["status"], "archived");
    assert_eq!(detail["membershipStatus"], "none");
}

#[tokio::test]
async fn concurrent_approvals_cannot_exceed_capacity() {
    let state = test_state().await;
    let (leader_access, _) = register_user(state.clone()).await;
    let (a_access, _) = register_user(state.clone()).await;
    let (b_access, _) = register_user(state.clone()).await;
    let group = create_group(state.clone(), &leader_access, 2).await;
    let group_id = group["id"].as_str().unwrap().to_string();

    let mut request_ids = Vec::new();
    for access in [&a_access, &b_access] {
        let (status, json) = send(
            state.clone(),
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/groups/{group_id}/join-requests"))
                .header("authorization", format!("Bearer {access}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{json}");
        request_ids.push(json["id"].as_str().unwrap().to_string());
    }

    let approve = |request_id: String| {
        Request::builder()
            .method("POST")
            .uri(format!(
                "/api/v1/groups/{group_id}/join-requests/{request_id}/approve"
            ))
            .header("authorization", format!("Bearer {leader_access}"))
            .body(Body::empty())
            .unwrap()
    };

    let (first, second) = tokio::join!(
        send(state.clone(), approve(request_ids[0].clone())),
        send(state.clone(), approve(request_ids[1].clone()))
    );
    let outcomes = [first, second];
    let wins = outcomes
        .iter()
        .filter(|(status, _)| *status == StatusCode::OK)
        .count();
    let full = outcomes
        .iter()
        .filter(|(_, json)| json["error"]["code"] == "group_full")
        .count();
    assert_eq!(wins, 1, "{outcomes:?}");
    assert_eq!(full, 1, "{outcomes:?}");

    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM group_members WHERE group_id = $1 AND status = 'active'",
    )
    .bind(Uuid::parse_str(&group_id).unwrap())
    .fetch_one(&state.db)
    .await
    .unwrap();
    assert_eq!(count, 2);
}

#[tokio::test]
async fn duplicate_join_request_race_keeps_one_pending() {
    let state = test_state().await;
    let (leader_access, _) = register_user(state.clone()).await;
    let (joiner_access, joiner_id) = register_user(state.clone()).await;
    let group = create_group(state.clone(), &leader_access, 5).await;
    let group_id = group["id"].as_str().unwrap().to_string();

    let req = || {
        Request::builder()
            .method("POST")
            .uri(format!("/api/v1/groups/{group_id}/join-requests"))
            .header("authorization", format!("Bearer {joiner_access}"))
            .body(Body::empty())
            .unwrap()
    };
    let (first, second) = tokio::join!(send(state.clone(), req()), send(state.clone(), req()));
    let wins = [&first, &second]
        .iter()
        .filter(|(status, _)| *status == StatusCode::CREATED)
        .count();
    let pending = [&first, &second]
        .iter()
        .filter(|(_, json)| json["error"]["code"] == "join_request_pending")
        .count();
    assert_eq!(wins, 1, "{first:?} {second:?}");
    assert_eq!(pending, 1, "{first:?} {second:?}");

    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM group_join_requests
         WHERE group_id = $1 AND requester_id = $2 AND status = 'pending'",
    )
    .bind(Uuid::parse_str(&group_id).unwrap())
    .bind(joiner_id)
    .fetch_one(&state.db)
    .await
    .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn full_group_rejects_new_join_request() {
    let state = test_state().await;
    let (leader_access, _) = register_user(state.clone()).await;
    let (member_access, _) = register_user(state.clone()).await;
    let (late_access, _) = register_user(state.clone()).await;
    let group = create_group(state.clone(), &leader_access, 2).await;
    let group_id = group["id"].as_str().unwrap();

    let (status, req) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri(format!("/api/v1/groups/{group_id}/join-requests"))
            .header("authorization", format!("Bearer {member_access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{req}");
    let request_id = req["id"].as_str().unwrap();
    let (status, json) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri(format!(
                "/api/v1/groups/{group_id}/join-requests/{request_id}/approve"
            ))
            .header("authorization", format!("Bearer {leader_access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");

    let (status, json) = send(
        state,
        Request::builder()
            .method("POST")
            .uri(format!("/api/v1/groups/{group_id}/join-requests"))
            .header("authorization", format!("Bearer {late_access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(json["error"]["code"], "group_full");
}
