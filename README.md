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

## Self-use Quick Start

Start the adapter:

```powershell
cargo build --release

$env:OPENCODE_GO_API_KEY = "<your-key>"
$env:CODEX_OPENCODE_LOCAL_TOKEN = "codex-opencode-local"
$env:CODEX_OPENCODE_PORT = "4010"
cargo run --release
```

Sanity check it:

```powershell
Invoke-RestMethod http://127.0.0.1:4010/health

$headers = @{ Authorization = "Bearer codex-opencode-local" }
(Invoke-RestMethod http://127.0.0.1:4010/v1/models -Headers $headers).data.id
```

Run the real smoke suite when needed:

```powershell
./scripts/run-real-smoke.ps1 -ApiKey "<your-key>"
```

If you only need one doc, start with [docs/USAGE.zh-CN.md](docs/USAGE.zh-CN.md). Real validation results are in [docs/REAL_VALIDATION_2026-06-25.zh-CN.md](docs/REAL_VALIDATION_2026-06-25.zh-CN.md).

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

## Tests

```bash
cargo test
```

Real upstream smoke:

```bash
OPENCODE_GO_API_KEY="your-key" cargo test --test e2e_real_smoke test_e2e_real_validation_suite -- --ignored --nocapture
```

## Current status

The adapter is usable for self-hosted Codex subagent routing. Text, stream, tool call, custom tool, tool search, continuation, and multimodal guard paths have mock coverage, and real OpenCode Go smoke validation has been run against the current setup.

See [docs/ROADMAP.md](docs/ROADMAP.md) for status and next milestones.

## More Docs

| File | Purpose |
|---|---|
| [docs/USAGE.zh-CN.md](docs/USAGE.zh-CN.md) | Short self-use setup and troubleshooting guide. |
| [docs/REAL_VALIDATION_2026-06-25.zh-CN.md](docs/REAL_VALIDATION_2026-06-25.zh-CN.md) | Latest real upstream smoke and partial Codex validation record. |
| [docs/VALIDATION.zh-CN.md](docs/VALIDATION.zh-CN.md) | Full manual validation checklist. |
| [docs/DIAGNOSTICS.md](docs/DIAGNOSTICS.md) | Runtime diagnostics and log interpretation. |
| [docs/COMPATIBILITY.md](docs/COMPATIBILITY.md) | Compatibility scope and non-goals. |
| [docs/ROADMAP.md](docs/ROADMAP.md) | Current status and future ideas. |

## Explicit non-goals

Not planned:

- full cc-switch port
- provider aggregation platform
- UI, hooks, plugins, statusLine, or OpenCode session management
- automatic model fallback/routing
- automatic multimodal retry after stripping media
- silent multimodal degradation that makes a text-only model pretend it saw media

See [docs/COMPATIBILITY.md](docs/COMPATIBILITY.md) for the full compatibility scope.
