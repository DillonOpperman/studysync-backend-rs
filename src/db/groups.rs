use chrono::{DateTime, NaiveTime, Utc};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::error::{is_unique_or_serialization, map_db_error, AppError};

pub const GROUP_STATUS_ACTIVE: &str = "active";
pub const GROUP_STATUS_ARCHIVED: &str = "archived";
pub const MEMBER_ROLE_LEADER: &str = "leader";
pub const MEMBER_ROLE_MEMBER: &str = "member";
pub const MEMBER_STATUS_ACTIVE: &str = "active";
pub const MEMBER_STATUS_ARCHIVED: &str = "archived";
pub const JOIN_PENDING: &str = "pending";
pub const JOIN_APPROVED: &str = "approved";
pub const JOIN_REJECTED: &str = "rejected";

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct StudyGroupRow {
    pub id: Uuid,
    pub title: String,
    pub subject: String,
    pub course_id: Option<Uuid>,
    pub description: String,
    pub location: String,
    pub max_members: i16,
    pub leader_id: Uuid,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RecurringSlotRow {
    pub id: Uuid,
    pub day_of_week: i16,
    pub start_time: NaiveTime,
    pub end_time: NaiveTime,
    pub timezone: String,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct GroupMemberRow {
    pub id: Uuid,
    pub group_id: Uuid,
    pub user_id: Uuid,
    pub role: String,
    pub status: String,
    pub joined_at: DateTime<Utc>,
    pub name: Option<String>,
    pub profile_picture_key: Option<String>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct JoinRequestRow {
    pub id: Uuid,
    pub group_id: Uuid,
    pub requester_id: Uuid,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub decided_at: Option<DateTime<Utc>>,
    pub requester_name: Option<String>,
    pub requester_picture_key: Option<String>,
}

pub async fn insert_group(
    tx: &mut Transaction<'_, Postgres>,
    row: &StudyGroupRow,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO study_groups (
            id, title, subject, course_id, description, location,
            max_members, leader_id, status, created_at, updated_at
         ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
    )
    .bind(row.id)
    .bind(&row.title)
    .bind(&row.subject)
    .bind(row.course_id)
    .bind(&row.description)
    .bind(&row.location)
    .bind(row.max_members)
    .bind(row.leader_id)
    .bind(&row.status)
    .bind(row.created_at)
    .bind(row.updated_at)
    .execute(&mut **tx)
    .await
    .map_err(map_db_error)?;
    Ok(())
}

pub async fn insert_slot(
    tx: &mut Transaction<'_, Postgres>,
    group_id: Uuid,
    slot: &(i16, NaiveTime, NaiveTime, String),
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO group_recurring_slots (
            id, group_id, day_of_week, start_time, end_time, timezone
         ) VALUES ($1,$2,$3,$4,$5,$6)",
    )
    .bind(Uuid::now_v7())
    .bind(group_id)
    .bind(slot.0)
    .bind(slot.1)
    .bind(slot.2)
    .bind(&slot.3)
    .execute(&mut **tx)
    .await
    .map_err(map_db_error)?;
    Ok(())
}

pub async fn insert_member(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    group_id: Uuid,
    user_id: Uuid,
    role: &str,
    status: &str,
    joined_at: DateTime<Utc>,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO group_members (id, group_id, user_id, role, status, joined_at)
         VALUES ($1,$2,$3,$4,$5,$6)",
    )
    .bind(id)
    .bind(group_id)
    .bind(user_id)
    .bind(role)
    .bind(status)
    .bind(joined_at)
    .execute(&mut **tx)
    .await
    .map_err(map_db_error)?;
    Ok(())
}

pub async fn lock_group(
    tx: &mut Transaction<'_, Postgres>,
    group_id: Uuid,
) -> Result<Option<StudyGroupRow>, AppError> {
    sqlx::query_as::<_, StudyGroupRow>(
        "SELECT id, title, subject, course_id, description, location,
                max_members, leader_id, status, created_at, updated_at
         FROM study_groups WHERE id = $1 FOR UPDATE",
    )
    .bind(group_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(map_db_error)
}

pub async fn get_group(
    executor: impl sqlx::Executor<'_, Database = Postgres>,
    group_id: Uuid,
) -> Result<Option<StudyGroupRow>, AppError> {
    sqlx::query_as::<_, StudyGroupRow>(
        "SELECT id, title, subject, course_id, description, location,
                max_members, leader_id, status, created_at, updated_at
         FROM study_groups WHERE id = $1",
    )
    .bind(group_id)
    .fetch_optional(executor)
    .await
    .map_err(map_db_error)
}

pub async fn count_active_members(
    executor: impl sqlx::Executor<'_, Database = Postgres>,
    group_id: Uuid,
) -> Result<i64, AppError> {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM group_members WHERE group_id = $1 AND status = 'active'",
    )
    .bind(group_id)
    .fetch_one(executor)
    .await
    .map_err(map_db_error)
}

pub async fn list_slots(
    executor: impl sqlx::Executor<'_, Database = Postgres>,
    group_id: Uuid,
) -> Result<Vec<RecurringSlotRow>, AppError> {
    sqlx::query_as::<_, RecurringSlotRow>(
        "SELECT id, day_of_week, start_time, end_time, timezone
         FROM group_recurring_slots
         WHERE group_id = $1
         ORDER BY day_of_week, start_time",
    )
    .bind(group_id)
    .fetch_all(executor)
    .await
    .map_err(map_db_error)
}

pub async fn share_active_group(
    executor: impl sqlx::Executor<'_, Database = Postgres>,
    user_a: Uuid,
    user_b: Uuid,
) -> Result<bool, AppError> {
    sqlx::query_scalar(
        "SELECT EXISTS (
            SELECT 1
            FROM group_members a
            JOIN group_members b ON b.group_id = a.group_id
            JOIN study_groups g ON g.id = a.group_id
            WHERE a.user_id = $1 AND b.user_id = $2
              AND a.status = 'active' AND b.status = 'active'
              AND g.status = 'active'
         )",
    )
    .bind(user_a)
    .bind(user_b)
    .fetch_one(executor)
    .await
    .map_err(map_db_error)
}

