use chrono::{Duration, NaiveDate, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;
use uuid::Uuid;

use crate::embed::cosine;
use crate::error::AppError;

pub const MATCH_THRESHOLD: f32 = 0.25;
pub const MATCH_TOP_K: usize = 10;
pub const W_GROUP_SUBJECT: f32 = 0.50;
pub const W_LEADER_SUBJECT: f32 = 0.20;
pub const W_SCHEDULE: f32 = 0.20;
pub const W_SIZE: f32 = 0.10;

#[derive(Debug, Clone)]
pub struct WeeklySlot {
    pub day_of_week: i16,
    pub start_time: NaiveTime,
    pub end_time: NaiveTime,
    pub timezone: String,
}

#[derive(Debug, Clone)]
pub struct MatchCandidate {
    pub group_id: Uuid,
    pub title: String,
    pub subject: String,
    pub institution_id: Uuid,
    pub leader_id: Uuid,
    pub max_members: i16,
    pub member_count: i16,
    pub leader_subjects: Vec<String>,
    pub slots: Vec<WeeklySlot>,
    pub group_embedding: Option<Vec<f32>>,
    pub is_leader: bool,
    pub is_active_member: bool,
    pub has_pending_request: bool,
}

#[derive(Debug, Clone)]
pub struct MatchQuery {
    pub user_id: Uuid,
    pub institution_id: Uuid,
    pub user_subjects: Vec<String>,
    pub user_embedding: Option<Vec<f32>>,
    pub slots: Vec<WeeklySlot>,
    pub min_size: i16,
    pub max_size: i16,
}

#[derive(Debug, Clone)]
pub struct ScoredGroup {
    pub group_id: Uuid,
    pub score: f32,
    pub group_subject: f32,
    pub leader_subject_similarity: f32,
    pub schedule_overlap: f32,
    pub size_match: f32,
}

#[derive(Debug, Clone)]
pub struct MatchResult {
    pub recommendations: Vec<ScoredGroup>,
    pub create_new: bool,
}

pub fn recommend(query: &MatchQuery, candidates: &[MatchCandidate]) -> MatchResult {
    let mut scored = Vec::new();
    for candidate in candidates {
        if candidate.institution_id != query.institution_id {
            continue;
        }
        if candidate.is_leader || candidate.is_active_member || candidate.has_pending_request {
            continue;
        }
        if candidate.leader_id == query.user_id {
            continue;
        }
        let group_subject = group_subject_score(
            &query.user_subjects,
            &candidate.subject,
            query.user_embedding.as_deref(),
            candidate.group_embedding.as_deref(),
        );
        let leader_subject_similarity =
            jaccard_similarity(&query.user_subjects, &candidate.leader_subjects);
        let schedule_overlap = schedule_overlap_ratio(&query.slots, &candidate.slots);
        let size_match = size_match_score(candidate.max_members, query.min_size, query.max_size);
        let score = W_GROUP_SUBJECT * group_subject
            + W_LEADER_SUBJECT * leader_subject_similarity
            + W_SCHEDULE * schedule_overlap
            + W_SIZE * size_match;
        if score >= MATCH_THRESHOLD {
            scored.push(ScoredGroup {
                group_id: candidate.group_id,
                score,
                group_subject,
                leader_subject_similarity,
                schedule_overlap,
                size_match,
            });
        }
    }
    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.group_id.cmp(&b.group_id))
    });
    scored.truncate(MATCH_TOP_K);
    let create_new = scored.is_empty();
    MatchResult {
        recommendations: scored,
        create_new,
    }
}

pub fn group_subject_score(
    user_subjects: &[String],
    group_subject: &str,
    user_embedding: Option<&[f32]>,
    group_embedding: Option<&[f32]>,
) -> f32 {
    let subject = group_subject.trim();
    if subject.is_empty() {
        return 0.0;
    }
    let lower = subject.to_lowercase();
    if user_subjects
        .iter()
        .any(|item| item.trim().eq_ignore_ascii_case(subject))
    {
        return 1.0;
    }
    if user_subjects.iter().any(|item| {
        let item = item.trim().to_lowercase();
        !item.is_empty() && (item.contains(&lower) || lower.contains(&item))
    }) {
        return 0.85;
    }
    match (user_embedding, group_embedding) {
        (Some(user), Some(group)) => cosine(user, group)
            .ok()
            .map(|value| value.max(0.0))
            .unwrap_or(0.0),
        _ => 0.0,
    }
}

