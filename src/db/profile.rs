use chrono::NaiveTime;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::error::{is_unique_or_serialization, map_db_error, unique_constraint, AppError};

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct CourseRow {
    pub id: Uuid,
    pub name: String,
    pub course_code: Option<String>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AvailabilityRow {
    pub day_of_week: i16,
    pub start_time: NaiveTime,
    pub end_time: NaiveTime,
    pub timezone: String,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct PreferenceRow {
    pub preferred_group_size_min: i16,
    pub preferred_group_size_max: i16,
    pub session_duration_minutes: i16,
}

#[derive(Debug, sqlx::FromRow)]
struct NamedRow {
    id: Uuid,
    name: String,
}

pub async fn upsert_preferences(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    min_size: i16,
    max_size: i16,
    minutes: i16,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO user_preferences (
            user_id, preferred_group_size_min, preferred_group_size_max, session_duration_minutes
         ) VALUES ($1, $2, $3, $4)
         ON CONFLICT (user_id) DO UPDATE SET
            preferred_group_size_min = EXCLUDED.preferred_group_size_min,
            preferred_group_size_max = EXCLUDED.preferred_group_size_max,
            session_duration_minutes = EXCLUDED.session_duration_minutes",
    )
    .bind(user_id)
    .bind(min_size)
    .bind(max_size)
    .bind(minutes)
    .execute(&mut **tx)
    .await
    .map_err(map_db_error)?;
    Ok(())
}

pub async fn replace_availability(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    slots: &[(i16, NaiveTime, NaiveTime, String)],
) -> Result<(), AppError> {
    sqlx::query("DELETE FROM availability WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut **tx)
        .await
        .map_err(map_db_error)?;

    for (day, start, end, timezone) in slots {
        sqlx::query(
            "INSERT INTO availability (id, user_id, day_of_week, start_time, end_time, timezone)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(Uuid::now_v7())
        .bind(user_id)
        .bind(day)
        .bind(start)
        .bind(end)
        .bind(timezone)
        .execute(&mut **tx)
        .await
        .map_err(map_db_error)?;
    }
    Ok(())
}

async fn select_course_by_name(
    tx: &mut Transaction<'_, Postgres>,
    institution_id: Uuid,
    name: &str,
) -> Result<Option<CourseRow>, AppError> {
    sqlx::query_as::<_, CourseRow>(
        "SELECT id, name, course_code FROM courses
         WHERE institution_id = $1 AND lower(name) = lower($2)
         LIMIT 1",
    )
    .bind(institution_id)
    .bind(name)
    .fetch_optional(&mut **tx)
    .await
    .map_err(map_db_error)
}

pub async fn find_or_create_course(
    tx: &mut Transaction<'_, Postgres>,
    institution_id: Uuid,
    name: &str,
) -> Result<Uuid, AppError> {
    for _ in 0..2 {
        if let Some(row) = select_course_by_name(tx, institution_id, name).await? {
            return Ok(row.id);
        }

        sqlx::query("SAVEPOINT course_insert")
            .execute(&mut **tx)
            .await
            .map_err(map_db_error)?;
        let id = Uuid::now_v7();
        let inserted = sqlx::query(
            "INSERT INTO courses (id, institution_id, name, course_code)
             VALUES ($1, $2, $3, NULL)",
        )
        .bind(id)
        .bind(institution_id)
        .bind(name)
        .execute(&mut **tx)
        .await;

        match inserted {
            Ok(_) => {
                sqlx::query("RELEASE SAVEPOINT course_insert")
                    .execute(&mut **tx)
                    .await
                    .map_err(map_db_error)?;
                return Ok(id);
            }
            Err(error)
                if unique_constraint(&error) == Some("courses_institution_name_lower_key")
                    || is_unique_or_serialization(&error) =>
            {
                sqlx::query("ROLLBACK TO SAVEPOINT course_insert")
                    .execute(&mut **tx)
                    .await
                    .map_err(map_db_error)?;
                sqlx::query("RELEASE SAVEPOINT course_insert")
                    .execute(&mut **tx)
                    .await
                    .map_err(map_db_error)?;
                continue;
            }
            Err(error) => {
                let _ = sqlx::query("ROLLBACK TO SAVEPOINT course_insert")
                    .execute(&mut **tx)
                    .await;
                return Err(map_db_error(error));
            }
        }
    }
    select_course_by_name(tx, institution_id, name)
        .await?
        .map(|row| row.id)
        .ok_or_else(|| AppError::validation("course already exists"))
}

pub async fn replace_user_courses(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    course_ids: &[Uuid],
) -> Result<(), AppError> {
    sqlx::query("DELETE FROM user_courses WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut **tx)
        .await
        .map_err(map_db_error)?;
    for course_id in course_ids {
        sqlx::query("INSERT INTO user_courses (user_id, course_id) VALUES ($1, $2)")
            .bind(user_id)
            .bind(course_id)
            .execute(&mut **tx)
            .await
            .map_err(map_db_error)?;
    }
    Ok(())
}

async fn find_or_create_named(
    tx: &mut Transaction<'_, Postgres>,
    select: &str,
    insert: &str,
    name: &str,
) -> Result<Uuid, AppError> {
    for _ in 0..2 {
        if let Some(row) = sqlx::query_as::<_, NamedRow>(select)
            .bind(name)
            .fetch_optional(&mut **tx)
            .await
            .map_err(map_db_error)?
        {
            return Ok(row.id);
        }
        sqlx::query("SAVEPOINT named_insert")
            .execute(&mut **tx)
            .await
            .map_err(map_db_error)?;
        let id = Uuid::now_v7();
        let inserted = sqlx::query(insert)
            .bind(id)
            .bind(name)
            .execute(&mut **tx)
            .await;
        match inserted {
            Ok(_) => {
                sqlx::query("RELEASE SAVEPOINT named_insert")
                    .execute(&mut **tx)
                    .await
                    .map_err(map_db_error)?;
                return Ok(id);
            }
            Err(error) if is_unique_or_serialization(&error) => {
                sqlx::query("ROLLBACK TO SAVEPOINT named_insert")
                    .execute(&mut **tx)
                    .await
                    .map_err(map_db_error)?;
                sqlx::query("RELEASE SAVEPOINT named_insert")
                    .execute(&mut **tx)
                    .await
                    .map_err(map_db_error)?;
                continue;
            }
            Err(error) => {
                let _ = sqlx::query("ROLLBACK TO SAVEPOINT named_insert")
                    .execute(&mut **tx)
                    .await;
                return Err(map_db_error(error));
            }
        }
    }
    sqlx::query_as::<_, NamedRow>(select)
        .bind(name)
        .fetch_optional(&mut **tx)
        .await
        .map_err(map_db_error)?
        .map(|row| row.id)
        .ok_or_else(|| AppError::validation("catalog value already exists"))
}

pub async fn replace_environments(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    names: &[String],
) -> Result<(), AppError> {
    sqlx::query("DELETE FROM user_study_environments WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut **tx)
        .await
        .map_err(map_db_error)?;
    for name in names {
        let id = find_or_create_named(
            tx,
            "SELECT id, name FROM study_environments WHERE lower(name) = lower($1)",
            "INSERT INTO study_environments (id, name) VALUES ($1, $2)",
            name,
        )
        .await?;
        sqlx::query(
            "INSERT INTO user_study_environments (user_id, environment_id) VALUES ($1, $2)",
        )
        .bind(user_id)
        .bind(id)
        .execute(&mut **tx)
        .await
        .map_err(map_db_error)?;
    }
    Ok(())
}

pub async fn replace_methods(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    names: &[String],
) -> Result<(), AppError> {
    sqlx::query("DELETE FROM user_study_methods WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut **tx)
        .await
        .map_err(map_db_error)?;
    for name in names {
        let id = find_or_create_named(
            tx,
            "SELECT id, name FROM study_methods WHERE lower(name) = lower($1)",
            "INSERT INTO study_methods (id, name) VALUES ($1, $2)",
            name,
        )
        .await?;
        sqlx::query("INSERT INTO user_study_methods (user_id, method_id) VALUES ($1, $2)")
            .bind(user_id)
            .bind(id)
            .execute(&mut **tx)
            .await
            .map_err(map_db_error)?;
    }
    Ok(())
}

pub async fn replace_goals(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    names: &[String],
) -> Result<(), AppError> {
    sqlx::query("DELETE FROM user_study_goals WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut **tx)
        .await
        .map_err(map_db_error)?;
    for name in names {
        let id = find_or_create_named(
            tx,
            "SELECT id, name FROM study_goals WHERE lower(name) = lower($1)",
            "INSERT INTO study_goals (id, name) VALUES ($1, $2)",
            name,
        )
        .await?;
        sqlx::query("INSERT INTO user_study_goals (user_id, goal_id) VALUES ($1, $2)")
            .bind(user_id)
            .bind(id)
            .execute(&mut **tx)
            .await
            .map_err(map_db_error)?;
    }
    Ok(())
}

pub async fn load_preferences(
    executor: impl sqlx::Executor<'_, Database = Postgres>,
    user_id: Uuid,
) -> Result<PreferenceRow, AppError> {
    sqlx::query_as::<_, PreferenceRow>(
        "SELECT preferred_group_size_min, preferred_group_size_max, session_duration_minutes
         FROM user_preferences WHERE user_id = $1",
    )
    .bind(user_id)
    .fetch_one(executor)
    .await
    .map_err(map_db_error)
}

pub async fn load_courses(
    executor: impl sqlx::Executor<'_, Database = Postgres>,
    user_id: Uuid,
) -> Result<Vec<CourseRow>, AppError> {
    sqlx::query_as::<_, CourseRow>(
        "SELECT c.id, c.name, c.course_code
         FROM courses c
         JOIN user_courses uc ON uc.course_id = c.id
         WHERE uc.user_id = $1
         ORDER BY c.name",
    )
    .bind(user_id)
    .fetch_all(executor)
    .await
    .map_err(map_db_error)
}

pub async fn load_availability(
    executor: impl sqlx::Executor<'_, Database = Postgres>,
    user_id: Uuid,
) -> Result<Vec<AvailabilityRow>, AppError> {
    sqlx::query_as::<_, AvailabilityRow>(
        "SELECT day_of_week, start_time, end_time, timezone
         FROM availability WHERE user_id = $1
         ORDER BY day_of_week, start_time",
    )
    .bind(user_id)
    .fetch_all(executor)
    .await
    .map_err(map_db_error)
}

pub async fn load_names(
    executor: impl sqlx::Executor<'_, Database = Postgres>,
    sql: &'static str,
    user_id: Uuid,
) -> Result<Vec<String>, AppError> {
    let rows = sqlx::query_as::<_, NamedRow>(sql)
        .bind(user_id)
        .fetch_all(executor)
        .await
        .map_err(map_db_error)?;
    Ok(rows.into_iter().map(|row| row.name).collect())
}
