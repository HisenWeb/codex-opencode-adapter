use clap::Parser;
use codex_opencode_adapter::cli::{AuthCommands, Cli, Commands, RunArgs};
use codex_opencode_adapter::config::{
    Config, ConfigOverrides, DEFAULT_HOST, DEFAULT_MAX_CONCURRENCY, DEFAULT_PORT,
};
use codex_opencode_adapter::init::run_init;
use codex_opencode_adapter::project::{
    current_environment, read_project_env, registry_dir_path, remember_active_project,
    sign_local_token, ProjectPaths, ProjectRegistry, PROJECT_ENV_FILENAME,
};
use codex_opencode_adapter::server::{router, AppState, ProjectRuntime};
use codex_opencode_adapter::state::StateStore;
use codex_opencode_adapter::upstream::OpenCodeGoClient;
use std::sync::{Arc, RwLock};
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
                let project = ProjectPaths::from_current_dir()
                    .map_err(|_| anyhow::anyhow!("project env not found"))?;
                let _ = remember_active_project(&project.root);
                let project_env = read_project_env(&project.env_file)?;
                let project_id = project_env
                    .get("CODEX_OPENCODE_PROJECT_ID")
                    .ok_or_else(|| anyhow::anyhow!("CODEX_OPENCODE_PROJECT_ID is missing"))?;
                let signed = codex_opencode_adapter::project::sign_local_token(project_id, &token);
                println!("{signed}");
                Ok(())
            }
        },
    }
}

async fn run_server(args: RunArgs) -> anyhow::Result<()> {
    if let Ok(cwd) = std::env::current_dir() {
        let _ = remember_active_project(&cwd);
    }
    let reg_dir = registry_dir_path()?;
    let registry = ProjectRegistry::load(&reg_dir);
    if registry.projects.is_empty() {
        return Err(anyhow::anyhow!(
            "No projects found in registry. Run 'codex-opencode-adapter init' first."
        ));
    }

    // Shared config overrides used during startup and runtime refresh.
    let config_overrides = ConfigOverrides {
        host: args.host.clone(),
        port: args.port,
        upstream_base: None,
        upstream_key: None,
        local_token: None,
        state_db: None,
        state_ttl_seconds: None,
        timeout_seconds: None,
        max_request_bytes: None,
        max_concurrency: args.max_concurrency,
    };

    let mut projects = std::collections::HashMap::new();
    for (project_id, entry) in &registry.projects {
        let root = std::path::PathBuf::from(&entry.root);
        let env_path = root.join(PROJECT_ENV_FILENAME);
        if !env_path.exists() {
            tracing::warn!(
                "project {project_id} missing env file at {}, skipping",
                env_path.display()
            );
            continue;
        }
        let project_env = read_project_env(&env_path)?;
        let env = current_environment();
        let config = Config::from_sources(&project_env, &env, config_overrides.clone())?;
        let state_db_path = root.join(&config.state_db);
        let state = StateStore::new(
            state_db_path.display().to_string(),
            config.state_ttl_seconds,
        )?;
        let client = OpenCodeGoClient::new(
            &config.upstream_base,
            &config.upstream_key,
            config.timeout_seconds,
        )?;
        tracing::info!(
            "loaded project {project_id} with upstream_base={}",
            config.upstream_base
        );
        projects.insert(
            project_id.clone(),
            ProjectRuntime {
                config,
                client,
                state,
            },
        );
    }
    if projects.is_empty() {
        return Err(anyhow::anyhow!(
            "No valid projects could be loaded from the registry."
        ));
    }
    let host = args
        .host
        .clone()
        .unwrap_or_else(|| DEFAULT_HOST.to_string());
    let port = args.port.unwrap_or(DEFAULT_PORT);
    let max_concurrency = args.max_concurrency.unwrap_or(DEFAULT_MAX_CONCURRENCY);
    let addr: std::net::SocketAddr = format!("{}:{}", host, port).parse()?;
    tracing::info!(max_concurrency, "adapter concurrency limit configured");
    let app_state = AppState {
        projects: Arc::new(RwLock::new(projects)),
        capacity: Arc::new(Semaphore::new(max_concurrency)),
        config_overrides,
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
    // Sign the local token with project_id for HMAC validation
    let project = ProjectPaths::from_current_dir()?;
    let _ = remember_active_project(&project.root);
    let project_env = read_project_env(&project.env_file)?;
    let project_id = project_env
        .get("CODEX_OPENCODE_PROJECT_ID")
        .ok_or_else(|| anyhow::anyhow!("CODEX_OPENCODE_PROJECT_ID is missing in project env"))?;
    let raw_token = config
        .local_token
        .as_deref()
        .filter(|v| !v.is_empty())
        .ok_or_else(|| anyhow::anyhow!("CODEX_OPENCODE_LOCAL_TOKEN is missing"))?;
    let signed_token = sign_local_token(project_id, raw_token);

    let base = format!("http://{}:{}", config.host, config.port);
    let client = reqwest::Client::new();
    let health = client
        .get(format!("{base}/health"))
        .send()
        .await
        .map_err(|_| {
            anyhow::anyhow!("Adapter is not running. Start it with 'codex-opencode-adapter run' or 'codex-opencode-adapter start'.")
        })?;
    anyhow::ensure!(health.status().is_success(), "health check failed");

    let models = client
        .get(format!("{base}/v1/models"))
        .bearer_auth(&signed_token)
        .send()
        .await?;
    anyhow::ensure!(models.status().is_success(), "/v1/models check failed");
    println!("Adapter check passed.");
    Ok(())
}

fn load_project_config(args: RunArgs) -> anyhow::Result<Config> {
    let project = ProjectPaths::from_current_dir()?;
    anyhow::ensure!(
        project.env_file.exists(),
        "Project is not initialized. Run 'codex-opencode-adapter init' from the project root first."
    );
    let project_env = read_project_env(&project.env_file)?;
    // local_token must come only from CLI args or project .env file;
    // strip from process env to prevent accidental pollution.
    let mut env = current_environment();
    env.remove("CODEX_OPENCODE_LOCAL_TOKEN");
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
