use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::error::{is_unique_or_serialization, map_db_error, unique_constraint, AppError};

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct InstitutionRow {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub domain: Option<String>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct UserRow {
    pub id: Uuid,
    pub name: String,
    pub email: String,
    pub password_hash: String,
    pub password_algorithm: String,
    pub institution_id: Uuid,
    pub major: String,
    pub year: String,
    pub learning_style: String,
    pub performance_level: i16,
    pub profile_picture_key: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

#[derive(Debug, sqlx::FromRow)]
pub struct RefreshSessionRow {
    pub id: Uuid,
    pub user_id: Uuid,
}

pub async fn list_institutions(pool: &PgPool) -> Result<Vec<InstitutionRow>, AppError> {
    sqlx::query_as::<_, InstitutionRow>(
        "SELECT id, name, slug, domain FROM institutions ORDER BY name",
    )
    .fetch_all(pool)
    .await
    .map_err(map_db_error)
}

pub async fn get_institution(
    executor: impl sqlx::Executor<'_, Database = Postgres>,
    id: Uuid,
) -> Result<Option<InstitutionRow>, AppError> {
    sqlx::query_as::<_, InstitutionRow>(
        "SELECT id, name, slug, domain FROM institutions WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(executor)
    .await
    .map_err(map_db_error)
}

pub async fn find_institution_by_name(
    tx: &mut Transaction<'_, Postgres>,
    name: &str,
) -> Result<Option<InstitutionRow>, AppError> {
    sqlx::query_as::<_, InstitutionRow>(
        "SELECT id, name, slug, domain FROM institutions WHERE lower(name) = lower($1)",
    )
    .bind(name)
    .fetch_optional(&mut **tx)
    .await
    .map_err(map_db_error)
}

pub async fn find_institution_by_domain(
    tx: &mut Transaction<'_, Postgres>,
    domain: &str,
) -> Result<Option<InstitutionRow>, AppError> {
    sqlx::query_as::<_, InstitutionRow>(
        "SELECT id, name, slug, domain FROM institutions WHERE lower(domain) = lower($1)",
    )
    .bind(domain)
    .fetch_optional(&mut **tx)
    .await
    .map_err(map_db_error)
}

pub async fn insert_institution(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    name: &str,
    slug: &str,
    domain: Option<&str>,
) -> Result<InstitutionRow, AppError> {
    for attempt in 0..2 {
        sqlx::query("SAVEPOINT institution_insert")
            .execute(&mut **tx)
            .await
            .map_err(map_db_error)?;

        let inserted = sqlx::query_as::<_, InstitutionRow>(
            "INSERT INTO institutions (id, name, slug, domain)
             VALUES ($1, $2, $3, $4)
             RETURNING id, name, slug, domain",
        )
        .bind(id)
        .bind(name)
        .bind(slug)
        .bind(domain)
        .fetch_one(&mut **tx)
        .await;

        match inserted {
            Ok(row) => {
                sqlx::query("RELEASE SAVEPOINT institution_insert")
                    .execute(&mut **tx)
                    .await
                    .map_err(map_db_error)?;
                return Ok(row);
            }
            Err(error) => {
                sqlx::query("ROLLBACK TO SAVEPOINT institution_insert")
                    .execute(&mut **tx)
                    .await
                    .map_err(map_db_error)?;
                sqlx::query("RELEASE SAVEPOINT institution_insert")
                    .execute(&mut **tx)
                    .await
                    .map_err(map_db_error)?;
                match recover_institution_insert(tx, name, domain, error).await {
                    Ok(Some(row)) => return Ok(row),
                    Ok(None) if attempt == 0 => continue,
                    Ok(None) => {
                        return Err(AppError::validation("institution already exists"));
                    }
                    Err(other) => return Err(other),
                }
            }
        }
    }
    Err(AppError::validation("institution already exists"))
}

async fn recover_institution_insert(
    tx: &mut Transaction<'_, Postgres>,
    name: &str,
    domain: Option<&str>,
    error: sqlx::Error,
) -> Result<Option<InstitutionRow>, AppError> {
    match unique_constraint(&error) {
        Some("institutions_name_lower_key") => find_institution_by_name(tx, name).await,
        Some("institutions_domain_lower_key") => match domain {
            Some(domain) => find_institution_by_domain(tx, domain).await,
            None => Ok(None),
        },
        _ if is_unique_or_serialization(&error) => {
            if let Some(row) = find_institution_by_name(tx, name).await? {
                return Ok(Some(row));
            }
            if let Some(domain) = domain {
                return find_institution_by_domain(tx, domain).await;
            }
            Ok(None)
        }
        _ => Err(map_db_error(error)),
    }
}

pub async fn insert_user(
    tx: &mut Transaction<'_, Postgres>,
    row: &UserRow,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO users (
            id, name, email, password_hash, password_algorithm, institution_id,
            major, year, learning_style, performance_level, profile_picture_key,
            created_at, updated_at, deleted_at
         ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)",
    )
    .bind(row.id)
    .bind(&row.name)
    .bind(&row.email)
    .bind(&row.password_hash)
    .bind(&row.password_algorithm)
    .bind(row.institution_id)
    .bind(&row.major)
    .bind(&row.year)
    .bind(&row.learning_style)
    .bind(row.performance_level)
    .bind(&row.profile_picture_key)
    .bind(row.created_at)
    .bind(row.updated_at)
    .bind(row.deleted_at)
    .execute(&mut **tx)
    .await
    .map_err(map_db_error)?;
    Ok(())
}

pub async fn find_user_by_email(
    executor: impl sqlx::Executor<'_, Database = Postgres>,
    email: &str,
) -> Result<Option<UserRow>, AppError> {
    sqlx::query_as::<_, UserRow>(
        "SELECT id, name, email, password_hash, password_algorithm, institution_id,
                major, year, learning_style, performance_level, profile_picture_key,
                created_at, updated_at, deleted_at
         FROM users WHERE email = $1",
    )
    .bind(email)
    .fetch_optional(executor)
    .await
    .map_err(map_db_error)
}

pub async fn get_user_by_id(
    executor: impl sqlx::Executor<'_, Database = Postgres>,
    id: Uuid,
) -> Result<Option<UserRow>, AppError> {
    sqlx::query_as::<_, UserRow>(
        "SELECT id, name, email, password_hash, password_algorithm, institution_id,
                major, year, learning_style, performance_level, profile_picture_key,
                created_at, updated_at, deleted_at
         FROM users WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(executor)
    .await
    .map_err(map_db_error)
}

pub async fn update_password(
    executor: impl sqlx::Executor<'_, Database = Postgres>,
    user_id: Uuid,
    password_hash: &str,
    algorithm: &str,
) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE users
         SET password_hash = $2, password_algorithm = $3, updated_at = now()
         WHERE id = $1",
    )
    .bind(user_id)
    .bind(password_hash)
    .bind(algorithm)
    .execute(executor)
    .await
    .map_err(map_db_error)?;
    Ok(())
}

pub async fn update_user_core(
    tx: &mut Transaction<'_, Postgres>,
    row: &UserRow,
) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE users SET
            name = $2,
            institution_id = $3,
            major = $4,
            year = $5,
            learning_style = $6,
            performance_level = $7,
            profile_picture_key = $8,
            updated_at = $9
         WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(row.id)
    .bind(&row.name)
    .bind(row.institution_id)
    .bind(&row.major)
    .bind(&row.year)
    .bind(&row.learning_style)
    .bind(row.performance_level)
    .bind(&row.profile_picture_key)
    .bind(row.updated_at)
    .execute(&mut **tx)
    .await
    .map_err(map_db_error)?;
    Ok(())
}

