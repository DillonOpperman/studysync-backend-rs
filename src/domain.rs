use chrono::NaiveTime;
use chrono_tz::Tz;
use serde::Deserialize;

use crate::error::AppError;

pub const VALID_YEARS: &[&str] = &["Freshman", "Sophomore", "Junior", "Senior", "Graduate"];

/// Clock-in request for a weekly availability slot (API camelCase).
///
/// Overnight windows (for example 23:00 → 01:00) are stored as a single row
/// with `end_time < start_time`. Matching must treat that as two half-open
/// intervals on the civil clock: `[start, 24:00)` on `day_of_week` and
/// `[00:00, end)` on the following day. Equal start and end is invalid (zero length).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailabilityInput {
    pub day_of_week: i16,
    pub start_time: String,
    pub end_time: String,
    pub timezone: String,
}

/// Group-size and duration input. `study_goals` is a name list resolved to lookup rows.
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

pub fn normalize_email(email: &str) -> Result<String, AppError> {
    let email = email.trim();
    if email.is_empty() || !email.contains('@') || email.len() > 254 {
        return Err(AppError::validation("email is invalid"));
    }
    Ok(email.to_string())
}

pub fn require_nonempty(value: &str, field: &str) -> Result<String, AppError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(AppError::validation(format!("{field} is required")));
    }
    Ok(trimmed.to_string())
}

pub fn validate_year(year: &str) -> Result<String, AppError> {
    if VALID_YEARS.contains(&year) {
        Ok(year.to_string())
    } else {
        Err(AppError::validation(
            "year must be Freshman, Sophomore, Junior, Senior, or Graduate",
        ))
    }
}

pub fn validate_performance_level(level: i16) -> Result<i16, AppError> {
    if (1..=5).contains(&level) {
        Ok(level)
    } else {
        Err(AppError::validation(
            "performanceLevel must be between 1 and 5",
        ))
    }
}

/// IANA names are validated in Rust (`chrono-tz`). PostgreSQL stores the
/// identifier as `text`; it does not embed the IANA database as a CHECK.
pub fn validate_timezone(timezone: &str) -> Result<String, AppError> {
    let timezone = timezone.trim();
    if timezone.is_empty() {
        return Err(AppError::validation("timezone is required"));
    }
    timezone
        .parse::<Tz>()
        .map(|_| timezone.to_string())
        .map_err(|_| AppError::validation("timezone must be a valid IANA name"))
}

pub fn parse_clock(value: &str, field: &str) -> Result<NaiveTime, AppError> {
    let value = value.trim();
    NaiveTime::parse_from_str(value, "%H:%M:%S")
        .or_else(|_| NaiveTime::parse_from_str(value, "%H:%M"))
        .map_err(|_| AppError::validation(format!("{field} must be HH:MM or HH:MM:SS")))
}

pub fn parse_availability(
    slots: &[AvailabilityInput],
) -> Result<Vec<(i16, NaiveTime, NaiveTime, String)>, AppError> {
    let mut parsed = Vec::with_capacity(slots.len());
    for slot in slots {
        if !(0..=6).contains(&slot.day_of_week) {
            return Err(AppError::validation(
                "dayOfWeek must be between 0 (Monday) and 6 (Sunday)",
            ));
        }
        let start = parse_clock(&slot.start_time, "startTime")?;
        let end = parse_clock(&slot.end_time, "endTime")?;
        if end == start {
            return Err(AppError::validation(
                "endTime must differ from startTime (zero-length windows are not allowed)",
            ));
        }
        let timezone = validate_timezone(&slot.timezone)?;
        parsed.push((slot.day_of_week, start, end, timezone));
    }
    Ok(parsed)
}

#[derive(Debug, Clone, Copy)]
pub struct Preferences {
    pub min_size: i16,
    pub max_size: i16,
    pub session_minutes: i16,
}

