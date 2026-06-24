# P2 history hardening

This branch implements behavior-level hardening for Codex tool continuation.

Implemented:

- helper module for tool output validation
- duplicate tool output detection
- previous response validation against requested call ids
- Chat history legality check before sending to upstream
- Responses-compatible failed response for history errors
- streaming `response.failed` event for early history errors
- regression tests for unknown, duplicate, orphan, and valid tool histories

This is not a full copy of cc-switch `codex_chat_history.rs`.
