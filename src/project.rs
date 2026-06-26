use anyhow::{anyhow, Context};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const PROJECT_ENV_FILENAME: &str = ".codex-opencode-adapter.env";
const REGISTRY_FILENAME: &str = "project-registry.toml";
const ACTIVE_PROJECT_FILENAME: &str = "active-project.toml";
const ACTIVE_PROJECT_TTL_SECONDS: i64 = 300;

/// Generate a deterministic short hex hash from an input string.
pub fn hex_hash(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let result = hasher.finalize();
    result
        .iter()
        .take(6)
        .map(|b| format!("{b:02x}"))
        .collect::<String>()
}

/// Generate a project ID from the project root path.
pub fn generate_project_id(root: &Path) -> String {
    let canonical = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let hash = hex_hash(&canonical.display().to_string());
    format!("opencode_adapter_{hash}")
}

/// Sign a project_id into a bearer token using HMAC-SHA256.
/// Token format: codex-opencode-<project_id>-<hex_hmac>
pub fn sign_local_token(project_id: &str, secret: &str) -> String {
    type HmacSha256 = Hmac<Sha256>;
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC key should be valid");
    mac.update(project_id.as_bytes());
    let result = mac.finalize();
    let hmac_bytes = result.into_bytes();
    let hmac_hex: String = hmac_bytes
        .iter()
        .take(16)
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("codex-opencode-{project_id}-{hmac_hex}")
}

/// Parse and validate a signed token.
/// Returns the project_id if valid, None otherwise.
pub fn validate_signed_token(token: &str, secret: &str) -> Option<String> {
    let prefix = "codex-opencode-";
    let rest = token.strip_prefix(prefix)?;
    let hyphen_pos = rest.rfind('-')?;
    let project_id = &rest[..hyphen_pos];
    let received_hmac = &rest[hyphen_pos + 1..];
    let expected = sign_local_token(project_id, secret);
    let expected_rest = expected.strip_prefix(prefix)?;
    let expected_hmac = &expected_rest[project_id.len() + 1..];
    if received_hmac == expected_hmac {
        Some(project_id.to_string())
    } else {
        None
    }
}

/// Parse a candidate project_id from a signed token WITHOUT validation.
/// Used by authorize to select which project secret to validate against.
pub fn parse_project_id_from_token(token: &str) -> Option<String> {
    let prefix = "codex-opencode-";
    let rest = token.strip_prefix(prefix)?;
    let hyphen_pos = rest.rfind('-')?;
    Some(rest[..hyphen_pos].to_string())
}

// --------------------------------------------------------------------------
// Project registry
// --------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectRegistryEntry {
    pub root: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectRegistry {
    pub projects: HashMap<String, ProjectRegistryEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ActiveProject {
    project_id: String,
    #[serde(default)]
    updated_at: Option<i64>,
}

impl ProjectRegistryEntry {
    pub fn new(root: PathBuf) -> Self {
        let canonical = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        Self {
            root: canonical.display().to_string(),
        }
    }
}

impl ProjectRegistry {
    pub fn load(registry_dir: &Path) -> Self {
        let path = registry_dir.join(REGISTRY_FILENAME);
        let contents = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => return Self::default(),
        };
        toml_edit::de::from_str(&contents).unwrap_or_default()
    }

    pub fn save(&self, registry_dir: &Path) -> anyhow::Result<()> {
        let path = registry_dir.join(REGISTRY_FILENAME);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let contents = toml_edit::ser::to_string_pretty(self)?;
        fs::write(&path, contents)?;
        Ok(())
    }

    pub fn upsert_project(&mut self, project_id: &str, root: &Path) {
        let canonical = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        self.projects.insert(
            project_id.to_string(),
            ProjectRegistryEntry {
                root: canonical.display().to_string(),
            },
        );
    }

    pub fn resolve_root(&self, project_id: &str) -> Option<PathBuf> {
        self.projects
            .get(project_id)
            .map(|entry| PathBuf::from(&entry.root))
    }

    pub fn resolve_env_path(&self, project_id: &str) -> Option<PathBuf> {
        self.resolve_root(project_id)
            .map(|root| root.join(PROJECT_ENV_FILENAME))
    }
}

pub fn registry_dir_path() -> anyhow::Result<PathBuf> {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .map_err(|_| anyhow::anyhow!("failed to resolve user home directory"))?;
    Ok(PathBuf::from(home).join(".codex-opencode-adapter"))
}

// --------------------------------------------------------------------------
// Project paths
// --------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ProjectPaths {
    pub root: PathBuf,
    pub env_file: PathBuf,
    pub agents_dir: PathBuf,
    pub state_dir: PathBuf,
}

impl ProjectPaths {
    /// Resolve project using the multi-priority strategy.
    /// See [`resolve_project`] for the full resolution order.
    pub fn from_current_dir() -> anyhow::Result<Self> {
        resolve_project()
    }

