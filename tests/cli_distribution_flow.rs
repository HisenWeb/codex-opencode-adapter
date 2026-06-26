use axum::http::{header::AUTHORIZATION, HeaderMap, StatusCode};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Arc;
use uuid::Uuid;

#[test]
fn init_writes_project_files_and_auth_prints_local_token() {
    let sandbox = TestSandbox::new("init-success");
    let output = sandbox.run(["init", "--api-key", "test-api-key"]);
    assert_success(&output);

    let env_text =
        fs::read_to_string(sandbox.project().join(".codex-opencode-adapter.env")).unwrap();
    assert!(env_text.contains("OPENCODE_GO_API_KEY=test-api-key"));
    assert!(env_text.contains("CODEX_OPENCODE_STATE_DB=.codex-opencode/state.sqlite"));

    let token = env_text
        .lines()
        .find_map(|line| line.strip_prefix("CODEX_OPENCODE_LOCAL_TOKEN="))
        .unwrap()
        .to_string();
    assert!(token.starts_with("codex-opencode-"));

    for name in [
        "oss-flash.toml",
        "oss-mimo.toml",
        "oss-minimax.toml",
        "oss-pro.toml",
    ] {
        assert!(
            sandbox
                .project()
                .join(".codex")
                .join("agents")
                .join(name)
                .exists(),
            "missing agent template: {name}"
        );
    }

    let config = fs::read_to_string(sandbox.home().join(".codex").join("config.toml")).unwrap();
    assert!(config.contains("[model_providers.opencode_go_adapter]"));
    assert!(config.contains("command = \"codex-opencode-adapter\""));
    assert!(config.contains("args = [\"auth\", \"print-local-token\"]"));

    let auth_output = sandbox.run(["auth", "print-local-token"]);
    assert_success(&auth_output);
    assert_eq!(stdout(&auth_output).trim(), token);

    let nested_dir = sandbox.project().join("src").join("nested");
    fs::create_dir_all(&nested_dir).unwrap();
    let nested_auth_output = sandbox.run_in(&nested_dir, ["auth", "print-local-token"]);
    assert_success(&nested_auth_output);
    assert_eq!(stdout(&nested_auth_output).trim(), token);

    let external_dir = sandbox.root().join("external");
    fs::create_dir_all(&external_dir).unwrap();
    sandbox.write_process_manager(&token, "thread-from-process-manager");
    let external_auth_output = sandbox.run_in_with_env(
        &external_dir,
        ["auth", "print-local-token"],
        [("CODEX_THREAD_ID", "thread-from-process-manager")],
    );
    assert_success(&external_auth_output);
    assert_eq!(stdout(&external_auth_output).trim(), token);

    let ambient_process_auth_output =
        sandbox.run_in(&external_dir, ["auth", "print-local-token"]);
    assert_success(&ambient_process_auth_output);
    assert_eq!(stdout(&ambient_process_auth_output).trim(), token);

    fs::remove_file(sandbox.process_manager_path()).unwrap();
    sandbox.write_session_meta("thread-from-session-meta");
    let session_auth_output = sandbox.run_in_with_env(
        &external_dir,
        ["auth", "print-local-token"],
        [("CODEX_THREAD_ID", "thread-from-session-meta")],
    );
    assert_success(&session_auth_output);
    assert_eq!(stdout(&session_auth_output).trim(), token);

    let ambient_session_auth_output =
        sandbox.run_in(&external_dir, ["auth", "print-local-token"]);
    assert_success(&ambient_session_auth_output);
    assert_eq!(stdout(&ambient_session_auth_output).trim(), token);
}

