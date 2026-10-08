use std::collections::HashMap;
use std::fmt;
use std::net::IpAddr;
use std::time::Duration;

use thiserror::Error;

const MIN_JWT_SECRET_LEN: usize = 32;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("missing required environment variable: {0}")]
    Missing(&'static str),
    #[error("invalid {name}: {reason}")]
    Invalid {
        name: &'static str,
        reason: &'static str,
    },
}

/// Process configuration. Secrets are never logged by this type's public helpers.
#[derive(Clone)]
pub struct Config {
    pub database_url: String,
    pub redis_url: Option<String>,
    pub jwt_secret: String,
    pub public_base_url: String,
    pub server_host: IpAddr,
    pub server_port: u16,
    pub rust_log: String,
    pub database_pool: DatabasePoolConfig,
    pub storage: Option<StorageConfig>,
    pub media_max_bytes: u64,
    pub media_signed_url_ttl: Duration,
    pub embedding_backend: EmbeddingBackend,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbeddingBackend {
    /// Deterministic hashed encoder with the selected model's dimensions and query format.
    Hash,
    /// Semantic encode/search fail closed. Structured matching may still run.
    Unavailable,
}

/// S3-compatible object store. Secrets are never included in Debug.
#[derive(Clone)]
pub struct StorageConfig {
    pub endpoint: String,
    pub bucket: String,
    pub access_key: String,
    pub secret_key: String,
    pub region: String,
    pub force_path_style: bool,
}

#[derive(Clone, Debug)]
pub struct DatabasePoolConfig {
    pub max_connections: u32,
    pub min_connections: u32,
    pub acquire_timeout: Duration,
    pub idle_timeout: Duration,
    pub max_lifetime: Duration,
}

impl Default for DatabasePoolConfig {
    fn default() -> Self {
        Self {
            max_connections: 10,
            min_connections: 1,
            acquire_timeout: Duration::from_secs(8),
            idle_timeout: Duration::from_secs(600),
            max_lifetime: Duration::from_secs(1800),
        }
    }
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        let vars: HashMap<String, String> = std::env::vars().collect();
        Self::from_vars(&vars)
    }

