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
async fn wrong_dimension_is_rejected() {
    let state = test_state().await;
    let mut tx = state.db.begin().await.expect("tx");
    let err = embeddings::upsert(
        &mut tx,
        "group",
        Uuid::now_v7(),
        catalog::PRODUCTION_MODEL_ID,
        "hash",
        &[0.1; 384],
    )
    .await
    .expect_err("dim");
    assert_eq!(err.code(), "embedding_dimension_mismatch");
}
