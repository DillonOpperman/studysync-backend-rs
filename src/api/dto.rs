use chrono::NaiveTime;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailabilityInput {
    pub day_of_week: i16,
    pub start_time: String,
    pub end_time: String,
    pub timezone: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupPreferencesInput {
    pub preferred_group_size_min: Option<i16>,
    pub preferred_group_size_max: Option<i16>,
    pub group_size: Option<i16>,
    pub session_duration_minutes: Option<i16>,
    pub session_duration: Option<i16>,
    #[serde(default)]
    pub study_goals: Vec<String>,
}

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
