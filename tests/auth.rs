mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::Utc;
use common::{register_body, send, test_state, unique_email};
use studysync_backend::auth::jwt::{decode_access_token, encode_access_token_with_exp};
use uuid::Uuid;

fn register_request(email: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/api/v1/auth/register")
        .header("content-type", "application/json")
        .body(Body::from(register_body(email, "password12").to_string()))
        .unwrap()
}

#[tokio::test]
async fn register_login_me_and_duplicate_email() {
    let Some(state) = test_state().await else {
        eprintln!("skipping: DATABASE_URL not reachable");
        return;
    };
    let email = unique_email();

    let (status, json) = send(state.clone(), register_request(&email)).await;
    assert_eq!(status, StatusCode::CREATED, "{json}");
    assert!(json["accessToken"].as_str().unwrap().contains('.'));
    assert!(json["refreshToken"].as_str().is_some());
    assert!(json["user"]["passwordHash"].is_null());
    assert_eq!(json["user"]["email"], email);
    assert_eq!(
        json["user"]["institution"]["name"],
        format!("Example University {email}")
    );
    assert_eq!(
        json["user"]["groupPreferences"]["sessionDurationMinutes"],
        90
    );
    assert_eq!(json["user"]["availability"][0]["dayOfWeek"], 0);

    let (status, json) = send(state.clone(), register_request(&email)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(json["error"]["code"], "duplicate_email");

    let (status, json) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/v1/auth/login")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({ "email": email, "password": "password12" }).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let access = json["accessToken"].as_str().unwrap().to_string();

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
    assert_eq!(json["subjects"][0]["name"], "Algorithms");

    let (status, json) = send(
        state,
        Request::builder()
            .method("POST")
            .uri("/api/v1/auth/login")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({ "email": email, "password": "wrong-password" }).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(json["error"]["code"], "invalid_credentials");
}

#[tokio::test]
async fn refresh_rotates_and_logout_revokes() {
    let Some(state) = test_state().await else {
        eprintln!("skipping: DATABASE_URL not reachable");
        return;
    };
    let email = unique_email();
    let (_, created) = send(state.clone(), register_request(&email)).await;
    let refresh = created["refreshToken"].as_str().unwrap().to_string();

    let (status, rotated) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/v1/auth/refresh")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({ "refreshToken": refresh }).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{rotated}");
    let new_refresh = rotated["refreshToken"].as_str().unwrap().to_string();
    assert_ne!(new_refresh, refresh);

    let (status, json) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/v1/auth/refresh")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({ "refreshToken": refresh }).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(json["error"]["code"], "invalid_refresh_token");

    let (status, _) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/v1/auth/logout")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({ "refreshToken": new_refresh }).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, json) = send(
        state,
        Request::builder()
            .method("POST")
            .uri("/api/v1/auth/refresh")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({ "refreshToken": new_refresh }).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(json["error"]["code"], "invalid_refresh_token");
}

#[tokio::test]
async fn expired_jwt_is_rejected() {
    let Some(state) = test_state().await else {
        eprintln!("skipping: DATABASE_URL not reachable");
        return;
    };
    let token = encode_access_token_with_exp(
        Uuid::now_v7(),
        &state.config.jwt_secret,
        Utc::now().timestamp() - 30,
    )
    .unwrap();
    assert!(decode_access_token(&token, &state.config.jwt_secret).is_err());

    let (status, json) = send(
        state,
        Request::builder()
            .method("GET")
            .uri("/api/v1/me")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(json["error"]["code"], "token_expired");
}

#[tokio::test]
async fn bcrypt_password_is_upgraded_on_login() {
    let Some(state) = test_state().await else {
        eprintln!("skipping: DATABASE_URL not reachable");
        return;
    };
    let email = unique_email();
    let password = "legacy-secret";
    let bcrypt_hash = bcrypt::hash(password, 4).expect("bcrypt");
    let institution_id = Uuid::now_v7();
    let user_id = Uuid::now_v7();
    let now = Utc::now();

    sqlx::query("INSERT INTO institutions (id, name, slug) VALUES ($1, $2, $3)")
        .bind(institution_id)
        .bind(format!("Legacy College {user_id}"))
        .bind(format!("legacy-{user_id}"))
        .execute(&state.db)
        .await
        .expect("institution");

    sqlx::query(
        "INSERT INTO users (
            id, name, email, password_hash, password_algorithm, institution_id,
            major, year, learning_style, performance_level, created_at, updated_at
         ) VALUES ($1,$2,$3,$4,'bcrypt',$5,'Math','Senior','',3,$6,$6)",
    )
    .bind(user_id)
    .bind("Legacy User")
    .bind(&email)
    .bind(&bcrypt_hash)
    .bind(institution_id)
    .bind(now)
    .execute(&state.db)
    .await
    .expect("user");

    sqlx::query(
        "INSERT INTO user_preferences (
            user_id, preferred_group_size_min, preferred_group_size_max, session_duration_minutes
         ) VALUES ($1, 2, 6, 120)",
    )
    .bind(user_id)
    .execute(&state.db)
    .await
    .expect("prefs");

    let (status, json) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/v1/auth/login")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({ "email": email, "password": password }).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");

    let algorithm: String =
        sqlx::query_scalar("SELECT password_algorithm FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_one(&state.db)
            .await
            .expect("algorithm");
    assert_eq!(algorithm, "argon2id");
    let hash: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(&state.db)
        .await
        .expect("hash");
    assert!(hash.starts_with("$argon2"));
    assert_ne!(hash, bcrypt_hash);
}
