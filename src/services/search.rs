use std::time::Instant;

use sqlx::PgPool;
use uuid::Uuid;

use crate::api::dto::{GroupSearchRequest, GroupSearchResponse, GroupSearchResultDto};
use crate::db::embeddings;
use crate::embed::EmbeddingService;
use crate::error::AppError;
use crate::matching::MATCH_THRESHOLD;
use crate::services::groups::get_group;

pub async fn search_groups(
    pool: &PgPool,
    embeddings: &EmbeddingService,
    user_id: Uuid,
    request: GroupSearchRequest,
) -> Result<GroupSearchResponse, AppError> {
    let query = request.query.trim();
    if query.is_empty() {
        return Err(AppError::validation("query must not be blank"));
    }
    let mode = request.mode.as_deref().unwrap_or("fullText");
    let limit = request.limit.unwrap_or(20).clamp(1, 50);
    let offset = request.offset.unwrap_or(0).max(0);
    let threshold = request.threshold.unwrap_or(MATCH_THRESHOLD);

    match mode {
        "fullText" => {
            let started = Instant::now();
            let hits = embeddings::search_groups_full_text(pool, query, limit, offset).await?;
            let mut results = Vec::new();
            for hit in hits {
                let group = get_group(pool, user_id, hit.id).await?;
                results.push(GroupSearchResultDto {
                    group,
                    score: hit.rank,
                });
            }
            tracing::info!(
                elapsed_ms = started.elapsed().as_secs_f64() * 1000.0,
                mode = "fullText",
                result_count = results.len(),
                "group search completed"
            );
            Ok(GroupSearchResponse {
                mode: "fullText".into(),
                query: query.to_string(),
                results,
                limit,
                offset,
            })
        }
        "semantic" => {
            if !embeddings.semantic_available() {
                return Err(AppError::SemanticUnavailable);
            }
            let started = Instant::now();
            let vector = embeddings.encode_query(query).await?;
            let hits = embeddings::search_groups(
                pool,
                embeddings.model_id(),
                &vector,
                threshold,
                limit,
                offset,
            )
            .await?;
            let mut results = Vec::new();
            for hit in hits {
                let group = get_group(pool, user_id, hit.entity_id).await?;
                results.push(GroupSearchResultDto {
                    group,
                    score: hit.similarity,
                });
            }
            tracing::info!(
                model_id = embeddings.model_id(),
                elapsed_ms = started.elapsed().as_secs_f64() * 1000.0,
                mode = "semantic",
                result_count = results.len(),
                "group search completed"
            );
            Ok(GroupSearchResponse {
                mode: "semantic".into(),
                query: query.to_string(),
                results,
                limit,
                offset,
            })
        }
        _ => Err(AppError::validation("mode must be fullText or semantic")),
    }
}