pub async fn user_is_active_group_leader(
    executor: impl sqlx::Executor<'_, Database = Postgres>,
    user_id: Uuid,
) -> Result<bool, AppError> {
    sqlx::query_scalar(
        "SELECT EXISTS (
            SELECT 1
            FROM group_members m
            JOIN study_groups g ON g.id = m.group_id
            WHERE m.user_id = $1
              AND m.role = 'leader'
              AND m.status = 'active'
              AND g.status = 'active'
         )",
    )
    .bind(user_id)
    .fetch_one(executor)
    .await
    .map_err(map_db_error)
}

pub async fn find_active_membership(
    executor: impl sqlx::Executor<'_, Database = Postgres>,
    group_id: Uuid,
    user_id: Uuid,
) -> Result<Option<GroupMemberRow>, AppError> {
    sqlx::query_as::<_, GroupMemberRow>(
        "SELECT m.id, m.group_id, m.user_id, m.role, m.status, m.joined_at,
                u.name, u.profile_picture_key
         FROM group_members m
         JOIN users u ON u.id = m.user_id
         WHERE m.group_id = $1 AND m.user_id = $2 AND m.status = 'active'",
    )
    .bind(group_id)
    .bind(user_id)
    .fetch_optional(executor)
    .await
    .map_err(map_db_error)
}

pub async fn find_membership(
    executor: impl sqlx::Executor<'_, Database = Postgres>,
    group_id: Uuid,
    user_id: Uuid,
) -> Result<Option<GroupMemberRow>, AppError> {
    sqlx::query_as::<_, GroupMemberRow>(
        "SELECT m.id, m.group_id, m.user_id, m.role, m.status, m.joined_at,
                u.name, u.profile_picture_key
         FROM group_members m
         JOIN users u ON u.id = m.user_id
         WHERE m.group_id = $1 AND m.user_id = $2",
    )
    .bind(group_id)
    .bind(user_id)
    .fetch_optional(executor)
    .await
    .map_err(map_db_error)
}

pub async fn list_my_groups(
    executor: impl sqlx::Executor<'_, Database = Postgres>,
    user_id: Uuid,
) -> Result<Vec<StudyGroupRow>, AppError> {
    sqlx::query_as::<_, StudyGroupRow>(
        "SELECT g.id, g.title, g.subject, g.course_id, g.description, g.location,
                g.max_members, g.leader_id, g.status, g.created_at, g.updated_at
         FROM study_groups g
         JOIN group_members m ON m.group_id = g.id
         WHERE m.user_id = $1 AND m.status = 'active'
         ORDER BY g.created_at DESC",
    )
    .bind(user_id)
    .fetch_all(executor)
    .await
    .map_err(map_db_error)
}