#[test]
fn init_updates_existing_provider_preserves_other_config_and_creates_backup() {
    let sandbox = TestSandbox::new("init-update");
    let config_dir = sandbox.home().join(".codex");
    fs::create_dir_all(&config_dir).unwrap();
    let config_path = config_dir.join("config.toml");
    let original = r#"[foo]
keep = true

[model_providers.other]
name = "Other"

[model_providers.opencode_go_adapter]
name = "Old"
base_url = "http://127.0.0.1:9999/v1"
wire_api = "responses"

[model_providers.opencode_go_adapter.auth]
command = "cmd.exe"
args = ["/d", "/s", "/c", "echo old"]
timeout_ms = 1000
"#;
    fs::write(&config_path, original).unwrap();

    let output = sandbox.run(["init", "--api-key", "test-api-key"]);
    assert_success(&output);

    let updated = fs::read_to_string(&config_path).unwrap();
    assert!(updated.contains("[foo]"));
    assert!(updated.contains("keep = true"));
    assert!(updated.contains("[model_providers.other]"));
    assert!(updated.contains("name = \"Other\""));
    assert!(updated.contains("name = \"OpenCode Go Adapter\""));
    assert!(updated.contains("command = \"codex-opencode-adapter\""));
    assert!(!updated.contains("echo old"));

    let backups = fs::read_dir(&config_dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .filter(|name| name.starts_with("config.toml.bak."))
        .collect::<Vec<_>>();
    assert!(!backups.is_empty(), "expected a config backup");
}

#[test]
fn init_rolls_back_when_agent_write_fails() {
    let sandbox = TestSandbox::new("init-rollback");
    let config_dir = sandbox.home().join(".codex");
    fs::create_dir_all(&config_dir).unwrap();
    let config_path = config_dir.join("config.toml");
    let original = "[preexisting]\nvalue = 1\n";
    fs::write(&config_path, original).unwrap();

    fs::write(sandbox.project().join(".codex"), "blocking file").unwrap();

    let output = sandbox.run(["init", "--api-key", "test-api-key"]);
    assert!(!output.status.success(), "init should have failed");
    assert!(stderr(&output).contains("failed to create"));
    assert_eq!(fs::read_to_string(&config_path).unwrap(), original);
    assert!(!sandbox
        .project()
        .join(".codex-opencode-adapter.env")
        .exists());

    let log_path = sandbox
        .home()
        .join(".codex-opencode-adapter")
        .join("init.log");
    let log_text = fs::read_to_string(log_path).unwrap();
    assert!(log_text.contains("write failed, starting rollback"));
}

#[test]
fn auth_run_and_start_require_init() {
    let sandbox = TestSandbox::new("not-initialized");

    for args in [
        vec!["auth", "print-local-token"],
        vec!["run"],
        vec!["start"],
    ] {
        let output = sandbox.run(args);
        assert!(!output.status.success());
        assert!(
            stderr(&output).contains(
                "project is not initialized; run codex-opencode-adapter init from the project root first"
            ),
            "stderr was: {}",
            stderr(&output)
        );
    }
}

#[test]
fn check_uses_project_env_and_succeeds() {
    let sandbox = TestSandbox::new("check-success");
    let local_token = Arc::new("project-local-token".to_string());
    let app = Router::new()
        .route("/health", get(|| async { Json(json!({ "status": "ok" })) }))
        .route(
            "/v1/models",
            get({
                let local_token = Arc::clone(&local_token);
                move |headers: HeaderMap| models_handler(headers, Arc::clone(&local_token))
            }),
        );
    let std_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = std_listener.local_addr().unwrap();
    std_listener.set_nonblocking(true).unwrap();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async move {
            let listener = tokio::net::TcpListener::from_std(std_listener).unwrap();
            axum::serve(listener, app).await.unwrap();
        });
    });
    std::thread::sleep(std::time::Duration::from_millis(50));

    fs::write(
        sandbox.project().join(".codex-opencode-adapter.env"),
        format!(
            "OPENCODE_GO_API_KEY=test-api-key\nCODEX_OPENCODE_LOCAL_TOKEN={token}\nCODEX_OPENCODE_HOST=127.0.0.1\nCODEX_OPENCODE_PORT={port}\n",
            token = local_token.as_str(),
            port = addr.port()
        ),
    )
    .unwrap();

    let output = sandbox.run(["check"]);
    assert_success(&output);
    assert!(stdout(&output).contains("Adapter check passed."));
}

