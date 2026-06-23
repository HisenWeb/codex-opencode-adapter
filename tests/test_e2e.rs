//! L2 integration tests: adapter ↔ mock upstream (no external dependency).
//!
//! Run: `cargo test --test test_e2e`
//!
//! Real smoke test (requires OPENCODE_GO_API_KEY):
//! `cargo test --test test_e2e test_e2e_real_smoke -- --ignored`

use axum::extract::State as AxumState;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Sse};
use axum::routing::post;
use axum::{Json, Router};
use futures::stream::Stream;
use serde_json::{json, Value};
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio_stream::StreamExt;

// ────────────────────────────────────────────────────────────
// Mock upstream server
// ────────────────────────────────────────────────────────────

/// Shared state for the mock upstream — records received payloads.
#[derive(Clone)]
struct MockState {
    received: Arc<Mutex<Vec<Value>>>,
}

/// Starts a mock OpenCode Go upstream server on a random port.
/// Returns (addr, join_handle, received_payloads_accessor).
async fn start_mock_upstream() -> (
    SocketAddr,
    tokio::task::JoinHandle<()>,
    Arc<Mutex<Vec<Value>>>,
) {
    let received: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let state = MockState {
        received: received.clone(),
    };

    let app = Router::new()
        .route("/chat/completions", post(mock_chat))
        .route("/models", post(mock_models).get(mock_models))
        .with_state(state);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    // Give the server a moment to start.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    (addr, handle, received)
}

async fn mock_chat(
    AxumState(state): AxumState<MockState>,
    Json(payload): Json<Value>,
) -> impl IntoResponse {
    // Record the received payload.
    state.received.lock().await.push(payload.clone());

    let is_stream = payload
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let model = payload
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();

    if is_stream {
        // Return SSE stream.
        let stream = mock_sse_stream(model);
        Sse::new(stream).into_response()
    } else {
        // Return JSON.
        Json(mock_completion(model)).into_response()
    }
}

fn mock_completion(model: String) -> Value {
    json!({
        "id": "chatcmpl-mock-001",
        "object": "chat.completion",
        "model": model,
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": "Hello from mock!"
            },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 10,
            "completion_tokens": 5,
            "total_tokens": 15
        }
    })
}

