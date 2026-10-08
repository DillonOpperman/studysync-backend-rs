mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{register_user, send, test_state};
use studysync_backend::media::Storage;
use uuid::Uuid;

const BOUNDARY: &str = "----StudysyncTestBoundary";

type MultipartField<'a> = (&'a str, Option<&'a str>, Option<&'a str>, &'a [u8]);

fn png_bytes() -> Vec<u8> {
    let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    bytes.extend_from_slice(&[0, 1, 2, 3, 4, 5, 6, 7]);
    bytes
}

fn pdf_bytes() -> Vec<u8> {
    b"%PDF-1.4\n1 0 obj<<>>endobj\ntrailer<<>>\n%%EOF".to_vec()
}

fn jpeg_bytes() -> Vec<u8> {
    let mut bytes = vec![0xFF, 0xD8, 0xFF, 0xE0];
    bytes.extend_from_slice(&[0, 1, 2, 3, 4, 5, 6, 7]);
    bytes
}

fn docx_bytes() -> Vec<u8> {
    use std::io::{Cursor, Write};
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    zip.start_file("[Content_Types].xml", options).unwrap();
    zip.write_all(b"<Types></Types>").unwrap();
    zip.start_file("word/document.xml", options).unwrap();
    zip.write_all(b"<w:document></w:document>").unwrap();
    zip.finish().unwrap().into_inner()
}

