mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::Utc;
use common::{register_body, send, test_state, unique_email};
use studysync_backend::auth::jwt::{
    decode_access_token, encode_access_token, encode_access_token_with_exp,
};
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
    let state = test_state().await;
    let email = unique_email();

    let (status, json) = send(state.clone(), register_request(&email)).await;
    assert_eq!(status, StatusCode::CREATED, "{json}");
    assert!(json["accessToken"].as_str().unwrap().contains('.'));
    assert!(json["refreshToken"].as_str().is_some());
    assert!(json["user"]["passwordHash"].is_null());
    assert!(json["user"]["passwordAlgorithm"].is_null());
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
    assert_eq!(json["user"]["subjects"][0]["name"], "Algorithms");
    assert_eq!(json["user"]["studyEnvironments"][0], "Library");
    assert_eq!(json["user"]["studyMethods"][0], "Practice problems");
    assert_eq!(json["user"]["studyGoals"][0], "Exam preparation");
    assert_eq!(json["user"]["major"], "Computer Science");
    assert_eq!(json["user"]["year"], "Junior");

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
async fn duplicate_email_is_case_insensitive() {
    let state = test_state().await;
    let local = Uuid::now_v7();
    let upper = format!("User-{local}@Example.COM");
    let lower = format!("user-{local}@example.com");

    let (status, json) = send(state.clone(), register_request(&upper)).await;
    assert_eq!(status, StatusCode::CREATED, "{json}");
    assert_eq!(json["user"]["email"], upper);

    let (status, json) = send(state, register_request(&lower)).await;
    assert_eq!(status, StatusCode::CONFLICT, "{json}");
    assert_eq!(json["error"]["code"], "duplicate_email");
}

#[tokio::test]
async fn institution_domain_collision_is_not_internal_error() {
    let state = test_state().await;
    let domain = format!("{}.edu", Uuid::now_v7());
    let email_a = unique_email();
    let email_b = unique_email();

    let mut body_a = register_body(&email_a, "password12");
    body_a["institutionName"] = serde_json::json!(format!("Alpha College {email_a}"));
    body_a["institutionDomain"] = serde_json::json!(domain);
    let (status, first) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/v1/auth/register")
            .header("content-type", "application/json")
            .body(Body::from(body_a.to_string()))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{first}");
    assert_ne!(status, StatusCode::INTERNAL_SERVER_ERROR);

    let mut body_b = register_body(&email_b, "password12");
    body_b["institutionName"] = serde_json::json!(format!("Beta College {email_b}"));
    body_b["institutionDomain"] = serde_json::json!(domain);
    let (status, second) = send(
        state,
        Request::builder()
            .method("POST")
            .uri("/api/v1/auth/register")
            .header("content-type", "application/json")
            .body(Body::from(body_b.to_string()))
            .unwrap(),
    )
    .await;
    assert_ne!(status, StatusCode::INTERNAL_SERVER_ERROR, "{second}");
    assert_eq!(status, StatusCode::CREATED, "{second}");
    assert_eq!(
        first["user"]["institution"]["id"],
        second["user"]["institution"]["id"]
    );
}

#[tokio::test]
async fn refresh_rotates_and_logout_revokes() {
    let state = test_state().await;
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
        state.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/v1/auth/logout")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({ "refreshToken": "" }).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(json["error"]["code"], "invalid_refresh_token");

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
async fn concurrent_refresh_allows_exactly_one_winner() {
    let state = test_state().await;
    let (_, created) = send(state.clone(), register_request(&unique_email())).await;
    let refresh = created["refreshToken"].as_str().unwrap().to_string();

    let request = || {
        Request::builder()
            .method("POST")
            .uri("/api/v1/auth/refresh")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({ "refreshToken": refresh }).to_string(),
            ))
            .unwrap()
    };

    let (first, second) = tokio::join!(
        send(state.clone(), request()),
        send(state.clone(), request())
    );

    let outcomes = [first, second];
    let wins: Vec<_> = outcomes
        .iter()
        .filter(|(status, _)| *status == StatusCode::OK)
        .collect();
    let losses: Vec<_> = outcomes
        .iter()
        .filter(|(status, _)| *status == StatusCode::UNAUTHORIZED)
        .collect();
    assert_eq!(wins.len(), 1, "{outcomes:?}");
    assert_eq!(losses.len(), 1, "{outcomes:?}");
    assert_eq!(losses[0].1["error"]["code"], "invalid_refresh_token");

    let new_refresh = wins[0].1["refreshToken"].as_str().unwrap().to_string();
    assert_ne!(new_refresh, refresh);

    let (status, _) = send(
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
    assert_eq!(status, StatusCode::OK, "{json}");
}

#[tokio::test]
async fn expired_jwt_is_rejected() {
    let state = test_state().await;
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
async fn missing_and_deleted_users_are_unauthorized() {
    let state = test_state().await;
    let missing = encode_access_token(Uuid::now_v7(), &state.config.jwt_secret).unwrap();
    let (status, json) = send(
        state.clone(),
        Request::builder()
            .method("GET")
            .uri("/api/v1/me")
            .header("authorization", format!("Bearer {missing}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(json["error"]["code"], "unauthorized");

    let email = unique_email();
    let (status, created) = send(state.clone(), register_request(&email)).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let access = created["accessToken"].as_str().unwrap();
    let user_id = created["user"]["id"].as_str().unwrap();
    let user_id = Uuid::parse_str(user_id).unwrap();

    sqlx::query("UPDATE users SET deleted_at = now() WHERE id = $1")
        .bind(user_id)
        .execute(&state.db)
        .await
        .expect("soft delete");

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
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(json["error"]["code"], "unauthorized");

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
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{json}");
}

#[tokio::test]
async fn bcrypt_password_is_upgraded_on_login() {
    let state = test_state().await;
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

    let login = |email: String, password: String| {
        Request::builder()
            .method("POST")
            .uri("/api/v1/auth/login")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({ "email": email, "password": password }).to_string(),
            ))
            .unwrap()
    };

    let (status, json) = send(state.clone(), login(email.clone(), password.to_string())).await;
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

    let (status, json) = send(state, login(email, password.to_string())).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert!(json["accessToken"].as_str().unwrap().contains('.'));
}