fn mock_sse_stream(
    model: String,
) -> impl Stream<Item = Result<axum::response::sse::Event, Infallible>> {
    let chunks = vec![
        json!({
            "id": "chatcmpl-mock-002",
            "object": "chat.completion.chunk",
            "model": model,
            "choices": [{
                "index": 0,
                "delta": {"role": "assistant", "content": "Hi"},
                "finish_reason": null
            }]
        }),
        json!({
            "id": "chatcmpl-mock-002",
            "object": "chat.completion.chunk",
            "model": model,
            "choices": [{
                "index": 0,
                "delta": {"content": " there"},
                "finish_reason": null
            }]
        }),
        json!({
            "id": "chatcmpl-mock-002",
            "object": "chat.completion.chunk",
            "model": model,
            "choices": [{
                "index": 0,
                "delta": {},
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 10, "completion_tokens": 2, "total_tokens": 12}
        }),
    ];

    futures::stream::iter(chunks)
        .map(|value| {
            let event = axum::response::sse::Event::default().data(value.to_string());
            Ok::<_, Infallible>(event)
        })
        .chain(futures::stream::once(async {
            Ok(axum::response::sse::Event::default().data("[DONE]"))
        }))
}

/// Mock streaming with reasoning_content chunks.
fn mock_reasoning_sse_stream(
    model: String,
) -> impl Stream<Item = Result<axum::response::sse::Event, Infallible>> {
    let chunks = vec![
        json!({
            "id": "chatcmpl-mock-r1",
            "object": "chat.completion.chunk",
            "model": model,
            "choices": [{
                "index": 0,
                "delta": {"reasoning_content": "Let me think..."},
                "finish_reason": null
            }]
        }),
        json!({
            "id": "chatcmpl-mock-r1",
            "object": "chat.completion.chunk",
            "model": model,
            "choices": [{
                "index": 0,
                "delta": {"content": "The answer is 42."},
                "finish_reason": null
            }]
        }),
        json!({
            "id": "chatcmpl-mock-r1",
            "object": "chat.completion.chunk",
            "model": model,
            "choices": [{
                "index": 0,
                "delta": {},
                "finish_reason": "stop"
            }]
        }),
    ];

    futures::stream::iter(chunks)
        .map(|value| {
            let event = axum::response::sse::Event::default().data(value.to_string());
            Ok::<_, Infallible>(event)
        })
        .chain(futures::stream::once(async {
            Ok(axum::response::sse::Event::default().data("[DONE]"))
        }))
}

/// Mock streaming with tool_call lifecycle chunks.
fn mock_tool_call_sse_stream(
    model: String,
) -> impl Stream<Item = Result<axum::response::sse::Event, Infallible>> {
    let chunks = vec![
        json!({
            "id": "chatcmpl-mock-t1",
            "object": "chat.completion.chunk",
            "model": model,
            "choices": [{
                "index": 0,
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "id": "call_abc",
                        "type": "function",
                        "function": {"name": "get_weather", "arguments": ""}
                    }]
                },
                "finish_reason": null
            }]
        }),
        json!({
            "id": "chatcmpl-mock-t1",
            "object": "chat.completion.chunk",
            "model": model,
            "choices": [{
                "index": 0,
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "function": {"arguments": "{\"city\":\"Tokyo\"}"}
                    }]
                },
                "finish_reason": null
            }]
        }),
        json!({
            "id": "chatcmpl-mock-t1",
            "object": "chat.completion.chunk",
            "model": model,
            "choices": [{
                "index": 0,
                "delta": {},
                "finish_reason": "tool_calls"
            }]
        }),
    ];

    futures::stream::iter(chunks)
        .map(|value| {
            let event = axum::response::sse::Event::default().data(value.to_string());
            Ok::<_, Infallible>(event)
        })
        .chain(futures::stream::once(async {
            Ok(axum::response::sse::Event::default().data("[DONE]"))
        }))
}

async fn mock_models() -> Json<Value> {
    Json(json!({
        "object": "list",
        "data": [
            {"id": "deepseek-v4-flash", "object": "model", "owned_by": "opencode-go"},
            {"id": "kimi-k2.6", "object": "model", "owned_by": "opencode-go"}
        ]
    }))
}

// ────────────────────────────────────────────────────────────
// Mock upstream: chat with reasoning
// ────────────────────────────────────────────────────────────

/// Upstream that returns a non-streaming response with reasoning_content.
async fn mock_chat_with_reasoning(
    AxumState(state): AxumState<MockState>,
    Json(payload): Json<Value>,
) -> impl IntoResponse {
    state.received.lock().await.push(payload.clone());
    let model = payload
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let is_stream = payload
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    if is_stream {
        let stream = mock_reasoning_sse_stream(model);
        Sse::new(stream).into_response()
    } else {
        Json(json!({
            "id": "chatcmpl-mock-r0",
            "object": "chat.completion",
            "model": model,
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "reasoning_content": "Let me think...",
                    "content": "The answer is 42."
                },
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 10, "completion_tokens": 20, "total_tokens": 30}
        }))
        .into_response()
    }
}

