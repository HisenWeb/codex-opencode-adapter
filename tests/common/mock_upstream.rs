#![allow(dead_code)]

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

#[derive(Clone)]
struct MockState {
    received: Arc<Mutex<Vec<Value>>>,
}

pub async fn start_mock_upstream() -> (
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
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    (addr, handle, received)
}

async fn mock_chat(
    AxumState(state): AxumState<MockState>,
    Json(payload): Json<Value>,
) -> impl IntoResponse {
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
        let stream = mock_sse_stream(model);
        Sse::new(stream).into_response()
    } else {
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

pub async fn start_mock_upstream_reasoning() -> (
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

pub async fn start_mock_upstream_tool_call() -> (
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

pub async fn start_mock_upstream_error() -> (
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

pub async fn start_mock_upstream_stream_error() -> (
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
