use bytes::Bytes;
use futures::Stream;
// use futures::StreamExt;
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
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        timeout_seconds: u64,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key: api_key.into(),
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(timeout_seconds))
                .build()?,
        })
    }

    pub async fn models(&self) -> Result<Value, UpstreamError> {
        self.request_json(reqwest::Method::GET, "/models", None)
            .await
    }

    pub async fn chat(&self, mut payload: Value) -> Result<Value, UpstreamError> {
        payload["stream"] = Value::Bool(false);
        self.request_json(reqwest::Method::POST, "/chat/completions", Some(payload))
            .await
    }

    pub async fn chat_stream(
        &self,
        mut payload: Value,
    ) -> Result<impl Stream<Item = Result<Bytes, reqwest::Error>>, UpstreamError> {
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

    async fn request_json(
        &self,
        method: reqwest::Method,
        path: &str,
        payload: Option<Value>,
    ) -> Result<Value, UpstreamError> {
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
        headers.insert(
            AUTHORIZATION,
            format!("Bearer {}", self.api_key).parse().map_err(
                |e: http::header::InvalidHeaderValue| UpstreamError::Invalid(e.to_string()),
            )?,
        );
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
            .and_then(|v| extract_error_message(&v))
            .unwrap_or(body);
        UpstreamError::Http {
            status,
            message: message.chars().take(2000).collect(),
        }
    }
}

