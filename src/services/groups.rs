use chrono::Utc;
use sqlx::PgPool;
use uuid::Uuid;

use crate::api::dto::{
    CreateGroupRequest, GroupDto, GroupMemberDto, JoinRequestDto, RecurringSlotDto, UserSummaryDto,
};
use crate::auth::group::{require_group_leader, require_group_member};
use crate::db::groups::{self, JoinRequestRow, RecurringSlotRow, StudyGroupRow};
use crate::db::identity;
use crate::db::notifications;
use crate::domain;
use crate::error::AppError;

const MIN_MAX_MEMBERS: i16 = 2;
const MAX_MAX_MEMBERS: i16 = 50;

/// Leadership transfer is not implemented in Phase 3. A leader with other
/// active members cannot leave. A leader who is the only active member may
/// leave; that archives the group (and pending join requests) instead of
/// deleting it.
pub async fn create_group(
    pool: &PgPool,
    leader_id: Uuid,
    request: CreateGroupRequest,
) -> Result<GroupDto, AppError> {
    let title = domain::require_nonempty(&request.title, "title")?;
    let subject = domain::require_nonempty(&request.subject, "subject")?;
    if request.max_members < MIN_MAX_MEMBERS || request.max_members > MAX_MAX_MEMBERS {
        return Err(AppError::validation("maxMembers must be between 2 and 50"));
    }
    let slots = domain::parse_availability(&request.recurring_slots)?;
    let now = Utc::now();
    let group_id = Uuid::now_v7();

    let mut tx = pool.begin().await.map_err(crate::error::map_db_error)?;
    identity::get_user_by_id(&mut *tx, leader_id)
        .await?
        .filter(|row| row.deleted_at.is_none())
        .ok_or(AppError::Unauthorized)?;

    if let Some(course_id) = request.course_id {
        if !groups::course_exists(&mut *tx, course_id).await? {
            return Err(AppError::validation("courseId does not exist"));
        }
    }

    groups::insert_group(
        &mut tx,
        &StudyGroupRow {
            id: group_id,
            title,
            subject,
            course_id: request.course_id,
            description: request.description.trim().to_string(),
            location: request.location.trim().to_string(),
            max_members: request.max_members,
            leader_id,
            status: groups::GROUP_STATUS_ACTIVE.to_string(),
            created_at: now,
            updated_at: now,
        },
    )
    .await?;
    groups::insert_member(
        &mut tx,
        Uuid::now_v7(),
        group_id,
        leader_id,
        groups::MEMBER_ROLE_LEADER,
        groups::MEMBER_STATUS_ACTIVE,
        now,
    )
    .await?;
    for slot in &slots {
        groups::insert_slot(&mut tx, group_id, slot).await?;
    }

    let dto = assemble_group_dto(&mut tx, group_id, leader_id).await?;
    tx.commit().await.map_err(crate::error::map_db_error)?;
    Ok(dto)
}

pub async fn list_mine(pool: &PgPool, user_id: Uuid) -> Result<Vec<GroupDto>, AppError> {
    let mut tx = pool.begin().await.map_err(crate::error::map_db_error)?;
    let rows = groups::list_my_groups(&mut *tx, user_id).await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(assemble_group_dto(&mut tx, row.id, user_id).await?);
    }
    tx.commit().await.map_err(crate::error::map_db_error)?;
    Ok(out)
}

pub async fn get_group(pool: &PgPool, user_id: Uuid, group_id: Uuid) -> Result<GroupDto, AppError> {
    let mut tx = pool.begin().await.map_err(crate::error::map_db_error)?;
    let dto = assemble_group_dto(&mut tx, group_id, user_id).await?;
    tx.commit().await.map_err(crate::error::map_db_error)?;
    Ok(dto)
}

