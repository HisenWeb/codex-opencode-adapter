use crate::cli::InitArgs;
use crate::config::{
    DEFAULT_HOST, DEFAULT_MAX_CONCURRENCY, DEFAULT_MAX_REQUEST_BYTES, DEFAULT_STATE_DB,
    DEFAULT_STATE_TTL_SECONDS, DEFAULT_TIMEOUT_SECONDS,
};
use crate::project::ProjectPaths;
use anyhow::{anyhow, Context};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use toml_edit::{value, Array, DocumentMut, Item, Table};
use uuid::Uuid;

const USER_DIR_NAME: &str = ".codex-opencode-adapter";
const INIT_LOG_FILE: &str = "init.log";

const OSS_FLASH_TEMPLATE: &str = include_str!("../resources/templates/oss-flash.toml");
const OSS_MIMO_TEMPLATE: &str = include_str!("../resources/templates/oss-mimo.toml");
const OSS_MINIMAX_TEMPLATE: &str = include_str!("../resources/templates/oss-minimax.toml");
const OSS_PRO_TEMPLATE: &str = include_str!("../resources/templates/oss-pro.toml");

pub fn run_init(args: InitArgs) -> anyhow::Result<()> {
    let project = ProjectPaths::from_current_dir()?;
    let logger = InitLogger::new()?;
    logger.log("init started")?;

    let api_key = match args.api_key {
        Some(value) if !value.trim().is_empty() => value,
        _ => prompt("OpenCode Go API key")?,
    };
    let local_token = format!("codex-opencode-{}", Uuid::new_v4().simple());

    let env_contents = format!(
        "OPENCODE_GO_API_KEY={api_key}\nCODEX_OPENCODE_LOCAL_TOKEN={local_token}\nCODEX_OPENCODE_HOST={host}\nCODEX_OPENCODE_PORT={port}\nOPENCODE_GO_BASE_URL={upstream_base}\nCODEX_OPENCODE_STATE_DB={state_db}\nCODEX_OPENCODE_STATE_TTL_SECONDS={ttl}\nCODEX_OPENCODE_TIMEOUT_SECONDS={timeout}\nCODEX_OPENCODE_MAX_REQUEST_BYTES={max_request_bytes}\nCODEX_OPENCODE_MAX_CONCURRENCY={max_concurrency}\n",
        host = args.host,
        port = args.port,
        upstream_base = args.upstream_base,
        state_db = DEFAULT_STATE_DB,
        ttl = DEFAULT_STATE_TTL_SECONDS,
        timeout = DEFAULT_TIMEOUT_SECONDS,
        max_request_bytes = DEFAULT_MAX_REQUEST_BYTES,
        max_concurrency = DEFAULT_MAX_CONCURRENCY,
    );

    let global_config_path = global_codex_config_path()?;
    let global_config_contents = build_global_codex_config(&global_config_path, args.port)?;
    let backup_path = create_backup_if_exists(&global_config_path)?;
    if let Some(path) = backup_path.as_ref() {
        logger.log(&format!("created global config backup: {}", path.display()))?;
    }

    let writes = vec![
        PendingWrite::new(global_config_path, global_config_contents.into_bytes()),
        PendingWrite::new(project.env_file.clone(), env_contents.into_bytes()),
        PendingWrite::new(
            project.agents_dir.join("oss-flash.toml"),
            OSS_FLASH_TEMPLATE.as_bytes().to_vec(),
        ),
        PendingWrite::new(
            project.agents_dir.join("oss-mimo.toml"),
            OSS_MIMO_TEMPLATE.as_bytes().to_vec(),
        ),
        PendingWrite::new(
            project.agents_dir.join("oss-minimax.toml"),
            OSS_MINIMAX_TEMPLATE.as_bytes().to_vec(),
        ),
        PendingWrite::new(
            project.agents_dir.join("oss-pro.toml"),
            OSS_PRO_TEMPLATE.as_bytes().to_vec(),
        ),
    ];

    if let Err(error) = apply_writes_with_rollback(writes, &logger) {
        logger.log(&format!("init failed: {error}"))?;
        return Err(error);
    }

    fs::create_dir_all(&project.state_dir)
        .with_context(|| format!("failed to create {}", project.state_dir.display()))?;
    logger.log("init completed successfully")?;
    println!("Initialization complete. Next: codex-opencode-adapter run");
    Ok(())
}

fn prompt(label: &str) -> anyhow::Result<String> {
    print!("{label}: ");
    io::stdout().flush()?;
    let mut value = String::new();
    io::stdin().read_line(&mut value)?;
    let value = value.trim().to_string();
    if value.is_empty() {
        return Err(anyhow!("{label} is required"));
    }
    Ok(value)
}

fn global_codex_config_path() -> anyhow::Result<PathBuf> {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .context("failed to resolve user home directory")?;
    Ok(PathBuf::from(home).join(".codex").join("config.toml"))
}

fn user_log_path() -> anyhow::Result<PathBuf> {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .context("failed to resolve user home directory")?;
    Ok(PathBuf::from(home).join(USER_DIR_NAME).join(INIT_LOG_FILE))
}

