mod log;
mod shell;
mod web;

use std::sync::Arc;

use log::LogStore;
use shell::AlpineShell;

use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    let cancel = tokio_util::sync::CancellationToken::new();
    let log_store = Arc::new(LogStore::new());

    let log_store_clone = log_store.clone();
    let mcp_service = StreamableHttpService::new(
        move || Ok(AlpineShell::new(log_store_clone.clone())),
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig::default()
            .with_cancellation_token(cancel.child_token())
            .disable_allowed_hosts()
            .disable_allowed_origins(),
    );

    let app = axum::Router::new()
        .merge(web::web_routes(log_store.clone()))
        .nest_service("/mcp", mcp_service);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    tracing::info!("mcp-alpine listening on 0.0.0.0:3000");

    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            tokio::signal::ctrl_c().await.ok();
            cancel.cancel();
        })
        .await?;

    Ok(())
}