fn multipart(fields: &[MultipartField<'_>]) -> Vec<u8> {
    let mut body = Vec::new();
    for (name, filename, content_type, data) in fields {
        body.extend_from_slice(format!("--{BOUNDARY}\r\n").as_bytes());
        if let Some(filename) = filename {
            body.extend_from_slice(
                format!(
                    "Content-Disposition: form-data; name=\"{name}\"; filename=\"{filename}\"\r\n"
                )
                .as_bytes(),
            );
            if let Some(content_type) = content_type {
                body.extend_from_slice(format!("Content-Type: {content_type}\r\n").as_bytes());
            }
        } else {
            body.extend_from_slice(
                format!("Content-Disposition: form-data; name=\"{name}\"\r\n").as_bytes(),
            );
        }
        body.extend_from_slice(b"\r\n");
        body.extend_from_slice(data);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
    body
}

async fn upload(
    state: studysync_backend::AppState,
    access: &str,
    fields: &[MultipartField<'_>],
) -> (StatusCode, serde_json::Value) {
    send(
        state,
        Request::builder()
            .method("POST")
            .uri("/api/v1/media")
            .header("authorization", format!("Bearer {access}"))
            .header(
                "content-type",
                format!("multipart/form-data; boundary={BOUNDARY}"),
            )
            .body(Body::from(multipart(fields)))
            .unwrap(),
    )
    .await
}

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
                    "title": "Media group",
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

async fn add_member(
    state: studysync_backend::AppState,
    leader: &str,
    member: &str,
    group_id: &str,
) {
    let (status, created) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri(format!("/api/v1/groups/{group_id}/join-requests"))
            .header("authorization", format!("Bearer {member}"))
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
            .header("authorization", format!("Bearer {leader}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
}

#[tokio::test]
async fn uploads_validate_type_size_and_auth() {
    let state = test_state().await;
    let (access, _) = register_user(state.clone()).await;
    let group_id = create_group(state.clone(), &access).await;
    let png = png_bytes();
    let (status, json) = upload(
        state.clone(),
        &access,
        &[
            ("purpose", None, None, b"group_message"),
            ("groupId", None, None, group_id.as_bytes()),
            ("file", Some("pic.png"), Some("image/png"), &png),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{json}");
    assert_eq!(json["contentType"], "image/png");
    assert_eq!(json["kind"], "image");
    assert!(json["storageKey"].is_null());
    assert!(json["downloadUrl"]
        .as_str()
        .unwrap()
        .starts_with("memory://"));

    let pdf = pdf_bytes();
    let (status, json) = upload(
        state.clone(),
        &access,
        &[
            ("purpose", None, None, b"group_message"),
            ("groupId", None, None, group_id.as_bytes()),
            ("file", Some("notes.pdf"), Some("application/pdf"), &pdf),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{json}");
    assert_eq!(json["kind"], "file");

    let docx = docx_bytes();
    let (status, json) = upload(
        state.clone(),
        &access,
        &[
            ("purpose", None, None, b"group_message"),
            ("groupId", None, None, group_id.as_bytes()),
            (
                "file",
                Some("essay.docx"),
                Some("application/vnd.openxmlformats-officedocument.wordprocessingml.document"),
                &docx,
            ),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{json}");

    let (status, json) = upload(
        state.clone(),
        &access,
        &[
            ("purpose", None, None, b"group_message"),
            ("groupId", None, None, group_id.as_bytes()),
            ("file", Some("pic.png"), Some("image/png"), &pdf),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{json}");

    let (status, json) = upload(
        state.clone(),
        &access,
        &[
            ("purpose", None, None, b"group_message"),
            ("groupId", None, None, group_id.as_bytes()),
            ("file", Some("../etc/passwd.png"), Some("image/png"), &png),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{json}");
    assert_eq!(json["fileName"], "passwd.png");

    let huge = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]
        .into_iter()
        .chain(std::iter::repeat_n(1u8, 11 * 1024 * 1024))
        .collect::<Vec<_>>();
    let (status, json) = upload(
        state.clone(),
        &access,
        &[
            ("purpose", None, None, b"group_message"),
            ("groupId", None, None, group_id.as_bytes()),
            ("file", Some("big.png"), Some("image/png"), &huge),
        ],
    )
    .await;
    assert!(
        status == StatusCode::PAYLOAD_TOO_LARGE || status == StatusCode::BAD_REQUEST,
        "{status} {json}"
    );

    let (status, json) = upload(
        state.clone(),
        "not-a-token",
        &[
            ("purpose", None, None, b"group_message"),
            ("groupId", None, None, group_id.as_bytes()),
            ("file", Some("pic.png"), Some("image/png"), &png),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{json}");
}

#[tokio::test]
async fn profile_picture_upload_replace_and_reject_key_patch() {
    let state = test_state().await;
    let (access, user_id) = register_user(state.clone()).await;
    let png = png_bytes();
    let (status, first) = send(
        state.clone(),
        Request::builder()
            .method("PUT")
            .uri("/api/v1/me/profile-picture")
            .header("authorization", format!("Bearer {access}"))
            .header(
                "content-type",
                format!("multipart/form-data; boundary={BOUNDARY}"),
            )
            .body(Body::from(multipart(&[(
                "file",
                Some("me.png"),
                Some("image/png"),
                &png,
            )])))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{first}");
    let first_id = first["id"].as_str().unwrap().to_string();

    let jpeg = jpeg_bytes();
    let (status, second) = send(
        state.clone(),
        Request::builder()
            .method("PUT")
            .uri("/api/v1/me/profile-picture")
            .header("authorization", format!("Bearer {access}"))
            .header(
                "content-type",
                format!("multipart/form-data; boundary={BOUNDARY}"),
            )
            .body(Body::from(multipart(&[(
                "file",
                Some("me.jpg"),
                Some("image/jpeg"),
                &jpeg,
            )])))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{second}");
    assert_ne!(second["id"], first["id"]);

    let (status, json) = send(
        state.clone(),
        Request::builder()
            .method("GET")
            .uri("/api/v1/me")
            .header("authorization", format!("Bearer {access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert!(json["profilePictureKey"]
        .as_str()
        .unwrap()
        .contains(&user_id.to_string()));

    let (status, json) = send(
        state.clone(),
        Request::builder()
            .method("GET")
            .uri(format!("/api/v1/media/{first_id}"))
            .header("authorization", format!("Bearer {access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{json}");

    let (status, json) = send(
        state.clone(),
        Request::builder()
            .method("PATCH")
            .uri("/api/v1/me")
            .header("authorization", format!("Bearer {access}"))
            .header("content-type", "application/json")
            .body(Body::from(r#"{"profilePictureKey":"stolen/key.png"}"#))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{json}");

    let pdf = pdf_bytes();
    let (status, json) = send(
        state,
        Request::builder()
            .method("PUT")
            .uri("/api/v1/me/profile-picture")
            .header("authorization", format!("Bearer {access}"))
            .header(
                "content-type",
                format!("multipart/form-data; boundary={BOUNDARY}"),
            )
            .body(Body::from(multipart(&[(
                "file",
                Some("me.pdf"),
                Some("application/pdf"),
                &pdf,
            )])))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{json}");
}

#[tokio::test]
async fn group_attachments_authorization_and_message_bind() {
    let state = test_state().await;
    let (leader, _) = register_user(state.clone()).await;
    let (member, _) = register_user(state.clone()).await;
    let (stranger, _) = register_user(state.clone()).await;
    let group_id = create_group(state.clone(), &leader).await;
    add_member(state.clone(), &leader, &member, &group_id).await;
    let png = png_bytes();

    let (status, json) = upload(
        state.clone(),
        &stranger,
        &[
            ("purpose", None, None, b"group_message"),
            ("groupId", None, None, group_id.as_bytes()),
            ("file", Some("pic.png"), Some("image/png"), &png),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{json}");

    let (status, uploaded) = upload(
        state.clone(),
        &member,
        &[
            ("purpose", None, None, b"group_message"),
            ("groupId", None, None, group_id.as_bytes()),
            ("file", Some("pic.png"), Some("image/png"), &png),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{uploaded}");
    let media_id = uploaded["id"].as_str().unwrap();

    let (status, stolen) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri(format!("/api/v1/groups/{group_id}/messages"))
            .header("authorization", format!("Bearer {leader}"))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "clientMsgId": Uuid::now_v7(),
                    "content": "",
                    "messageType": "image",
                    "attachmentIds": [media_id]
                })
                .to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{stolen}");

    let (status, created) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri(format!("/api/v1/groups/{group_id}/messages"))
            .header("authorization", format!("Bearer {member}"))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "clientMsgId": Uuid::now_v7(),
                    "content": "look",
                    "messageType": "image",
                    "attachmentIds": [media_id]
                })
                .to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["attachments"][0]["id"], media_id);
    let message_id = created["id"].as_str().unwrap();

    let (status, json) = send(
        state.clone(),
        Request::builder()
            .method("GET")
            .uri(format!("/api/v1/media/{media_id}"))
            .header("authorization", format!("Bearer {stranger}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{json}");

    let (status, meta) = send(
        state.clone(),
        Request::builder()
            .method("GET")
            .uri(format!("/api/v1/media/{media_id}"))
            .header("authorization", format!("Bearer {leader}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{meta}");

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
            .method("GET")
            .uri(format!("/api/v1/media/{media_id}"))
            .header("authorization", format!("Bearer {member}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{json}");
}

#[tokio::test]
async fn direct_message_attachments_and_blocks() {
    let state = test_state().await;
    let (a, a_id) = register_user(state.clone()).await;
    let (b, b_id) = register_user(state.clone()).await;
    let (status, created) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/v1/friends/requests")
            .header("authorization", format!("Bearer {a}"))
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
            .header("authorization", format!("Bearer {b}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let png = png_bytes();
    let (status, uploaded) = upload(
        state.clone(),
        &a,
        &[
            ("purpose", None, None, b"direct_message"),
            ("recipientId", None, None, b_id.to_string().as_bytes()),
            ("file", Some("pic.png"), Some("image/png"), &png),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{uploaded}");
    let media_id = uploaded["id"].as_str().unwrap();

    let (c, _) = register_user(state.clone()).await;
    let (status, json) = send(
        state.clone(),
        Request::builder()
            .method("GET")
            .uri(format!("/api/v1/media/{media_id}"))
            .header("authorization", format!("Bearer {c}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{json}");

    let (status, created) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/v1/direct-messages")
            .header("authorization", format!("Bearer {a}"))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "recipientId": b_id,
                    "content": "",
                    "messageType": "image",
                    "attachmentIds": [media_id]
                })
                .to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["message"]["attachments"][0]["id"], media_id);

    let (status, thread) = send(
        state.clone(),
        Request::builder()
            .method("GET")
            .uri(format!("/api/v1/direct-messages/{a_id}"))
            .header("authorization", format!("Bearer {b}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{thread}");
    assert_eq!(thread[0]["attachments"][0]["id"], media_id);

    let (status, _) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/v1/blocks")
            .header("authorization", format!("Bearer {b}"))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({ "userId": a_id }).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, json) = upload(
        state,
        &a,
        &[
            ("purpose", None, None, b"direct_message"),
            ("recipientId", None, None, b_id.to_string().as_bytes()),
            ("file", Some("pic.png"), Some("image/png"), &png),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{json}");
}

#[tokio::test]
async fn signed_url_expiry_and_orphan_cleanup() {
    let state = test_state().await;
    let (access, _) = register_user(state.clone()).await;
    let group_id = create_group(state.clone(), &access).await;
    let png = png_bytes();
    let (status, uploaded) = upload(
        state.clone(),
        &access,
        &[
            ("purpose", None, None, b"group_message"),
            ("groupId", None, None, group_id.as_bytes()),
            ("file", Some("pic.png"), Some("image/png"), &png),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{uploaded}");
    let media_id = uploaded["id"].as_str().unwrap();
    let url = uploaded["downloadUrl"].as_str().unwrap();
    let exp: i64 = url
        .split("exp=")
        .nth(1)
        .unwrap()
        .split('&')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    let sig = url.split("sig=").nth(1).unwrap();

    let (status, json) = send(
        state.clone(),
        Request::builder()
            .method("GET")
            .uri(format!(
                "/api/v1/media/{media_id}/content?exp={}&sig={sig}",
                exp - 10_000
            ))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{json}");

    sqlx::query("UPDATE media_objects SET created_at = now() - interval '25 hours' WHERE id = $1")
        .bind(Uuid::parse_str(media_id).unwrap())
        .execute(&state.db)
        .await
        .unwrap();
    let removed = studysync_backend::services::media::cleanup_orphaned_media(&state)
        .await
        .unwrap();
    assert!(removed >= 1);
    let (status, json) = send(
        state,
        Request::builder()
            .method("GET")
            .uri(format!("/api/v1/media/{media_id}"))
            .header("authorization", format!("Bearer {access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{json}");
}

#[tokio::test]
async fn s3_compatible_put_get_delete() {
    let config = studysync_backend::config::StorageConfig {
        endpoint: std::env::var("S3_ENDPOINT").unwrap_or_else(|_| "http://127.0.0.1:9000".into()),
        bucket: std::env::var("S3_BUCKET").unwrap_or_else(|_| "studysync-media".into()),
        access_key: std::env::var("S3_ACCESS_KEY").unwrap_or_else(|_| "studysync".into()),
        secret_key: std::env::var("S3_SECRET_KEY")
            .unwrap_or_else(|_| "studysync_dev_minio_change_me".into()),
        region: "us-east-1".into(),
        force_path_style: true,
    };
    let store = studysync_backend::media::S3Storage::new(&config).unwrap_or_else(|_| {
        panic!(
            "local S3-compatible storage is required for media integration. Start docker compose (studysync-minio on port 9000)."
        )
    });
    let key = format!("tests/{}.bin", Uuid::now_v7());
    if store
        .put(&key, b"hello-s3", "application/octet-stream")
        .await
        .is_err()
    {
        panic!(
            "local S3-compatible storage is required for media integration. Start docker compose (studysync-minio on port 9000)."
        );
    }
    assert_eq!(store.get(&key).await.unwrap(), b"hello-s3");
    store.delete(&key).await.unwrap();
}
