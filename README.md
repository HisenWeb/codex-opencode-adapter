# Codex OpenCode Go Adapter

A deliberately thin adapter between the OpenAI Responses API used by Codex
subagents and the OpenCode Go Chat Completions API.

中文安装、配置与低 Token 排障说明见
[docs/USAGE.zh-CN.md](docs/USAGE.zh-CN.md)。

It converts protocol shapes; it does not run another agent system. Mission
tiers, semantic gates, automatic patch application, answer grading, OpenCode
sessions, and OpenCode tools are intentionally absent.

## Data flow

```text
Codex subagent
  -> POST /v1/responses (Responses API)
  -> this adapter
  -> POST /chat/completions (Chat Completions API)
  -> OpenCode Go (https://opencode.ai/zen/go/v1)
  -> Chat Completions response / SSE stream
  -> this adapter
  -> Responses API response / SSE stream
  -> Codex subagent
```

Codex remains responsible for task roles, sandbox permissions, tool execution,
review, and deciding whether work is complete.

## Reference implementation scope

This adapter is not a fork of cc-switch and does not attempt to reproduce its
provider routing, UI, session management, hooks, plugins, status line, model
pricing, fallback routing, or provider registry.

cc-switch was used only as a behavior reference for protocol compatibility
around Codex Responses <-> Chat Completions conversion. The relevant reference
files are:

| cc-switch file | Reference scope in this adapter |
|---|---|
| `src-tauri/src/proxy/providers/transform_codex_chat.rs` | Responses request -> Chat payload conversion, including `input_text`, `input_image`, `input_file`, `input_audio`, tool calls, tool outputs, and response history replay |
| `src-tauri/src/proxy/providers/streaming_codex_chat.rs` | Chat streaming delta -> Responses SSE event lifecycle for text, reasoning, function calls, custom tool calls, tool search, terminal events, and incomplete/failed streams |
| `src-tauri/src/proxy/providers/transform_opencode_go.rs` | OpenCode Go-specific compatibility details, especially model/reasoning quirks and OpenAI-compatible upstream behavior |
| `src-tauri/src/proxy/providers/transform_responses.rs` | Responses-format output/event shape, `output_text` items, and image/base64 shape handling used for compatibility checks |
| `src-tauri/src/proxy/providers/transform.rs` | Anthropic/OpenAI-style content block normalization, especially base64 image source -> `image_url` data URL behavior |
| `src-tauri/src/proxy/media_sanitizer.rs` | Multimodal capability guard behavior and detection of upstream errors such as `unknown variant image_url, expected text` |
| `src-tauri/src/proxy/forwarder.rs` | Request/response integration points and reactive multimodal error handling around upstream calls |
| `src-tauri/src/proxy/providers/models/openai.rs` | OpenAI-compatible Chat content block shapes such as `image_url` |

The implemented Rust equivalents are intentionally narrower:

| Adapter area | Local files |
|---|---|
| Responses -> Chat request conversion | `src/conversion/responses_to_chat.rs`, `src/conversion/tool_context.rs`, `src/conversion/multimodal_input.rs` |
| Chat -> Responses non-stream conversion | `src/conversion/chat_to_responses.rs`, `src/conversion/text.rs` |
| Chat stream -> Responses stream conversion | `src/conversion/stream_chat_to_responses.rs`, `src/upstream.rs` |
| OpenCode Go request/response integration | `src/server.rs`, `src/upstream.rs` |
| Multimodal model capability guard | `src/media_guard.rs` |
| State replay for tool continuations | `src/state.rs` |

The intended compatibility target is the Codex subagent -> OpenCode Go path, not
a general-purpose provider aggregation platform.

## Quick start (Rust)

```bash
# Build
cargo build --release

# Run
OPENCODE_GO_API_KEY="your-key" \
CODEX_OPENCODE_LOCAL_TOKEN="your-local-token" \
cargo run
```

The adapter listens on `127.0.0.1:4010` by default.

## Environment variables