pub fn jaccard_similarity(left: &[String], right: &[String]) -> f32 {
    let a = normalize_set(left);
    let b = normalize_set(right);
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let inter = a.iter().filter(|item| b.contains(*item)).count();
    let union = a.len() + b.len() - inter;
    if union == 0 {
        0.0
    } else {
        inter as f32 / union as f32
    }
}

fn normalize_set(values: &[String]) -> Vec<String> {
    let mut out: Vec<String> = values
        .iter()
        .map(|v| v.trim().to_lowercase())
        .filter(|v| !v.is_empty())
        .collect();
    out.sort();
    out.dedup();
    out
}

pub fn size_match_score(max_members: i16, min_size: i16, max_size: i16) -> f32 {
    if max_members >= min_size && max_members <= max_size {
        return 1.0;
    }
    let dist = if max_members < min_size {
        min_size - max_members
    } else {
        max_members - max_size
    };
    (1.0 - f32::from(dist) / 50.0).max(0.0)
}

pub fn schedule_overlap_ratio(user: &[WeeklySlot], group: &[WeeklySlot]) -> f32 {
    let user_ranges = expand_slots(user);
    let group_ranges = expand_slots(group);
    if user_ranges.is_empty() || group_ranges.is_empty() {
        return 0.0;
    }
    let mut overlap = 0i64;
    for (us, ue) in &user_ranges {
        for (gs, ge) in &group_ranges {
            let start = (*us).max(*gs);
            let end = (*ue).min(*ge);
            if end > start {
                overlap += (end - start).num_minutes();
            }
        }
    }
    let user_total: i64 = user_ranges
        .iter()
        .map(|(s, e)| (*e - *s).num_minutes().max(0))
        .sum();
    if user_total <= 0 {
        0.0
    } else {
        ((overlap as f32) / (user_total as f32)).clamp(0.0, 1.0)
    }
}

fn expand_slots(slots: &[WeeklySlot]) -> Vec<(chrono::DateTime<Utc>, chrono::DateTime<Utc>)> {
    let mut out = Vec::new();
    for slot in slots {
        let Ok(tz) = slot.timezone.parse::<Tz>() else {
            continue;
        };
        let monday = NaiveDate::from_ymd_opt(2024, 1, 1).expect("date");
        let date = monday + Duration::days(i64::from(slot.day_of_week));
        if slot.end_time > slot.start_time {
            if let Some(range) = local_range(date, slot.start_time, slot.end_time, tz) {
                out.push(range);
            }
        } else {
            let next = date + Duration::days(1);
            if let Some(range) = local_range(
                date,
                slot.start_time,
                NaiveTime::from_hms_opt(23, 59, 59).expect("time"),
                tz,
            ) {
                out.push(range);
            }
            if let Some(range) = local_range(
                next,
                NaiveTime::from_hms_opt(0, 0, 0).expect("time"),
                slot.end_time,
                tz,
            ) {
                out.push(range);
            }
        }
    }
    out
}

fn local_range(
    date: NaiveDate,
    start: NaiveTime,
    end: NaiveTime,
    tz: Tz,
) -> Option<(chrono::DateTime<Utc>, chrono::DateTime<Utc>)> {
    let start_local = tz.from_local_datetime(&date.and_time(start)).single()?;
    let end_local = tz.from_local_datetime(&date.and_time(end)).single()?;
    Some((
        start_local.with_timezone(&Utc),
        end_local.with_timezone(&Utc),
    ))
}