pub async fn create_join_request(
    pool: &PgPool,
    user_id: Uuid,
    group_id: Uuid,
) -> Result<JoinRequestDto, AppError> {
    let now = Utc::now();
    let request_id = Uuid::now_v7();
    let mut tx = pool.begin().await.map_err(crate::error::map_db_error)?;
    identity::get_user_by_id(&mut *tx, user_id)
        .await?
        .filter(|row| row.deleted_at.is_none())
        .ok_or(AppError::Unauthorized)?;

    let group = groups::lock_group(&mut tx, group_id)
        .await?
        .ok_or(AppError::GroupNotFound)?;
    if group.status != groups::GROUP_STATUS_ACTIVE {
        return Err(AppError::GroupArchived);
    }
    if group.leader_id == user_id {
        return Err(AppError::AlreadyMember);
    }
    if groups::find_active_membership(&mut *tx, group_id, user_id)
        .await?
        .is_some()
    {
        return Err(AppError::AlreadyMember);
    }
    if groups::find_pending_request(&mut *tx, group_id, user_id)
        .await?
        .is_some()
    {
        return Err(AppError::JoinRequestPending);
    }
    let members = groups::count_active_members(&mut *tx, group_id).await?;
    if members >= i64::from(group.max_members) {
        return Err(AppError::GroupFull);
    }

    groups::insert_join_request(&mut tx, request_id, group_id, user_id, now).await?;
    notifications::insert_notification(
        &mut tx,
        group.leader_id,
        "join_request",
        "New join request",
        "Someone requested to join your study group.",
        serde_json::json!({
            "groupId": group_id,
            "requestId": request_id,
            "requesterId": user_id
        }),
    )
    .await?;

    let row = groups::find_pending_request(&mut *tx, group_id, user_id)
        .await?
        .ok_or(AppError::Internal)?;
    tx.commit().await.map_err(crate::error::map_db_error)?;
    Ok(join_request_dto(&row))
}

pub async fn approve_join_request(
    pool: &PgPool,
    leader_id: Uuid,
    group_id: Uuid,
    request_id: Uuid,
) -> Result<JoinRequestDto, AppError> {
    let now = Utc::now();
    let mut tx = pool.begin().await.map_err(crate::error::map_db_error)?;
    let group = groups::lock_group(&mut tx, group_id)
        .await?
        .ok_or(AppError::GroupNotFound)?;
    require_group_leader(&mut tx, group_id, leader_id).await?;
    if group.status != groups::GROUP_STATUS_ACTIVE {
        return Err(AppError::GroupArchived);
    }

    let request = groups::lock_join_request(&mut tx, group_id, request_id)
        .await?
        .ok_or(AppError::JoinRequestNotFound)?;
    if request.status != groups::JOIN_PENDING {
        return Err(AppError::JoinRequestDecided);
    }

    identity::get_user_by_id(&mut *tx, request.requester_id)
        .await?
        .filter(|row| row.deleted_at.is_none())
        .ok_or(AppError::UserNotFound)?;

    if groups::find_active_membership(&mut *tx, group_id, request.requester_id)
        .await?
        .is_some()
    {
        return Err(AppError::AlreadyMember);
    }

    let members = groups::count_active_members(&mut *tx, group_id).await?;
    if members >= i64::from(group.max_members) {
        return Err(AppError::GroupFull);
    }

    groups::upsert_active_member(&mut tx, group_id, request.requester_id, now).await?;
    groups::decide_join_request(&mut tx, request_id, groups::JOIN_APPROVED, now).await?;
    notifications::insert_notification(
        &mut tx,
        request.requester_id,
        "request_approved",
        "Join request approved",
        "You were added to a study group.",
        serde_json::json!({ "groupId": group_id, "requestId": request_id }),
    )
    .await?;

    let row = groups::lock_join_request(&mut tx, group_id, request_id)
        .await?
        .ok_or(AppError::Internal)?;
    tx.commit().await.map_err(crate::error::map_db_error)?;
    Ok(join_request_dto(&row))
}

