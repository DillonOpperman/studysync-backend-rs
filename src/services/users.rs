use chrono::NaiveTime;
use chrono::Utc;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::api::dto::{
    AvailabilityDto, CourseDto, GroupPreferencesDto, InstitutionDto, PatchMeRequest, UserProfileDto,
};
use crate::db::identity::{self, InstitutionRow, UserRow};
use crate::db::profile::{self, PreferenceRow};
use crate::domain::{self, Preferences};
use crate::error::AppError;

pub async fn resolve_institution(
    tx: &mut Transaction<'_, Postgres>,
    institution_id: Option<Uuid>,
    institution_name: Option<&str>,
    institution_domain: Option<&str>,
) -> Result<InstitutionRow, AppError> {
    if let Some(id) = institution_id {
        return identity::get_institution(&mut **tx, id)
            .await?
            .ok_or_else(|| AppError::validation("institutionId does not exist"));
    }
    let name = institution_name
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AppError::validation("institutionName or institutionId is required"))?;
    if let Some(existing) = identity::find_institution_by_name(tx, name).await? {
        return Ok(existing);
    }
    let mut slug = domain::slugify(name);
    if slug.is_empty() {
        return Err(AppError::validation("institutionName is invalid"));
    }
    slug = format!("{slug}-{}", &Uuid::now_v7().to_string()[..8]);
    let domain = institution_domain
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase);
    identity::insert_institution(tx, Uuid::now_v7(), name, &slug, domain.as_deref()).await
}

pub struct ProfileGraph<'a> {
    pub prefs: &'a Preferences,
    pub availability: &'a [(i16, NaiveTime, NaiveTime, String)],
    pub subjects: &'a [String],
    pub environments: &'a [String],
    pub methods: &'a [String],
    pub goals: &'a [String],
}

pub async fn write_profile_graph(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    institution_id: Uuid,
    graph: ProfileGraph<'_>,
) -> Result<(), AppError> {
    profile::upsert_preferences(
        tx,
        user_id,
        graph.prefs.min_size,
        graph.prefs.max_size,
        graph.prefs.session_minutes,
    )
    .await?;
    profile::replace_availability(tx, user_id, graph.availability).await?;
    let mut course_ids = Vec::new();
    for subject in graph.subjects {
        course_ids.push(profile::find_or_create_course(tx, institution_id, subject).await?);
    }
    profile::replace_user_courses(tx, user_id, &course_ids).await?;
    profile::replace_environments(tx, user_id, graph.environments).await?;
    profile::replace_methods(tx, user_id, graph.methods).await?;
    profile::replace_goals(tx, user_id, graph.goals).await?;
    Ok(())
}

pub async fn assemble_profile(pool: &PgPool, user_id: Uuid) -> Result<UserProfileDto, AppError> {
    let mut tx = pool.begin().await.map_err(crate::error::map_db_error)?;
    let profile = assemble_profile_in_tx(&mut tx, user_id).await?;
    tx.commit().await.map_err(crate::error::map_db_error)?;
    Ok(profile)
}

pub async fn assemble_profile_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
) -> Result<UserProfileDto, AppError> {
    let user = identity::get_user_by_id(&mut **tx, user_id)
        .await?
        .filter(|row| row.deleted_at.is_none())
        .ok_or(AppError::UserNotFound)?;
    let institution = identity::get_institution(&mut **tx, user.institution_id)
        .await?
        .ok_or(AppError::Internal)?;
    let prefs = profile::load_preferences(&mut **tx, user_id).await?;
    to_dto(tx, user, institution, prefs).await
}

async fn to_dto(
    tx: &mut Transaction<'_, Postgres>,
    user: UserRow,
    institution: InstitutionRow,
    prefs: PreferenceRow,
) -> Result<UserProfileDto, AppError> {
    let courses = profile::load_courses(&mut **tx, user.id).await?;
    let availability = profile::load_availability(&mut **tx, user.id).await?;
    let environments = profile::load_names(
        &mut **tx,
        "SELECT e.id, e.name FROM study_environments e
         JOIN user_study_environments ue ON ue.environment_id = e.id
         WHERE ue.user_id = $1 ORDER BY e.name",
        user.id,
    )
    .await?;
    let methods = profile::load_names(
        &mut **tx,
        "SELECT m.id, m.name FROM study_methods m
         JOIN user_study_methods um ON um.method_id = m.id
         WHERE um.user_id = $1 ORDER BY m.name",
        user.id,
    )
    .await?;
    let goals = profile::load_names(
        &mut **tx,
        "SELECT g.id, g.name FROM study_goals g
         JOIN user_study_goals ug ON ug.goal_id = g.id
         WHERE ug.user_id = $1 ORDER BY g.name",
        user.id,
    )
    .await?;

    Ok(UserProfileDto {
        id: user.id,
        name: user.name,
        email: user.email,
        institution: InstitutionDto {
            id: institution.id,
            name: institution.name,
            slug: institution.slug,
            domain: institution.domain,
        },
        major: user.major,
        year: user.year,
        learning_style: user.learning_style,
        performance_level: user.performance_level,
        profile_picture_key: user.profile_picture_key,
        profile_picture: None,
        study_environments: environments,
        study_methods: methods,
        study_goals: goals,
        subjects: courses
            .into_iter()
            .map(|course| CourseDto {
                id: course.id,
                name: course.name,
                course_code: course.course_code,
            })
            .collect(),
        availability: availability
            .into_iter()
            .map(|slot| AvailabilityDto {
                day_of_week: slot.day_of_week,
                start_time: slot.start_time,
                end_time: slot.end_time,
                timezone: slot.timezone,
            })
            .collect(),
        group_preferences: GroupPreferencesDto {
            preferred_group_size_min: prefs.preferred_group_size_min,
            preferred_group_size_max: prefs.preferred_group_size_max,
            session_duration_minutes: prefs.session_duration_minutes,
        },
        created_at: user.created_at,
        updated_at: user.updated_at,
    })
}

