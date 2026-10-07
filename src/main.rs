use std::net::SocketAddr;
use std::process::ExitCode;

use studysync_backend::config::Config;
use studysync_backend::db;
use studysync_backend::{api, telemetry, AppState};

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), String> {
    let _ = dotenvy::dotenv();

    let config = Config::from_env().map_err(|error| error.to_string())?;
    telemetry::init(&config).map_err(|error| error.to_string())?;
    config.log_safe_summary();

    let pool = db::create_pool(&config).await.map_err(|_| {
        "failed to connect to PostgreSQL; confirm DATABASE_URL and that the database is running"
            .to_string()
    })?;

    tracing::info!("postgresql pool ready");

    let addr = SocketAddr::new(config.server_host, config.server_port);
    let state = AppState::new(config, pool);
    let app = api::router(state);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|_| format!("failed to bind HTTP listener on {addr}"))?;

    tracing::info!(%addr, "http server listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|_| "http server error".to_string())?;

    tracing::info!("server shutdown complete");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        if tokio::signal::ctrl_c().await.is_err() {
            tracing::error!("failed to install Ctrl+C handler");
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(_) => tracing::error!("failed to install SIGTERM handler"),
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
}