/// Extract error message from upstream response body, handling multiple formats:
/// 1. Standard OpenAI: {"error": {"message": "..."}}
/// 2. MiniMax: {"base_resp": {"status_code": 2013, "status_msg": "..."}}
/// 3. Top-level message/detail fields
/// 4. Bare string body
pub fn extract_error_message(value: &Value) -> Option<String> {
    let source = value.get("error").unwrap_or(value);
    source
        .get("message")
        .or_else(|| source.get("detail"))
        .or_else(|| source.get("status_msg"))
        .or_else(|| source.pointer("/base_resp/status_msg"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
}

pub fn parse_chat_sse_bytes(
    buffer: &mut String,
    utf8_remainder: &mut Vec<u8>,
    bytes: &[u8],
) -> Vec<String> {
    // Handle UTF-8 characters split across TCP chunk boundaries:
    // prepend any leftover incomplete bytes from the previous chunk.
    let mut combined = Vec::with_capacity(utf8_remainder.len() + bytes.len());
    combined.extend_from_slice(utf8_remainder);
    combined.extend_from_slice(bytes);
    utf8_remainder.clear();

    // Find the last valid UTF-8 boundary.
    match std::str::from_utf8(&combined) {
        Ok(s) => buffer.push_str(s),
        Err(e) => {
            let valid_up_to = e.valid_up_to();
            // Safety: we know the bytes up to valid_up_to are valid UTF-8.
            buffer.push_str(unsafe { std::str::from_utf8_unchecked(&combined[..valid_up_to]) });
            // Save incomplete trailing bytes for the next chunk.
            utf8_remainder.extend_from_slice(&combined[valid_up_to..]);
        }
    }

    let mut blocks = Vec::new();
    while let Some((pos, delimiter_len)) = next_sse_block_delimiter(buffer) {
        let block = buffer[..pos].to_string();
        buffer.drain(..pos + delimiter_len);
        blocks.push(block);
    }
    blocks
}

fn next_sse_block_delimiter(buffer: &str) -> Option<(usize, usize)> {
    let mut best: Option<(usize, usize)> = None;
    for (delimiter, len) in [("\r\n\r\n", 4usize), ("\n\n", 2usize)] {
        if let Some(pos) = buffer.find(delimiter) {
            if best.is_none_or(|(best_pos, _)| pos < best_pos) {
                best = Some((pos, len));
            }
        }
    }
    best
}

pub fn sse_event_from_block(block: &str) -> Option<String> {
    for line in block.lines() {
        if let Some(rest) = line.strip_prefix("event:") {
            let event = rest.trim();
            if !event.is_empty() {
                return Some(event.to_string());
            }
        }
    }
    None
}

pub fn sse_data_from_block(block: &str) -> Option<String> {
    let mut data = Vec::new();
    for line in block.lines() {
        if let Some(rest) = line.strip_prefix("data:") {
            data.push(rest.trim_start().to_string());
        }
    }
    if data.is_empty() {
        None
    } else {
        Some(data.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── extract_error_message ──

    #[test]
    fn extract_error_standard_openai_format() {
        let val = json!({"error": {"message": "rate limit exceeded", "type": "rate_limit_error"}});
        assert_eq!(extract_error_message(&val).unwrap(), "rate limit exceeded");
    }

    #[test]
    fn extract_error_standard_openai_nested() {
        let val = json!({"error": {"message": "invalid request", "code": 400}});
        assert_eq!(extract_error_message(&val).unwrap(), "invalid request");
    }

    #[test]
    fn extract_error_top_level_message() {
        let val = json!({"message": "something went wrong"});
        // "error" key missing → source = value itself → "message" found
        assert_eq!(extract_error_message(&val).unwrap(), "something went wrong");
    }

    #[test]
    fn extract_error_top_level_detail() {
        let val = json!({"detail": "not found"});
        assert_eq!(extract_error_message(&val).unwrap(), "not found");
    }

    #[test]
    fn extract_error_empty_message_returns_none() {
        let val = json!({"error": {"message": ""}});
        assert!(extract_error_message(&val).is_none());
    }

    #[test]
    fn extract_error_no_message_field_returns_none() {
        let val = json!({"error": {"code": 500}});
        assert!(extract_error_message(&val).is_none());
    }

    #[test]
    fn extract_error_status_msg_at_top_level() {
        let val = json!({"status_msg": "bad request"});
        assert_eq!(extract_error_message(&val).unwrap(), "bad request");
    }

    // ── parse_chat_sse_bytes ──

    #[test]
    fn parse_sse_single_block() {
        let mut buf = String::new();
        let mut remainder = Vec::new();
        let bytes = b"data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\n";
        let blocks = parse_chat_sse_bytes(&mut buf, &mut remainder, bytes);
        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].contains("\"content\":\"hi\""));
    }

    #[test]
    fn parse_sse_crlf_block() {
        let mut buf = String::new();
        let mut remainder = Vec::new();
        let bytes = b"data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\r\n\r\n";
        let blocks = parse_chat_sse_bytes(&mut buf, &mut remainder, bytes);
        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].contains("\"content\":\"hi\""));
    }

    #[test]
    fn parse_sse_multiple_blocks() {
        let mut buf = String::new();
        let mut remainder = Vec::new();
        let bytes = b"data: {\"a\":1}\n\ndata: {\"b\":2}\n\ndata: [DONE]\n\n";
        let blocks = parse_chat_sse_bytes(&mut buf, &mut remainder, bytes);
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[2], "data: [DONE]");
    }

    #[test]
    fn parse_sse_partial_block_across_chunks() {
        let mut buf = String::new();
        let mut remainder = Vec::new();

        // First chunk: incomplete block (no \n\n terminator)
        let blocks1 = parse_chat_sse_bytes(&mut buf, &mut remainder, b"data: {\"partial\":");
        assert_eq!(blocks1.len(), 0);

        // Second chunk: completes the block
        let blocks2 = parse_chat_sse_bytes(&mut buf, &mut remainder, b"true}\n\n");
        assert_eq!(blocks2.len(), 1);
        assert!(blocks2[0].contains("\"partial\":true"));
    }

    #[test]
    fn parse_sse_utf8_split_across_chunks() {
        let mut buf = String::new();
        let mut remainder = Vec::new();

        // "你好" in UTF-8 = E4 BD A0 E5 A5 BD
        // Split in the middle of a multi-byte character
        let chunk1: &[u8] = &[0xE4, 0xBD]; // incomplete "你"
        let chunk2: &[u8] = &[0xA0, 0xE5, 0xA5, 0xBD, 0x0A, 0x0A]; // rest of "你好\n\n"

        let blocks1 = parse_chat_sse_bytes(&mut buf, &mut remainder, chunk1);
        assert_eq!(blocks1.len(), 0);
        assert!(
            !remainder.is_empty(),
            "incomplete UTF-8 bytes should be in remainder"
        );

        let blocks2 = parse_chat_sse_bytes(&mut buf, &mut remainder, chunk2);
        assert_eq!(blocks2.len(), 1);
        assert!(blocks2[0].contains("你好"));
    }

    // ── sse event/data parsing ──

    #[test]
    fn sse_event_single_line() {
        let block = "event: error\ndata: {\"message\":\"bad\"}";
        assert_eq!(sse_event_from_block(block).unwrap(), "error");
    }

    #[test]
    fn sse_event_no_event_returns_none() {
        let block = "data: {\"choices\":[{\"delta\":{}}]}";
        assert!(sse_event_from_block(block).is_none());
    }

    #[test]
    fn sse_data_single_line() {
        let block = "data: {\"choices\":[{\"delta\":{}}]}";
        let data = sse_data_from_block(block).unwrap();
        assert_eq!(data, "{\"choices\":[{\"delta\":{}}]}");
    }

    #[test]
    fn sse_data_multiple_data_lines() {
        let block = "data: line1\ndata: line2";
        let data = sse_data_from_block(block).unwrap();
        assert_eq!(data, "line1\nline2");
    }

    #[test]
    fn sse_data_done_marker() {
        let block = "data: [DONE]";
        let data = sse_data_from_block(block).unwrap();
        assert_eq!(data, "[DONE]");
    }

    #[test]
    fn sse_data_no_data_lines_returns_none() {
        let block = "event: message\nid: 1";
        assert!(sse_data_from_block(block).is_none());
    }

    #[test]
    fn sse_data_with_event_line() {
        let block = "event: delta\ndata: {\"text\":\"hello\"}";
        let data = sse_data_from_block(block).unwrap();
        assert_eq!(data, "{\"text\":\"hello\"}");
    }

    #[test]
    fn sse_data_trims_leading_space() {
        let block = "data:   {\"key\":\"value\"}";
        let data = sse_data_from_block(block).unwrap();
        assert_eq!(data, "{\"key\":\"value\"}");
    }
}
