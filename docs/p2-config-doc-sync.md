# P2 config and documentation sync

## Scope

This document records the current configuration and documentation alignment after the P0/P1 protocol hardening work.

No runtime behavior is changed by this note.

## Current runtime configuration

The adapter currently reads these environment variables:

| Variable | Default | Source |
|---|---:|---|
| `OPENCODE_GO_API_KEY` | required | `src/config.rs` |
| `CODEX_OPENCODE_LOCAL_TOKEN` | none | `src/config.rs` |
| `CODEX_OPENCODE_HOST` | `127.0.0.1` | `src/config.rs` |
| `CODEX_OPENCODE_PORT` | `4010` | `src/config.rs` |
| `OPENCODE_GO_BASE_URL` | `https://opencode.ai/zen/go/v1` | `src/config.rs` |
| `CODEX_OPENCODE_STATE_DB` | `.codex-opencode/state.sqlite` | `src/config.rs` |
| `CODEX_OPENCODE_STATE_TTL_SECONDS` | `21600` | `src/config.rs` |
| `CODEX_OPENCODE_TIMEOUT_SECONDS` | `300` | `src/config.rs` |
| `CODEX_OPENCODE_MAX_REQUEST_BYTES` | `8388608` | `src/config.rs` |
| `CODEX_OPENCODE_MAX_CONCURRENCY` | `8` | `src/main.rs` |
| `RUST_LOG` | `codex_opencode_adapter=info` | `src/main.rs` |

`CODEX_OPENCODE_MAX_CONCURRENCY` is intentionally read directly in `main.rs` at startup. It is not part of the clap `Config` struct.

## Documentation sync notes

### README

The README should mention:

- `CODEX_OPENCODE_MAX_CONCURRENCY`, default `8`.
- `RUST_LOG`, especially `codex_opencode_adapter=debug` for continuation diagnostics.
- New regression suites:
  - `stream_content_tool_buffer`
  - `stream_tool_delta_regression`
  - `nonstream_upstream_error_regression`
- Non-stream upstream errors already return a Responses `status: failed` body, but still preserve the upstream HTTP status. Real Codex validation should decide whether `/v1/responses` needs HTTP 200 for failed upstream calls.
- Tool continuation diagnostics are documented in `docs/p1-continuation-diagnostics.md`.

### docs/USAGE.zh-CN.md

The Chinese usage guide should not refer to these stale fields unless runtime logging adds them later:

- `request_prepared`
- `reasoning_applied`
- `reasoning_reason`
- `effort_not_declared_by_model`

The current runtime logs continuation-related events instead:

- `stored_response_not_found`
- `tool_history_unique_fallback_hit`
- `tool_history_call_id_ambiguous`
- `tool_history_response_ambiguous`
- `tool_history_call_id_not_found`
- `stateless_tool_history_bypass_state_lookup`

## Current P0/P1 completion state

- P0 stateless full-history fallback: complete.
- P1 streaming content-before-tool buffering: complete.
- P1 streaming tool delta regression coverage: complete.
- P1 non-stream upstream failed-response shape regression: complete.
- P1 continuation diagnostics: complete.

## Validation

Run locally after documentation-only changes:

```bash
cargo fmt --check
cargo test
```
