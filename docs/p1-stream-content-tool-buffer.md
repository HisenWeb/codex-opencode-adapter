# P1 streaming content/tool buffering fix

## Scope

This branch fixes the streaming adoption risk where an upstream Chat Completions stream may emit assistant `content` first and later emit `tool_calls` in the same turn.

Previously, tool-enabled streams emitted Responses message/text events immediately. If a later tool call appeared, Codex could observe both an assistant message and a tool call for the same response turn, which is a known adoption risk from adapter-layer experience.

## Behavior after this change

- Tool-enabled streaming requests buffer assistant text until the stream's tool-call decision is known.
- If a tool-enabled stream finishes without valid tool calls, buffered text is emitted as a normal Responses message during `finalize()`.
- If a tool-enabled stream contains valid tool calls, buffered text is not emitted as a Responses message output.
- Buffered assistant text is still retained in stored chat history alongside the tool calls, so the next continuation still has upstream context.
- Requests without tools keep normal token-by-token `response.output_text.delta` behavior.
- Reasoning events are still emitted through the existing reasoning path.

## Tradeoff

Only tool-enabled text streaming may delay visible text until finalization. This is intentional for the current adapter phase because correct Codex tool-call adoption is more important than fine-grained text streaming when tools are available.

## Validation

Run locally:

```bash
cargo fmt --check
cargo test --test stream_content_tool_buffer
cargo test --test tool_search_regression
cargo test
```

This branch was edited through the GitHub connector, so local cargo execution is required before merge.