pub async fn get_me(pool: &PgPool, user_id: Uuid) -> Result<UserProfileDto, AppError> {
    assemble_profile(pool, user_id).await
}

pub async fn list_institutions(pool: &PgPool) -> Result<Vec<InstitutionDto>, AppError> {
    let rows = identity::list_institutions(pool).await?;
    Ok(rows
        .into_iter()
        .map(|row| InstitutionDto {
            id: row.id,
            name: row.name,
            slug: row.slug,
            domain: row.domain,
        })
        .collect())
}

pub async fn patch_me(
    pool: &PgPool,
    user_id: Uuid,
    request: PatchMeRequest,
) -> Result<UserProfileDto, AppError> {
    let mut tx = pool.begin().await.map_err(crate::error::map_db_error)?;
    let mut user = identity::get_user_by_id(&mut *tx, user_id)
        .await?
        .filter(|row| row.deleted_at.is_none())
        .ok_or(AppError::UserNotFound)?;

    if let Some(name) = request.name.as_deref() {
        user.name = domain::require_nonempty(name, "name")?;
    }
    if request.institution_id.is_some() || request.institution_name.is_some() {
        let institution = resolve_institution(
            &mut tx,
            request.institution_id,
            request.institution_name.as_deref(),
            request.institution_domain.as_deref(),
        )
        .await?;
        user.institution_id = institution.id;
    }
    if let Some(major) = request.major.as_deref() {
        user.major = domain::require_nonempty(major, "major")?;
    }
    if let Some(year) = request.year.as_deref() {
        user.year = domain::validate_year(year.trim())?;
    }
    if let Some(style) = request.learning_style.as_deref() {
        user.learning_style = style.trim().to_string();
    }
    if let Some(level) = request.performance_level {
        user.performance_level = domain::validate_performance_level(level)?;
    }
    if request.profile_picture_key.is_some() {
        return Err(AppError::validation(
            "upload profile pictures with PUT /api/v1/me/profile-picture",
        ));
    }
    user.updated_at = Utc::now();
    identity::update_user_core(&mut tx, &user).await?;

    if let Some(prefs_in) = request.group_preferences.as_ref() {
        let prefs = Preferences::from_input(Some(prefs_in))?;
        profile::upsert_preferences(
            &mut tx,
            user.id,
            prefs.min_size,
            prefs.max_size,
            prefs.session_minutes,
        )
        .await?;
        if !prefs_in.study_goals.is_empty() {
            let goals = domain::normalize_names(&prefs_in.study_goals)?;
            profile::replace_goals(&mut tx, user.id, &goals).await?;
        }
    }
    if let Some(slots) = request.availability.as_ref() {
        let parsed = domain::parse_availability(slots)?;
        profile::replace_availability(&mut tx, user.id, &parsed).await?;
    }
    if let Some(subjects) = request.subjects.as_ref() {
        let names = domain::normalize_names(subjects)?;
        let mut course_ids = Vec::new();
        for name in &names {
            course_ids
                .push(profile::find_or_create_course(&mut tx, user.institution_id, name).await?);
        }
        profile::replace_user_courses(&mut tx, user.id, &course_ids).await?;
    }
    if let Some(values) = request.study_environments.as_ref() {
        profile::replace_environments(&mut tx, user.id, &domain::normalize_names(values)?).await?;
    }
    if let Some(values) = request.study_methods.as_ref() {
        profile::replace_methods(&mut tx, user.id, &domain::normalize_names(values)?).await?;
    }

    let profile = assemble_profile_in_tx(&mut tx, user_id).await?;
    tx.commit().await.map_err(crate::error::map_db_error)?;
    Ok(profile)
}