    /// Find project rooted exactly at the current directory (used by init).
    /// Never walks ancestors, never uses thread context.
    pub fn from_init_dir() -> anyhow::Result<Self> {
        let cwd = std::env::current_dir().context("failed to resolve current directory")?;
        Ok(Self::from_root(cwd))
    }

    pub fn from_root(root: PathBuf) -> Self {
        Self {
            env_file: root.join(PROJECT_ENV_FILENAME),
            agents_dir: root.join(".codex").join("agents"),
            state_dir: root.join(".codex-opencode"),
            root,
        }
    }

    pub fn discover_from(start: &Path) -> Self {
        for candidate in start.ancestors() {
            let paths = Self::from_root(candidate.to_path_buf());
            if paths.env_file.exists() {
                return paths;
            }
        }
        Self::from_root(start.to_path_buf())
    }


}

pub fn remember_active_project(root: &Path) -> anyhow::Result<()> {
    let paths = ProjectPaths::discover_from(root);
    validate_recovered_project(&paths)?;
    let project_env = read_project_env(&paths.env_file)?;
    let project_id = project_env
        .get("CODEX_OPENCODE_PROJECT_ID")
        .ok_or_else(|| anyhow!("CODEX_OPENCODE_PROJECT_ID is missing in project env"))?;
    let registry_dir = registry_dir_path()?;
    fs::create_dir_all(&registry_dir)?;
    let active = ActiveProject {
        project_id: project_id.to_string(),
        updated_at: Some(now_ts()),
    };
    let contents = toml_edit::ser::to_string_pretty(&active)?;
    fs::write(registry_dir.join(ACTIVE_PROJECT_FILENAME), contents)?;
    Ok(())
}

fn now_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn active_project_root(registry: &ProjectRegistry) -> Option<PathBuf> {
    let registry_dir = registry_dir_path().ok()?;
    let contents = fs::read_to_string(registry_dir.join(ACTIVE_PROJECT_FILENAME)).ok()?;
    let active: ActiveProject = toml_edit::de::from_str(&contents).ok()?;
    let updated_at = active.updated_at?;
    if now_ts().saturating_sub(updated_at) > ACTIVE_PROJECT_TTL_SECONDS {
        return None;
    }
    registry.resolve_root(&active.project_id)
}

fn codex_thread_ids() -> Vec<String> {
    [
        "CODEX_THREAD_ID",
        "CODEX_SESSION_ID",
        "CODEX_CONVERSATION_ID",
        "CODEX_PARENT_THREAD_ID",
    ]
    .into_iter()
    .filter_map(|key| std::env::var(key).ok())
    .filter(|value| !value.trim().is_empty())
    .collect()
}

fn codex_home_dir() -> Option<PathBuf> {
    if let Ok(value) = std::env::var("CODEX_HOME") {
        if !value.trim().is_empty() {
            return Some(PathBuf::from(value));
        }
    }
    std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .ok()
        .map(|home| PathBuf::from(home).join(".codex"))
}

fn find_cwd_in_process_manager(codex_home: &Path, ids: &[String]) -> Option<PathBuf> {
    let path = codex_home
        .join("process_manager")
        .join("chat_processes.json");
    let contents = fs::read_to_string(path).ok()?;
    let value: Value = serde_json::from_str(&contents).ok()?;
    let entries = value.as_array()?;
    entries.iter().rev().find_map(|entry| {
        let matches_id = ["conversationId", "turnId", "id"]
            .iter()
            .filter_map(|key| entry.get(*key).and_then(Value::as_str))
            .any(|value| ids.iter().any(|id| value.contains(id)));
        if !matches_id {
            return None;
        }
        entry.get("cwd").and_then(Value::as_str).map(PathBuf::from)
    })
}

fn find_cwd_in_sessions(codex_home: &Path, ids: &[String]) -> Option<PathBuf> {
    let sessions_dir = codex_home.join("sessions");
    let mut files = Vec::new();
    collect_session_files(&sessions_dir, ids, &mut files);
    files.sort_by_key(|path| {
        fs::metadata(path)
            .and_then(|metadata| metadata.modified())
            .ok()
    });
    files
        .into_iter()
        .rev()
        .find_map(|path| find_cwd_in_session_file(&path))
}

fn collect_session_files(dir: &Path, ids: &[String], files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_session_files(&path, ids, files);
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if name.ends_with(".jsonl") && ids.iter().any(|id| name.contains(id)) {
            files.push(path);
        }
    }
}

fn find_cwd_in_session_file(path: &Path) -> Option<PathBuf> {
    let contents = fs::read_to_string(path).ok()?;
    contents.lines().find_map(|line| {
        let value: Value = serde_json::from_str(line).ok()?;
        let payload = value.get("payload")?;
        payload
            .get("cwd")
            .and_then(Value::as_str)
            .map(PathBuf::from)
            .or_else(|| {
                payload
                    .get("session_meta")
                    .and_then(|meta| meta.get("cwd"))
                    .and_then(Value::as_str)
                    .map(PathBuf::from)
            })
    })
}

