# P0 streaming tool_calls patch

Generated patch is too large to safely apply through this connector without local Rust tooling validation. Use the artifact provided in chat, or ask Codex/Claude Code to apply the same patch locally and run:

```bash
cargo test stream_chat_to_responses
cargo test
```

Scope:
- Replace Value-backed `tool_calls` map with deterministic `ToolCallState`
- Require upstream `index` instead of falling back to `0`
- Bind `call_id` once and never rebind after `output_item.added`
- Emit argument deltas after start, including same-chunk name+arguments
- Flush pending tool calls once in `finalize`
- Make terminal finalize/fail idempotent
