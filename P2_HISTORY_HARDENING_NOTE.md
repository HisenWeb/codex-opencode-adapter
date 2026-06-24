# P2 history hardening

This draft branch adds the first P2 behavior-level hardening layer for Codex tool continuation.

Added files:

- `src/codex_chat_history.rs`
- `tests/codex_chat_history_hardening.rs`

Covered behavior:

- previous response id priority over call id fallback
- unique call id fallback
- ambiguous call id rejection
- parallel call ids from one response
- split call ids across responses rejection
- unknown call id handling
- duplicate tool output handling
- chat history legality checks

Limitations:

- This is not a full copy of cc-switch `codex_chat_history.rs`.
- The helper is not wired into the live server request path yet.
- The branch currently needs update/rebase onto latest `main` before merge.
