use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response, Sse};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::{Stream, StreamExt};
use serde_json::{json, Value};
use std::convert::Infallible;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::Semaphore;

use crate::config::Config;
use crate::conversion::{build_chat_payload, build_response, function_output_call_ids, StreamAssembler};
use crate::state::StateStore;
use crate::upstream::{parse_chat_sse_bytes, sse_data_from_block, OpenCodeGoClient, UpstreamError};

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub client: OpenCodeGoClient,
    pub state: StateStore,
    pub capacity: Arc<Semaphore>,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/v1/models", get(models))
        .route("/models", get(models))
        .route("/v1/responses", post(responses))
        .route("/responses", post(responses))
        .with_state(state)
}

async fn health() -> Json<Value> {
    Json(json!({"status":"ok"}))
}

async fn models(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = authorize(&state.config, &headers) {
        return response;
    }
    match state.client.models().await {
        Ok(raw) => {
            let rows = raw.get("data").and_then(Value::as_array).cloned().unwrap_or_default();
            let data = rows
                .into_iter()
                .filter_map(|row| row.get("id").and_then(Value::as_str).map(|id| json!({"id":format!("opencode-go/{id}"),"object":"model","owned_by":"opencode-go"})))
                .collect::<Vec<_>>();
            Json(json!({"object":"list","data":data})).into_response()
        }
        Err(error) => upstream_error(error),
    }
}

async fn responses(State(state): State<AppState>, headers: HeaderMap, body: String) -> Response {
    if let Err(response) = authorize(&state.config, &headers) {
        return response;
    }
    if body.is_empty() || body.len() > state.config.max_request_bytes {
        return error_response(StatusCode::PAYLOAD_TOO_LARGE, "request_too_large", "Invalid request size");
    }
    let body: Value = match serde_json::from_str(&body) {
        Ok(Value::Object(_)) => serde_json::from_str(&body).unwrap(),
        Ok(_) => return error_response(StatusCode::BAD_REQUEST, "invalid_request_error", "request body must be an object"),
        Err(error) => return error_response(StatusCode::BAD_REQUEST, "invalid_request_error", &error.to_string()),
    };

    if body.get("stream").and_then(Value::as_bool).unwrap_or(false) {
        stream_response(state, body).await
    } else {
        complete_response(state, body).await
    }
}

async fn complete_response(state: AppState, body: Value) -> Response {
    let model_alias = match body.get("model").and_then(Value::as_str) {
        Some(model) => model.to_string(),
        None => return error_response(StatusCode::BAD_REQUEST, "invalid_request_error", "model is required"),
    };
    let model_upstream = match strip_model_prefix(&model_alias) {
        Ok(model) => model,
        Err(message) => return error_response(StatusCode::BAD_REQUEST, "invalid_request_error", message),
    };
    let previous = match previous_response(&state, &body) {
        Ok(previous) => previous,
        Err(message) => return error_response(StatusCode::BAD_REQUEST, "invalid_request_error", &message),
    };
    let (payload, messages, reverse) = match build_chat_payload(&body, &model_upstream, previous.as_ref(), json!({})) {
        Ok(value) => value,
        Err(error) => return error_response(StatusCode::BAD_REQUEST, "invalid_request_error", &error.to_string()),
    };
    let permit = match state.capacity.clone().try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => return error_response(StatusCode::TOO_MANY_REQUESTS, "rate_limit_error", "adapter concurrency limit reached"),
    };
    let upstream = state.client.chat(payload).await;
    drop(permit);
    let upstream = match upstream {
        Ok(value) => value,
        Err(error) => return upstream_error(error),
    };
    match build_response(&body, &upstream, &model_alias, &model_upstream, &messages, &reverse, |item| state.state.put(&item)) {
        Ok(response) => Json(response).into_response(),
        Err(error) => error_response(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", &error.to_string()),
    }
}

