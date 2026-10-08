use sqlx::PgPool;
use uuid::Uuid;

use crate::db::{embeddings, groups, identity, profile};
use crate::embed::{canonical_group_text, canonical_user_text, source_hash, EmbeddingService};
use crate::error::AppError;

pub async fn upsert_group(
    pool: &PgPool,
    embeddings: &EmbeddingService,
    group_id: Uuid,
) -> Result<(), AppError> {
    if !embeddings.semantic_available() {
        tracing::info!(
            model_id = embeddings.model_id(),
            "skipping group embedding; semantic backend unavailable"
        );
        return Ok(());
    }
    let mut tx = pool.begin().await.map_err(crate::error::map_db_error)?;
    let group = groups::get_group(&mut *tx, group_id)
        .await?
        .ok_or(AppError::GroupNotFound)?;
    let leader = identity::get_user_by_id(&mut *tx, group.leader_id)
        .await?
        .ok_or(AppError::UserNotFound)?;
    let text = canonical_group_text(
        &group.title,
        &group.subject,
        &group.description,
        &leader.name,
        &leader.major,
    );
    let hash = source_hash(embeddings.model_id(), &text);
    if let Some(existing) =
        embeddings::get(&mut *tx, "group", group_id, embeddings.model_id()).await?
    {
        if existing.source_hash == hash {
            return Ok(());
        }
    }
    tx.commit().await.map_err(crate::error::map_db_error)?;

    let vector = embeddings.encode_document(&text).await?;
    let mut tx = pool.begin().await.map_err(crate::error::map_db_error)?;
    embeddings::upsert(
        &mut tx,
        "group",
        group_id,
        embeddings.model_id(),
        &hash,
        &vector,
    )
    .await?;
    tx.commit().await.map_err(crate::error::map_db_error)?;
    Ok(())
}

pub async fn upsert_user(
    pool: &PgPool,
    embeddings: &EmbeddingService,
    user_id: Uuid,
) -> Result<(), AppError> {
    if !embeddings.semantic_available() {
        tracing::info!(
            model_id = embeddings.model_id(),
            "skipping user embedding; semantic backend unavailable"
        );
        return Ok(());
    }
    let mut tx = pool.begin().await.map_err(crate::error::map_db_error)?;
    let user = identity::get_user_by_id(&mut *tx, user_id)
        .await?
        .filter(|row| row.deleted_at.is_none())
        .ok_or(AppError::UserNotFound)?;
    let courses = profile::load_courses(&mut *tx, user_id).await?;
    let names: Vec<String> = courses.into_iter().map(|c| c.name).collect();
    let text = canonical_user_text(&user.name, &user.major, &names);
    let hash = source_hash(embeddings.model_id(), &text);
    if let Some(existing) =
        embeddings::get(&mut *tx, "user", user_id, embeddings.model_id()).await?
    {
        if existing.source_hash == hash {
            return Ok(());
        }
    }
    tx.commit().await.map_err(crate::error::map_db_error)?;

    let vector = embeddings.encode_document(&text).await?;
    let mut tx = pool.begin().await.map_err(crate::error::map_db_error)?;
    embeddings::upsert(
        &mut tx,
        "user",
        user_id,
        embeddings.model_id(),
        &hash,
        &vector,
    )
    .await?;
    tx.commit().await.map_err(crate::error::map_db_error)?;
    Ok(())
}