| Variable | Default | Description |
|---|---|---|
| `OPENCODE_GO_API_KEY` | (required) | API key for OpenCode Go |
| `CODEX_OPENCODE_LOCAL_TOKEN` | (none) | Bearer token the adapter requires from callers; if empty, auth is skipped |
| `CODEX_OPENCODE_HOST` | `127.0.0.1` | Listen host |
| `CODEX_OPENCODE_PORT` | `4010` | Listen port |
| `OPENCODE_GO_BASE_URL` | `https://opencode.ai/zen/go/v1` | Upstream base URL |
| `CODEX_OPENCODE_STATE_DB` | `.codex-opencode/state.sqlite` | SQLite state database path |
| `CODEX_OPENCODE_STATE_TTL_SECONDS` | `21600` | State TTL (6 hours) |
| `CODEX_OPENCODE_TIMEOUT_SECONDS` | `300` | Upstream request timeout |
| `CODEX_OPENCODE_MAX_REQUEST_BYTES` | `8388608` | Max request body size (8 MB) |

The upstream API key and local client token must be different. The adapter
never logs either token.

## Run tests

### Rust tests (unit + integration)

```bash
# All tests
cargo test

# Unit tests only
cargo test --lib

# Conversion regression tests
cargo test --test conversion_rs
cargo test --test tool_search_regression
cargo test --test multimodal_regression

# L2 integration tests (mock upstream, no external dependency)
cargo test --test test_e2e

# L2 real smoke test (requires OPENCODE_GO_API_KEY)
OPENCODE_GO_API_KEY="your-key" cargo test --test test_e2e test_e2e_real_smoke -- --ignored
```

### Test structure

```text
tests/
├── conversion_rs.rs             # Rust unit tests for conversion modules
├── tool_search_regression.rs    # Tool search/custom tool streaming regressions
├── multimodal_regression.rs     # Multimodal input and guard regressions
└── test_e2e.rs                  # L2 integration tests (mock upstream + real smoke)
```

## Endpoints

- `POST /v1/responses` — Responses API (streaming and non-streaming)
- `GET /v1/models` — List available models (prefixed with `opencode-go/`)
- `GET /health` — Health check

## Reasoning compatibility

The adapter reads `reasoning.effort` or `reasoning_effort` from a Responses
request. It sends `reasoning_effort` upstream only when model metadata declares
the requested variant using the verified OpenAI-compatible protocol.

Current verified profiles:

- DeepSeek V4 Pro/Flash: `low`, `medium`, `high`, `max`
- MiMo V2.5/Pro: `low`, `medium`, `high`

Models that support reasoning but do not declare adjustable variants keep their
default behavior. Unsupported settings are reported in structured adapter
metadata and logs rather than silently pretending to work.

Reasoning content is retained only in stored chat history so tool continuations
remain valid. It is not exposed as user-visible chain of thought.

## Multimodal compatibility

The adapter supports request-side multimodal conversion for Codex Responses
inputs that can be represented in an OpenAI-compatible Chat content array:

- `input_image` -> Chat `image_url`
- Anthropic-style base64 image source -> Chat `image_url` data URL
- `input_file` with `file_id` or `file_data` -> Chat `file`
- `input_audio` -> Chat `input_audio`
- mixed text/image/file/audio message content arrays

Known text-only models are guarded before upstream dispatch. If a text-only
model receives image/file/audio input, the adapter returns a valid Responses
object with `status: failed` and `error.code: unsupported_multimodal_input`
instead of returning an HTTP 4xx/5xx provider error. For streaming requests, it
emits `response.created`, `response.in_progress`, `response.failed`, then
`[DONE]`.

Unknown model capabilities are passed through to OpenCode Go. If the upstream
returns a multimodal unsupported error, the adapter translates it into the same
Responses-level failure.

Multimodal output generation is intentionally out of scope for this adapter.
Image/audio/file generation should be handled by external CLI tools, MCP tools,
or normal Codex tool calls; this adapter only preserves the protocol chain.

## Supported models

All models available on OpenCode Go, prefixed with `opencode-go/` when calling
the adapter:

- deepseek-v4-flash, deepseek-v4-pro
- glm-5.1, glm-5.2
- kimi-k2.6, kimi-k2.7-code
- mimo-v2.5, mimo-v2.5-pro
- minimax-m2.7, minimax-m3
- qwen3.6-plus, qwen3.7-max, qwen3.7-plus

## State management

State needed for `previous_response_id` and tool results is stored in a local
SQLite database and expires according to `CODEX_OPENCODE_STATE_TTL_SECONDS`.
