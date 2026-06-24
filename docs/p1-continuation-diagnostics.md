# P1 continuation diagnostics

## Scope

This document records the diagnostic events added around Codex tool-result continuation and state replay.

The change is logging-only. It does not change protocol conversion, state lookup rules, fallback behavior, or Responses output shape.

## Events

| Event | Level | Location | Meaning |
|---|---:|---|---|
| `stored_response_not_found` | warn | `StateStore::get` | A requested `previous_response_id` was not found in non-expired state. The log includes `response_id` and TTL cutoff. |
| `tool_history_unique_fallback_hit` | debug | `StateStore::find_by_call_ids` | No `previous_response_id` was available, but the adapter restored continuation via a unique pending `call_id` match. The log includes the restored `response_id`, requested call IDs, and stored pending call IDs. |
| `tool_history_call_id_ambiguous` | warn | `StateStore::find_by_call_ids` | The same requested `call_id` matched multiple stored responses. The log includes both candidate response IDs and `candidate_count = 2`. |
| `tool_history_response_ambiguous` | warn | `StateStore::find_by_call_ids` | The requested call-id set matched multiple stored responses. The log includes both candidate response IDs and `candidate_count = 2`. |
| `tool_history_call_id_not_found` | warn | `StateStore::find_by_call_ids` | The adapter could not restore continuation via pending `call_id` fallback. The log includes requested and matched call-id counts. |
| `stateless_tool_history_bypass_state_lookup` | debug | `conversion::function_output_call_ids` | Responses input already contains matching tool calls for every tool output, so the server bypasses stored-state lookup and allows stateless history repair. |

## Operational use

When a real Codex subagent continuation fails, check logs in this order:

1. `stored_response_not_found` means Codex supplied a `previous_response_id`, but the adapter no longer has the stored response, commonly because the state TTL expired or a different state DB is being used.
2. `tool_history_unique_fallback_hit` means Codex omitted or rewrote `previous_response_id`, but the adapter recovered through unique `call_id` fallback.
3. `tool_history_call_id_ambiguous` or `tool_history_response_ambiguous` means the adapter refused recovery because multiple stored responses could match the requested tool output.
4. `stateless_tool_history_bypass_state_lookup` means Codex sent a self-contained stateless history; recovery should continue through `build_chat_payload()` and `repair_stateless_history()`.

## Validation

Run locally:

```bash
cargo fmt --check
cargo test --lib
cargo test
```
