mod common;

use common::test_state;
use studysync_backend::db::embeddings;
use studysync_backend::embed::catalog;
use uuid::Uuid;

#[tokio::test]
async fn embedding_upsert_is_idempotent_per_model() {
    let state = test_state().await;
    let mut tx = state.db.begin().await.expect("tx");
    let entity = Uuid::now_v7();
    let vector = vec![0.01; catalog::PRODUCTION_EMBEDDING_DIM];
    embeddings::upsert(
        &mut tx,
        "group",
        entity,
        catalog::PRODUCTION_MODEL_ID,
        "hash-a",
        &vector,
    )
    .await
    .expect("insert");
    embeddings::upsert(
        &mut tx,
        "group",
        entity,
        catalog::PRODUCTION_MODEL_ID,
        "hash-a",
        &vector,
    )
    .await
    .expect("noop");
    let row = embeddings::get(&mut *tx, "group", entity, catalog::PRODUCTION_MODEL_ID)
        .await
        .expect("get")
        .expect("row");
    assert_eq!(row.model_id, catalog::PRODUCTION_MODEL_ID);
    assert_eq!(row.dimensions, catalog::PRODUCTION_EMBEDDING_DIM as i32);
    tx.commit().await.expect("commit");
}

#[tokio::test]
async fn embedding_versions_can_coexist() {
    let state = test_state().await;
    let mut tx = state.db.begin().await.expect("tx");
    let entity = Uuid::now_v7();
    let vector = vec![0.01; catalog::PRODUCTION_EMBEDDING_DIM];
    embeddings::upsert(
        &mut tx,
        "user",
        entity,
        catalog::PRODUCTION_MODEL_ID,
        "hash-prod",
        &vector,
    )
    .await
    .expect("prod");
    embeddings::upsert(
        &mut tx,
        "user",
        entity,
        "all-minilm-l6-v2:legacy-padded",
        "hash-legacy",
        &vector,
    )
    .await
    .expect("legacy id");
    assert!(
        embeddings::get(&mut *tx, "user", entity, catalog::PRODUCTION_MODEL_ID)
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        embeddings::get(&mut *tx, "user", entity, "all-minilm-l6-v2:legacy-padded")
            .await
            .unwrap()
            .is_some()
    );
    tx.commit().await.expect("commit");
}

#[tokio::test]
async fn embeddings_of_different_dimensions_do_not_share_a_column() {
    let state = test_state().await;
    let mut tx = state.db.begin().await.expect("tx");
    let entity = Uuid::now_v7();
    embeddings::upsert(
        &mut tx,
        "group",
        entity,
        "all-minilm-l6-v2:legacy",
        "hash-384",
        &vec![0.01; 384],
    )
    .await
    .expect("384");
    embeddings::upsert(
        &mut tx,
        "group",
        entity,
        "nomic-embed-text-v1.5:v1",
        "hash-768",
        &vec![0.01; 768],
    )
    .await
    .expect("768");
    let a = embeddings::load_vector(&mut *tx, "group", entity, "all-minilm-l6-v2:legacy")
        .await
        .expect("load")
        .expect("row");
    let b = embeddings::load_vector(&mut *tx, "group", entity, "nomic-embed-text-v1.5:v1")
        .await
        .expect("load")
        .expect("row");
    tx.commit().await.expect("commit");
    assert_eq!(a.len(), 384);
    assert_eq!(b.len(), 768);
}

#[tokio::test]
async fn nomic_search_ignores_other_model_ids_and_widths() {
    let state = test_state().await;
    let (access, leader) = common::register_user(state.clone()).await;
    let _ = access;
    let mut tx = state.db.begin().await.expect("tx");
    let nomic_group = Uuid::now_v7();
    let minilm_group = Uuid::now_v7();
    let now = chrono::Utc::now();
    for (id, title) in [
        (nomic_group, "Nomic Networks"),
        (minilm_group, "MiniLM Chemistry"),
    ] {
        sqlx::query(
            "INSERT INTO study_groups (
                id, title, subject, course_id, description, location,
                max_members, leader_id, status, created_at, updated_at
             ) VALUES ($1,$2,$3,NULL,$4,$5,6,$6,'active',$7,$7)",
        )
        .bind(id)
        .bind(title)
        .bind(title)
        .bind("test")
        .bind("Library")
        .bind(leader)
        .bind(now)
        .execute(&mut *tx)
        .await
        .expect("group");
    }
    let mut nomic_vec = vec![0.0; 768];
    nomic_vec[0] = 1.0;
    let mut minilm_vec = vec![0.0; 384];
    minilm_vec[0] = 1.0;
    embeddings::upsert(
        &mut tx,
        "group",
        nomic_group,
        catalog::PRODUCTION_MODEL_ID,
        "nomic-hash",
        &nomic_vec,
    )
    .await
    .expect("nomic vec");
    embeddings::upsert(
        &mut tx,
        "group",
        minilm_group,
        "all-minilm-l6-v2:legacy",
        "minilm-hash",
        &minilm_vec,
    )
    .await
    .expect("minilm vec");
    let nomic_hits = embeddings::search_groups(
        &mut *tx,
        catalog::PRODUCTION_MODEL_ID,
        &nomic_vec,
        0.1,
        10,
        0,
    )
    .await
    .expect("nomic search");
    let minilm_hits =
        embeddings::search_groups(&mut *tx, "all-minilm-l6-v2:legacy", &minilm_vec, 0.1, 10, 0)
            .await
            .expect("minilm search");
    tx.commit().await.expect("commit");
    assert!(nomic_hits.iter().any(|hit| hit.entity_id == nomic_group));
    assert!(nomic_hits.iter().all(|hit| hit.entity_id != minilm_group));
    assert!(minilm_hits.iter().any(|hit| hit.entity_id == minilm_group));
    assert!(minilm_hits.iter().all(|hit| hit.entity_id != nomic_group));
}

#[tokio::test]
async fn wrong_dimension_is_rejected() {
    let state = test_state().await;
    let mut tx = state.db.begin().await.expect("tx");
    let err = embeddings::upsert(
        &mut tx,
        "group",
        Uuid::now_v7(),
        catalog::PRODUCTION_MODEL_ID,
        "hash",
        &[0.1; 16],
    )
    .await
    .expect_err("dim");
    assert_eq!(err.code(), "embedding_dimension_mismatch");
}
