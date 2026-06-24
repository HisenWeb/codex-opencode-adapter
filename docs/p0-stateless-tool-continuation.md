# P0 stateless tool continuation fix

## Scope

This branch fixes the P0 issue where a Responses request without `previous_response_id` could contain a complete stateless history with both a tool call and its matching tool output, but the server still attempted stored-state lookup first and failed before `repair_stateless_history()` could run.

## Behavior after this change

- Requests with `previous_response_id` still use stored-state validation.
- Requests with tool outputs but no matching tool calls in `input` still require stored-state lookup and continue to fail if no unique stored response exists.
- Requests with full stateless history, where every tool output has a matching `function_call`, `custom_tool_call`, or `tool_search_call` in `input`, bypass stored-state lookup and enter the existing stateless repair path.
- Duplicate tool outputs are still rejected by the repair/validation path.

## Validation

Run locally:

```bash
cargo fmt --check
cargo test --lib
cargo test --test stateless_tool_continuation
cargo test
```

This branch was edited through the GitHub connector, so local cargo execution on the Windows working copy is still required before merge.
