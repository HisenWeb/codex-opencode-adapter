# P1 non-stream upstream error behavior

## Scope

This note documents the current non-stream `/v1/responses` behavior when the upstream Chat Completions request fails with an HTTP error.

## Current behavior

- The adapter returns a protocol-shaped Responses object with `status: failed`.
- The response body includes `object: response`, an empty `output`, zeroed `usage`, the original model alias, request metadata, and an `error` object with `type/code: upstream_error`.
- The adapter currently preserves the upstream HTTP status for non-stream requests.
- Streaming upstream errors continue to be delivered through SSE as `response.failed` followed by `[DONE]`.

## Reasoning

The P1-3 risk was that Codex could receive only a plain HTTP `{error: ...}` body and lose the Responses protocol chain. The runtime already emits a Responses `failed` body for non-stream upstream failures, so this milestone locks the behavior with a regression test instead of changing runtime code.

If real Codex subagent validation shows that non-2xx HTTP status still breaks the chain, the next policy change should be explicit: return HTTP 200 with `response.status = failed` for `/v1/responses` upstream errors while leaving `/v1/models` HTTP error behavior unchanged.

## Validation

Run locally:

```bash
cargo fmt --check
cargo test --test nonstream_upstream_error_regression
cargo test --test test_e2e
cargo test
```