#[derive(Debug, Clone, sqlx::FromRow)]
#[allow(dead_code)]
pub struct ActiveGroupMatchRow {
    pub id: Uuid,
    pub title: String,
    pub subject: String,
    pub description: String,
    pub max_members: i16,
    pub leader_id: Uuid,
    pub leader_institution_id: Uuid,
    pub leader_name: String,
    pub leader_major: String,
    pub member_count: i64,
}

pub async fn list_active_groups_for_matching(
    executor: impl sqlx::Executor<'_, Database = Postgres>,
) -> Result<Vec<ActiveGroupMatchRow>, AppError> {
    sqlx::query_as::<_, ActiveGroupMatchRow>(
        "SELECT g.id, g.title, g.subject, g.description, g.max_members, g.leader_id,
                u.institution_id AS leader_institution_id,
                u.name AS leader_name, u.major AS leader_major,
                (SELECT COUNT(*) FROM group_members m
                  WHERE m.group_id = g.id AND m.status = 'active') AS member_count
         FROM study_groups g
         JOIN users u ON u.id = g.leader_id
         WHERE g.status = 'active'",
    )
    .fetch_all(executor)
    .await
    .map_err(map_db_error)
}

pub async fn list_active_members(
    executor: impl sqlx::Executor<'_, Database = Postgres>,
    group_id: Uuid,
) -> Result<Vec<GroupMemberRow>, AppError> {
    sqlx::query_as::<_, GroupMemberRow>(
        "SELECT m.id, m.group_id, m.user_id, m.role, m.status, m.joined_at,
                u.name, u.profile_picture_key
         FROM group_members m
         JOIN users u ON u.id = m.user_id
         WHERE m.group_id = $1 AND m.status = 'active'
         ORDER BY CASE WHEN m.role = 'leader' THEN 0 ELSE 1 END, m.joined_at",
    )
    .bind(group_id)
    .fetch_all(executor)
    .await
    .map_err(map_db_error)
}

pub async fn find_pending_request(
    executor: impl sqlx::Executor<'_, Database = Postgres>,
    group_id: Uuid,
    requester_id: Uuid,
) -> Result<Option<JoinRequestRow>, AppError> {
    sqlx::query_as::<_, JoinRequestRow>(
        "SELECT r.id, r.group_id, r.requester_id, r.status, r.created_at, r.decided_at,
                u.name AS requester_name, u.profile_picture_key AS requester_picture_key
         FROM group_join_requests r
         JOIN users u ON u.id = r.requester_id
         WHERE r.group_id = $1 AND r.requester_id = $2 AND r.status = 'pending'",
    )
    .bind(group_id)
    .bind(requester_id)
    .fetch_optional(executor)
    .await
    .map_err(map_db_error)
}

pub async fn insert_join_request(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    group_id: Uuid,
    requester_id: Uuid,
    created_at: DateTime<Utc>,
) -> Result<(), AppError> {
    sqlx::query("SAVEPOINT join_request_insert")
        .execute(&mut **tx)
        .await
        .map_err(map_db_error)?;
    let inserted = sqlx::query(
        "INSERT INTO group_join_requests (
            id, group_id, requester_id, status, created_at, decided_at
         ) VALUES ($1,$2,$3,'pending',$4,NULL)",
    )
    .bind(id)
    .bind(group_id)
    .bind(requester_id)
    .bind(created_at)
    .execute(&mut **tx)
    .await;
    match inserted {
        Ok(_) => {
            sqlx::query("RELEASE SAVEPOINT join_request_insert")
                .execute(&mut **tx)
                .await
                .map_err(map_db_error)?;
            Ok(())
        }
        Err(error) if is_unique_or_serialization(&error) => {
            sqlx::query("ROLLBACK TO SAVEPOINT join_request_insert")
                .execute(&mut **tx)
                .await
                .map_err(map_db_error)?;
            sqlx::query("RELEASE SAVEPOINT join_request_insert")
                .execute(&mut **tx)
                .await
                .map_err(map_db_error)?;
            Err(AppError::JoinRequestPending)
        }
        Err(error) => {
            let _ = sqlx::query("ROLLBACK TO SAVEPOINT join_request_insert")
                .execute(&mut **tx)
                .await;
            Err(map_db_error(error))
        }
    }
}

