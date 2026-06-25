use anyhow::{anyhow, Context};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

pub const PROJECT_ENV_FILENAME: &str = ".codex-opencode-adapter.env";

#[derive(Debug, Clone)]
pub struct ProjectPaths {
    pub root: PathBuf,
    pub env_file: PathBuf,
    pub agents_dir: PathBuf,
    pub state_dir: PathBuf,
}

impl ProjectPaths {
    pub fn from_current_dir() -> anyhow::Result<Self> {
        let root = std::env::current_dir().context("failed to resolve current directory")?;
        Ok(Self::from_root(root))
    }

    pub fn from_root(root: PathBuf) -> Self {
        Self {
            env_file: root.join(PROJECT_ENV_FILENAME),
            agents_dir: root.join(".codex").join("agents"),
            state_dir: root.join(".codex-opencode"),
            root,
        }
    }
}

pub fn read_project_env(path: &Path) -> anyhow::Result<HashMap<String, String>> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("failed to read project config at {}", path.display()))?;
    parse_env_text(&contents)
}

pub fn parse_env_text(contents: &str) -> anyhow::Result<HashMap<String, String>> {
    let mut values = HashMap::new();
    for (index, raw_line) in contents.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(anyhow!("invalid env line {}: {}", index + 1, raw_line));
        };
        values.insert(key.trim().to_string(), value.trim().to_string());
    }
    Ok(values)
}

pub fn current_environment() -> HashMap<String, String> {
    std::env::vars().collect()
}
