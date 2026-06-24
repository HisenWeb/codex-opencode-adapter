# P1 streaming content/tool buffering fix

## Scope

This branch fixes the streaming adoption risk where an upstream Chat Completions stream may emit assistant `content` first and later emit `tool_calls` in the same turn.

Previously, the adapter emitted Responses message/text events immediately. If a later tool call appeared, Codex could observe both an assistant message and a tool call for the same response turn, which is a known adoption risk from adapter-layer experience.

## Behavior after this change

- Streaming text content is buffered instead of emitted immediately.
- If the stream finishes without tool calls, the buffered text is emitted as a normal Responses message during `finalize()`.
- If the stream contains valid tool calls, buffered text is not emitted as a Responses message output.
- Buffered assistant text is still retained in stored chat history alongside the tool calls, so the next continuation still has upstream context.
- Reasoning events are still emitted through the existing reasoning path.

## Tradeoff

Text-only streaming now emits text at finalization time rather than token-by-token. This is intentional for the current adapter phase because correct Codex tool-call adoption is more important than fine-grained text streaming.

## Validation

Run locally:

```bash
cargo fmt --check
cargo test --test stream_content_tool_buffer
cargo test --test tool_search_regression
cargo test
```

This branch was edited through the GitHub connector, so local cargo execution is required before merge.
