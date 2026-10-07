use chrono::NaiveTime;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::{AvailabilityInput, GroupPreferencesInput};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterRequest {
    pub name: String,
    pub email: String,
    pub password: String,
    pub institution_id: Option<Uuid>,
    pub institution_name: Option<String>,
    pub institution_domain: Option<String>,
    pub major: String,
    pub year: String,
    pub learning_style: Option<String>,
    #[serde(default)]
    pub study_environments: Vec<String>,
    #[serde(default)]
    pub study_methods: Vec<String>,
    #[serde(default)]
    pub subjects: Vec<String>,
    #[serde(default)]
    pub availability: Vec<AvailabilityInput>,
    pub performance_level: Option<i16>,
    pub group_preferences: Option<GroupPreferencesInput>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshRequest {
    pub refresh_token: String,
}

/// Partial profile update. `profilePictureKey` is a storage-key placeholder only.
/// Future media APIs must mint keys after an authenticated upload; presenting a
/// key here must never authorize object-storage GET/PUT on its own.
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PatchMeRequest {
    pub name: Option<String>,
    pub institution_id: Option<Uuid>,
    pub institution_name: Option<String>,
    pub institution_domain: Option<String>,
    pub major: Option<String>,
    pub year: Option<String>,
    pub learning_style: Option<String>,
    pub study_environments: Option<Vec<String>>,
    pub study_methods: Option<Vec<String>>,
    pub subjects: Option<Vec<String>>,
    pub availability: Option<Vec<AvailabilityInput>>,
    pub performance_level: Option<i16>,
    pub group_preferences: Option<GroupPreferencesInput>,
    pub profile_picture_key: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub user: UserProfileDto,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserProfileDto {
    pub id: Uuid,
    pub name: String,
    pub email: String,
    pub institution: InstitutionDto,
    pub major: String,
    pub year: String,
    pub learning_style: String,
    pub performance_level: i16,
    pub profile_picture_key: Option<String>,
    pub study_environments: Vec<String>,
    pub study_methods: Vec<String>,
    pub study_goals: Vec<String>,
    pub subjects: Vec<CourseDto>,
    pub availability: Vec<AvailabilityDto>,
    pub group_preferences: GroupPreferencesDto,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstitutionDto {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub domain: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CourseDto {
    pub id: Uuid,
    pub name: String,
    pub course_code: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailabilityDto {
    pub day_of_week: i16,
    pub start_time: NaiveTime,
    pub end_time: NaiveTime,
    pub timezone: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupPreferencesDto {
    pub preferred_group_size_min: i16,
    pub preferred_group_size_max: i16,
    pub session_duration_minutes: i16,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateGroupRequest {
    pub title: String,
    pub subject: String,
    pub course_id: Option<Uuid>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub location: String,
    pub max_members: i16,
    #[serde(default)]
    pub recurring_slots: Vec<AvailabilityInput>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserSummaryDto {
    pub id: Uuid,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_picture_key: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecurringSlotDto {
    pub id: Uuid,
    pub day_of_week: i16,
    pub start_time: NaiveTime,
    pub end_time: NaiveTime,
    pub timezone: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupDto {
    pub id: Uuid,
    pub title: String,
    pub subject: String,
    pub course_id: Option<Uuid>,
    pub description: String,
    pub location: String,
    pub max_members: i16,
    pub member_count: i64,
    pub leader: UserSummaryDto,
    pub status: String,
    pub membership_status: String,
    pub pending_join_request: bool,
    pub is_leader: bool,
    pub recurring_slots: Vec<RecurringSlotDto>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupMemberDto {
    pub user_id: Uuid,
    pub name: String,
    pub profile_picture_key: Option<String>,
    pub role: String,
    pub joined_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JoinRequestDto {
    pub id: Uuid,
    pub group_id: Uuid,
    pub status: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub decided_at: Option<chrono::DateTime<chrono::Utc>>,
    pub requester: UserSummaryDto,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationListQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationDto {
    pub id: Uuid,
    #[serde(rename = "type")]
    pub ntype: String,
    pub title: String,
    pub body: String,
    pub data: serde_json::Value,
    pub read_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}
