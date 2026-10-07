use argon2::password_hash::{
    rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
};
use argon2::Argon2;

use crate::error::AppError;

pub const ALG_ARGON2ID: &str = "argon2id";
pub const ALG_BCRYPT: &str = "bcrypt";

const MIN_PASSWORD_LEN: usize = 8;
const MAX_PASSWORD_LEN: usize = 128;

pub fn validate_plaintext(password: &str) -> Result<(), AppError> {
    if password.len() < MIN_PASSWORD_LEN {
        return Err(AppError::validation(
            "password must be at least 8 characters",
        ));
    }
    if password.len() > MAX_PASSWORD_LEN {
        return Err(AppError::validation("password is too long"));
    }
    if password.chars().any(char::is_control) {
        return Err(AppError::validation("password contains invalid characters"));
    }
    Ok(())
}

pub fn hash_argon2id(password: &str) -> Result<String, AppError> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|_| {
            tracing::error!("argon2 hashing failed");
            AppError::Internal
        })
}

pub fn verify_password(password: &str, hash: &str, algorithm: &str) -> Result<bool, AppError> {
    match algorithm {
        ALG_ARGON2ID => {
            let parsed = PasswordHash::new(hash).map_err(|_| {
                tracing::error!("stored argon2 hash is malformed");
                AppError::Internal
            })?;
            Ok(Argon2::default()
                .verify_password(password.as_bytes(), &parsed)
                .is_ok())
        }
        ALG_BCRYPT => bcrypt::verify(password, hash).map_err(|_| {
            tracing::error!("stored bcrypt hash is malformed");
            AppError::Internal
        }),
        _ => {
            tracing::error!("unknown password algorithm");
            Err(AppError::Internal)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_short_passwords() {
        assert!(validate_plaintext("short").is_err());
    }

    #[test]
    fn argon2id_round_trip() {
        let hash = hash_argon2id("correct-horse").expect("hash");
        assert!(hash.starts_with("$argon2"));
        assert!(verify_password("correct-horse", &hash, ALG_ARGON2ID).unwrap());
        assert!(!verify_password("wrong-horse", &hash, ALG_ARGON2ID).unwrap());
    }

    #[test]
    fn bcrypt_verify() {
        let hash = bcrypt::hash("legacy-pass", 4).expect("bcrypt");
        assert!(verify_password("legacy-pass", &hash, ALG_BCRYPT).unwrap());
        assert!(!verify_password("nope", &hash, ALG_BCRYPT).unwrap());
    }
}