pub async fn insert_refresh_session(
    executor: impl sqlx::Executor<'_, Database = Postgres>,
    id: Uuid,
    user_id: Uuid,
    token_hash: &str,
    expires_at: DateTime<Utc>,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO refresh_sessions (id, user_id, token_hash, expires_at)
         VALUES ($1, $2, $3, $4)",
    )
    .bind(id)
    .bind(user_id)
    .bind(token_hash)
    .bind(expires_at)
    .execute(executor)
    .await
    .map_err(map_db_error)?;
    Ok(())
}

pub async fn find_active_refresh_session(
    tx: &mut Transaction<'_, Postgres>,
    token_hash: &str,
    now: DateTime<Utc>,
) -> Result<Option<RefreshSessionRow>, AppError> {
    sqlx::query_as::<_, RefreshSessionRow>(
        "SELECT id, user_id FROM refresh_sessions
         WHERE token_hash = $1 AND revoked_at IS NULL AND expires_at > $2
         FOR UPDATE",
    )
    .bind(token_hash)
    .bind(now)
    .fetch_optional(&mut **tx)
    .await
    .map_err(map_db_error)
}

pub async fn revoke_refresh_session(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    now: DateTime<Utc>,
) -> Result<(), AppError> {
    sqlx::query("UPDATE refresh_sessions SET revoked_at = $2 WHERE id = $1 AND revoked_at IS NULL")
        .bind(id)
        .bind(now)
        .execute(&mut **tx)
        .await
        .map_err(map_db_error)?;
    Ok(())
}
