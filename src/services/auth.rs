use chrono::{Duration, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::api::dto::{AuthResponse, LoginRequest, RegisterRequest};
use crate::auth::jwt::encode_access_token;
use crate::auth::password::{
    equalize_unknown_user_timing, hash_argon2id, validate_plaintext, verify_password, ALG_ARGON2ID,
    ALG_BCRYPT,
};
use crate::auth::refresh::{generate_refresh_token, hash_refresh_token};
use crate::db::identity::{
    self, find_active_refresh_session, insert_refresh_session, insert_user, revoke_refresh_session,
    UserRow,
};
use crate::error::AppError;
use crate::services::users::{
    assemble_profile_in_tx, resolve_institution, write_profile_graph, ProfileGraph,
};
use crate::Config;

const REFRESH_TTL_DAYS: i64 = 30;

pub async fn register(
    pool: &PgPool,
    config: &Config,
    request: RegisterRequest,
) -> Result<AuthResponse, AppError> {
    let name = crate::domain::require_nonempty(&request.name, "name")?;
    let email = crate::domain::normalize_email(&request.email)?;
    validate_plaintext(&request.password)?;
    let major = crate::domain::require_nonempty(&request.major, "major")?;
    let year = crate::domain::validate_year(request.year.trim())?;
    let learning_style = request
        .learning_style
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_string();
    let performance_level =
        crate::domain::validate_performance_level(request.performance_level.unwrap_or(3))?;
    let prefs = crate::domain::Preferences::from_input(request.group_preferences.as_ref())?;
    let availability = crate::domain::parse_availability(&request.availability)?;
    let environments = crate::domain::normalize_names(&request.study_environments)?;
    let methods = crate::domain::normalize_names(&request.study_methods)?;
    let subjects = crate::domain::normalize_names(&request.subjects)?;
    let goals = request
        .group_preferences
        .as_ref()
        .map(|prefs| prefs.study_goals.clone())
        .unwrap_or_default();
    let goals = crate::domain::normalize_names(&goals)?;

    let password_hash = hash_argon2id(&request.password)?;
    let now = Utc::now();
    let user_id = Uuid::now_v7();

    let mut tx = pool.begin().await.map_err(crate::error::map_db_error)?;
    let institution = resolve_institution(
        &mut tx,
        request.institution_id,
        request.institution_name.as_deref(),
        request.institution_domain.as_deref(),
    )
    .await?;

    let user = UserRow {
        id: user_id,
        name,
        email,
        password_hash,
        password_algorithm: ALG_ARGON2ID.to_string(),
        institution_id: institution.id,
        major,
        year,
        learning_style,
        performance_level,
        profile_picture_key: None,
        created_at: now,
        updated_at: now,
        deleted_at: None,
    };
    insert_user(&mut tx, &user).await?;
    write_profile_graph(
        &mut tx,
        user_id,
        institution.id,
        ProfileGraph {
            prefs: &prefs,
            availability: &availability,
            subjects: &subjects,
            environments: &environments,
            methods: &methods,
            goals: &goals,
        },
    )
    .await?;

    let (access_token, refresh_token) = issue_tokens(&mut tx, config, user_id).await?;
    let profile = assemble_profile_in_tx(&mut tx, user_id).await?;
    tx.commit().await.map_err(crate::error::map_db_error)?;

    Ok(AuthResponse {
        access_token,
        refresh_token,
        user: profile,
    })
}

pub async fn login(
    pool: &PgPool,
    config: &Config,
    request: LoginRequest,
) -> Result<AuthResponse, AppError> {
    let email = crate::domain::normalize_email(&request.email)?;
    if request.password.is_empty() {
        return Err(AppError::InvalidCredentials);
    }

    let mut tx = pool.begin().await.map_err(crate::error::map_db_error)?;
    let user = identity::find_user_by_email(&mut *tx, &email)
        .await?
        .filter(|row| row.deleted_at.is_none());

    let Some(user) = user else {
        drop(tx);
        equalize_unknown_user_timing(&request.password);
        return Err(AppError::InvalidCredentials);
    };

    let matches = verify_password(
        &request.password,
        &user.password_hash,
        &user.password_algorithm,
    )?;
    if !matches {
        return Err(AppError::InvalidCredentials);
    }

    if user.password_algorithm == ALG_BCRYPT {
        let upgraded = hash_argon2id(&request.password)?;
        identity::update_password(&mut *tx, user.id, &upgraded, ALG_ARGON2ID).await?;
    }

    let (access_token, refresh_token) = issue_tokens(&mut tx, config, user.id).await?;
    let profile = assemble_profile_in_tx(&mut tx, user.id).await?;
    tx.commit().await.map_err(crate::error::map_db_error)?;

    Ok(AuthResponse {
        access_token,
        refresh_token,
        user: profile,
    })
}

pub async fn refresh(
    pool: &PgPool,
    config: &Config,
    refresh_token: &str,
) -> Result<AuthResponse, AppError> {
    if !refresh_token_well_formed(refresh_token) {
        return Err(AppError::InvalidRefresh);
    }
    let token_hash = hash_refresh_token(refresh_token);
    let now = Utc::now();
    let mut tx = pool.begin().await.map_err(crate::error::map_db_error)?;
    let session = find_active_refresh_session(&mut tx, &token_hash, now)
        .await?
        .ok_or(AppError::InvalidRefresh)?;

    let user = identity::get_user_by_id(&mut *tx, session.user_id)
        .await?
        .filter(|row| row.deleted_at.is_none())
        .ok_or(AppError::InvalidRefresh)?;

    revoke_refresh_session(&mut tx, session.id, now).await?;
    let (access_token, new_refresh) = issue_tokens(&mut tx, config, user.id).await?;
    let profile = assemble_profile_in_tx(&mut tx, user.id).await?;
    tx.commit().await.map_err(crate::error::map_db_error)?;

    Ok(AuthResponse {
        access_token,
        refresh_token: new_refresh,
        user: profile,
    })
}

/// Revokes this refresh session only (one device). Already revoked or expired
/// tokens are a no-op success. Empty/malformed tokens are rejected.
pub async fn logout(pool: &PgPool, refresh_token: &str) -> Result<(), AppError> {
    if !refresh_token_well_formed(refresh_token) {
        return Err(AppError::InvalidRefresh);
    }
    let token_hash = hash_refresh_token(refresh_token);
    let now = Utc::now();
    let mut tx = pool.begin().await.map_err(crate::error::map_db_error)?;
    if let Some(session) = find_active_refresh_session(&mut tx, &token_hash, now).await? {
        revoke_refresh_session(&mut tx, session.id, now).await?;
    }
    tx.commit().await.map_err(crate::error::map_db_error)?;
    Ok(())
}

fn refresh_token_well_formed(token: &str) -> bool {
    !token.is_empty()
        && token
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
}

async fn issue_tokens(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    config: &Config,
    user_id: Uuid,
) -> Result<(String, String), AppError> {
    let access_token = encode_access_token(user_id, &config.jwt_secret)?;
    let refresh_token = generate_refresh_token();
    let token_hash = hash_refresh_token(&refresh_token);
    insert_refresh_session(
        &mut **tx,
        Uuid::now_v7(),
        user_id,
        &token_hash,
        Utc::now() + Duration::days(REFRESH_TTL_DAYS),
    )
    .await?;
    Ok((access_token, refresh_token))
}
