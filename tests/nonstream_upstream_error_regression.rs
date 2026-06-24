use axum::extract::State as AxumState;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use codex_opencode_adapter::config::Config;
use codex_opencode_adapter::server::{self, AppState};
use codex_opencode_adapter::state::StateStore;
use codex_opencode_adapter::upstream::OpenCodeGoClient;
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::{Mutex, Semaphore};

#[derive(Clone)]
struct MockState {
    received: Arc<Mutex<Vec<Value>>>,
}

#[tokio::test]
async fn nonstream_upstream_http_error_returns_responses_failed_body() {
    let (upstream_addr, _mock, received) = start_error_upstream().await;
    let adapter_addr = start_adapter(upstream_addr).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("http://{adapter_addr}/v1/responses"))
        .json(&json!({
            "model": "opencode-go/deepseek-v4-flash",
            "input": "Hello",
            "stream": false,
            "metadata": {"case": "nonstream-upstream-error"}
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_GATEWAY);
    let body: Value = resp.json().await.unwrap();

    assert_eq!(body["object"], "response");
    assert_eq!(body["status"], "failed");
    assert_eq!(body["model"], "opencode-go/deepseek-v4-flash");
    assert_eq!(body["metadata"]["case"], "nonstream-upstream-error");
    assert!(body["output"].as_array().unwrap().is_empty());
    assert_eq!(body["usage"]["input_tokens"], 0);
    assert_eq!(body["usage"]["output_tokens"], 0);
    assert_eq!(body["usage"]["total_tokens"], 0);
    assert_eq!(body["error"]["type"], "upstream_error");
    assert_eq!(body["error"]["code"], "upstream_error");
    assert!(body["error"]["message"]
        .as_str()
        .unwrap()
        .contains("upstream unavailable"));

    let received = received.lock().await;
    assert_eq!(received.len(), 1);
    assert_eq!(received[0]["model"], "deepseek-v4-flash");
}

async fn start_error_upstream() -> (
    SocketAddr,
    tokio::task::JoinHandle<()>,
    Arc<Mutex<Vec<Value>>>,
) {
    let received: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let state = MockState {
        received: Arc::clone(&received),
    };
    let app = Router::new()
        .route("/chat/completions", post(mock_chat_error))
        .with_state(state);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    (addr, handle, received)
}

async fn mock_chat_error(
    AxumState(state): AxumState<MockState>,
    Json(payload): Json<Value>,
) -> impl IntoResponse {
    state.received.lock().await.push(payload);
    (
        StatusCode::BAD_GATEWAY,
        Json(json!({"error": {"message": "upstream unavailable"}})),
    )
}

async fn start_adapter(upstream_addr: SocketAddr) -> SocketAddr {
    let db_path = std::env::temp_dir().join(format!(
        "nonstream_upstream_error_{}.sqlite",
        uuid::Uuid::new_v4()
    ));
    let config = Config {
        host: "127.0.0.1".to_string(),
        port: 0,
        upstream_base: format!("http://{upstream_addr}"),
        upstream_key: "test-api-key".to_string(),
        local_token: None,
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
    let app_state = AppState {
        config,
        client,
        state,
        capacity: Arc::new(Semaphore::new(10)),
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