pub fn ensure_embedding_dim(vector: &[f32], expected: usize) -> Result<(), AppError> {
    if vector.len() != expected {
        Err(AppError::EmbeddingDimensionMismatch)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(day: i16, start: &str, end: &str) -> WeeklySlot {
        WeeklySlot {
            day_of_week: day,
            start_time: NaiveTime::parse_from_str(start, "%H:%M").unwrap(),
            end_time: NaiveTime::parse_from_str(end, "%H:%M").unwrap(),
            timezone: "America/Chicago".into(),
        }
    }

    fn id(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    #[test]
    fn exact_subject_scores_one() {
        assert_eq!(
            group_subject_score(
                &["Computer Networks".into()],
                "Computer Networks",
                None,
                None
            ),
            1.0
        );
    }

    #[test]
    fn substring_subject_scores_high() {
        assert_eq!(
            group_subject_score(&["Networks".into()], "Computer Networks", None, None),
            0.85
        );
    }

    #[test]
    fn semantic_subject_uses_cosine_when_no_string_match() {
        let user = vec![1.0, 0.0, 0.0];
        let group = vec![0.9, 0.1, 0.0];
        let unrelated = vec![0.0, 0.0, 1.0];
        let related = group_subject_score(
            &["Algorithms".into()],
            "Networking",
            Some(&user),
            Some(&group),
        );
        let other = group_subject_score(
            &["Algorithms".into()],
            "Orgo",
            Some(&user),
            Some(&unrelated),
        );
        assert!(related > other);
    }

    #[test]
    fn leader_jaccard_is_set_overlap() {
        let score = jaccard_similarity(
            &["Calc II".into(), "Networks".into()],
            &["networks".into(), "Physics".into()],
        );
        assert!((score - 1.0 / 3.0).abs() < 1e-6);
    }

    #[test]
    fn overnight_slots_overlap_after_midnight() {
        let user = vec![slot(1, "00:30", "02:00")];
        let group = vec![slot(0, "23:00", "01:00")];
        assert!(schedule_overlap_ratio(&user, &group) > 0.0);
    }

    #[test]
    fn disjoint_schedules_have_zero_overlap() {
        let user = vec![slot(1, "09:00", "10:00")];
        let group = vec![slot(2, "09:00", "10:00")];
        assert_eq!(schedule_overlap_ratio(&user, &group), 0.0);
    }

    #[test]
    fn size_match_in_range_is_one() {
        assert_eq!(size_match_score(6, 2, 8), 1.0);
        assert!(size_match_score(20, 2, 6) < 1.0);
    }

    #[test]
    fn exclusions_and_threshold_and_top_ten() {
        let campus = id(7);
        let query = MatchQuery {
            user_id: id(1),
            institution_id: campus,
            user_subjects: vec!["Computer Networks".into()],
            user_embedding: None,
            slots: vec![slot(0, "18:00", "20:00")],
            min_size: 2,
            max_size: 8,
        };
        let mut candidates = Vec::new();
        for n in 10u128..25 {
            candidates.push(MatchCandidate {
                group_id: id(n),
                title: "Networks".into(),
                subject: "Computer Networks".into(),
                institution_id: campus,
                leader_id: id(n + 100),
                max_members: 6,
                member_count: 2,
                leader_subjects: vec!["Computer Networks".into()],
                slots: vec![slot(0, "18:00", "20:00")],
                group_embedding: None,
                is_leader: false,
                is_active_member: false,
                has_pending_request: false,
            });
        }
        candidates[0].is_leader = true;
        candidates[1].is_active_member = true;
        candidates[2].has_pending_request = true;
        candidates.push(MatchCandidate {
            group_id: id(99),
            title: "Cooking".into(),
            subject: "Culinary Arts".into(),
            institution_id: campus,
            leader_id: id(500),
            max_members: 40,
            member_count: 2,
            leader_subjects: vec!["Baking".into()],
            slots: vec![slot(5, "08:00", "09:00")],
            group_embedding: None,
            is_leader: false,
            is_active_member: false,
            has_pending_request: false,
        });
        let result = recommend(&query, &candidates);
        assert!(!result.create_new);
        assert_eq!(result.recommendations.len(), 10);
        assert!(result
            .recommendations
            .iter()
            .all(|row| row.score >= MATCH_THRESHOLD));
        let ids: Vec<Uuid> = result.recommendations.iter().map(|r| r.group_id).collect();
        assert!(!ids.contains(&id(10)));
        assert!(!ids.contains(&id(11)));
        assert!(!ids.contains(&id(12)));
        assert!(!ids.contains(&id(99)));
    }

    #[test]
    fn create_new_when_nothing_passes_threshold() {
        let query = MatchQuery {
            user_id: id(1),
            institution_id: id(7),
            user_subjects: vec!["Basket Weaving".into()],
            user_embedding: None,
            slots: vec![],
            min_size: 2,
            max_size: 4,
        };
        let candidates = vec![MatchCandidate {
            group_id: id(2),
            title: "Orgo".into(),
            subject: "Organic Chemistry".into(),
            institution_id: id(7),
            leader_id: id(3),
            max_members: 40,
            member_count: 2,
            leader_subjects: vec!["Chemistry".into()],
            slots: vec![],
            group_embedding: None,
            is_leader: false,
            is_active_member: false,
            has_pending_request: false,
        }];
        let result = recommend(&query, &candidates);
        assert!(result.create_new);
        assert!(result.recommendations.is_empty());
    }
}