async fn stream_response(state: AppState, body: Value) -> Response {
    let model_alias = match body.get("model").and_then(Value::as_str) {
        Some(model) => model.to_string(),
        None => return error_response(StatusCode::BAD_REQUEST, "invalid_request_error", "model is required"),
    };
    let model_upstream = match strip_model_prefix(&model_alias) {
        Ok(model) => model,
        Err(message) => return error_response(StatusCode::BAD_REQUEST, "invalid_request_error", message),
    };
    let previous = match previous_response(&state, &body) {
        Ok(previous) => previous,
        Err(message) => return error_response(StatusCode::BAD_REQUEST, "invalid_request_error", &message),
    };
    let (payload, messages, reverse) = match build_chat_payload(&body, &model_upstream, previous.as_ref(), json!({})) {
        Ok(value) => value,
        Err(error) => return error_response(StatusCode::BAD_REQUEST, "invalid_request_error", &error.to_string()),
    };

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Result<axum::response::sse::Event, Infallible>>();
    let state_for_task = state.clone();
    tokio::spawn(async move {
        let emit_tx = tx.clone();
        let mut assembler = StreamAssembler::new(
            body.clone(),
            model_alias.clone(),
            model_upstream.clone(),
            messages,
            reverse,
            Box::new(move |item| state_for_task.state.put(&item)),
            Box::new(move |event, data| {
                let sse = axum::response::sse::Event::default().event(event.to_string()).data(data.to_string());
                let _ = emit_tx.send(Ok(sse));
                Ok(())
            }),
        );
        let _ = assembler.start();
        let permit = match state.capacity.clone().try_acquire_owned() {
            Ok(permit) => permit,
            Err(_) => {
                let _ = assembler.fail("rate_limit_error", "adapter concurrency limit reached");
                let _ = tx.send(Ok(axum::response::sse::Event::default().data("[DONE]")));
                return;
            }
        };
        let upstream = state.client.chat_stream(payload).await;
        drop(permit);
        match upstream {
            Ok(mut stream) => {
                let mut buffer = String::new();
                while let Some(chunk) = stream.next().await {
                    match chunk {
                        Ok(bytes) => {
                            for block in parse_chat_sse_bytes(&mut buffer, &bytes) {
                                if let Some(data) = sse_data_from_block(&block) {
                                    if data.trim() == "[DONE]" {
                                        let _ = assembler.finalize();
                                        let _ = tx.send(Ok(axum::response::sse::Event::default().data("[DONE]")));
                                        return;
                                    }
                                    if let Ok(value) = serde_json::from_str::<Value>(&data) {
                                        if value.get("error").is_some() {
                                            let message = value.get("error").and_then(|e| e.get("message")).and_then(Value::as_str).unwrap_or("upstream stream error");
                                            let _ = assembler.fail("upstream_error", message);
                                            let _ = tx.send(Ok(axum::response::sse::Event::default().data("[DONE]")));
                                            return;
                                        }
                                        let _ = assembler.accept(&value);
                                    }
                                }
                            }
                        }
                        Err(error) => {
                            let _ = assembler.fail("upstream_error", &error.to_string());
                            let _ = tx.send(Ok(axum::response::sse::Event::default().data("[DONE]")));
                            return;
                        }
                    }
                }
                let _ = assembler.finalize();
                let _ = tx.send(Ok(axum::response::sse::Event::default().data("[DONE]")));
            }
            Err(error) => {
                let _ = assembler.fail("upstream_error", &error.to_string());
                let _ = tx.send(Ok(axum::response::sse::Event::default().data("[DONE]")));
            }
        }
    });

    let stream = tokio_stream::wrappers::UnboundedReceiverStream::new(rx);
    Sse::new(stream).into_response()
}

fn previous_response(state: &AppState, body: &Value) -> Result<Option<crate::state::StoredResponse>, String> {
    if let Some(previous_id) = body.get("previous_response_id").and_then(Value::as_str).filter(|v| !v.is_empty()) {
        return state.state.get(previous_id).map_err(|e| e.to_string());
    }
    let ids = function_output_call_ids(body).map_err(|e| e.to_string())?;
    if ids.is_empty() {
        Ok(None)
    } else {
        state.state.find_by_call_ids(&ids).map_err(|e| e.to_string())
    }
}

fn authorize(config: &Config, headers: &HeaderMap) -> Result<(), Response> {
    let Some(token) = config.local_token.as_ref().filter(|token| !token.is_empty()) else {
        return Ok(());
    };
    let expected = format!("Bearer {token}");
    if headers.get("authorization").and_then(|v| v.to_str().ok()) == Some(expected.as_str()) {
        Ok(())
    } else {
        Err(error_response(StatusCode::UNAUTHORIZED, "unauthorized", "Unauthorized"))
    }
}

fn strip_model_prefix(model: &str) -> Result<String, &'static str> {
    let Some(rest) = model.strip_prefix("opencode-go/") else {
        return Err("model must use the opencode-go/ prefix");
    };
    if rest.is_empty() {
        Err("model id is empty")
    } else {
        Ok(rest.to_string())
    }
}

fn error_response(status: StatusCode, kind: &str, message: &str) -> Response {
    (status, Json(json!({"error":{"type":kind,"message":message}}))).into_response()
}

fn upstream_error(error: UpstreamError) -> Response {
    match error {
        UpstreamError::Http { status, message } => error_response(StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY), "upstream_error", &message),
        UpstreamError::Network(message) | UpstreamError::Invalid(message) => error_response(StatusCode::BAD_GATEWAY, "upstream_error", &message),
    }
}
