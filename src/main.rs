use clap::Parser;
use codex_opencode_adapter::config::Config;
use codex_opencode_adapter::server::{router, AppState};
use codex_opencode_adapter::state::StateStore;
use codex_opencode_adapter::upstream::OpenCodeGoClient;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("codex_opencode_adapter=info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    let config = Config::parse();
    if config.local_token.as_deref() == Some(config.upstream_key.as_str()) {
        anyhow::bail!("CODEX_OPENCODE_LOCAL_TOKEN must differ from OPENCODE_GO_API_KEY");
    }
    let addr = config.addr()?;
    let client = OpenCodeGoClient::new(
        &config.upstream_base,
        &config.upstream_key,
        config.timeout_seconds,
    )?;
    let state = StateStore::new(&config.state_db, config.state_ttl_seconds)?;
    let app_state = AppState {
        config,
        client,
        state,
        capacity: Arc::new(Semaphore::new(8)),
    };
    let app = router(app_state);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("listening on http://{}", addr);
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