/// Upstream that returns a non-streaming response with tool_calls.
async fn mock_chat_with_tool_call(
    AxumState(state): AxumState<MockState>,
    Json(payload): Json<Value>,
) -> impl IntoResponse {
    state.received.lock().await.push(payload.clone());
    let model = payload
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let is_stream = payload
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    if is_stream {
        let stream = mock_tool_call_sse_stream(model);
        Sse::new(stream).into_response()
    } else {
        Json(json!({
            "id": "chatcmpl-mock-t0",
            "object": "chat.completion",
            "model": model,
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "tool_calls": [{
                        "id": "call_abc",
                        "type": "function",
                        "function": {
                            "name": "get_weather",
                            "arguments": "{\"city\":\"Tokyo\"}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
        }))
        .into_response()
    }
}

/// Upstream that always returns HTTP 500.
async fn mock_chat_error(
    AxumState(state): AxumState<MockState>,
    Json(payload): Json<Value>,
) -> impl IntoResponse {
    state.received.lock().await.push(payload.clone());
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({"error": {"message": "upstream internal error"}})),
    )
}

/// Upstream that sends an error chunk mid-stream.
fn mock_error_sse_stream() -> impl Stream<Item = Result<axum::response::sse::Event, Infallible>> {
    let chunks: Vec<Value> = vec![json!({
        "error": {"message": "stream broke mid-flight"}
    })];

    futures::stream::iter(chunks)
        .map(|value| {
            let event = axum::response::sse::Event::default().data(value.to_string());
            Ok::<_, Infallible>(event)
        })
        .chain(futures::stream::once(async {
            Ok(axum::response::sse::Event::default().data("[DONE]"))
        }))
}

async fn mock_chat_stream_error(
    AxumState(state): AxumState<MockState>,
    Json(payload): Json<Value>,
) -> impl IntoResponse {
    state.received.lock().await.push(payload.clone());
    let stream = mock_error_sse_stream();
    Sse::new(stream).into_response()
}

// ────────────────────────────────────────────────────────────
// Adapter server helpers
// ────────────────────────────────────────────────────────────

use codex_opencode_adapter::config::Config;
use codex_opencode_adapter::server::{self, AppState};
use codex_opencode_adapter::state::StateStore;
use codex_opencode_adapter::upstream::OpenCodeGoClient;
use tokio::sync::Semaphore;

async fn start_adapter(upstream_addr: SocketAddr, local_token: Option<String>) -> SocketAddr {
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

fn adapter_url(addr: SocketAddr, path: &str) -> String {
    format!("http://{}{}", addr, path)
}

// ────────────────────────────────────────────────────────────
// Specialized mock routers for different scenarios
// ────────────────────────────────────────────────────────────

/// Start mock upstream that handles reasoning responses.
async fn start_mock_upstream_reasoning() -> (
    SocketAddr,
    tokio::task::JoinHandle<()>,
    Arc<Mutex<Vec<Value>>>,
) {
    let received: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let state = MockState {
        received: received.clone(),
    };

    let app = Router::new()
        .route("/chat/completions", post(mock_chat_with_reasoning))
        .route("/models", post(mock_models).get(mock_models))
        .with_state(state);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    (addr, handle, received)
}

/// Start mock upstream that handles tool_call responses.
async fn start_mock_upstream_tool_call() -> (
    SocketAddr,
    tokio::task::JoinHandle<()>,
    Arc<Mutex<Vec<Value>>>,
) {
    let received: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let state = MockState {
        received: received.clone(),
    };

    let app = Router::new()
        .route("/chat/completions", post(mock_chat_with_tool_call))
        .route("/models", post(mock_models).get(mock_models))
        .with_state(state);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    (addr, handle, received)
}

/// Start mock upstream that returns HTTP 500 errors.
async fn start_mock_upstream_error() -> (
    SocketAddr,
    tokio::task::JoinHandle<()>,
    Arc<Mutex<Vec<Value>>>,
) {
    let received: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let state = MockState {
        received: received.clone(),
    };

    let app = Router::new()
        .route("/chat/completions", post(mock_chat_error))
        .route("/models", post(mock_models).get(mock_models))
        .with_state(state);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    (addr, handle, received)
}

/// Start mock upstream that sends error chunk in stream.
async fn start_mock_upstream_stream_error() -> (
    SocketAddr,
    tokio::task::JoinHandle<()>,
    Arc<Mutex<Vec<Value>>>,
) {
    let received: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let state = MockState {
        received: received.clone(),
    };

    let app = Router::new()
        .route("/chat/completions", post(mock_chat_stream_error))
        .route("/models", post(mock_models).get(mock_models))
        .with_state(state);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    (addr, handle, received)
}

// ────────────────────────────────────────────────────────────
// Helper to parse SSE response body into events
// ────────────────────────────────────────────────────────────

/// Parse an SSE text body into (event_name, data_json) pairs.
fn parse_sse_events(body: &str) -> Vec<(String, Value)> {
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

// ────────────────────────────────────────────────────────────
// L2 Tests
// ────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_e2e_nonstreaming_text() {
    let (upstream_addr, _mock, _received) = start_mock_upstream().await;
    let adapter_addr = start_adapter(upstream_addr, None).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(adapter_url(adapter_addr, "/v1/responses"))
        .json(&json!({
            "model": "opencode-go/deepseek-v4-flash",
            "input": "Hello",
            "stream": false
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200, "non-streaming should return 200");
    let body: Value = resp.json().await.unwrap();

    // Verify Responses API structure.
    assert!(body.get("output").is_some(), "response must have output");
    let output = body["output"].as_array().unwrap();
    assert!(!output.is_empty(), "output must not be empty");

    // Should have a message item.
    let message_items: Vec<_> = output.iter().filter(|o| o["type"] == "message").collect();
    assert_eq!(
        message_items.len(),
        1,
        "should have exactly one message item"
    );

    // Content should contain mock response.
    let content_text: String = message_items[0]["content"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["text"].as_str())
        .collect();
    assert!(
        content_text.contains("Hello from mock!"),
        "content should contain mock response text"
    );

    // Model field should be present.
    assert!(
        body.get("model").is_some(),
        "response must have model field"
    );

    // Usage should be present.
    assert!(body.get("usage").is_some(), "response must have usage");
}

#[tokio::test]
async fn test_e2e_streaming_text() {
    let (upstream_addr, _mock, _received) = start_mock_upstream().await;
    let adapter_addr = start_adapter(upstream_addr, None).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(adapter_url(adapter_addr, "/v1/responses"))
        .json(&json!({
            "model": "opencode-go/deepseek-v4-flash",
            "input": "Hello",
            "stream": true
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200, "streaming should return 200");

    let body_text = resp.text().await.unwrap();
    let events = parse_sse_events(&body_text);

    // Should have output_text.delta events.
    let text_deltas: Vec<_> = events
        .iter()
        .filter(|(name, _)| name == "response.output_text.delta")
        .collect();
    assert!(
        !text_deltas.is_empty(),
        "should have output_text.delta events"
    );

    // Should have a response.completed terminal event.
    let completed = events.iter().find(|(name, _)| name == "response.completed");
    assert!(completed.is_some(), "should have response.completed event");

    // Final completed event should have output.
    let completed_data = &completed.unwrap().1;
    assert!(
        completed_data.get("response").is_some(),
        "completed event should have response"
    );
}

#[tokio::test]
async fn test_e2e_nonstreaming_with_reasoning() {
    let (upstream_addr, _mock, _received) = start_mock_upstream_reasoning().await;
    let adapter_addr = start_adapter(upstream_addr, None).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(adapter_url(adapter_addr, "/v1/responses"))
        .json(&json!({
            "model": "opencode-go/deepseek-v4-flash",
            "input": "Think about it",
            "stream": false
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    let output = body["output"].as_array().unwrap();

    // Should have a reasoning item.
    let reasoning_items: Vec<_> = output.iter().filter(|o| o["type"] == "reasoning").collect();
    assert_eq!(
        reasoning_items.len(),
        1,
        "should have exactly one reasoning item"
    );

    // Reasoning content should contain expected text.
    // Note: reasoning_item uses "summary" field, not "content".
    let reasoning_summary: String = reasoning_items[0]["summary"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["text"].as_str())
        .collect();
    assert!(
        reasoning_summary.contains("Let me think..."),
        "reasoning summary should contain expected text"
    );

    // Should also have a message item after reasoning.
    let message_items: Vec<_> = output.iter().filter(|o| o["type"] == "message").collect();
    assert_eq!(
        message_items.len(),
        1,
        "should have exactly one message item"
    );
}

#[tokio::test]
async fn test_e2e_streaming_with_reasoning() {
    let (upstream_addr, _mock, _received) = start_mock_upstream_reasoning().await;
    let adapter_addr = start_adapter(upstream_addr, None).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(adapter_url(adapter_addr, "/v1/responses"))
        .json(&json!({
            "model": "opencode-go/deepseek-v4-flash",
            "input": "Think about it",
            "stream": true
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body_text = resp.text().await.unwrap();
    let events = parse_sse_events(&body_text);

    // Should have reasoning summary text delta events.
    let reasoning_deltas: Vec<_> = events
        .iter()
        .filter(|(name, _)| name == "response.reasoning_summary_text.delta")
        .collect();
    assert!(
        !reasoning_deltas.is_empty(),
        "should have reasoning_summary_text.delta events"
    );

    // Should have output_text delta events for the content.
    let text_deltas: Vec<_> = events
        .iter()
        .filter(|(name, _)| name == "response.output_text.delta")
        .collect();
    assert!(
        !text_deltas.is_empty(),
        "should have output_text.delta events"
    );

    // Should have response.completed.
    assert!(
        events.iter().any(|(name, _)| name == "response.completed"),
        "should have response.completed event"
    );
}

#[tokio::test]
async fn test_e2e_nonstreaming_tool_call() {
    let (upstream_addr, _mock, _received) = start_mock_upstream_tool_call().await;
    let adapter_addr = start_adapter(upstream_addr, None).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(adapter_url(adapter_addr, "/v1/responses"))
        .json(&json!({
            "model": "opencode-go/deepseek-v4-flash",
            "input": "What's the weather?",
            "tools": [{
                "type": "function",
                "name": "get_weather",
                "description": "Get weather for a city",
                "parameters": {"type": "object", "properties": {"city": {"type": "string"}}}
            }],
            "stream": false
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    let output = body["output"].as_array().unwrap();

    // Should have a function_call item.
    let tool_items: Vec<_> = output
        .iter()
        .filter(|o| o["type"] == "function_call")
        .collect();
    assert_eq!(
        tool_items.len(),
        1,
        "should have exactly one function_call item"
    );

    let tool = &tool_items[0];
    assert_eq!(
        tool["name"].as_str().unwrap(),
        "get_weather",
        "tool name should match"
    );
    assert_eq!(
        tool["call_id"].as_str().unwrap(),
        "call_abc",
        "call_id should match"
    );

    let args: Value = serde_json::from_str(tool["arguments"].as_str().unwrap()).unwrap();
    assert_eq!(
        args["city"].as_str().unwrap(),
        "Tokyo",
        "arguments should contain city"
    );
}

#[tokio::test]
async fn test_e2e_streaming_tool_call() {
    let (upstream_addr, _mock, _received) = start_mock_upstream_tool_call().await;
    let adapter_addr = start_adapter(upstream_addr, None).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(adapter_url(adapter_addr, "/v1/responses"))
        .json(&json!({
            "model": "opencode-go/deepseek-v4-flash",
            "input": "What's the weather?",
            "tools": [{
                "type": "function",
                "name": "get_weather",
                "description": "Get weather for a city",
                "parameters": {"type": "object", "properties": {"city": {"type": "string"}}}
            }],
            "stream": true
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body_text = resp.text().await.unwrap();
    let events = parse_sse_events(&body_text);

    // Should have output_item.added for the function_call.
    let added_events: Vec<_> = events
        .iter()
        .filter(|(name, _)| name == "response.output_item.added")
        .collect();
    assert!(
        !added_events.is_empty(),
        "should have output_item.added events"
    );

    // Should have function_call_arguments.delta events.
    let arg_deltas: Vec<_> = events
        .iter()
        .filter(|(name, _)| name == "response.function_call_arguments.delta")
        .collect();
    assert!(
        !arg_deltas.is_empty(),
        "should have function_call_arguments.delta events"
    );

    // Should have function_call_arguments.done with complete args.
    let arg_done: Vec<_> = events
        .iter()
        .filter(|(name, _)| name == "response.function_call_arguments.done")
        .collect();
    assert_eq!(
        arg_done.len(),
        1,
        "should have exactly one function_call_arguments.done"
    );
    let done_args = &arg_done[0].1;
    assert!(
        done_args["arguments"].as_str().unwrap().contains("Tokyo"),
        "done arguments should contain Tokyo"
    );

    // Should have response.completed.
    assert!(
        events.iter().any(|(name, _)| name == "response.completed"),
        "should have response.completed event"
    );
}

#[tokio::test]
async fn test_e2e_upstream_http_error() {
    let (upstream_addr, _mock, _received) = start_mock_upstream_error().await;
    let adapter_addr = start_adapter(upstream_addr, None).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(adapter_url(adapter_addr, "/v1/responses"))
        .json(&json!({
            "model": "opencode-go/deepseek-v4-flash",
            "input": "Hello",
            "stream": false
        }))
        .send()
        .await
        .unwrap();

    // UpstreamError::Http preserves the upstream status code.
    assert_eq!(
        resp.status(),
        500,
        "upstream HTTP error status should be preserved"
    );
    let body: Value = resp.json().await.unwrap();
    assert!(
        body["error"]["type"].as_str().unwrap() == "upstream_error",
        "error type should be upstream_error"
    );
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("upstream internal error"),
        "error message should contain upstream message"
    );
}

#[tokio::test]
async fn test_e2e_upstream_stream_error() {
    let (upstream_addr, _mock, _received) = start_mock_upstream_stream_error().await;
    let adapter_addr = start_adapter(upstream_addr, None).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(adapter_url(adapter_addr, "/v1/responses"))
        .json(&json!({
            "model": "opencode-go/deepseek-v4-flash",
            "input": "Hello",
            "stream": true
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        200,
        "streaming errors come through the SSE stream, not HTTP status"
    );
    let body_text = resp.text().await.unwrap();
    let events = parse_sse_events(&body_text);

    // Should have a response.failed event.
    let failed = events.iter().find(|(name, _)| name == "response.failed");
    assert!(
        failed.is_some(),
        "should have response.failed event for stream error"
    );
}

#[tokio::test]
async fn test_e2e_request_payload_shape() {
    let (upstream_addr, _mock, received) = start_mock_upstream().await;
    let adapter_addr = start_adapter(upstream_addr, None).await;
    let client = reqwest::Client::new();

    let _ = client
        .post(adapter_url(adapter_addr, "/v1/responses"))
        .json(&json!({
            "model": "opencode-go/deepseek-v4-flash",
            "instructions": "You are a helpful assistant.",
            "input": [{"type": "message", "role": "user", "content": "Hi"}],
            "stream": false
        }))
        .send()
        .await
        .unwrap();

    // Give a moment for the mock to record the payload.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let payloads = received.lock().await;
    assert_eq!(
        payloads.len(),
        1,
        "mock should have received exactly one payload"
    );

    let payload = &payloads[0];

    // Model prefix should be stripped.
    assert_eq!(
        payload["model"].as_str().unwrap(),
        "deepseek-v4-flash",
        "model prefix should be stripped"
    );

    // Should have messages array.
    let messages = payload["messages"].as_array().unwrap();
    assert!(!messages.is_empty(), "messages should not be empty");

    // First message should include the system instruction.
    let first_content = messages[0]["content"].as_str().unwrap();
    assert!(
        first_content.contains("You are a helpful assistant"),
        "system instruction should be in messages"
    );

    // stream should be false (adapter forces it).
    assert_eq!(
        payload["stream"].as_bool().unwrap(),
        false,
        "stream should be false for non-streaming"
    );
}

#[tokio::test]
async fn test_e2e_request_payload_streaming_shape() {
    let (upstream_addr, _mock, received) = start_mock_upstream().await;
    let adapter_addr = start_adapter(upstream_addr, None).await;
    let client = reqwest::Client::new();

    let _ = client
        .post(adapter_url(adapter_addr, "/v1/responses"))
        .json(&json!({
            "model": "opencode-go/deepseek-v4-flash",
            "input": "Hi",
            "stream": true
        }))
        .send()
        .await
        .unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let payloads = received.lock().await;
    assert_eq!(payloads.len(), 1);

    let payload = &payloads[0];

    // stream should be true.
    assert_eq!(payload["stream"].as_bool().unwrap(), true);

    // stream_options should include_usage.
    assert_eq!(
        payload["stream_options"]["include_usage"]
            .as_bool()
            .unwrap(),
        true,
        "stream_options.include_usage should be true"
    );
}

#[tokio::test]
async fn test_e2e_auth_required() {
    let (upstream_addr, _mock, _received) = start_mock_upstream().await;
    let adapter_addr = start_adapter(upstream_addr, Some("my-secret-token".to_string())).await;
    let client = reqwest::Client::new();

    // Without auth → 401.
    let resp = client
        .post(adapter_url(adapter_addr, "/v1/responses"))
        .json(&json!({
            "model": "opencode-go/deepseek-v4-flash",
            "input": "Hello",
            "stream": false
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401, "should return 401 without auth");

    // With auth → 200.
    let resp = client
        .post(adapter_url(adapter_addr, "/v1/responses"))
        .header("Authorization", "Bearer my-secret-token")
        .json(&json!({
            "model": "opencode-go/deepseek-v4-flash",
            "input": "Hello",
            "stream": false
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "should return 200 with correct auth");
}

#[tokio::test]
async fn test_e2e_missing_model_prefix() {
    let (upstream_addr, _mock, _received) = start_mock_upstream().await;
    let adapter_addr = start_adapter(upstream_addr, None).await;
    let client = reqwest::Client::new();

    // Model without opencode-go/ prefix should be rejected.
    let resp = client
        .post(adapter_url(adapter_addr, "/v1/responses"))
        .json(&json!({
            "model": "deepseek-v4-flash",
            "input": "Hello",
            "stream": false
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400, "model without prefix should return 400");
}

// ────────────────────────────────────────────────────────────
// Real smoke test (conditional on OPENCODE_GO_API_KEY)
// ────────────────────────────────────────────────────────────

#[tokio::test]
#[ignore] // Run explicitly: `cargo test test_e2e_real_smoke -- --ignored`
async fn test_e2e_real_smoke() {
    let api_key = match std::env::var("OPENCODE_GO_API_KEY") {
        Ok(key) if !key.is_empty() => key,
        _ => {
            eprintln!("SKIP: OPENCODE_GO_API_KEY not set");
            return;
        }
    };

    let temp_dir = std::env::temp_dir();
    let db_name = format!("test_e2e_smoke_{}.sqlite", uuid::Uuid::new_v4());
    let db_path = temp_dir.join(db_name);

    let config = Config {
        host: "127.0.0.1".to_string(),
        port: 0,
        upstream_base: "https://opencode.ai/zen/go/v1".to_string(),
        upstream_key: api_key,
        local_token: None,
        state_db: db_path.to_string_lossy().to_string(),
        state_ttl_seconds: 21_600,
        timeout_seconds: 60,
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
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let http_client = reqwest::Client::new();

    // Non-streaming smoke test.
    let resp = http_client
        .post(format!("http://{}/v1/responses", addr))
        .json(&json!({
            "model": "opencode-go/deepseek-v4-flash",
            "input": "Say hello in one word",
            "stream": false
        }))
        .send()
        .await
        .expect("real smoke test request should succeed");

    assert_eq!(resp.status(), 200, "real smoke test should return 200");
    let body: Value = resp.json().await.unwrap();
    let output = body["output"].as_array().unwrap();
    assert!(
        !output.is_empty(),
        "real smoke test output should not be empty"
    );

    eprintln!(
        "Real smoke test passed: {}",
        serde_json::to_string_pretty(&body).unwrap_or_default()
    );
}
