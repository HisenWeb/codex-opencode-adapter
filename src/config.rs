use clap::Parser;
use std::net::SocketAddr;

#[derive(Debug, Clone, Parser)]
#[command(name = "codex-opencode-adapter")]
pub struct Config {
    #[arg(long, env = "CODEX_OPENCODE_HOST", default_value = "127.0.0.1")]
    pub host: String,

    #[arg(long, env = "CODEX_OPENCODE_PORT", default_value_t = 4010)]
    pub port: u16,

    #[arg(long, env = "OPENCODE_GO_BASE_URL", default_value = "https://opencode.ai/zen/go/v1")]
    pub upstream_base: String,

    #[arg(long, env = "OPENCODE_GO_API_KEY")]
    pub upstream_key: String,

    #[arg(long, env = "CODEX_OPENCODE_LOCAL_TOKEN")]
    pub local_token: Option<String>,

    #[arg(long, env = "CODEX_OPENCODE_STATE_DB", default_value = ".codex-opencode/state.sqlite")]
    pub state_db: String,

    #[arg(long, env = "CODEX_OPENCODE_STATE_TTL_SECONDS", default_value_t = 21_600)]
    pub state_ttl_seconds: i64,

    #[arg(long, env = "CODEX_OPENCODE_TIMEOUT_SECONDS", default_value_t = 300)]
    pub timeout_seconds: u64,

    #[arg(long, env = "CODEX_OPENCODE_MAX_REQUEST_BYTES", default_value_t = 8 * 1024 * 1024)]
    pub max_request_bytes: usize,
}

impl Config {
    pub fn addr(&self) -> anyhow::Result<SocketAddr> {
        Ok(format!("{}:{}", self.host, self.port).parse()?)
    }
}
