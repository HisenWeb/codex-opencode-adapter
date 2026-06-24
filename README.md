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
| `src-tauri/src/proxy/providers/codex_chat_common.rs` | Shared Codex Chat conversion helpers for reasoning extraction, `<think>` block splitting, Responses tool-call item construction, `call_id` extraction, and empty-value handling |
| `src-tauri/src/proxy/providers/codex_chat_history.rs` | Cross-request Codex tool-call history replay, `previous_response_id` restoration, unique `call_id` fallback, and subagent cases where `previous_response_id` may be omitted or rewritten |

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

## Current status

This project is in the protocol-compatibility and validation phase.

| Area | Status | Notes |
|---|---|---|
| Responses -> Chat request conversion | Implemented | Covers instructions, input history, tool calls, tool outputs, tool choice, reasoning parameters, and multimodal input blocks |
| Chat -> Responses non-stream conversion | Implemented | Covers text, reasoning, usage, finish reasons, function calls, custom tool calls, tool search calls, and stored replay state |
| Chat stream -> Responses stream conversion | Implemented | Covers Responses SSE lifecycle, text deltas, reasoning deltas, streamed tool-call assembly, custom tool input finalization, tool search, terminal events, failed streams, and truncated streams |
| Tool lifecycle and state replay | Implemented | `previous_response_id` and pending tool call IDs are stored locally so tool-result continuations can be repaired |
| OpenCode Go reasoning compatibility | Implemented for known profiles | DeepSeek V4 and MiMo reasoning variants are handled through explicit metadata/config behavior |
| Tool search compatibility | Implemented | `tool_search` has a dedicated schema and converts to/from Responses `tool_search_call` instead of being treated as a custom tool |
| Multimodal input conversion | Implemented | Request-side `input_image`, base64 image source, `input_file`, `input_audio`, and mixed content arrays are converted to Chat-compatible content blocks |
| Multimodal text-only model guard | Implemented | Known text-only models return a valid Responses `failed` result instead of an HTTP provider error when media input is present |
| Multimodal output generation | Planned | Current phase keeps text output reliable; image/audio/file output mapping is planned for later phases |
| Other upstream providers | Planned | Current default target is OpenCode Go; future work may add additional OpenAI-compatible or Chat-like upstream profiles without turning this into a provider aggregation platform |
| Mock integration tests | Implemented | L2 tests use mock upstream behavior and do not require an external OpenCode Go call |
| Real OpenCode Go / Codex subagent validation | Pending | Must be verified with a real OpenCode Go API key and a real Codex subagent workflow |

Latest local validation expected after changing this file set:

```bash
cargo fmt --check
cargo test --lib
cargo test --test conversion_rs
cargo test --test tool_search_regression
cargo test --test multimodal_regression
cargo test --test test_e2e
cargo test
```

## Roadmap

### P0/P1/P2 protocol migration

Implemented in the current codebase:

- Responses request -> Chat request conversion.
- Chat non-stream response -> Responses response conversion.
- Chat SSE stream -> Responses SSE stream conversion.
- Function tool, namespace tool, custom tool, and tool search conversion.
- Tool result continuation and `previous_response_id` state replay.
- Reasoning field extraction and compatibility handling.
- Usage, finish reason, incomplete, failed, and terminal event mapping.
- SSE parsing compatibility for LF/CRLF blocks and upstream `event:error` cases.

### P2.5 compatibility hardening

Implemented:

- Tool search schema and lifecycle fixes.
- Streaming custom/tool search event separation.
- Custom tool partial argument handling.
- Upstream stream error translation into Responses failed events.
- Additional regression tests for custom tool and tool search behavior.

### P2.6 multimodal compatibility

Implemented, but still requires local `cargo fmt` and `cargo test` validation after pull:

- Mixed text/image/file/audio Responses input conversion.
- Anthropic-style base64 image source -> Chat `image_url` data URL.
- `input_file` mapping only when a usable `file_id` or `file_data` is present.
- `input_audio` mapping.
- Known text-only model guard.
- Reactive upstream multimodal unsupported error detection.
- Multimodal regression tests.

### P3-lite real validation

Next planned milestone:

- Start the adapter locally with a real OpenCode Go API key.
- Verify `/v1/models` and basic `/v1/responses` non-stream requests.
- Verify `/v1/responses` streaming requests.
- Verify reasoning output with known DeepSeek/MiMo models.
- Verify normal function-call round trip.
- Verify streamed function-call round trip.
- Verify custom tool-call round trip.
- Verify tool-search call round trip.
- Verify `function_call_output` / `custom_tool_call_output` / `tool_search_output` continuation through stored state.

### P3-full Codex subagent validation

After P3-lite passes:

- Configure Codex subagent to call this adapter as `opencode-go/...` models.
- Run a real text-only subagent task.
- Run a real tool-using subagent task.
- Run a real streaming tool-using subagent task.
- Run a real multimodal input smoke test against a model believed to support vision.
- Run a real text-only-model multimodal failure test and confirm the parent agent receives a protocol-valid Responses `failed` result rather than a broken provider error.
- Record exact model IDs and observed OpenCode Go response shapes.

### Long-term multimodal output support

Planned after the text/tool/reasoning path is stable against real Codex subagent
traffic:

- Preserve Chat response content arrays instead of flattening all non-text output to plain text.
- Map upstream image/file/audio output blocks to valid Responses output items when the upstream provides a stable shape.
- Add streaming event support for non-text output parts only after a real upstream format is observed.
- Keep generated image/audio/file artifacts out of adapter-owned state unless Codex requires protocol-level references.
- Prefer external CLI, MCP, or Codex tools for actual media generation/extraction work; the adapter should translate protocol metadata, not become a media runtime.
- Add regression tests for every verified multimodal output shape.

### Long-term additional upstream support

Planned after OpenCode Go is validated:

- Add a small provider/profile boundary for upstreams that are mostly OpenAI Chat Completions-compatible.
- Keep Codex Responses as the stable frontend contract.
- Add provider-specific request/response quirks only when a real model/API requires them.
- Start with configuration/profile differences such as base URL, headers, model IDs, reasoning fields, stream usage, tool-call shape, and error shape.
- Introduce a dedicated provider adapter trait only if a second upstream cannot be handled by profile-level differences.
- Add one upstream at a time with mock tests plus real smoke tests.
- Avoid provider aggregation features such as automatic routing, fallback, price optimization, UI management, or model marketplace behavior.

Candidate future upstream categories:

- OpenAI-compatible Chat Completions APIs.
- Chat-like APIs with small schema differences.
- Responses-compatible APIs that can bypass some conversion steps.

### Stabilization

Only after real validation:

- Patch concrete upstream shape mismatches found in P3.
- Add regression tests for every real incompatibility found.
- Tighten model capability metadata only when a model has been verified.
- Improve diagnostics/logging around failed upstream streams and unsupported media.
- Keep protocol conversion code small and avoid introducing a provider registry unless a second provider becomes an explicit project goal.

### Explicit non-goals

Not planned for this project:

- Full cc-switch port.
- Provider aggregation platform.
- UI, hooks, plugins, statusLine, or OpenCode session management.
- Automatic model fallback/routing.
- Automatic multimodal retry after stripping media.
- Silent multimodal degradation that makes a text-only model pretend it saw media.

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

Multimodal output generation is planned as future work. Until the output shape
is validated against a real upstream, image/audio/file generation should be
handled by external CLI tools, MCP tools, or normal Codex tool calls; this
adapter preserves the protocol chain.

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