pub async fn lock_join_request(
    tx: &mut Transaction<'_, Postgres>,
    group_id: Uuid,
    request_id: Uuid,
) -> Result<Option<JoinRequestRow>, AppError> {
    sqlx::query_as::<_, JoinRequestRow>(
        "SELECT r.id, r.group_id, r.requester_id, r.status, r.created_at, r.decided_at,
                u.name AS requester_name, u.profile_picture_key AS requester_picture_key
         FROM group_join_requests r
         JOIN users u ON u.id = r.requester_id
         WHERE r.id = $1 AND r.group_id = $2
         FOR UPDATE OF r",
    )
    .bind(request_id)
    .bind(group_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(map_db_error)
}

pub async fn decide_join_request(
    tx: &mut Transaction<'_, Postgres>,
    request_id: Uuid,
    status: &str,
    decided_at: DateTime<Utc>,
) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE group_join_requests
         SET status = $2, decided_at = $3
         WHERE id = $1 AND status = 'pending'",
    )
    .bind(request_id)
    .bind(status)
    .bind(decided_at)
    .execute(&mut **tx)
    .await
    .map_err(map_db_error)?;
    Ok(())
}

pub async fn list_pending_requests(
    executor: impl sqlx::Executor<'_, Database = Postgres>,
    group_id: Uuid,
) -> Result<Vec<JoinRequestRow>, AppError> {
    sqlx::query_as::<_, JoinRequestRow>(
        "SELECT r.id, r.group_id, r.requester_id, r.status, r.created_at, r.decided_at,
                u.name AS requester_name, u.profile_picture_key AS requester_picture_key
         FROM group_join_requests r
         JOIN users u ON u.id = r.requester_id
         WHERE r.group_id = $1 AND r.status = 'pending'
         ORDER BY r.created_at",
    )
    .bind(group_id)
    .fetch_all(executor)
    .await
    .map_err(map_db_error)
}

pub async fn upsert_active_member(
    tx: &mut Transaction<'_, Postgres>,
    group_id: Uuid,
    user_id: Uuid,
    now: DateTime<Utc>,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO group_members (id, group_id, user_id, role, status, joined_at)
         VALUES ($1,$2,$3,'member','active',$4)
         ON CONFLICT (group_id, user_id) DO UPDATE SET
            role = 'member',
            status = 'active',
            joined_at = EXCLUDED.joined_at
         WHERE group_members.status = 'archived'",
    )
    .bind(Uuid::now_v7())
    .bind(group_id)
    .bind(user_id)
    .bind(now)
    .execute(&mut **tx)
    .await
    .map_err(map_db_error)?;
    Ok(())
}

pub async fn archive_membership(
    tx: &mut Transaction<'_, Postgres>,
    group_id: Uuid,
    user_id: Uuid,
) -> Result<u64, AppError> {
    let result = sqlx::query(
        "UPDATE group_members SET status = 'archived'
         WHERE group_id = $1 AND user_id = $2 AND status = 'active'",
    )
    .bind(group_id)
    .bind(user_id)
    .execute(&mut **tx)
    .await
    .map_err(map_db_error)?;
    Ok(result.rows_affected())
}

pub async fn archive_group(
    tx: &mut Transaction<'_, Postgres>,
    group_id: Uuid,
    now: DateTime<Utc>,
) -> Result<(), AppError> {
    sqlx::query("UPDATE study_groups SET status = 'archived', updated_at = $2 WHERE id = $1")
        .bind(group_id)
        .bind(now)
        .execute(&mut **tx)
        .await
        .map_err(map_db_error)?;
    sqlx::query(
        "UPDATE group_join_requests
         SET status = 'rejected', decided_at = $2
         WHERE group_id = $1 AND status = 'pending'",
    )
    .bind(group_id)
    .bind(now)
    .execute(&mut **tx)
    .await
    .map_err(map_db_error)?;
    Ok(())
}

pub async fn course_exists(
    executor: impl sqlx::Executor<'_, Database = Postgres>,
    course_id: Uuid,
) -> Result<bool, AppError> {
    let found: Option<Uuid> = sqlx::query_scalar("SELECT id FROM courses WHERE id = $1")
        .bind(course_id)
        .fetch_optional(executor)
        .await
        .map_err(map_db_error)?;
    Ok(found.is_some())
}
