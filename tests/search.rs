mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{register_user, send, test_state};

async fn create_named_group(
    state: studysync_backend::AppState,
    access: &str,
    title: &str,
    subject: &str,
) {
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
                    "description": "Weekly problem set and exam preparation",
                    "location": "Library",
                    "maxMembers": 6,
                    "recurringSlots": [{
                        "dayOfWeek": 0,
                        "startTime": "17:00",
                        "endTime": "21:00",
                        "timezone": "America/Chicago"
                    }]
                })
                .to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{json}");
}

#[tokio::test]
async fn full_text_search_does_not_require_embeddings() {
    let state = test_state().await;
    let (access, _) = register_user(state.clone()).await;
    create_named_group(state.clone(), &access, "Algorithms crew", "Algorithms").await;
    create_named_group(state.clone(), &access, "Orgo night", "Organic Chemistry").await;

    let (status, json) = send(
        state,
        Request::builder()
            .method("POST")
            .uri("/api/v1/search/groups")
            .header("authorization", format!("Bearer {access}"))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "query": "Algorithms",
                    "mode": "fullText",
                    "limit": 10,
                    "offset": 0
                })
                .to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["mode"], "fullText");
    assert!(!json["results"].as_array().unwrap().is_empty());
    assert!(json["results"]
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["group"]["subject"] == "Algorithms"));
}

#[tokio::test]
#[ignore = "requires cached or downloadable nomic-embed-text-v1.5 weights"]
async fn semantic_search_ranks_related_groups() {
    use std::sync::Arc;
    use studysync_backend::embed::nomic::NomicEmbedder;
    use studysync_backend::embed::EmbeddingService;

    let cache = std::env::var("EMBEDDING_CACHE_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from(".fastembed_cache"));
    let nomic = NomicEmbedder::load(&cache, 512)
        .unwrap_or_else(|error| panic!("Nomic weights are required for semantic search: {error}"));
    let mut state = test_state().await;
    state.embeddings = EmbeddingService::from_loaded(Arc::new(nomic));
    let (access, _) = register_user(state.clone()).await;
    let marker = uuid::Uuid::now_v7();
    let net_subject = format!("Networking {marker}");
    let orgo_subject = format!("Organic Chemistry {marker}");
    create_named_group(state.clone(), &access, "Networking Circle", &net_subject).await;
    create_named_group(state.clone(), &access, "Organic Chemistry", &orgo_subject).await;

    let (status, json) = send(
        state,
        Request::builder()
            .method("POST")
            .uri("/api/v1/search/groups")
            .header("authorization", format!("Bearer {access}"))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "query": format!("computer networks study group {marker}"),
                    "mode": "semantic",
                    "threshold": 0.05,
                    "limit": 50,
                    "offset": 0
                })
                .to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["mode"], "semantic");
    let results = json["results"].as_array().unwrap();
    assert!(!results.is_empty());
    let net_score = results
        .iter()
        .find(|row| row["group"]["subject"] == net_subject)
        .expect("networking group")["score"]
        .as_f64()
        .unwrap();
    let orgo_score = results
        .iter()
        .find(|row| row["group"]["subject"] == orgo_subject)
        .map(|row| row["score"].as_f64().unwrap())
        .unwrap_or(0.0);
    assert!(net_score > orgo_score);
}

#[tokio::test]
async fn semantic_search_unavailable_is_503() {
    let mut state = test_state().await;
    state.embeddings = studysync_backend::embed::EmbeddingService::unavailable();
    let (access, _) = register_user(state.clone()).await;
    let (status, json) = send(
        state,
        Request::builder()
            .method("POST")
            .uri("/api/v1/search/groups")
            .header("authorization", format!("Bearer {access}"))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "query": "calculus",
                    "mode": "semantic"
                })
                .to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{json}");
    assert_eq!(json["error"]["code"], "semantic_unavailable");
}

#[tokio::test]
async fn search_paginates() {
    let state = test_state().await;
    let (access, _) = register_user(state.clone()).await;
    create_named_group(state.clone(), &access, "Algorithms one", "Algorithms").await;
    create_named_group(state.clone(), &access, "Algorithms two", "Algorithms").await;
    let (status, page) = send(
        state,
        Request::builder()
            .method("POST")
            .uri("/api/v1/search/groups")
            .header("authorization", format!("Bearer {access}"))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "query": "Algorithms",
                    "mode": "fullText",
                    "limit": 1,
                    "offset": 0
                })
                .to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page["results"].as_array().unwrap().len(), 1);
    assert_eq!(page["limit"], 1);
}
