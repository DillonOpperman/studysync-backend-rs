use std::time::Instant;

use sqlx::PgPool;
use uuid::Uuid;

use crate::api::dto::{GroupDto, RecommendResponse};
use crate::db::{embeddings, groups, identity, profile};
use crate::embed::EmbeddingService;
use crate::error::AppError;
use crate::matching::{self, MatchCandidate, MatchQuery, WeeklySlot};
use crate::services::groups::get_group;

pub async fn recommend_groups(
    pool: &PgPool,
    embeddings: &EmbeddingService,
    user_id: Uuid,
) -> Result<RecommendResponse, AppError> {
    let started = Instant::now();
    let mut tx = pool.begin().await.map_err(crate::error::map_db_error)?;
    let user = identity::get_user_by_id(&mut *tx, user_id)
        .await?
        .filter(|row| row.deleted_at.is_none())
        .ok_or(AppError::Unauthorized)?;

    let courses = profile::load_courses(&mut *tx, user_id).await?;
    let user_subjects: Vec<String> = courses.into_iter().map(|c| c.name).collect();
    let prefs = profile::load_preferences(&mut *tx, user_id).await?;
    let (min_size, max_size) = (
        prefs.preferred_group_size_min,
        prefs.preferred_group_size_max,
    );
    let user_slots = profile::load_availability(&mut *tx, user_id)
        .await?
        .into_iter()
        .map(|row| WeeklySlot {
            day_of_week: row.day_of_week,
            start_time: row.start_time,
            end_time: row.end_time,
            timezone: row.timezone,
        })
        .collect::<Vec<_>>();

    let user_embedding = if embeddings.semantic_available() {
        embeddings::load_vector(&mut *tx, "user", user_id, embeddings.model_id()).await?
    } else {
        None
    };

    let rows = groups::list_active_groups_for_matching(&mut *tx).await?;
    let mut candidates = Vec::with_capacity(rows.len());
    for row in rows {
        let slots = groups::list_slots(&mut *tx, row.id)
            .await?
            .into_iter()
            .map(|slot| WeeklySlot {
                day_of_week: slot.day_of_week,
                start_time: slot.start_time,
                end_time: slot.end_time,
                timezone: slot.timezone,
            })
            .collect();
        let leader_courses = profile::load_courses(&mut *tx, row.leader_id).await?;
        let membership = groups::find_active_membership(&mut *tx, row.id, user_id).await?;
        let pending = groups::find_pending_request(&mut *tx, row.id, user_id).await?;
        let group_embedding = if embeddings.semantic_available() {
            embeddings::load_vector(&mut *tx, "group", row.id, embeddings.model_id()).await?
        } else {
            None
        };
        candidates.push(MatchCandidate {
            group_id: row.id,
            title: row.title,
            subject: row.subject,
            institution_id: row.leader_institution_id,
            leader_id: row.leader_id,
            max_members: row.max_members,
            member_count: row.member_count as i16,
            leader_subjects: leader_courses.into_iter().map(|c| c.name).collect(),
            slots,
            group_embedding,
            is_leader: membership
                .as_ref()
                .is_some_and(|m| m.role == groups::MEMBER_ROLE_LEADER),
            is_active_member: membership.is_some(),
            has_pending_request: pending.is_some(),
        });
    }
    tx.commit().await.map_err(crate::error::map_db_error)?;

    let result = matching::recommend(
        &MatchQuery {
            user_id,
            institution_id: user.institution_id,
            user_subjects,
            user_embedding,
            slots: user_slots,
            min_size,
            max_size,
        },
        &candidates,
    );

    let mut groups_out = Vec::with_capacity(result.recommendations.len());
    for scored in &result.recommendations {
        groups_out.push(RecommendGroup {
            group: get_group(pool, user_id, scored.group_id).await?,
            score: scored.score,
            group_subject: scored.group_subject,
            leader_subject_similarity: scored.leader_subject_similarity,
            schedule_overlap: scored.schedule_overlap,
            size_match: scored.size_match,
        });
    }

    tracing::info!(
        model_id = embeddings.model_id(),
        elapsed_ms = started.elapsed().as_secs_f64() * 1000.0,
        semantic_available = embeddings.semantic_available(),
        result_count = groups_out.len(),
        "recommendation completed"
    );

    Ok(RecommendResponse {
        groups: groups_out.into_iter().map(|row| row.into_dto()).collect(),
        create_new: result.create_new,
        formula: "0.50*group_subject + 0.20*leader_subject_similarity + 0.20*schedule_overlap + 0.10*size_match",
        model_id: embeddings.model_id().to_string(),
        semantic_available: embeddings.semantic_available(),
    })
}

struct RecommendGroup {
    group: GroupDto,
    score: f32,
    group_subject: f32,
    leader_subject_similarity: f32,
    schedule_overlap: f32,
    size_match: f32,
}

impl RecommendGroup {
    fn into_dto(self) -> crate::api::dto::RecommendedGroupDto {
        crate::api::dto::RecommendedGroupDto {
            group: self.group,
            score: self.score,
            group_subject: self.group_subject,
            leader_subject_similarity: self.leader_subject_similarity,
            schedule_overlap: self.schedule_overlap,
            size_match: self.size_match,
        }
    }
}
