use bytes::Bytes;
use futures::Stream;
use futures::StreamExt;
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, USER_AGENT};
use serde_json::Value;
use std::time::Duration;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum UpstreamError {
    #[error("OpenCode Go HTTP {status}: {message}")]
    Http { status: u16, message: String },
    #[error("OpenCode Go network error: {0}")]
    Network(String),
    #[error("OpenCode Go returned invalid response: {0}")]
    Invalid(String),
}

#[derive(Clone)]
pub struct OpenCodeGoClient {
    base_url: String,
    api_key: String,
    client: reqwest::Client,
}

impl OpenCodeGoClient {
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>, timeout_seconds: u64) -> anyhow::Result<Self> {
        Ok(Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key: api_key.into(),
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(timeout_seconds))
                .build()?,
        })
    }

    pub async fn models(&self) -> Result<Value, UpstreamError> {
        self.request_json(reqwest::Method::GET, "/models", None).await
    }

    pub async fn chat(&self, mut payload: Value) -> Result<Value, UpstreamError> {
        payload["stream"] = Value::Bool(false);
        self.request_json(reqwest::Method::POST, "/chat/completions", Some(payload)).await
    }

    pub async fn chat_stream(&self, mut payload: Value) -> Result<impl Stream<Item = Result<Bytes, reqwest::Error>>, UpstreamError> {
        payload["stream"] = Value::Bool(true);
        let response = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .headers(self.headers("text/event-stream")?)
            .json(&payload)
            .send()
            .await
            .map_err(|e| UpstreamError::Network(e.to_string()))?;
        if !response.status().is_success() {
            return Err(Self::error_from_response(response).await);
        }
        Ok(response.bytes_stream())
    }

    async fn request_json(&self, method: reqwest::Method, path: &str, payload: Option<Value>) -> Result<Value, UpstreamError> {
        let mut request = self
            .client
            .request(method, format!("{}{}", self.base_url, path))
            .headers(self.headers("application/json")?);
        if let Some(payload) = payload {
            request = request.json(&payload);
        }
        let response = request
            .send()
            .await
            .map_err(|e| UpstreamError::Network(e.to_string()))?;
        if !response.status().is_success() {
            return Err(Self::error_from_response(response).await);
        }
        response
            .json::<Value>()
            .await
            .map_err(|e| UpstreamError::Invalid(e.to_string()))
    }

    fn headers(&self, accept: &'static str) -> Result<reqwest::header::HeaderMap, UpstreamError> {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(AUTHORIZATION, format!("Bearer {}", self.api_key).parse().map_err(|e: http::header::InvalidHeaderValue| UpstreamError::Invalid(e.to_string()))?);
        headers.insert(CONTENT_TYPE, "application/json".parse().unwrap());
        headers.insert(ACCEPT, accept.parse().unwrap());
        headers.insert(USER_AGENT, "codex-opencode-adapter-rs/0.2".parse().unwrap());
        Ok(headers)
    }

    async fn error_from_response(response: reqwest::Response) -> UpstreamError {
        let status = response.status().as_u16();
        let body = response.text().await.unwrap_or_default();
        let message = serde_json::from_str::<Value>(&body)
            .ok()
            .and_then(|v| v.get("error").and_then(|e| e.get("message")).and_then(Value::as_str).map(ToString::to_string))
            .unwrap_or(body);
        UpstreamError::Http { status, message: message.chars().take(2000).collect() }
    }
}

pub fn parse_chat_sse_bytes(buffer: &mut String, bytes: &[u8]) -> Vec<String> {
    buffer.push_str(&String::from_utf8_lossy(bytes));
    let mut blocks = Vec::new();
    while let Some(pos) = buffer.find("\n\n") {
        let block = buffer[..pos].to_string();
        buffer.drain(..pos + 2);
        blocks.push(block);
    }
    blocks
}

pub fn sse_data_from_block(block: &str) -> Option<String> {
    let mut data = Vec::new();
    for line in block.lines() {
        if let Some(rest) = line.strip_prefix("data:") {
            data.push(rest.trim_start().to_string());
        }
    }
    if data.is_empty() { None } else { Some(data.join("\n")) }
}
