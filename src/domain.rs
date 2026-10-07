use chrono::NaiveTime;
use chrono_tz::Tz;

use crate::api::dto::{AvailabilityInput, GroupPreferencesInput};
use crate::error::AppError;

pub const VALID_YEARS: &[&str] = &["Freshman", "Sophomore", "Junior", "Senior", "Graduate"];

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

pub fn validate_timezone(timezone: &str) -> Result<String, AppError> {
    let timezone = timezone.trim();
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
        if end <= start {
            return Err(AppError::validation("endTime must be after startTime"));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::dto::AvailabilityInput;

    #[test]
    fn availability_rejects_inverted_window() {
        let slots = [AvailabilityInput {
            day_of_week: 0,
            start_time: "18:00".into(),
            end_time: "17:00".into(),
            timezone: "America/Chicago".into(),
        }];
        assert!(parse_availability(&slots).is_err());
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
    fn preferences_reject_inverted_sizes() {
        assert!(Preferences::new(6, 2, 120).is_err());
    }
}