async fn models_handler(
    headers: HeaderMap,
    expected_token: Arc<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    let auth = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if auth != format!("Bearer {}", expected_token.as_str()) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "unauthorized" })),
        );
    }
    (
        StatusCode::OK,
        Json(json!({
            "data": [
                { "id": "opencode-go/deepseek-v4-flash" }
            ]
        })),
    )
}

struct TestSandbox {
    root: PathBuf,
    project: PathBuf,
    home: PathBuf,
}

impl TestSandbox {
    fn new(label: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("codex-opencode-adapter-{label}-{}", Uuid::new_v4()));
        let project = root.join("project");
        let home = root.join("home");
        fs::create_dir_all(&project).unwrap();
        fs::create_dir_all(&home).unwrap();
        Self {
            root,
            project,
            home,
        }
    }

    fn run<I, S>(&self, args: I) -> Output
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.run_in(&self.project, args)
    }

    fn run_in<I, S>(&self, current_dir: &Path, args: I) -> Output
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.run_in_with_env(current_dir, args, std::iter::empty::<(&str, &str)>())
    }

    fn run_in_with_env<I, S, J, K, V>(&self, current_dir: &Path, args: I, envs: J) -> Output
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
        J: IntoIterator<Item = (K, V)>,
        K: AsRef<str>,
        V: AsRef<str>,
    {
        let mut command = Command::new(binary_path());
        for arg in args {
            command.arg(arg.as_ref());
        }
        for (key, value) in envs {
            command.env(key.as_ref(), value.as_ref());
        }
        command
            .current_dir(current_dir)
            .env("USERPROFILE", &self.home)
            .env("HOME", &self.home)
            .output()
            .unwrap()
    }

    fn write_process_manager(&self, token: &str, thread_id: &str) {
        let content = format!(
            "[{{\"conversationId\":\"{thread_id}\",\"cwd\":\"{}\",\"command\":\"auth\",\"itemId\":\"call_1\",\"updatedAtMs\":1}}]",
            escape_json_path(self.project())
        );
        let path = self.process_manager_path();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();

        let env_path = self.project().join(".codex-opencode-adapter.env");
        let env_text = fs::read_to_string(&env_path).unwrap();
        assert!(env_text.contains(token));
    }

    fn write_session_meta(&self, thread_id: &str) {
        let session_path = self
            .home()
            .join(".codex")
            .join("sessions")
            .join("2026")
            .join("06")
            .join("26")
            .join(format!("rollout-{thread_id}.jsonl"));
        fs::create_dir_all(session_path.parent().unwrap()).unwrap();
        let content = format!(
            "{{\"timestamp\":\"2026-06-26T09:09:30.375Z\",\"type\":\"session_meta\",\"payload\":{{\"session_id\":\"{thread_id}\",\"id\":\"child-thread\",\"cwd\":\"{}\"}}}}\n",
            escape_json_path(self.project())
        );
        fs::write(session_path, content).unwrap();
    }

    fn process_manager_path(&self) -> PathBuf {
        self.home()
            .join(".codex")
            .join("process_manager")
            .join("chat_processes.json")
    }

    fn project(&self) -> &Path {
        &self.project
    }

    fn root(&self) -> &Path {
        &self.root
    }

    fn home(&self) -> &Path {
        &self.home
    }
}

impl Drop for TestSandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn binary_path() -> &'static str {
    env!("CARGO_BIN_EXE_codex-opencode-adapter")
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        stdout(output),
        stderr(output)
    );
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn escape_json_path(path: &Path) -> String {
    path.display().to_string().replace('\\', "\\\\")
}
