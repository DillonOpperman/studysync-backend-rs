use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::AppError;

pub const TOKEN_TYPE_ACCESS: &str = "access";
const ACCESS_TTL_MINUTES: i64 = 15;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AccessClaims {
    pub sub: String,
    pub iat: i64,
    pub exp: i64,
    pub typ: String,
    pub jti: String,
}

pub fn encode_access_token(user_id: Uuid, secret: &str) -> Result<String, AppError> {
    let now = Utc::now();
    let claims = AccessClaims {
        sub: user_id.to_string(),
        iat: now.timestamp(),
        exp: (now + Duration::minutes(ACCESS_TTL_MINUTES)).timestamp(),
        typ: TOKEN_TYPE_ACCESS.to_string(),
        jti: Uuid::now_v7().to_string(),
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|_| {
        tracing::error!("jwt encode failed");
        AppError::Internal
    })
}

pub fn decode_access_token(token: &str, secret: &str) -> Result<AccessClaims, AppError> {
    let mut validation = Validation::default();
    validation.validate_exp = true;
    validation.leeway = 0;

    let data = decode::<AccessClaims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )
    .map_err(|error| match error.kind() {
        jsonwebtoken::errors::ErrorKind::ExpiredSignature => AppError::ExpiredToken,
        _ => AppError::Unauthorized,
    })?;

    if data.claims.typ != TOKEN_TYPE_ACCESS {
        return Err(AppError::Unauthorized);
    }
    if Uuid::parse_str(&data.claims.sub).is_err() {
        return Err(AppError::Unauthorized);
    }
    if Uuid::parse_str(&data.claims.jti).is_err() {
        return Err(AppError::Unauthorized);
    }
    Ok(data.claims)
}

/// Test helper: issue a token with an explicit expiry timestamp.
pub fn encode_access_token_with_exp(
    user_id: Uuid,
    secret: &str,
    exp: i64,
) -> Result<String, AppError> {
    let now = Utc::now().timestamp();
    let claims = AccessClaims {
        sub: user_id.to_string(),
        iat: now.min(exp),
        exp,
        typ: TOKEN_TYPE_ACCESS.to_string(),
        jti: Uuid::now_v7().to_string(),
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|_| AppError::Internal)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "abcdefghijklmnopqrstuvwxyz012345";

    #[test]
    fn generates_and_extracts_access_jwt() {
        let user_id = Uuid::now_v7();
        let token = encode_access_token(user_id, SECRET).unwrap();
        let claims = decode_access_token(&token, SECRET).unwrap();
        assert_eq!(claims.sub, user_id.to_string());
        assert_eq!(claims.typ, TOKEN_TYPE_ACCESS);
        assert!(Uuid::parse_str(&claims.jti).is_ok());
        assert!(claims.exp > claims.iat);
    }

    #[test]
    fn rejects_expired_jwt() {
        let user_id = Uuid::now_v7();
        let token =
            encode_access_token_with_exp(user_id, SECRET, Utc::now().timestamp() - 60).unwrap();
        let err = decode_access_token(&token, SECRET).unwrap_err();
        assert!(matches!(err, AppError::ExpiredToken));
    }

    #[test]
    fn rejects_wrong_secret() {
        let token = encode_access_token(Uuid::now_v7(), SECRET).unwrap();
        let err = decode_access_token(&token, "different-secret-that-is-32-chars!!").unwrap_err();
        assert!(matches!(err, AppError::Unauthorized));
    }
}
