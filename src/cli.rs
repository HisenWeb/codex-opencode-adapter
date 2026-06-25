use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "codex-opencode-adapter")]
#[command(about = "OpenCode Go adapter and project initializer for Codex")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    Init(InitArgs),
    Run(RunArgs),
    Start(RunArgs),
    Check,
    Auth(AuthArgs),
}

#[derive(Debug, Args, Clone)]
pub struct InitArgs {
    #[arg(long)]
    pub api_key: Option<String>,

    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,

    #[arg(long, default_value_t = 4010)]
    pub port: u16,

    #[arg(long, default_value = "https://opencode.ai/zen/go/v1")]
    pub upstream_base: String,
}

#[derive(Debug, Args, Clone, Default)]
pub struct RunArgs {
    #[arg(long)]
    pub host: Option<String>,
    #[arg(long)]
    pub port: Option<u16>,
    #[arg(long)]
    pub upstream_base: Option<String>,
    #[arg(long)]
    pub upstream_key: Option<String>,
    #[arg(long)]
    pub local_token: Option<String>,
    #[arg(long)]
    pub state_db: Option<String>,
    #[arg(long)]
    pub state_ttl_seconds: Option<i64>,
    #[arg(long)]
    pub timeout_seconds: Option<u64>,
    #[arg(long)]
    pub max_request_bytes: Option<usize>,
    #[arg(long)]
    pub max_concurrency: Option<usize>,
}

#[derive(Debug, Args)]
pub struct AuthArgs {
    #[command(subcommand)]
    pub command: AuthCommands,
}

#[derive(Debug, Subcommand)]
pub enum AuthCommands {
    PrintLocalToken,
}
