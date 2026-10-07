mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{register_body, send, test_state, unique_email};
use uuid::Uuid;

#[tokio::test]
async fn patch_me_updates_postgres_and_validates_availability() {
    let state = test_state().await;
    let email = unique_email();
    let (status, created) = send(
        state.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/v1/auth/register")
            .header("content-type", "application/json")
            .body(Body::from(register_body(&email, "password12").to_string()))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let access = created["accessToken"].as_str().unwrap();
    let user_id = Uuid::parse_str(created["user"]["id"].as_str().unwrap()).unwrap();

    let course_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM user_courses WHERE user_id = $1")
            .bind(user_id)
            .fetch_one(&state.db)
            .await
            .unwrap();
    assert_eq!(course_count, 1);
    let env_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM user_study_environments WHERE user_id = $1")
            .bind(user_id)
            .fetch_one(&state.db)
            .await
            .unwrap();
    assert_eq!(env_count, 1);
    let method_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM user_study_methods WHERE user_id = $1")
            .bind(user_id)
            .fetch_one(&state.db)
            .await
            .unwrap();
    assert_eq!(method_count, 1);
    let goal_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM user_study_goals WHERE user_id = $1")
            .bind(user_id)
            .fetch_one(&state.db)
            .await
            .unwrap();
    assert_eq!(goal_count, 1);

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
                    "studyEnvironments": ["Library", "Online"],
                    "studyMethods": ["Flashcards"],
                    "groupPreferences": {
                        "preferredGroupSizeMin": 2,
                        "preferredGroupSizeMax": 4,
                        "sessionDurationMinutes": 60,
                        "studyGoals": ["Homework", "Project work"]
                    },
                    "availability": [{
                        "dayOfWeek": 2,
                        "startTime": "23:00",
                        "endTime": "01:00",
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

    let majors: Vec<(String, i16, i16, i16)> = sqlx::query_as(
        "SELECT u.major, p.preferred_group_size_min, p.preferred_group_size_max, p.session_duration_minutes
         FROM users u
         JOIN user_preferences p ON p.user_id = u.id
         WHERE u.id = $1",
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await
    .unwrap();
    assert_eq!(majors[0].0, "Mathematics");
    assert_eq!(majors[0].1, 2);
    assert_eq!(majors[0].2, 4);
    assert_eq!(majors[0].3, 60);

    let course_names: Vec<String> = sqlx::query_scalar(
        "SELECT c.name FROM courses c
         JOIN user_courses uc ON uc.course_id = c.id
         WHERE uc.user_id = $1 ORDER BY c.name",
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await
    .unwrap();
    assert_eq!(course_names, vec!["Linear Algebra", "Statistics"]);

    let overnight: (i16, String, String) = sqlx::query_as(
        "SELECT day_of_week, start_time::text, end_time::text FROM availability WHERE user_id = $1",
    )
    .bind(user_id)
    .fetch_one(&state.db)
    .await
    .unwrap();
    assert_eq!(overnight.0, 2);
    assert!(overnight.1.starts_with("23:00"));
    assert!(overnight.2.starts_with("01:00"));

    let env_names: Vec<String> = sqlx::query_scalar(
        "SELECT e.name FROM study_environments e
         JOIN user_study_environments ue ON ue.environment_id = e.id
         WHERE ue.user_id = $1 ORDER BY e.name",
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await
    .unwrap();
    assert_eq!(env_names, vec!["Library", "Online"]);

    let method_names: Vec<String> = sqlx::query_scalar(
        "SELECT m.name FROM study_methods m
         JOIN user_study_methods um ON um.method_id = m.id
         WHERE um.user_id = $1 ORDER BY m.name",
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await
    .unwrap();
    assert_eq!(method_names, vec!["Flashcards"]);

    let goal_names: Vec<String> = sqlx::query_scalar(
        "SELECT g.name FROM study_goals g
         JOIN user_study_goals ug ON ug.goal_id = g.id
         WHERE ug.user_id = $1 ORDER BY g.name",
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await
    .unwrap();
    assert_eq!(goal_names, vec!["Homework", "Project work"]);

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
                        "endTime": "15:00",
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
    let state = test_state().await;
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

#[tokio::test]
async fn register_rejects_invalid_timezone() {
    let state = test_state().await;
    let mut body = register_body(&unique_email(), "password12");
    body["availability"] = serde_json::json!([{
        "dayOfWeek": 0,
        "startTime": "17:00",
        "endTime": "21:00",
        "timezone": "Not/AZone"
    }]);
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
    body["availability"] = serde_json::json!([{
        "dayOfWeek": 0,
        "startTime": "17:00",
        "endTime": "21:00",
        "timezone": "   "
    }]);
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
    assert_eq!(status, StatusCode::BAD_REQUEST, "{json}");
    assert_eq!(json["error"]["code"], "validation_error");
}
