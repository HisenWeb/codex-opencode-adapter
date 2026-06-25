use clap::Parser;
use codex_opencode_adapter::cli::{AuthCommands, Cli, Commands, RunArgs};
use codex_opencode_adapter::config::{Config, ConfigOverrides};
use codex_opencode_adapter::init::run_init;
use codex_opencode_adapter::project::{current_environment, read_project_env, ProjectPaths};
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

    let cli = Cli::parse();
    match cli.command {
        Commands::Init(args) => run_init(args),
        Commands::Run(args) | Commands::Start(args) => run_server(args).await,
        Commands::Check => run_check().await,
        Commands::Auth(args) => match args.command {
            AuthCommands::PrintLocalToken => {
                let config = load_project_config(RunArgs::default())?;
                let token = config
                    .local_token
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| anyhow::anyhow!("CODEX_OPENCODE_LOCAL_TOKEN is missing"))?;
                println!("{token}");
                Ok(())
            }
        },
    }
}

async fn run_server(args: RunArgs) -> anyhow::Result<()> {
    let config = load_project_config(args)?;
    let addr = config.addr()?;
    let client = OpenCodeGoClient::new(
        &config.upstream_base,
        &config.upstream_key,
        config.timeout_seconds,
    )?;
    let state = StateStore::new(&config.state_db, config.state_ttl_seconds)?;
    let max_concurrency = config.max_concurrency;
    tracing::info!(max_concurrency, "adapter concurrency limit configured");
    let app_state = AppState {
        config,
        client,
        state,
        capacity: Arc::new(Semaphore::new(max_concurrency)),
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

async fn run_check() -> anyhow::Result<()> {
    let config = load_project_config(RunArgs::default())?;
    let base = format!("http://{}:{}", config.host, config.port);
    let client = reqwest::Client::new();
    let health = client
        .get(format!("{base}/health"))
        .send()
        .await
        .map_err(|_| {
            anyhow::anyhow!("adapter is not running; start it with codex-opencode-adapter run")
        })?;
    anyhow::ensure!(health.status().is_success(), "health check failed");

    let mut request = client.get(format!("{base}/v1/models"));
    if let Some(token) = config.local_token.filter(|value| !value.is_empty()) {
        request = request.bearer_auth(token);
    }
    let models = request.send().await?;
    anyhow::ensure!(models.status().is_success(), "/v1/models check failed");
    println!("Adapter check passed.");
    Ok(())
}

fn load_project_config(args: RunArgs) -> anyhow::Result<Config> {
    let project = ProjectPaths::from_current_dir()?;
    anyhow::ensure!(
        project.env_file.exists(),
        "project is not initialized; run codex-opencode-adapter init from the project root first"
    );
    let project_env = read_project_env(&project.env_file)?;
    let env = current_environment();
    let overrides = ConfigOverrides {
        host: args.host,
        port: args.port,
        upstream_base: args.upstream_base,
        upstream_key: args.upstream_key,
        local_token: args.local_token,
        state_db: args.state_db,
        state_ttl_seconds: args.state_ttl_seconds,
        timeout_seconds: args.timeout_seconds,
        max_request_bytes: args.max_request_bytes,
        max_concurrency: args.max_concurrency,
    };
    Config::from_sources(&project_env, &env, overrides)
}
