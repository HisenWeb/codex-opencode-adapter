# Diagnostics

This document explains adapter log events that are useful when Codex tool-result continuation, upstream calls, or streaming responses fail.

## Enable debug logs

```powershell
$env:RUST_LOG = "codex_opencode_adapter=debug"
cargo run --release
```

The default tracing filter is `codex_opencode_adapter=info`.

## Tool continuation events

These events are emitted around Codex tool-result continuation and stored state replay.

| Event | Level | Location | Meaning |
|---|---:|---|---|
| `stored_response_not_found` | warn | `StateStore::get` | A requested `previous_response_id` was not found in non-expired state. The log includes `response_id` and TTL cutoff. |
| `tool_history_unique_fallback_hit` | debug | `StateStore::find_by_call_ids` | No `previous_response_id` was available, but the adapter restored continuation via a unique pending `call_id` match. The log includes the restored `response_id`, requested call IDs, and stored pending call IDs. |
| `tool_history_call_id_ambiguous` | warn | `StateStore::find_by_call_ids` | The same requested `call_id` matched multiple stored responses. The adapter refuses fallback to avoid linking output to the wrong response. |
| `tool_history_response_ambiguous` | warn | `StateStore::find_by_call_ids` | The requested call-id set matched multiple stored responses. The adapter refuses fallback to avoid ambiguous recovery. |
| `tool_history_call_id_not_found` | warn | `StateStore::find_by_call_ids` | The adapter could not restore continuation via pending `call_id` fallback. The log includes requested and matched call-id counts. |
| `stateless_tool_history_bypass_state_lookup` | debug | `conversion::function_output_call_ids` | Responses input already contains matching tool calls for every tool output, so the server bypasses stored-state lookup and allows stateless history repair. |

Recommended interpretation order:

1. `stored_response_not_found`: Codex supplied `previous_response_id`, but the adapter state DB no longer contains it. Check state DB path, TTL, and whether the adapter process changed.
2. `tool_history_unique_fallback_hit`: Codex omitted or rewrote `previous_response_id`, but recovery succeeded through unique `call_id` fallback.
3. `tool_history_call_id_ambiguous` or `tool_history_response_ambiguous`: recovery was refused because multiple stored responses could match.
4. `stateless_tool_history_bypass_state_lookup`: Codex sent a self-contained history. Recovery should continue through stateless repair.

## Non-stream upstream errors

Current `/v1/responses` non-stream upstream HTTP errors return a Responses body with:

```text
status = failed
error.type = upstream_error
error.code = upstream_error
```

The adapter currently preserves the upstream HTTP status. Real Codex validation must confirm whether non-2xx HTTP status breaks subagent control flow. If it does, only `/v1/responses` upstream failures should be changed to HTTP 200 with `response.status = failed`.

## Multimodal unsupported input

Known text-only models are guarded before upstream dispatch. When media input is present, the expected Responses failure is:

```text
status = failed
error.code = unsupported_multimodal_input
```

For streaming requests, the expected terminal sequence is:

```text
response.created
response.in_progress
response.failed
[DONE]
```

Unknown model capabilities are passed through to OpenCode Go. If the upstream returns a known multimodal unsupported error, the adapter translates it into the same Responses-level failure.

## Streaming failures

For a healthy stream, the expected terminal shape is:

```text
response.completed
[DONE]
```

If the upstream ends before a normal finish reason, record:

- last upstream chunk
- last emitted Responses event
- whether `response.incomplete` or `response.failed` was emitted
- whether tool-call adoption had already started
- whether text was buffered because tools were enabled

Add a regression test before patching any newly observed stream terminal shape.
