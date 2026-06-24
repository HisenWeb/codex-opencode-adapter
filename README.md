# Codex OpenCode Go Adapter

A deliberately thin adapter between the OpenAI Responses API used by Codex subagents and the OpenCode Go Chat Completions API.

It converts protocol shapes; it does not run another agent system.

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

Codex remains responsible for task roles, sandbox permissions, tool execution, review, and deciding whether work is complete.

## Documentation

| File | Purpose |
|---|---|
| [docs/USAGE.zh-CN.md](docs/USAGE.zh-CN.md) | Chinese setup, usage, and low-token troubleshooting guide. |
| [docs/VALIDATION.zh-CN.md](docs/VALIDATION.zh-CN.md) | Real OpenCode Go and Codex validation checklist. |
| [docs/DIAGNOSTICS.md](docs/DIAGNOSTICS.md) | Runtime diagnostics and log interpretation. |
| [docs/COMPATIBILITY.md](docs/COMPATIBILITY.md) | Compatibility scope, references, implementation map, and non-goals. |
| [docs/ROADMAP.md](docs/ROADMAP.md) | Current status, known gaps, and validation roadmap. |

## Quick start

```bash
cargo build --release

OPENCODE_GO_API_KEY="your-key" \
CODEX_OPENCODE_LOCAL_TOKEN="your-local-token" \
cargo run
```

## Environment variables

| Variable | Default | Description |
|---|---|---|
| `OPENCODE_GO_API_KEY` | required | API key for OpenCode Go. |
| `CODEX_OPENCODE_LOCAL_TOKEN` | none | Bearer token required from local callers. If empty, auth is skipped. |
| `CODEX_OPENCODE_HOST` | `127.0.0.1` | Listen host. |
| `CODEX_OPENCODE_PORT` | `4010` | Listen port. |
| `OPENCODE_GO_BASE_URL` | `https://opencode.ai/zen/go/v1` | Upstream base URL. |
| `CODEX_OPENCODE_STATE_DB` | `.codex-opencode/state.sqlite` | SQLite state database path. |
| `CODEX_OPENCODE_STATE_TTL_SECONDS` | `21600` | State TTL, 6 hours. |
| `CODEX_OPENCODE_TIMEOUT_SECONDS` | `300` | Upstream request timeout. |
| `CODEX_OPENCODE_MAX_REQUEST_BYTES` | `8388608` | Max request body size, 8 MB. |
| `CODEX_OPENCODE_MAX_CONCURRENCY` | `8` | Maximum concurrent upstream requests, read at startup. |
| `RUST_LOG` | `codex_opencode_adapter=info` | Tracing filter. Use `codex_opencode_adapter=debug` for detailed diagnostics. |

The upstream API key and local client token must be different. The adapter never logs either token.

## Endpoints

- `POST /v1/responses` — Responses API, streaming and non-streaming.
- `GET /v1/models` — List available models with the `opencode-go/` prefix.
- `GET /health` — Health check.

## Run tests

```bash
cargo fmt --check
cargo test --lib
cargo test --test conversion_rs
cargo test --test stateless_tool_continuation
cargo test --test stream_content_tool_buffer
cargo test --test stream_tool_delta_regression
cargo test --test nonstream_upstream_error_regression
cargo test --test tool_search_regression
cargo test --test multimodal_regression
cargo test --test test_e2e
cargo test
```

Real smoke test, requires `OPENCODE_GO_API_KEY`:

```bash
OPENCODE_GO_API_KEY="your-key" cargo test --test test_e2e test_e2e_real_smoke -- --ignored
```

## Current status

The text, tool, reasoning, stream, multimodal-input guard, and state-continuation paths are implemented in the mock/regression suite. Real OpenCode Go and Codex subagent validation is still pending.

See [docs/ROADMAP.md](docs/ROADMAP.md) for status and next milestones.

## Explicit non-goals

Not planned:

- full cc-switch port
- provider aggregation platform
- UI, hooks, plugins, statusLine, or OpenCode session management
- automatic model fallback/routing
- automatic multimodal retry after stripping media
- silent multimodal degradation that makes a text-only model pretend it saw media

See [docs/COMPATIBILITY.md](docs/COMPATIBILITY.md) for the full compatibility scope.