    pub fn from_vars(vars: &HashMap<String, String>) -> Result<Self, ConfigError> {
        let database_url = required(vars, "DATABASE_URL")?;
        if !(database_url.starts_with("postgres://") || database_url.starts_with("postgresql://")) {
            return Err(ConfigError::Invalid {
                name: "DATABASE_URL",
                reason: "must start with postgres:// or postgresql://",
            });
        }

        let jwt_secret = required(vars, "JWT_SECRET")?;
        if jwt_secret.len() < MIN_JWT_SECRET_LEN {
            return Err(ConfigError::Invalid {
                name: "JWT_SECRET",
                reason: "must be at least 32 characters",
            });
        }

        let public_base_url = required(vars, "PUBLIC_BASE_URL")?;
        if !(public_base_url.starts_with("http://") || public_base_url.starts_with("https://")) {
            return Err(ConfigError::Invalid {
                name: "PUBLIC_BASE_URL",
                reason: "must start with http:// or https://",
            });
        }

        let redis_url = optional(vars, "REDIS_URL").filter(|value| !value.is_empty());

        let host_raw = optional(vars, "SERVER_HOST").unwrap_or_else(|| "0.0.0.0".to_string());
        let server_host: IpAddr = host_raw.parse().map_err(|_| ConfigError::Invalid {
            name: "SERVER_HOST",
            reason: "must be a valid IP address",
        })?;

        let port_raw = optional(vars, "SERVER_PORT").unwrap_or_else(|| "8080".to_string());
        let server_port: u16 = port_raw.parse().map_err(|_| ConfigError::Invalid {
            name: "SERVER_PORT",
            reason: "must be a valid port number (0-65535)",
        })?;

        let rust_log = optional(vars, "RUST_LOG").unwrap_or_else(|| "info".to_string());

        let mut database_pool = DatabasePoolConfig::default();
        if let Some(value) = optional(vars, "DATABASE_MAX_CONNECTIONS") {
            database_pool.max_connections = parse_u32(value, "DATABASE_MAX_CONNECTIONS", 1, 100)?;
        }
        if let Some(value) = optional(vars, "DATABASE_MIN_CONNECTIONS") {
            database_pool.min_connections = parse_u32(value, "DATABASE_MIN_CONNECTIONS", 0, 50)?;
        }
        if database_pool.min_connections > database_pool.max_connections {
            return Err(ConfigError::Invalid {
                name: "DATABASE_MIN_CONNECTIONS",
                reason: "must be less than or equal to DATABASE_MAX_CONNECTIONS",
            });
        }
        if let Some(value) = optional(vars, "DATABASE_ACQUIRE_TIMEOUT_SECS") {
            database_pool.acquire_timeout = Duration::from_secs(u64::from(parse_u32(
                value,
                "DATABASE_ACQUIRE_TIMEOUT_SECS",
                1,
                120,
            )?));
        }
        if let Some(value) = optional(vars, "DATABASE_IDLE_TIMEOUT_SECS") {
            database_pool.idle_timeout = Duration::from_secs(u64::from(parse_u32(
                value,
                "DATABASE_IDLE_TIMEOUT_SECS",
                1,
                86_400,
            )?));
        }
        if let Some(value) = optional(vars, "DATABASE_MAX_LIFETIME_SECS") {
            database_pool.max_lifetime = Duration::from_secs(u64::from(parse_u32(
                value,
                "DATABASE_MAX_LIFETIME_SECS",
                1,
                86_400,
            )?));
        }

        let media_max_bytes = optional(vars, "MEDIA_MAX_BYTES")
            .map(|value| parse_u64(value, "MEDIA_MAX_BYTES", 1024, 52_428_800))
            .transpose()?
            .unwrap_or(10 * 1024 * 1024);
        let media_signed_url_ttl = optional(vars, "MEDIA_SIGNED_URL_TTL_SECS")
            .map(|value| {
                parse_u32(value, "MEDIA_SIGNED_URL_TTL_SECS", 30, 3600)
                    .map(|secs| Duration::from_secs(u64::from(secs)))
            })
            .transpose()?
            .unwrap_or(Duration::from_secs(300));

        let storage = if optional(vars, "S3_ENDPOINT").is_some() {
            Some(StorageConfig {
                endpoint: required(vars, "S3_ENDPOINT")?,
                bucket: required(vars, "S3_BUCKET")?,
                access_key: required(vars, "S3_ACCESS_KEY")?,
                secret_key: required(vars, "S3_SECRET_KEY")?,
                region: optional(vars, "S3_REGION").unwrap_or_else(|| "us-east-1".to_string()),
                force_path_style: optional(vars, "S3_FORCE_PATH_STYLE")
                    .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
                    .unwrap_or(true),
            })
        } else {
            None
        };

        let embedding_backend = match optional(vars, "EMBEDDING_BACKEND")
            .unwrap_or_else(|| "hash".to_string())
            .to_ascii_lowercase()
            .as_str()
        {
            "hash" => EmbeddingBackend::Hash,
            "unavailable" => EmbeddingBackend::Unavailable,
            _ => {
                return Err(ConfigError::Invalid {
                    name: "EMBEDDING_BACKEND",
                    reason: "must be hash or unavailable",
                })
            }
        };

        Ok(Self {
            database_url,
            redis_url,
            jwt_secret,
            public_base_url,
            server_host,
            server_port,
            rust_log,
            database_pool,
            storage,
            media_max_bytes,
            media_signed_url_ttl,
            embedding_backend,
        })
    }

    /// Safe summary for startup logs. Omits URLs, secrets, and credentials.
    pub fn log_safe_summary(&self) {
        tracing::info!(
            host = %self.server_host,
            port = self.server_port,
            public_base_url_configured = !self.public_base_url.is_empty(),
            redis_configured = self.redis_url.is_some(),
            jwt_secret_chars = self.jwt_secret.len(),
            pool_max = self.database_pool.max_connections,
            pool_min = self.database_pool.min_connections,
            object_storage = self.storage.is_some(),
            media_max_bytes = self.media_max_bytes,
            embedding_backend = ?self.embedding_backend,
            "configuration loaded"
        );
    }
}

impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("database_url", &"<redacted>")
            .field("redis_url", &self.redis_url.as_ref().map(|_| "<redacted>"))
            .field("jwt_secret", &"<redacted>")
            .field("public_base_url", &self.public_base_url)
            .field("server_host", &self.server_host)
            .field("server_port", &self.server_port)
            .field("rust_log", &self.rust_log)
            .field("database_pool", &self.database_pool)
            .field("storage", &self.storage.as_ref().map(|_| "<redacted>"))
            .field("media_max_bytes", &self.media_max_bytes)
            .field("media_signed_url_ttl", &self.media_signed_url_ttl)
            .field("embedding_backend", &self.embedding_backend)
            .finish()
    }
}