pub async fn reject_join_request(
    pool: &PgPool,
    leader_id: Uuid,
    group_id: Uuid,
    request_id: Uuid,
) -> Result<JoinRequestDto, AppError> {
    let now = Utc::now();
    let mut tx = pool.begin().await.map_err(crate::error::map_db_error)?;
    let group = groups::lock_group(&mut tx, group_id)
        .await?
        .ok_or(AppError::GroupNotFound)?;
    require_group_leader(&mut tx, group_id, leader_id).await?;
    let request = groups::lock_join_request(&mut tx, group_id, request_id)
        .await?
        .ok_or(AppError::JoinRequestNotFound)?;
    if request.status != groups::JOIN_PENDING {
        return Err(AppError::JoinRequestDecided);
    }

    groups::decide_join_request(&mut tx, request_id, groups::JOIN_REJECTED, now).await?;
    notifications::insert_notification(
        &mut tx,
        request.requester_id,
        "request_rejected",
        "Join request declined",
        "Your request to join a study group was declined.",
        serde_json::json!({ "groupId": group_id, "requestId": request_id }),
    )
    .await?;
    let _ = group;

    let row = groups::lock_join_request(&mut tx, group_id, request_id)
        .await?
        .ok_or(AppError::Internal)?;
    tx.commit().await.map_err(crate::error::map_db_error)?;
    Ok(join_request_dto(&row))
}

pub async fn list_join_requests(
    pool: &PgPool,
    leader_id: Uuid,
    group_id: Uuid,
) -> Result<Vec<JoinRequestDto>, AppError> {
    let mut tx = pool.begin().await.map_err(crate::error::map_db_error)?;
    groups::get_group(&mut *tx, group_id)
        .await?
        .ok_or(AppError::GroupNotFound)?;
    require_group_leader(&mut tx, group_id, leader_id).await?;
    let rows = groups::list_pending_requests(&mut *tx, group_id).await?;
    tx.commit().await.map_err(crate::error::map_db_error)?;
    Ok(rows.iter().map(join_request_dto).collect())
}

pub async fn list_members(
    pool: &PgPool,
    user_id: Uuid,
    group_id: Uuid,
) -> Result<Vec<GroupMemberDto>, AppError> {
    let mut tx = pool.begin().await.map_err(crate::error::map_db_error)?;
    groups::get_group(&mut *tx, group_id)
        .await?
        .ok_or(AppError::GroupNotFound)?;
    require_group_member(&mut tx, group_id, user_id).await?;
    let rows = groups::list_active_members(&mut *tx, group_id).await?;
    tx.commit().await.map_err(crate::error::map_db_error)?;
    Ok(rows
        .into_iter()
        .map(|row| GroupMemberDto {
            user_id: row.user_id,
            name: row.name.unwrap_or_default(),
            profile_picture_key: row.profile_picture_key,
            role: row.role,
            joined_at: row.joined_at,
        })
        .collect())
}

pub async fn leave_group(pool: &PgPool, user_id: Uuid, group_id: Uuid) -> Result<(), AppError> {
    let now = Utc::now();
    let mut tx = pool.begin().await.map_err(crate::error::map_db_error)?;
    let group = groups::lock_group(&mut tx, group_id)
        .await?
        .ok_or(AppError::GroupNotFound)?;
    let membership = require_group_member(&mut tx, group_id, user_id).await?;
    let active = groups::count_active_members(&mut *tx, group_id).await?;

    if membership.role == groups::MEMBER_ROLE_LEADER {
        if active > 1 {
            return Err(AppError::CannotLeaveAsLeader);
        }
        groups::archive_membership(&mut tx, group_id, user_id).await?;
        groups::archive_group(&mut tx, group_id, now).await?;
    } else {
        groups::archive_membership(&mut tx, group_id, user_id).await?;
    }
    let _ = group;
    tx.commit().await.map_err(crate::error::map_db_error)?;
    Ok(())
}

