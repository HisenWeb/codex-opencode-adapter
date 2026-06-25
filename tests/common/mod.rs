#![allow(dead_code)]

pub mod mock_upstream;

use codex_opencode_adapter::config::Config;
use codex_opencode_adapter::server::{self, AppState};
use codex_opencode_adapter::state::StateStore;
use codex_opencode_adapter::upstream::OpenCodeGoClient;
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::Semaphore;

pub async fn start_adapter(upstream_addr: SocketAddr, local_token: Option<String>) -> SocketAddr {
    let temp_dir = std::env::temp_dir();
    let db_name = format!("test_e2e_{}.sqlite", uuid::Uuid::new_v4());
    let db_path = temp_dir.join(db_name);

    let upstream_base = format!("http://{}", upstream_addr);
    let config = Config {
        host: "127.0.0.1".to_string(),
        port: 0,
        upstream_base,
        upstream_key: "test-api-key".to_string(),
        local_token,
        state_db: db_path.to_string_lossy().to_string(),
        state_ttl_seconds: 21_600,
        timeout_seconds: 30,
        max_request_bytes: 8 * 1024 * 1024,
    };

    let client = OpenCodeGoClient::new(
        &config.upstream_base,
        &config.upstream_key,
        config.timeout_seconds,
    )
    .unwrap();
    let state = StateStore::new(&config.state_db, config.state_ttl_seconds).unwrap();
    let capacity = Arc::new(Semaphore::new(10));

    let app_state = AppState {
        config,
        client,
        state,
        capacity,
    };
    let app = server::router(app_state);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    addr
}

pub fn adapter_url(addr: SocketAddr, path: &str) -> String {
    format!("http://{}{}", addr, path)
}

pub struct RealSmokeConfig {
    pub upstream_base: String,
    pub upstream_key: String,
    pub text_model: String,
    pub vision_model: String,
}

impl RealSmokeConfig {
    pub fn from_env() -> Option<Self> {
        let upstream_key = match std::env::var("OPENCODE_GO_API_KEY") {
            Ok(key) if !key.is_empty() => key,
            _ => {
                eprintln!("SKIP: OPENCODE_GO_API_KEY not set");
                return None;
            }
        };

        Some(Self {
            upstream_base: std::env::var("OPENCODE_GO_BASE_URL")
                .unwrap_or_else(|_| "https://opencode.ai/zen/go/v1".to_string()),
            upstream_key,
            text_model: std::env::var("OPENCODE_GO_REAL_TEXT_MODEL")
                .unwrap_or_else(|_| "opencode-go/deepseek-v4-flash".to_string()),
            vision_model: std::env::var("OPENCODE_GO_REAL_VISION_MODEL")
                .unwrap_or_else(|_| "opencode-go/mimo-v2.5".to_string()),
        })
    }
}

pub async fn start_real_adapter(config: &RealSmokeConfig) -> SocketAddr {
    let temp_dir = std::env::temp_dir();
    let db_name = format!("test_e2e_smoke_{}.sqlite", uuid::Uuid::new_v4());
    let db_path = temp_dir.join(db_name);

    let adapter_config = Config {
        host: "127.0.0.1".to_string(),
        port: 0,
        upstream_base: config.upstream_base.clone(),
        upstream_key: config.upstream_key.clone(),
        local_token: None,
        state_db: db_path.to_string_lossy().to_string(),
        state_ttl_seconds: 21_600,
        timeout_seconds: 60,
        max_request_bytes: 8 * 1024 * 1024,
    };

    let client = OpenCodeGoClient::new(
        &adapter_config.upstream_base,
        &adapter_config.upstream_key,
        adapter_config.timeout_seconds,
    )
    .unwrap();
    let state =
        StateStore::new(&adapter_config.state_db, adapter_config.state_ttl_seconds).unwrap();
    let capacity = Arc::new(Semaphore::new(4));

    let app_state = AppState {
        config: adapter_config,
        client,
        state,
        capacity,
    };
    let app = server::router(app_state);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    addr
}

#[allow(dead_code)]
pub fn real_multimodal_input() -> Value {
    const TEST_IMAGE_BASE64: &str = "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";

    json!([{
        "type": "message",
        "role": "user",
        "content": [
            {"type": "input_text", "text": "Describe the image briefly and mention the visible word."},
            {
                "type": "input_image",
                "image_url": format!("data:image/png;base64,{TEST_IMAGE_BASE64}")
            }
        ]
    }])
}

pub fn extract_output_text(body: &Value) -> String {
    body.get("output")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("message"))
        .flat_map(|item| {
            item.get("content")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|part| part.get("text").and_then(Value::as_str))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn find_output_item<'a>(body: &'a Value, item_type: &str) -> Option<&'a Value> {
    body.get("output")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|item| item.get("type").and_then(Value::as_str) == Some(item_type))
}

pub fn real_multimodal_smoke_input() -> Value {
    const TEST_IMAGE_BASE64: &str =
        "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+a7d0AAAAASUVORK5CYII=";

    json!([{
        "type": "message",
        "role": "user",
        "content": [
            {"type": "input_text", "text": "Acknowledge that you received an image."},
            {
                "type": "input_image",
                "image_url": format!("data:image/png;base64,{TEST_IMAGE_BASE64}")
            }
        ]
    }])
}

pub fn parse_sse_events(body: &str) -> Vec<(String, Value)> {
    let mut events = Vec::new();
    let mut current_event = String::new();

    for block in body.split("\n\n") {
        let mut event_name = String::new();
        let mut data_lines = Vec::new();

        for line in block.lines() {
            if let Some(rest) = line.strip_prefix("event:") {
                event_name = rest.trim().to_string();
            } else if let Some(rest) = line.strip_prefix("data:") {
                data_lines.push(rest.trim().to_string());
            }
        }

        if data_lines.is_empty() {
            continue;
        }

        let data_str = data_lines.join("\n");
        if data_str == "[DONE]" {
            continue;
        }

        if event_name.is_empty() {
            current_event.clone_from(&event_name);
        }

        match serde_json::from_str::<Value>(&data_str) {
            Ok(value) => events.push((event_name.clone(), value)),
            Err(_) => continue,
        }
    }
    events
}