/// Resolve the current project using a multi-priority strategy:
///
/// 1. **`CODEX_OPENCODE_PROJECT_ID` env var** — explicit registry lookup.
/// 2. **cwd / ancestor walk** — look for `.codex-opencode-adapter.env` upward.
/// 3. **Codex thread/session context** — recover cwd from process_manager or session files.
/// 4. **Constrained fallback** — single registered project only.
///
/// Returns a clear, actionable error when resolution is ambiguous or no project exists.
pub fn resolve_project() -> anyhow::Result<ProjectPaths> {
    let reg_dir = registry_dir_path()?;
    let registry = ProjectRegistry::load(&reg_dir);

    // Priority 1: Explicit CODEX_OPENCODE_PROJECT_ID
    if let Ok(pid_value) = std::env::var("CODEX_OPENCODE_PROJECT_ID") {
        let pid = pid_value.trim().to_string();
        if !pid.is_empty() {
            if let Some(root) = registry.resolve_root(&pid) {
                let paths = ProjectPaths::from_root(root);
                validate_recovered_project(&paths)?;
                return Ok(paths);
            }
            return Err(anyhow!(
                concat!(
                "CODEX_OPENCODE_PROJECT_ID is set to \"{pid}\" but no matching project was found in the registry.\n",
                " Run 'codex-opencode-adapter init' from the project root to register it."
            )
            ));
        }
    }

    // Priority 2: cwd / ancestor walk
    let cwd = std::env::current_dir()
        .context("failed to resolve current working directory")?;
    let discovered = ProjectPaths::discover_from(&cwd);
    if discovered.env_file.exists() {
        validate_recovered_project(&discovered)
            .context("found project env file but registry check failed")?;
        return Ok(discovered);
    }

    // Priority 3: Codex thread/session context
    let thread_ids = codex_thread_ids();
    if let Some(home) = codex_home_dir().as_ref() {
        if !thread_ids.is_empty() {
            if let Some(thread_cwd) = find_cwd_in_process_manager(home, &thread_ids)
                .or_else(|| find_cwd_in_sessions(home, &thread_ids))
            {
                let thread_paths = ProjectPaths::discover_from(&thread_cwd);
                if thread_paths.env_file.exists() {
                    validate_recovered_project(&thread_paths)
                        .context("found project via Codex thread context but registry check failed")?;
                    return Ok(thread_paths);
                }
            }
        }
    }

    // Priority 4: Short-lived active project marker. This exists for Codex
    // provider auth calls that currently run without cwd/thread context.
    if let Some(root) = active_project_root(&registry) {
        let paths = ProjectPaths::from_root(root);
        validate_recovered_project(&paths)?;
        tracing::info!("using recently active project for context-free provider auth");
        return Ok(paths);
    }

    // Priority 5: Constrained fallback -- single registered project only
    if registry.projects.is_empty() {
        return Err(anyhow!(
            concat!(
            "No OpenCode adapter projects found.\n", " Run 'codex-opencode-adapter init' from your project root to create one."
        )
        ));
    }
    if registry.projects.len() == 1 {
        let (pid, entry) = registry.projects.iter().next().unwrap();
        let root = PathBuf::from(&entry.root);
        let paths = ProjectPaths::from_root(root);
        validate_recovered_project(&paths)?;
        tracing::info!("using the only registered project: {pid}");
        return Ok(paths);
    }

    // Multiple projects with no context -> clear error with guidance
    let project_ids: Vec<&str> = registry.projects.keys().map(|k| k.as_str()).collect();
    Err(anyhow!(
        concat!(
        "Multiple OpenCode adapter projects are registered, but no project context was found.\n",
        " Registered projects: {}\n", " To fix this, set CODEX_OPENCODE_PROJECT_ID=<project_id>, run from a project ", "directory, or start Codex from within an initialized project directory."
    ),
        project_ids.join(", ")
    ))
}

fn validate_recovered_project(paths: &ProjectPaths) -> anyhow::Result<()> {
    anyhow::ensure!(paths.env_file.exists(), "project has no env file");
    let project_env = read_project_env(&paths.env_file)?;
    let project_id = project_env
        .get("CODEX_OPENCODE_PROJECT_ID")
        .ok_or_else(|| anyhow!("CODEX_OPENCODE_PROJECT_ID is missing in project env"))?;
    let registry = ProjectRegistry::load(&registry_dir_path()?);
    let registered_root = registry
        .resolve_root(project_id)
        .ok_or_else(|| anyhow!("project is not registered in the registry"))?;
    anyhow::ensure!(
        same_path(&registered_root, &paths.root),
        "project root does not match registry entry"
    );
    Ok(())
}

fn same_path(left: &Path, right: &Path) -> bool {
    let left = left.canonicalize().unwrap_or_else(|_| left.to_path_buf());
    let right = right.canonicalize().unwrap_or_else(|_| right.to_path_buf());
    if cfg!(windows) {
        left.to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy())
    } else {
        left == right
    }
}

// --------------------------------------------------------------------------
// Env file helpers
// --------------------------------------------------------------------------

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