pub async fn remove_member(
    pool: &PgPool,
    leader_id: Uuid,
    group_id: Uuid,
    target_user_id: Uuid,
) -> Result<(), AppError> {
    let mut tx = pool.begin().await.map_err(crate::error::map_db_error)?;
    groups::lock_group(&mut tx, group_id)
        .await?
        .ok_or(AppError::GroupNotFound)?;
    require_group_leader(&mut tx, group_id, leader_id).await?;
    if target_user_id == leader_id {
        return Err(AppError::CannotRemoveLeader);
    }
    let membership = groups::find_membership(&mut *tx, group_id, target_user_id)
        .await?
        .ok_or(AppError::NotGroupMember)?;
    if membership.role == groups::MEMBER_ROLE_LEADER {
        return Err(AppError::CannotRemoveLeader);
    }
    if membership.status != groups::MEMBER_STATUS_ACTIVE {
        return Err(AppError::NotGroupMember);
    }
    groups::archive_membership(&mut tx, group_id, target_user_id).await?;
    tx.commit().await.map_err(crate::error::map_db_error)?;
    Ok(())
}

pub async fn archive_group(
    pool: &PgPool,
    leader_id: Uuid,
    group_id: Uuid,
) -> Result<GroupDto, AppError> {
    let now = Utc::now();
    let mut tx = pool.begin().await.map_err(crate::error::map_db_error)?;
    let group = groups::lock_group(&mut tx, group_id)
        .await?
        .ok_or(AppError::GroupNotFound)?;
    require_group_leader(&mut tx, group_id, leader_id).await?;
    if group.status != groups::GROUP_STATUS_ACTIVE {
        return Err(AppError::GroupArchived);
    }
    groups::archive_group(&mut tx, group_id, now).await?;
    let dto = assemble_group_dto(&mut tx, group_id, leader_id).await?;
    tx.commit().await.map_err(crate::error::map_db_error)?;
    Ok(dto)
}

async fn assemble_group_dto(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    group_id: Uuid,
    viewer_id: Uuid,
) -> Result<GroupDto, AppError> {
    let group = groups::get_group(&mut **tx, group_id)
        .await?
        .ok_or(AppError::GroupNotFound)?;
    let leader = identity::get_user_by_id(&mut **tx, group.leader_id)
        .await?
        .ok_or(AppError::Internal)?;
    let member_count = groups::count_active_members(&mut **tx, group_id).await?;
    let slots = groups::list_slots(&mut **tx, group_id).await?;
    let membership = groups::find_active_membership(&mut **tx, group_id, viewer_id).await?;
    let pending = groups::find_pending_request(&mut **tx, group_id, viewer_id)
        .await?
        .is_some();
    let is_leader = membership
        .as_ref()
        .is_some_and(|row| row.role == groups::MEMBER_ROLE_LEADER);
    let membership_status = match membership.as_ref().map(|row| row.role.as_str()) {
        Some(groups::MEMBER_ROLE_LEADER) => "leader",
        Some(_) => "member",
        None => "none",
    };

    Ok(GroupDto {
        id: group.id,
        title: group.title,
        subject: group.subject,
        course_id: group.course_id,
        description: group.description,
        location: group.location,
        max_members: group.max_members,
        member_count,
        leader: UserSummaryDto {
            id: leader.id,
            name: leader.name,
            profile_picture_key: leader.profile_picture_key,
        },
        status: group.status,
        membership_status: membership_status.to_string(),
        pending_join_request: pending,
        is_leader,
        recurring_slots: slots.iter().map(slot_dto).collect(),
        created_at: group.created_at,
        updated_at: group.updated_at,
    })
}

fn slot_dto(row: &RecurringSlotRow) -> RecurringSlotDto {
    RecurringSlotDto {
        id: row.id,
        day_of_week: row.day_of_week,
        start_time: row.start_time,
        end_time: row.end_time,
        timezone: row.timezone.clone(),
    }
}

fn join_request_dto(row: &JoinRequestRow) -> JoinRequestDto {
    JoinRequestDto {
        id: row.id,
        group_id: row.group_id,
        status: row.status.clone(),
        created_at: row.created_at,
        decided_at: row.decided_at,
        requester: UserSummaryDto {
            id: row.requester_id,
            name: row.requester_name.clone().unwrap_or_default(),
            profile_picture_key: row.requester_picture_key.clone(),
        },
    }
}