impl fmt::Debug for StorageConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StorageConfig")
            .field("endpoint", &self.endpoint)
            .field("bucket", &self.bucket)
            .field("access_key", &"<redacted>")
            .field("secret_key", &"<redacted>")
            .field("region", &self.region)
            .field("force_path_style", &self.force_path_style)
            .finish()
    }
}

fn required(vars: &HashMap<String, String>, name: &'static str) -> Result<String, ConfigError> {
    vars.get(name)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or(ConfigError::Missing(name))
}

fn optional(vars: &HashMap<String, String>, name: &str) -> Option<String> {
    vars.get(name)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn parse_u32(value: String, name: &'static str, min: u32, max: u32) -> Result<u32, ConfigError> {
    let parsed: u32 = value.parse().map_err(|_| ConfigError::Invalid {
        name,
        reason: "must be a positive integer",
    })?;
    if parsed < min || parsed > max {
        return Err(ConfigError::Invalid {
            name,
            reason: "value is out of the allowed range",
        });
    }
    Ok(parsed)
}

fn parse_u64(value: String, name: &'static str, min: u64, max: u64) -> Result<u64, ConfigError> {
    let parsed: u64 = value.parse().map_err(|_| ConfigError::Invalid {
        name,
        reason: "must be a positive integer",
    })?;
    if parsed < min || parsed > max {
        return Err(ConfigError::Invalid {
            name,
            reason: "value is out of the allowed range",
        });
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_vars() -> HashMap<String, String> {
        HashMap::from([
            (
                "DATABASE_URL".into(),
                "postgres://studysync:dev@127.0.0.1:5432/studysync".into(),
            ),
            (
                "JWT_SECRET".into(),
                "abcdefghijklmnopqrstuvwxyz012345".into(),
            ),
            ("PUBLIC_BASE_URL".into(), "http://127.0.0.1:8080".into()),
        ])
    }

    #[test]
    fn loads_defaults_for_optional_server_bind() {
        let config = Config::from_vars(&base_vars()).expect("config");
        assert_eq!(config.server_host, IpAddr::from([0, 0, 0, 0]));
        assert_eq!(config.server_port, 8080);
        assert!(config.redis_url.is_none());
        assert_eq!(config.database_pool.max_connections, 10);
        assert_eq!(config.embedding_backend, EmbeddingBackend::Hash);
    }

    #[test]
    fn rejects_missing_database_url() {
        let mut vars = base_vars();
        vars.remove("DATABASE_URL");
        let err = Config::from_vars(&vars).unwrap_err();
        assert!(matches!(err, ConfigError::Missing("DATABASE_URL")));
    }

    #[test]
    fn rejects_short_jwt_secret() {
        let mut vars = base_vars();
        vars.insert("JWT_SECRET".into(), "too-short".into());
        let err = Config::from_vars(&vars).unwrap_err();
        assert!(matches!(
            err,
            ConfigError::Invalid {
                name: "JWT_SECRET",
                ..
            }
        ));
    }

    #[test]
    fn rejects_non_postgres_url() {
        let mut vars = base_vars();
        vars.insert("DATABASE_URL".into(), "mysql://localhost/studysync".into());
        let err = Config::from_vars(&vars).unwrap_err();
        assert!(matches!(
            err,
            ConfigError::Invalid {
                name: "DATABASE_URL",
                ..
            }
        ));
    }

    #[test]
    fn accepts_optional_redis_url() {
        let mut vars = base_vars();
        vars.insert("REDIS_URL".into(), "redis://127.0.0.1:6379".into());
        let config = Config::from_vars(&vars).expect("config");
        assert_eq!(config.redis_url.as_deref(), Some("redis://127.0.0.1:6379"));
    }

    #[test]
    fn debug_redacts_secrets() {
        let mut vars = base_vars();
        vars.insert(
            "DATABASE_URL".into(),
            "postgres://studysync:super-secret-db-pass@127.0.0.1:5432/studysync".into(),
        );
        vars.insert(
            "JWT_SECRET".into(),
            "abcdefghijklmnopqrstuvwxyz012345".into(),
        );
        vars.insert(
            "REDIS_URL".into(),
            "redis://:redis-secret@127.0.0.1:6379".into(),
        );
        let config = Config::from_vars(&vars).expect("config");
        let rendered = format!("{config:?}");
        assert!(!rendered.contains("super-secret-db-pass"));
        assert!(!rendered.contains("abcdefghijklmnopqrstuvwxyz012345"));
        assert!(!rendered.contains("redis-secret"));
        assert!(rendered.contains("<redacted>"));
    }
}
