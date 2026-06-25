use anyhow::anyhow;
use std::collections::HashMap;
use std::net::SocketAddr;

pub const DEFAULT_HOST: &str = "127.0.0.1";
pub const DEFAULT_PORT: u16 = 4010;
pub const DEFAULT_UPSTREAM_BASE: &str = "https://opencode.ai/zen/go/v1";
pub const DEFAULT_STATE_DB: &str = ".codex-opencode/state.sqlite";
pub const DEFAULT_STATE_TTL_SECONDS: i64 = 21_600;
pub const DEFAULT_TIMEOUT_SECONDS: u64 = 300;
pub const DEFAULT_MAX_REQUEST_BYTES: usize = 8 * 1024 * 1024;
pub const DEFAULT_MAX_CONCURRENCY: usize = 8;

#[derive(Debug, Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub upstream_base: String,
    pub upstream_key: String,
    pub local_token: Option<String>,
    pub state_db: String,
    pub state_ttl_seconds: i64,
    pub timeout_seconds: u64,
    pub max_request_bytes: usize,
    pub max_concurrency: usize,
}

#[derive(Debug, Clone, Default)]
pub struct ConfigOverrides {
    pub host: Option<String>,
    pub port: Option<u16>,
    pub upstream_base: Option<String>,
    pub upstream_key: Option<String>,
    pub local_token: Option<String>,
    pub state_db: Option<String>,
    pub state_ttl_seconds: Option<i64>,
    pub timeout_seconds: Option<u64>,
    pub max_request_bytes: Option<usize>,
    pub max_concurrency: Option<usize>,
}

impl Config {
    pub fn addr(&self) -> anyhow::Result<SocketAddr> {
        Ok(format!("{}:{}", self.host, self.port).parse()?)
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        if self.local_token.as_deref() == Some(self.upstream_key.as_str()) {
            anyhow::bail!("CODEX_OPENCODE_LOCAL_TOKEN must differ from OPENCODE_GO_API_KEY");
        }
        if self.max_concurrency == 0 {
            anyhow::bail!("CODEX_OPENCODE_MAX_CONCURRENCY must be greater than zero");
        }
        Ok(())
    }

    pub fn from_sources(
        project_env: &HashMap<String, String>,
        env: &HashMap<String, String>,
        overrides: ConfigOverrides,
    ) -> anyhow::Result<Self> {
        let host = choose_string(
            overrides.host,
            project_env,
            env,
            "CODEX_OPENCODE_HOST",
            DEFAULT_HOST,
        );
        let port = choose_parse(
            overrides.port,
            project_env,
            env,
            "CODEX_OPENCODE_PORT",
            DEFAULT_PORT,
        )?;
        let upstream_base = choose_string(
            overrides.upstream_base,
            project_env,
            env,
            "OPENCODE_GO_BASE_URL",
            DEFAULT_UPSTREAM_BASE,
        );
        let upstream_key = choose_required_string(
            overrides.upstream_key,
            project_env,
            env,
            "OPENCODE_GO_API_KEY",
        )?;
        let local_token = choose_optional_string(
            overrides.local_token,
            project_env,
            env,
            "CODEX_OPENCODE_LOCAL_TOKEN",
        );
        let state_db = choose_string(
            overrides.state_db,
            project_env,
            env,
            "CODEX_OPENCODE_STATE_DB",
            DEFAULT_STATE_DB,
        );
        let state_ttl_seconds = choose_parse(
            overrides.state_ttl_seconds,
            project_env,
            env,
            "CODEX_OPENCODE_STATE_TTL_SECONDS",
            DEFAULT_STATE_TTL_SECONDS,
        )?;
        let timeout_seconds = choose_parse(
            overrides.timeout_seconds,
            project_env,
            env,
            "CODEX_OPENCODE_TIMEOUT_SECONDS",
            DEFAULT_TIMEOUT_SECONDS,
        )?;
        let max_request_bytes = choose_parse(
            overrides.max_request_bytes,
            project_env,
            env,
            "CODEX_OPENCODE_MAX_REQUEST_BYTES",
            DEFAULT_MAX_REQUEST_BYTES,
        )?;
        let max_concurrency = choose_parse(
            overrides.max_concurrency,
            project_env,
            env,
            "CODEX_OPENCODE_MAX_CONCURRENCY",
            DEFAULT_MAX_CONCURRENCY,
        )?;

        let config = Self {
            host,
            port,
            upstream_base,
            upstream_key,
            local_token,
            state_db,
            state_ttl_seconds,
            timeout_seconds,
            max_request_bytes,
            max_concurrency,
        };
        config.validate()?;
        Ok(config)
    }
}

fn choose_string(
    cli: Option<String>,
    project_env: &HashMap<String, String>,
    env: &HashMap<String, String>,
    key: &str,
    default: &str,
) -> String {
    cli.or_else(|| project_env.get(key).cloned())
        .or_else(|| env.get(key).cloned())
        .unwrap_or_else(|| default.to_string())
}

fn choose_optional_string(
    cli: Option<String>,
    project_env: &HashMap<String, String>,
    env: &HashMap<String, String>,
    key: &str,
) -> Option<String> {
    cli.or_else(|| project_env.get(key).cloned())
        .or_else(|| env.get(key).cloned())
}

fn choose_required_string(
    cli: Option<String>,
    project_env: &HashMap<String, String>,
    env: &HashMap<String, String>,
    key: &str,
) -> anyhow::Result<String> {
    choose_optional_string(cli, project_env, env, key)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow!("{key} is required"))
}

fn choose_parse<T>(
    cli: Option<T>,
    project_env: &HashMap<String, String>,
    env: &HashMap<String, String>,
    key: &str,
    default: T,
) -> anyhow::Result<T>
where
    T: std::str::FromStr + Copy,
    <T as std::str::FromStr>::Err: std::fmt::Display,
{
    if let Some(value) = cli {
        return Ok(value);
    }
    if let Some(value) = project_env.get(key) {
        return value
            .parse::<T>()
            .map_err(|error| anyhow!("invalid {key} value in project config: {error}"));
    }
    if let Some(value) = env.get(key) {
        return value
            .parse::<T>()
            .map_err(|error| anyhow!("invalid {key} value in environment: {error}"));
    }
    Ok(default)
}