impl Preferences {
    pub fn from_input(input: Option<&GroupPreferencesInput>) -> Result<Self, AppError> {
        let group_size = input.and_then(|value| value.group_size);
        let min = input
            .and_then(|value| value.preferred_group_size_min)
            .or(group_size)
            .unwrap_or(2);
        let max = input
            .and_then(|value| value.preferred_group_size_max)
            .or(group_size)
            .unwrap_or(6);
        let minutes = if let Some(minutes) = input.and_then(|value| value.session_duration_minutes)
        {
            minutes
        } else if let Some(hours) = input.and_then(|value| value.session_duration) {
            hours
                .checked_mul(60)
                .ok_or_else(|| AppError::validation("sessionDuration is invalid"))?
        } else {
            120
        };
        Self::new(min, max, minutes)
    }

    pub fn new(min_size: i16, max_size: i16, session_minutes: i16) -> Result<Self, AppError> {
        if min_size < 2 || max_size > 50 || max_size < min_size {
            return Err(AppError::validation(
                "preferred group size must be between 2 and 50 with min <= max",
            ));
        }
        if session_minutes <= 0 || session_minutes > 24 * 60 {
            return Err(AppError::validation(
                "sessionDurationMinutes must be between 1 and 1440",
            ));
        }
        Ok(Self {
            min_size,
            max_size,
            session_minutes,
        })
    }
}

pub fn slugify(name: &str) -> String {
    let mut slug = String::new();
    let mut prev_dash = false;
    for ch in name.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !slug.is_empty() {
            slug.push('-');
            prev_dash = true;
        }
    }
    slug.trim_end_matches('-').to_string()
}

/// Object-storage key placeholder. The future media service must own uploads,
/// bind keys to the authenticated user, and issue authorized URLs. Clients
/// must not treat this field as permission to read arbitrary bucket objects.
pub fn validate_storage_key(key: &str) -> Result<String, AppError> {
    let key = key.trim();
    if key.is_empty() {
        return Err(AppError::validation("profilePictureKey is invalid"));
    }
    if key.contains("://") || key.contains("..") || key.len() > 512 {
        return Err(AppError::validation("profilePictureKey is invalid"));
    }
    if !key
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '_' | '-' | '.'))
    {
        return Err(AppError::validation("profilePictureKey is invalid"));
    }
    Ok(key.to_string())
}

pub fn normalize_names(values: &[String]) -> Result<Vec<String>, AppError> {
    let mut names = Vec::new();
    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err(AppError::validation("list values must not be blank"));
        }
        if !names
            .iter()
            .any(|existing: &String| existing.eq_ignore_ascii_case(trimmed))
        {
            names.push(trimmed.to_string());
        }
    }
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn availability_rejects_zero_length() {
        let slots = [AvailabilityInput {
            day_of_week: 0,
            start_time: "18:00".into(),
            end_time: "18:00".into(),
            timezone: "America/Chicago".into(),
        }];
        assert!(parse_availability(&slots).is_err());
    }

    #[test]
    fn availability_accepts_overnight_window() {
        let slots = [AvailabilityInput {
            day_of_week: 0,
            start_time: "23:00".into(),
            end_time: "01:00".into(),
            timezone: "America/Chicago".into(),
        }];
        let parsed = parse_availability(&slots).unwrap();
        assert!(parsed[0].2 < parsed[0].1);
    }

    #[test]
    fn availability_accepts_structured_slot() {
        let slots = [AvailabilityInput {
            day_of_week: 0,
            start_time: "17:00".into(),
            end_time: "21:00".into(),
            timezone: "America/Chicago".into(),
        }];
        let parsed = parse_availability(&slots).unwrap();
        assert_eq!(parsed[0].0, 0);
        assert_eq!(parsed[0].3, "America/Chicago");
    }

    #[test]
    fn rejects_unknown_timezone() {
        assert!(validate_timezone("Not/AZone").is_err());
    }

    #[test]
    fn rejects_missing_timezone() {
        assert!(validate_timezone("  ").is_err());
    }

    #[test]
    fn accepts_valid_iana_timezone() {
        assert_eq!(
            validate_timezone("America/New_York").unwrap(),
            "America/New_York"
        );
    }

    #[test]
    fn preferences_reject_inverted_sizes() {
        assert!(Preferences::new(6, 2, 120).is_err());
    }
}
