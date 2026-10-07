mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{register_user, send, test_state};
use uuid::Uuid;

fn group_body() -> serde_json::Value {
    serde_json::json!({
        "title": "Chat group",
        "subject": "CS",
        "maxMembers": 8,
        "recurringSlots": [{
            "dayOfWeek": 1,
            "startTime": "10:00",
            "endTime": "11:00",
            "timezone": "America/Chicago"
        }]
    })
}

async fn create_group(state: studysync_backend::AppState, access: &str) -> String {
    let (status, json) = send(
        state,
        Request::builder()
            .method("POST")
            .uri("/api/v1/groups")
            .header("authorization", format!("Bearer {access}"))
            .header("content-type", "application/json")
            .body(Body::from(group_body().to_string()))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{json}");
    json["id"].as_str().unwrap().to_string()
}

async fn add_member(
    state: studysync_backend::AppState,
    leader_access: &str,
    member_access: &str,
    group_id: &str,
) {
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
    let (status, json) = send(
        state,
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
}

async fn post_message(
    state: studysync_backend::AppState,
    access: &str,
    group_id: &str,
    client_msg_id: Uuid,
    content: &str,
    message_type: Option<&str>,
) -> (StatusCode, serde_json::Value) {
    let mut body = serde_json::json!({
        "clientMsgId": client_msg_id,
        "content": content,
    });
    if let Some(message_type) = message_type {
        body["messageType"] = serde_json::Value::String(message_type.to_string());
    }
    send(
        state,
        Request::builder()
            .method("POST")
            .uri(format!("/api/v1/groups/{group_id}/messages"))
            .header("authorization", format!("Bearer {access}"))
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap(),
    )
    .await
}

#[tokio::test]
async fn member_can_send_and_list_history() {
    let state = test_state().await;
    let (leader, _) = register_user(state.clone()).await;
    let (member, member_id) = register_user(state.clone()).await;
    let group_id = create_group(state.clone(), &leader).await;
    add_member(state.clone(), &leader, &member, &group_id).await;

    let (status, json) = post_message(
        state.clone(),
        &member,
        &group_id,
        Uuid::now_v7(),
        "hello group",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{json}");
    assert_eq!(json["content"], "hello group");
    assert_eq!(json["messageType"], "text");
    assert_eq!(json["senderId"], member_id.to_string());
    assert!(json["deletedAt"].is_null());

    let (status, page) = send(
        state,
        Request::builder()
            .method("GET")
            .uri(format!("/api/v1/groups/{group_id}/messages?limit=50"))
            .header("authorization", format!("Bearer {member}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page["messages"].as_array().unwrap().len(), 1);
    assert_eq!(page["messages"][0]["content"], "hello group");
    assert_eq!(page["pagination"]["limit"], 50);
}

#[tokio::test]
async fn pagination_before_after_and_recovery() {
    let state = test_state().await;
    let (leader, _) = register_user(state.clone()).await;
    let group_id = create_group(state.clone(), &leader).await;

    let mut ids = Vec::new();
    for i in 0..5 {
        let (status, json) = post_message(
            state.clone(),
            &leader,
            &group_id,
            Uuid::now_v7(),
            &format!("m{i}"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{json}");
        ids.push(json["id"].as_str().unwrap().to_string());
    }

    let (status, page) = send(
        state.clone(),
        Request::builder()
            .method("GET")
            .uri(format!("/api/v1/groups/{group_id}/messages?limit=2"))
            .header("authorization", format!("Bearer {leader}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    let newest_page = page["messages"].as_array().unwrap();
    assert_eq!(newest_page.len(), 2);
    assert_eq!(newest_page[0]["content"], "m3");
    assert_eq!(newest_page[1]["content"], "m4");
    assert_eq!(page["pagination"]["hasMore"], true);

    let before = newest_page[0]["id"].as_str().unwrap();
    let (status, older) = send(
        state.clone(),
        Request::builder()
            .method("GET")
            .uri(format!(
                "/api/v1/groups/{group_id}/messages?before={before}&limit=10"
            ))
            .header("authorization", format!("Bearer {leader}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{older}");
    assert_eq!(older["messages"][0]["content"], "m0");
    assert_eq!(older["messages"][2]["content"], "m2");

    let last_seen = &ids[1];
    let (status, missed) = send(
        state,
        Request::builder()
            .method("GET")
            .uri(format!(
                "/api/v1/groups/{group_id}/messages?after={last_seen}&limit=10"
            ))
            .header("authorization", format!("Bearer {leader}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{missed}");
    let recovered: Vec<_> = missed["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["content"].as_str().unwrap())
        .collect();
    assert_eq!(recovered, vec!["m2", "m3", "m4"]);
}

#[tokio::test]
async fn idempotent_and_concurrent_client_msg_id() {
    let state = test_state().await;
    let (leader, _) = register_user(state.clone()).await;
    let group_id = create_group(state.clone(), &leader).await;
    let client_msg_id = Uuid::now_v7();

    let (status, first) = post_message(
        state.clone(),
        &leader,
        &group_id,
        client_msg_id,
        "once",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, second) = post_message(
        state.clone(),
        &leader,
        &group_id,
        client_msg_id,
        "once",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(first["id"], second["id"]);

    let concurrent_id = Uuid::now_v7();
    let a = post_message(
        state.clone(),
        &leader,
        &group_id,
        concurrent_id,
        "race",
        None,
    );
    let b = post_message(
        state.clone(),
        &leader,
        &group_id,
        concurrent_id,
        "race",
        None,
    );
    let (left, right) = tokio::join!(a, b);
    assert_eq!(left.0, StatusCode::CREATED, "{}", left.1);
    assert_eq!(right.0, StatusCode::CREATED, "{}", right.1);
    assert_eq!(left.1["id"], right.1["id"]);

    let (status, page) = send(
        state,
        Request::builder()
            .method("GET")
            .uri(format!("/api/v1/groups/{group_id}/messages?limit=50"))
            .header("authorization", format!("Bearer {leader}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["messages"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn edit_rules() {
    let state = test_state().await;
    let (leader, _) = register_user(state.clone()).await;
    let (member, _) = register_user(state.clone()).await;
    let group_id = create_group(state.clone(), &leader).await;
    add_member(state.clone(), &leader, &member, &group_id).await;

    let (status, created) = post_message(
        state.clone(),
        &member,
        &group_id,
        Uuid::now_v7(),
        "draft",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let message_id = created["id"].as_str().unwrap();

    let (status, edited) = send(
        state.clone(),
        Request::builder()
            .method("PATCH")
            .uri(format!("/api/v1/messages/{message_id}"))
            .header("authorization", format!("Bearer {member}"))
            .header("content-type", "application/json")
            .body(Body::from(r#"{"content":"edited"}"#))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{edited}");
    assert_eq!(edited["content"], "edited");
    assert!(edited["editedAt"].is_string());

    let (status, json) = send(
        state.clone(),
        Request::builder()
            .method("PATCH")
            .uri(format!("/api/v1/messages/{message_id}"))
            .header("authorization", format!("Bearer {leader}"))
            .header("content-type", "application/json")
            .body(Body::from(r#"{"content":"hijack"}"#))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{json}");
    assert_eq!(json["error"]["code"], "cannot_modify_message");

    sqlx::query("UPDATE messages SET created_at = now() - interval '20 minutes' WHERE id = $1")
        .bind(Uuid::parse_str(message_id).unwrap())
        .execute(&state.db)
        .await
        .unwrap();
    let (status, json) = send(
        state.clone(),
        Request::builder()
            .method("PATCH")
            .uri(format!("/api/v1/messages/{message_id}"))
            .header("authorization", format!("Bearer {member}"))
            .header("content-type", "application/json")
            .body(Body::from(r#"{"content":"too late"}"#))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{json}");
    assert_eq!(json["error"]["code"], "edit_window_expired");

    let (status, _) = send(
        state.clone(),
        Request::builder()
            .method("DELETE")
            .uri(format!("/api/v1/messages/{message_id}"))
            .header("authorization", format!("Bearer {member}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, json) = send(
        state,
        Request::builder()
            .method("PATCH")
            .uri(format!("/api/v1/messages/{message_id}"))
            .header("authorization", format!("Bearer {member}"))
            .header("content-type", "application/json")
            .body(Body::from(r#"{"content":"nope"}"#))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{json}");
    assert_eq!(json["error"]["code"], "message_deleted");
}

#[tokio::test]
async fn delete_sender_leader_and_unauthorized() {
    let state = test_state().await;
    let (leader, leader_id) = register_user(state.clone()).await;
    let (member, _) = register_user(state.clone()).await;
    let (other, _) = register_user(state.clone()).await;
    let group_id = create_group(state.clone(), &leader).await;
    add_member(state.clone(), &leader, &member, &group_id).await;
    add_member(state.clone(), &leader, &other, &group_id).await;

    let (status, msg) = post_message(
        state.clone(),
        &member,
        &group_id,
        Uuid::now_v7(),
        "keep me",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let member_msg = msg["id"].as_str().unwrap().to_string();

    let (status, json) = send(
        state.clone(),
        Request::builder()
            .method("DELETE")
            .uri(format!("/api/v1/messages/{member_msg}"))
            .header("authorization", format!("Bearer {other}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{json}");
    assert_eq!(json["error"]["code"], "cannot_modify_message");

    let (status, tombstone) = send(
        state.clone(),
        Request::builder()
            .method("DELETE")
            .uri(format!("/api/v1/messages/{member_msg}"))
            .header("authorization", format!("Bearer {leader}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{tombstone}");
    assert_eq!(tombstone["content"], "");
    assert!(tombstone["deletedAt"].is_string());

    let (status, again) = send(
        state.clone(),
        Request::builder()
            .method("DELETE")
            .uri(format!("/api/v1/messages/{member_msg}"))
            .header("authorization", format!("Bearer {leader}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(again["id"], member_msg);

    let (status, own) = post_message(
        state.clone(),
        &other,
        &group_id,
        Uuid::now_v7(),
        "mine",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let own_id = own["id"].as_str().unwrap();
    let (status, json) = send(
        state.clone(),
        Request::builder()
            .method("DELETE")
            .uri(format!("/api/v1/messages/{own_id}"))
            .header("authorization", format!("Bearer {other}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");

    sqlx::query("UPDATE group_members SET role = 'member' WHERE group_id = $1 AND user_id = $2")
        .bind(Uuid::parse_str(&group_id).unwrap())
        .bind(leader_id)
        .execute(&state.db)
        .await
        .unwrap();
    let other_id = Uuid::parse_str(own["senderId"].as_str().unwrap()).unwrap();
    sqlx::query("UPDATE study_groups SET leader_id = $2 WHERE id = $1")
        .bind(Uuid::parse_str(&group_id).unwrap())
        .bind(other_id)
        .execute(&state.db)
        .await
        .unwrap();
    sqlx::query("UPDATE group_members SET role = 'leader' WHERE group_id = $1 AND user_id = $2")
        .bind(Uuid::parse_str(&group_id).unwrap())
        .bind(other_id)
        .execute(&state.db)
        .await
        .unwrap();

    let (status, leftover) = post_message(
        state.clone(),
        &member,
        &group_id,
        Uuid::now_v7(),
        "still here",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let leftover_id = leftover["id"].as_str().unwrap();
    let (status, json) = send(
        state,
        Request::builder()
            .method("DELETE")
            .uri(format!("/api/v1/messages/{leftover_id}"))
            .header("authorization", format!("Bearer {leader}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{json}");
    assert_eq!(json["error"]["code"], "cannot_modify_message");
}

#[tokio::test]
async fn reactions_toggle_and_deleted_and_removed() {
    let state = test_state().await;
    let (leader, _) = register_user(state.clone()).await;
    let (member, member_id) = register_user(state.clone()).await;
    let group_id = create_group(state.clone(), &leader).await;
    add_member(state.clone(), &leader, &member, &group_id).await;

    let (status, msg) = post_message(
        state.clone(),
        &leader,
        &group_id,
        Uuid::now_v7(),
        "react",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let message_id = msg["id"].as_str().unwrap();

    let (status, added) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri(format!("/api/v1/messages/{message_id}/reactions"))
            .header("authorization", format!("Bearer {member}"))
            .header("content-type", "application/json")
            .body(Body::from(r#"{"emoji":"👍"}"#))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{added}");
    assert_eq!(added["reactions"][0]["emoji"], "👍");
    assert_eq!(added["reactions"][0]["count"], 1);
    assert_eq!(added["reactions"][0]["me"], true);

    let (status, removed) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri(format!("/api/v1/messages/{message_id}/reactions"))
            .header("authorization", format!("Bearer {member}"))
            .header("content-type", "application/json")
            .body(Body::from(r#"{"emoji":"👍"}"#))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{removed}");
    assert!(removed["reactions"].as_array().unwrap().is_empty());

    let emoji = "🎉";
    let a = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri(format!("/api/v1/messages/{message_id}/reactions"))
            .header("authorization", format!("Bearer {member}"))
            .header("content-type", "application/json")
            .body(Body::from(format!(r#"{{"emoji":"{emoji}"}}"#)))
            .unwrap(),
    );
    let b = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri(format!("/api/v1/messages/{message_id}/reactions"))
            .header("authorization", format!("Bearer {member}"))
            .header("content-type", "application/json")
            .body(Body::from(format!(r#"{{"emoji":"{emoji}"}}"#)))
            .unwrap(),
    );
    let (left, right) = tokio::join!(a, b);
    assert_eq!(left.0, StatusCode::OK);
    assert_eq!(right.0, StatusCode::OK);

    let (status, _) = send(
        state.clone(),
        Request::builder()
            .method("DELETE")
            .uri(format!("/api/v1/messages/{message_id}"))
            .header("authorization", format!("Bearer {leader}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, json) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri(format!("/api/v1/messages/{message_id}/reactions"))
            .header("authorization", format!("Bearer {member}"))
            .header("content-type", "application/json")
            .body(Body::from(r#"{"emoji":"👍"}"#))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{json}");
    assert_eq!(json["error"]["code"], "message_deleted");

    let (status, live) = post_message(
        state.clone(),
        &leader,
        &group_id,
        Uuid::now_v7(),
        "after",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let live_id = live["id"].as_str().unwrap();

    let (status, _) = send(
        state.clone(),
        Request::builder()
            .method("DELETE")
            .uri(format!("/api/v1/groups/{group_id}/members/{member_id}"))
            .header("authorization", format!("Bearer {leader}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, json) = send(
        state,
        Request::builder()
            .method("POST")
            .uri(format!("/api/v1/messages/{live_id}/reactions"))
            .header("authorization", format!("Bearer {member}"))
            .header("content-type", "application/json")
            .body(Body::from(r#"{"emoji":"👍"}"#))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{json}");
    assert_eq!(json["error"]["code"], "not_group_member");
}

#[tokio::test]
async fn announcements_leader_only_and_notify() {
    let state = test_state().await;
    let (leader, _) = register_user(state.clone()).await;
    let (member, _) = register_user(state.clone()).await;
    let group_id = create_group(state.clone(), &leader).await;
    add_member(state.clone(), &leader, &member, &group_id).await;

    let (status, json) = post_message(
        state.clone(),
        &member,
        &group_id,
        Uuid::now_v7(),
        "not allowed",
        Some("announcement"),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{json}");
    assert_eq!(json["error"]["code"], "not_group_leader");

    let (status, json) = post_message(
        state.clone(),
        &leader,
        &group_id,
        Uuid::now_v7(),
        "exam moved",
        Some("announcement"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{json}");
    assert_eq!(json["messageType"], "announcement");

    let (status, notes) = send(
        state,
        Request::builder()
            .method("GET")
            .uri("/api/v1/notifications")
            .header("authorization", format!("Bearer {member}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(notes
        .as_array()
        .unwrap()
        .iter()
        .any(|n| n["type"] == "group_announcement"));
}

#[tokio::test]
async fn authorization_non_member_archived_and_empty_content() {
    let state = test_state().await;
    let (leader, _) = register_user(state.clone()).await;
    let (member, _) = register_user(state.clone()).await;
    let (stranger, _) = register_user(state.clone()).await;
    let group_id = create_group(state.clone(), &leader).await;
    add_member(state.clone(), &leader, &member, &group_id).await;

    let (status, json) = post_message(
        state.clone(),
        &stranger,
        &group_id,
        Uuid::now_v7(),
        "nope",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{json}");
    assert_eq!(json["error"]["code"], "not_group_member");

    let (status, json) = post_message(
        state.clone(),
        &leader,
        &group_id,
        Uuid::now_v7(),
        "   ",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{json}");

    let (status, _) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri(format!("/api/v1/groups/{group_id}/leave"))
            .header("authorization", format!("Bearer {member}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, json) = post_message(
        state.clone(),
        &member,
        &group_id,
        Uuid::now_v7(),
        "archived member",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{json}");
    assert_eq!(json["error"]["code"], "not_group_member");

    let (status, page) = send(
        state,
        Request::builder()
            .method("GET")
            .uri(format!("/api/v1/groups/{group_id}/messages"))
            .header("authorization", format!("Bearer {member}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{page}");
}

#[tokio::test]
async fn blocked_user_cannot_react_to_sender() {
    let state = test_state().await;
    let (leader, leader_id) = register_user(state.clone()).await;
    let (member, _) = register_user(state.clone()).await;
    let group_id = create_group(state.clone(), &leader).await;
    add_member(state.clone(), &leader, &member, &group_id).await;

    let (status, _) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/v1/blocks")
            .header("authorization", format!("Bearer {member}"))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({ "userId": leader_id }).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, msg) = post_message(
        state.clone(),
        &leader,
        &group_id,
        Uuid::now_v7(),
        "still posted",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{msg}");
    let message_id = msg["id"].as_str().unwrap();

    let (status, json) = send(
        state,
        Request::builder()
            .method("POST")
            .uri(format!("/api/v1/messages/{message_id}/reactions"))
            .header("authorization", format!("Bearer {member}"))
            .header("content-type", "application/json")
            .body(Body::from(r#"{"emoji":"👍"}"#))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{json}");
    assert_eq!(json["error"]["code"], "user_blocked");
}