fn build_global_codex_config(path: &Path, port: u16) -> anyhow::Result<String> {
    let mut document = if path.exists() {
        fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?
            .parse::<DocumentMut>()
            .with_context(|| format!("failed to parse {}", path.display()))?
    } else {
        DocumentMut::new()
    };

    let providers = ensure_table(&mut document, "model_providers")?;
    let provider = ensure_subtable(providers, "opencode_go_adapter")?;
    provider["name"] = value("OpenCode Go Adapter");
    provider["base_url"] = value(format!("http://{}:{}/v1", DEFAULT_HOST, port));
    provider["wire_api"] = value("responses");
    provider["request_max_retries"] = value(0);
    provider["stream_max_retries"] = value(0);
    provider["stream_idle_timeout_ms"] = value(120000);

    let auth = ensure_subtable(provider, "auth")?;
    auth["command"] = value("codex-opencode-adapter");
    let mut args = Array::default();
    args.push("auth");
    args.push("print-local-token");
    auth["args"] = Item::Value(args.into());
    auth["timeout_ms"] = value(1000);

    Ok(document.to_string())
}

fn ensure_table<'a>(document: &'a mut DocumentMut, key: &str) -> anyhow::Result<&'a mut Table> {
    if !document.as_table().contains_key(key) {
        document[key] = Item::Table(Table::new());
    }
    document[key]
        .as_table_mut()
        .ok_or_else(|| anyhow!("{key} must be a TOML table"))
}

fn ensure_subtable<'a>(table: &'a mut Table, key: &str) -> anyhow::Result<&'a mut Table> {
    if !table.contains_key(key) {
        table[key] = Item::Table(Table::new());
    }
    table[key]
        .as_table_mut()
        .ok_or_else(|| anyhow!("{key} must be a TOML table"))
}

fn create_backup_if_exists(path: &Path) -> anyhow::Result<Option<PathBuf>> {
    if !path.exists() {
        return Ok(None);
    }
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let backup_path = path.with_extension(format!("toml.bak.{timestamp}"));
    fs::copy(path, &backup_path).with_context(|| format!("failed to backup {}", path.display()))?;
    Ok(Some(backup_path))
}

fn apply_writes_with_rollback(
    writes: Vec<PendingWrite>,
    logger: &InitLogger,
) -> anyhow::Result<()> {
    let mut applied = Vec::new();
    for write in writes {
        logger.log(&format!("writing {}", write.path.display()))?;
        match write.apply() {
            Ok(snapshot) => applied.push(snapshot),
            Err(error) => {
                logger.log(&format!("write failed, starting rollback: {error}"))?;
                for snapshot in applied.iter().rev() {
                    if let Err(rollback_error) = snapshot.rollback() {
                        logger.log(&format!(
                            "rollback failed for {}: {rollback_error}",
                            snapshot.path.display()
                        ))?;
                    } else {
                        logger.log(&format!("rolled back {}", snapshot.path.display()))?;
                    }
                }
                return Err(error);
            }
        }
    }
    Ok(())
}

struct PendingWrite {
    path: PathBuf,
    contents: Vec<u8>,
}

impl PendingWrite {
    fn new(path: PathBuf, contents: Vec<u8>) -> Self {
        Self { path, contents }
    }

    fn apply(&self) -> anyhow::Result<WrittenFile> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let original = if self.path.exists() {
            Some(
                fs::read(&self.path)
                    .with_context(|| format!("failed to snapshot {}", self.path.display()))?,
            )
        } else {
            None
        };

        let tmp = temp_path(&self.path);
        fs::write(&tmp, &self.contents)
            .with_context(|| format!("failed to write temp file {}", tmp.display()))?;
        if let Err(error) = replace_path(&tmp, &self.path) {
            let _ = fs::remove_file(&tmp);
            return Err(error);
        }
        Ok(WrittenFile {
            path: self.path.clone(),
            original,
        })
    }
}

struct WrittenFile {
    path: PathBuf,
    original: Option<Vec<u8>>,
}

impl WrittenFile {
    fn rollback(&self) -> anyhow::Result<()> {
        match &self.original {
            Some(contents) => fs::write(&self.path, contents)
                .with_context(|| format!("failed to restore {}", self.path.display())),
            None => {
                if self.path.exists() {
                    fs::remove_file(&self.path)
                        .with_context(|| format!("failed to remove {}", self.path.display()))?;
                }
                Ok(())
            }
        }
    }
}

fn temp_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("tmp");
    path.with_file_name(format!("{name}.tmp.{}", Uuid::new_v4().simple()))
}

#[cfg(not(windows))]
fn replace_path(source: &Path, destination: &Path) -> anyhow::Result<()> {
    fs::rename(source, destination).with_context(|| {
        format!(
            "failed to move temp file {} into {}",
            source.display(),
            destination.display()
        )
    })
}

#[cfg(windows)]
fn replace_path(source: &Path, destination: &Path) -> anyhow::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let source_wide: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination_wide: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();

    let flags = MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH;
    let ok = unsafe { MoveFileExW(source_wide.as_ptr(), destination_wide.as_ptr(), flags) };
    if ok == 0 {
        return Err(std::io::Error::last_os_error()).with_context(|| {
            format!(
                "failed to move temp file {} into {}",
                source.display(),
                destination.display()
            )
        });
    }
    Ok(())
}

struct InitLogger {
    path: PathBuf,
}

impl InitLogger {
    fn new() -> anyhow::Result<Self> {
        let path = user_log_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        Ok(Self { path })
    }

    fn log(&self, message: &str) -> anyhow::Result<()> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .with_context(|| format!("failed to open init log {}", self.path.display()))?;
        writeln!(file, "[{timestamp}] {message}")?;
        Ok(())
    }
}
