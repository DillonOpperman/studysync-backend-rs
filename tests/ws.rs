mod common;

use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{register_user, send, spawn_app, test_state};
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

fn group_body() -> serde_json::Value {
    serde_json::json!({
        "title": "WS group",
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

type WsStream = tokio_tungstenite::WebSocketStream<
    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
>;

async fn connect(port: u16, token: &str) -> WsStream {
    let url = format!("ws://127.0.0.1:{port}/api/v1/ws?access_token={token}");
    let (stream, _) = tokio_tungstenite::connect_async(&url)
        .await
        .expect("ws connect");
    stream
}

async fn recv_json(ws: &mut WsStream) -> serde_json::Value {
    let timeout = Duration::from_secs(3);
    loop {
        let next = tokio::time::timeout(timeout, ws.next())
            .await
            .expect("ws recv timeout")
            .expect("ws closed")
            .expect("ws error");
        match next {
            Message::Text(text) => {
                return serde_json::from_str(&text).expect("json");
            }
            Message::Ping(_) | Message::Pong(_) => continue,
            other => panic!("unexpected ws frame {other:?}"),
        }
    }
}

async fn send_json(ws: &mut WsStream, value: serde_json::Value) {
    ws.send(Message::Text(value.to_string().into()))
        .await
        .expect("ws send");
}

#[tokio::test]
async fn websocket_rejects_invalid_and_expired_tokens() {
    let state = test_state().await;
    let port = spawn_app(state.clone()).await;

    let url = format!("ws://127.0.0.1:{port}/api/v1/ws?access_token=not-a-jwt");
    let result = tokio_tungstenite::connect_async(&url).await;
    assert!(result.is_err());

    let (access, user_id) = register_user(state.clone()).await;
    let _ = access;
    let expired = studysync_backend::auth::jwt::encode_access_token_with_exp(
        user_id,
        &state.config.jwt_secret,
        chrono::Utc::now().timestamp() - 60,
    )
    .unwrap();
    let url = format!("ws://127.0.0.1:{port}/api/v1/ws?access_token={expired}");
    let result = tokio_tungstenite::connect_async(&url).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn subscribe_send_broadcast_ping_and_unauthorized() {
    let state = test_state().await;
    let (leader, _) = register_user(state.clone()).await;
    let (member, _) = register_user(state.clone()).await;
    let (stranger, _) = register_user(state.clone()).await;
    let group_id = create_group(state.clone(), &leader).await;
    add_member(state.clone(), &leader, &member, &group_id).await;
    let port = spawn_app(state.clone()).await;

    let mut leader_ws = connect(port, &leader).await;
    let mut member_ws = connect(port, &member).await;
    let mut stranger_ws = connect(port, &stranger).await;

    send_json(
        &mut stranger_ws,
        serde_json::json!({ "op": "subscribe", "groupId": group_id }),
    )
    .await;
    let err = recv_json(&mut stranger_ws).await;
    assert_eq!(err["op"], "error");
    assert_eq!(err["code"], "not_group_member");

    send_json(
        &mut leader_ws,
        serde_json::json!({ "op": "subscribe", "groupId": group_id }),
    )
    .await;
    send_json(
        &mut member_ws,
        serde_json::json!({ "op": "subscribe", "groupId": group_id }),
    )
    .await;
    assert_eq!(recv_json(&mut leader_ws).await["op"], "subscribed");
    assert_eq!(recv_json(&mut member_ws).await["op"], "subscribed");

    send_json(
        &mut member_ws,
        serde_json::json!({
            "op": "message.send",
            "groupId": group_id,
            "clientMsgId": Uuid::now_v7(),
            "content": "from socket",
            "messageType": "text"
        }),
    )
    .await;
    let created_leader = recv_json(&mut leader_ws).await;
    let created_member = recv_json(&mut member_ws).await;
    assert_eq!(created_leader["op"], "message.created");
    assert_eq!(created_member["op"], "message.created");
    assert_eq!(created_leader["payload"]["content"], "from socket");
    let message_id = created_leader["payload"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    send_json(
        &mut leader_ws,
        serde_json::json!({
            "op": "message.edit",
            "messageId": message_id,
            "content": "nope"
        }),
    )
    .await;
    let edit_err = recv_json(&mut leader_ws).await;
    assert_eq!(edit_err["code"], "cannot_modify_message");

    send_json(
        &mut member_ws,
        serde_json::json!({
            "op": "message.edit",
            "messageId": message_id,
            "content": "edited live"
        }),
    )
    .await;
    assert_eq!(recv_json(&mut leader_ws).await["op"], "message.edited");
    assert_eq!(recv_json(&mut member_ws).await["op"], "message.edited");

    send_json(
        &mut member_ws,
        serde_json::json!({
            "op": "reaction.toggle",
            "messageId": message_id,
            "emoji": "👍"
        }),
    )
    .await;
    let reaction = recv_json(&mut leader_ws).await;
    assert_eq!(reaction["op"], "reaction.updated");
    assert_eq!(reaction["payload"]["reactions"][0]["count"], 1);
    let _ = recv_json(&mut member_ws).await;

    send_json(
        &mut leader_ws,
        serde_json::json!({ "op": "message.delete", "messageId": message_id }),
    )
    .await;
    assert_eq!(recv_json(&mut leader_ws).await["op"], "message.deleted");
    assert_eq!(recv_json(&mut member_ws).await["op"], "message.deleted");

    send_json(
        &mut member_ws,
        serde_json::json!({ "op": "ping", "ts": 42 }),
    )
    .await;
    let pong = recv_json(&mut member_ws).await;
    assert_eq!(pong["op"], "pong");
    assert_eq!(pong["ts"], 42);

    send_json(&mut member_ws, serde_json::json!({ "op": "nope" })).await;
    let unknown = recv_json(&mut member_ws).await;
    assert_eq!(unknown["op"], "error");
    assert_eq!(unknown["code"], "unknown_op");
}

#[tokio::test]
async fn multiple_groups_unsubscribe_not_subscribed_and_cleanup() {
    let state = test_state().await;
    let (leader, _) = register_user(state.clone()).await;
    let group_a = create_group(state.clone(), &leader).await;
    let group_b = create_group(state.clone(), &leader).await;
    let port = spawn_app(state.clone()).await;

    let mut ws = connect(port, &leader).await;
    send_json(
        &mut ws,
        serde_json::json!({ "op": "subscribe", "groupId": group_a }),
    )
    .await;
    send_json(
        &mut ws,
        serde_json::json!({ "op": "subscribe", "groupId": group_b }),
    )
    .await;
    assert_eq!(recv_json(&mut ws).await["groupId"], group_a);
    assert_eq!(recv_json(&mut ws).await["groupId"], group_b);

    send_json(
        &mut ws,
        serde_json::json!({ "op": "unsubscribe", "groupId": group_b }),
    )
    .await;
    send_json(
        &mut ws,
        serde_json::json!({
            "op": "message.send",
            "groupId": group_b,
            "clientMsgId": Uuid::now_v7(),
            "content": "should fail",
            "messageType": "text"
        }),
    )
    .await;
    let err = recv_json(&mut ws).await;
    assert_eq!(err["code"], "not_subscribed");

    send_json(
        &mut ws,
        serde_json::json!({
            "op": "message.send",
            "groupId": group_a,
            "clientMsgId": Uuid::now_v7(),
            "content": "still a",
            "messageType": "text"
        }),
    )
    .await;
    assert_eq!(recv_json(&mut ws).await["op"], "message.created");

    let before = state.chat.connection_count().await;
    assert!(before >= 1);
    ws.close(None).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(state.chat.connection_count().await < before);
}

#[tokio::test]
async fn rest_send_broadcasts_to_sockets_and_history_recovers_after_disconnect() {
    let state = test_state().await;
    let (leader, _) = register_user(state.clone()).await;
    let (member, _) = register_user(state.clone()).await;
    let group_id = create_group(state.clone(), &leader).await;
    add_member(state.clone(), &leader, &member, &group_id).await;
    let port = spawn_app(state.clone()).await;

    let mut ws = connect(port, &member).await;
    send_json(
        &mut ws,
        serde_json::json!({ "op": "subscribe", "groupId": group_id }),
    )
    .await;
    assert_eq!(recv_json(&mut ws).await["op"], "subscribed");

    let first_id = {
        let (status, json) = send(
            state.clone(),
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/groups/{group_id}/messages"))
                .header("authorization", format!("Bearer {leader}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "clientMsgId": Uuid::now_v7(),
                        "content": "persisted",
                        "messageType": "text"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{json}");
        json["id"].as_str().unwrap().to_string()
    };
    let event = recv_json(&mut ws).await;
    assert_eq!(event["op"], "message.created");
    assert_eq!(event["payload"]["id"], first_id);

    ws.close(None).await.unwrap();

    let (status, missed) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri(format!("/api/v1/groups/{group_id}/messages"))
            .header("authorization", format!("Bearer {leader}"))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "clientMsgId": Uuid::now_v7(),
                    "content": "while away",
                    "messageType": "text"
                })
                .to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{missed}");

    let mut ws2 = connect(port, &member).await;
    send_json(
        &mut ws2,
        serde_json::json!({ "op": "subscribe", "groupId": group_id }),
    )
    .await;
    assert_eq!(recv_json(&mut ws2).await["op"], "subscribed");

    let (status, page) = send(
        state,
        Request::builder()
            .method("GET")
            .uri(format!(
                "/api/v1/groups/{group_id}/messages?after={first_id}&limit=50"
            ))
            .header("authorization", format!("Bearer {member}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page["messages"].as_array().unwrap().len(), 1);
    assert_eq!(page["messages"][0]["content"], "while away");
}

#[tokio::test]
async fn removed_member_send_fails_on_live_socket() {
    let state = test_state().await;
    let (leader, _) = register_user(state.clone()).await;
    let (member, member_id) = register_user(state.clone()).await;
    let group_id = create_group(state.clone(), &leader).await;
    add_member(state.clone(), &leader, &member, &group_id).await;
    let port = spawn_app(state.clone()).await;

    let mut ws = connect(port, &member).await;
    send_json(
        &mut ws,
        serde_json::json!({ "op": "subscribe", "groupId": group_id }),
    )
    .await;
    assert_eq!(recv_json(&mut ws).await["op"], "subscribed");

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

    send_json(
        &mut ws,
        serde_json::json!({
            "op": "message.send",
            "groupId": group_id,
            "clientMsgId": Uuid::now_v7(),
            "content": "after kick",
            "messageType": "text"
        }),
    )
    .await;
    let err = recv_json(&mut ws).await;
    assert_eq!(err["code"], "not_group_member");
}
