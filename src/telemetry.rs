use tracing_subscriber::EnvFilter;

use crate::config::Config;

/// Initialize structured logging. Filtering is controlled by `RUST_LOG`.
///
/// Do not log passwords, tokens, JWT secrets, Authorization headers, or
/// database URLs. Request tracing is configured in `api` via tower-http
/// (method, path, status, duration, request id).
pub fn init(config: &Config) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(config.rust_log.clone()))
        .unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .compact()
        .try_init()?;

    Ok(())
}
