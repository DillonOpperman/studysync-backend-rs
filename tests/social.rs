mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{register_user, send, test_state};
async fn create_group(state: studysync_backend::AppState, access: &str) -> String {
    let (status, json) = send(
        state,
        Request::builder()
            .method("POST")
            .uri("/api/v1/groups")
            .header("authorization", format!("Bearer {access}"))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "title": "Social group",
                    "subject": "CS",
                    "maxMembers": 8,
                    "recurringSlots": [{
                        "dayOfWeek": 1,
                        "startTime": "10:00",
                        "endTime": "11:00",
                        "timezone": "America/Chicago"
                    }]
                })
                .to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{json}");
    json["id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn friend_request_lifecycle_and_notifications() {
    let state = test_state().await;
    let (a_access, a_id) = register_user(state.clone()).await;
    let (b_access, b_id) = register_user(state.clone()).await;

    let (status, json) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/v1/friends/requests")
            .header("authorization", format!("Bearer {a_access}"))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({ "userId": a_id }).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{json}");

    let (status, created) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/v1/friends/requests")
            .header("authorization", format!("Bearer {a_access}"))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({ "userId": b_id }).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let request_id = created["id"].as_str().unwrap();

    let (status, dup) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/v1/friends/requests")
            .header("authorization", format!("Bearer {a_access}"))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({ "userId": b_id }).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(dup["error"]["code"], "friend_request_pending");

    let (status, reverse) = send(
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
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(reverse["error"]["code"], "friend_request_pending");

    let (status, notes) = send(
        state.clone(),
        Request::builder()
            .method("GET")
            .uri("/api/v1/notifications")
            .header("authorization", format!("Bearer {b_access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(notes[0]["type"], "friend_request");

    let (status, json) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri(format!("/api/v1/friends/requests/{request_id}/reject"))
            .header("authorization", format!("Bearer {b_access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");

    let (status, notes) = send(
        state.clone(),
        Request::builder()
            .method("GET")
            .uri("/api/v1/notifications")
            .header("authorization", format!("Bearer {a_access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(notes[0]["type"], "friend_request_rejected");

    let (status, again) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/v1/friends/requests")
            .header("authorization", format!("Bearer {a_access}"))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({ "userId": b_id }).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{again}");
    let request_id = again["id"].as_str().unwrap();

    let (status, json) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri(format!("/api/v1/friends/requests/{request_id}/accept"))
            .header("authorization", format!("Bearer {a_access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{json}");

    let (status, json) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri(format!("/api/v1/friends/requests/{request_id}/accept"))
            .header("authorization", format!("Bearer {b_access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["status"], "accepted");

    let pair: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM friendships WHERE user_low = LEAST($1::uuid, $2::uuid)
         AND user_high = GREATEST($1::uuid, $2::uuid)",
    )
    .bind(a_id)
    .bind(b_id)
    .fetch_one(&state.db)
    .await
    .unwrap();
    assert_eq!(pair, 1);

    let (status, friends) = send(
        state.clone(),
        Request::builder()
            .method("GET")
            .uri("/api/v1/friends")
            .header("authorization", format!("Bearer {a_access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(friends[0]["user"]["id"], b_id.to_string());

    let (status, friends_b) = send(
        state.clone(),
        Request::builder()
            .method("GET")
            .uri("/api/v1/friends")
            .header("authorization", format!("Bearer {b_access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(friends_b[0]["user"]["id"], a_id.to_string());

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
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(json["error"]["code"], "already_friends");

    let (status, dm) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/v1/direct-messages")
            .header("authorization", format!("Bearer {a_access}"))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({ "recipientId": b_id, "content": "hello friend" }).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{dm}");
    assert_eq!(dm["outcome"], "directMessage");
    assert_eq!(dm["message"]["content"], "hello friend");

    let (status, thread) = send(
        state.clone(),
        Request::builder()
            .method("GET")
            .uri(format!("/api/v1/direct-messages/{a_id}"))
            .header("authorization", format!("Bearer {b_access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(thread[0]["content"], "hello friend");
    assert!(thread[0]["readAt"].as_str().is_some());

    let (status, notes) = send(
        state,
        Request::builder()
            .method("GET")
            .uri("/api/v1/notifications")
            .header("authorization", format!("Bearer {b_access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(notes[0]["type"], "direct_message");
}

#[tokio::test]
async fn blocking_prevents_friends_and_messages() {
    let state = test_state().await;
    let (a_access, a_id) = register_user(state.clone()).await;
    let (b_access, b_id) = register_user(state.clone()).await;

    let (status, _) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/v1/blocks")
            .header("authorization", format!("Bearer {b_access}"))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({ "userId": a_id }).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, json) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/v1/friends/requests")
            .header("authorization", format!("Bearer {a_access}"))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({ "userId": b_id }).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(json["error"]["code"], "user_blocked");

    let (status, json) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/v1/direct-messages")
            .header("authorization", format!("Bearer {a_access}"))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({ "recipientId": b_id, "content": "hi" }).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(json["error"]["code"], "user_blocked");

    let (status, json) = send(
        state,
        Request::builder()
            .method("GET")
            .uri(format!("/api/v1/direct-messages/{b_id}"))
            .header("authorization", format!("Bearer {a_access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(json["error"]["code"], "user_blocked");
}

#[tokio::test]
async fn shared_group_and_message_request_authorization() {
    let state = test_state().await;
    let (leader_access, leader_id) = register_user(state.clone()).await;
    let (member_access, member_id) = register_user(state.clone()).await;
    let (stranger_access, stranger_id) = register_user(state.clone()).await;
    let group_id = create_group(state.clone(), &leader_access).await;

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
    let join_id = req["id"].as_str().unwrap();
    let (status, json) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri(format!(
                "/api/v1/groups/{group_id}/join-requests/{join_id}/approve"
            ))
            .header("authorization", format!("Bearer {leader_access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");

    let (status, dm) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/v1/direct-messages")
            .header("authorization", format!("Bearer {member_access}"))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({ "recipientId": leader_id, "content": "group chat later" })
                    .to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{dm}");
    assert_eq!(dm["outcome"], "directMessage");

    let (status, json) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/v1/direct-messages")
            .header("authorization", format!("Bearer {stranger_access}"))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({ "recipientId": member_id, "content": "hello" }).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(json["error"]["code"], "cannot_message");

    let (status, created) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/v1/direct-messages")
            .header("authorization", format!("Bearer {stranger_access}"))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({ "recipientId": leader_id, "content": "may I message?" })
                    .to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{created}");
    assert_eq!(created["outcome"], "messageRequest");
    let request_id = created["messageRequest"]["id"].as_str().unwrap();

    let (status, dup) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/v1/direct-messages")
            .header("authorization", format!("Bearer {stranger_access}"))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({ "recipientId": leader_id, "content": "again" }).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{dup}");
    assert_eq!(dup["messageRequest"]["id"], request_id);

    let (status, listed) = send(
        state.clone(),
        Request::builder()
            .method("GET")
            .uri("/api/v1/message-requests")
            .header("authorization", format!("Bearer {leader_access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed[0]["id"], request_id);

    let (status, json) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri(format!("/api/v1/message-requests/{request_id}/reject"))
            .header("authorization", format!("Bearer {leader_access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");

    let (status, again) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/v1/direct-messages")
            .header("authorization", format!("Bearer {stranger_access}"))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({ "recipientId": leader_id, "content": "retry" }).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{again}");
    let request_id = again["messageRequest"]["id"].as_str().unwrap();

    let (status, json) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri(format!("/api/v1/message-requests/{request_id}/accept"))
            .header("authorization", format!("Bearer {leader_access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");

    let (status, notes) = send(
        state.clone(),
        Request::builder()
            .method("GET")
            .uri("/api/v1/notifications")
            .header("authorization", format!("Bearer {stranger_access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(notes[0]["type"], "message_request_accepted");

    let (status, dm) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/v1/direct-messages")
            .header("authorization", format!("Bearer {stranger_access}"))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({ "recipientId": leader_id, "content": "now friends-ish" })
                    .to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{dm}");

    let (status, reply) = send(
        state,
        Request::builder()
            .method("POST")
            .uri("/api/v1/direct-messages")
            .header("authorization", format!("Bearer {leader_access}"))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({ "recipientId": stranger_id, "content": "welcome" }).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{reply}");
}

#[tokio::test]
async fn deleted_user_cannot_be_friended_or_messaged() {
    let state = test_state().await;
    let (a_access, _) = register_user(state.clone()).await;
    let (_, b_id) = register_user(state.clone()).await;
    sqlx::query("UPDATE users SET deleted_at = now() WHERE id = $1")
        .bind(b_id)
        .execute(&state.db)
        .await
        .unwrap();

    let (status, json) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/v1/friends/requests")
            .header("authorization", format!("Bearer {a_access}"))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({ "userId": b_id }).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(json["error"]["code"], "user_not_found");

    let (status, json) = send(
        state,
        Request::builder()
            .method("POST")
            .uri("/api/v1/direct-messages")
            .header("authorization", format!("Bearer {a_access}"))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({ "recipientId": b_id, "content": "gone" }).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{json}");
    assert_eq!(json["error"]["code"], "user_not_found");
}

#[tokio::test]
async fn dm_pagination_and_read_state() {
    let state = test_state().await;
    let (a_access, a_id) = register_user(state.clone()).await;
    let (b_access, b_id) = register_user(state.clone()).await;
    let (status, created) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/v1/friends/requests")
            .header("authorization", format!("Bearer {a_access}"))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({ "userId": b_id }).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let request_id = created["id"].as_str().unwrap();
    let (status, _) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri(format!("/api/v1/friends/requests/{request_id}/accept"))
            .header("authorization", format!("Bearer {b_access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let mut ids = Vec::new();
    for i in 0..5 {
        let (status, json) = send(
            state.clone(),
            Request::builder()
                .method("POST")
                .uri("/api/v1/direct-messages")
                .header("authorization", format!("Bearer {a_access}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "recipientId": b_id, "content": format!("m{i}") })
                        .to_string(),
                ))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{json}");
        ids.push(json["message"]["id"].as_str().unwrap().to_string());
    }

    let (status, page) = send(
        state.clone(),
        Request::builder()
            .method("GET")
            .uri(format!("/api/v1/direct-messages/{a_id}?limit=2"))
            .header("authorization", format!("Bearer {b_access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page.as_array().unwrap().len(), 2);
    assert_eq!(page[0]["content"], "m3");
    assert_eq!(page[1]["content"], "m4");
    assert!(page[0]["readAt"].as_str().is_some());

    let before = page[0]["id"].as_str().unwrap();
    let (status, older) = send(
        state.clone(),
        Request::builder()
            .method("GET")
            .uri(format!(
                "/api/v1/direct-messages/{a_id}?limit=2&before={before}"
            ))
            .header("authorization", format!("Bearer {b_access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(older.as_array().unwrap().len(), 2);
    assert_eq!(older[0]["content"], "m1");
    assert_eq!(older[1]["content"], "m2");

    let (status, sender_view) = send(
        state,
        Request::builder()
            .method("GET")
            .uri(format!("/api/v1/direct-messages/{b_id}?limit=5"))
            .header("authorization", format!("Bearer {a_access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(sender_view.as_array().unwrap().len(), 5);
    assert!(sender_view[0]["readAt"].as_str().is_some());
}

#[tokio::test]
async fn concurrent_friend_and_message_requests_stay_unique() {
    let state = test_state().await;
    let (a_access, a_id) = register_user(state.clone()).await;
    let (b_access, b_id) = register_user(state.clone()).await;
    let _group = create_group(state.clone(), &b_access).await;

    let friend_body = serde_json::json!({ "userId": b_id }).to_string();
    let req = || {
        Request::builder()
            .method("POST")
            .uri("/api/v1/friends/requests")
            .header("authorization", format!("Bearer {a_access}"))
            .header("content-type", "application/json")
            .body(Body::from(friend_body.clone()))
            .unwrap()
    };
    let (first, second) = tokio::join!(send(state.clone(), req()), send(state.clone(), req()));
    let wins = [&first, &second]
        .iter()
        .filter(|(status, _)| *status == StatusCode::CREATED)
        .count();
    let pending = [&first, &second]
        .iter()
        .filter(|(_, json)| json["error"]["code"] == "friend_request_pending")
        .count();
    assert_eq!(wins, 1, "{first:?} {second:?}");
    assert_eq!(pending, 1, "{first:?} {second:?}");
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM friend_requests
         WHERE status = 'pending'
           AND LEAST(requester_id, recipient_id) = LEAST($1::uuid, $2::uuid)
           AND GREATEST(requester_id, recipient_id) = GREATEST($1::uuid, $2::uuid)",
    )
    .bind(a_id)
    .bind(b_id)
    .fetch_one(&state.db)
    .await
    .unwrap();
    assert_eq!(count, 1);

    let (c_access, c_id) = register_user(state.clone()).await;
    let dm_body = serde_json::json!({ "recipientId": b_id, "content": "hi" }).to_string();
    let dm = || {
        Request::builder()
            .method("POST")
            .uri("/api/v1/direct-messages")
            .header("authorization", format!("Bearer {c_access}"))
            .header("content-type", "application/json")
            .body(Body::from(dm_body.clone()))
            .unwrap()
    };
    let (d1, d2) = tokio::join!(send(state.clone(), dm()), send(state.clone(), dm()));
    let created = [&d1, &d2]
        .iter()
        .filter(|(status, json)| {
            *status == StatusCode::ACCEPTED
                || (*status == StatusCode::OK && json["outcome"] == "messageRequest")
        })
        .count();
    assert_eq!(created, 2, "{d1:?} {d2:?}");
    let pending_msg: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM message_requests
         WHERE sender_id = $1 AND receiver_id = $2 AND status = 'pending'",
    )
    .bind(c_id)
    .bind(b_id)
    .fetch_one(&state.db)
    .await
    .unwrap();
    assert_eq!(pending_msg, 1);
}
