# P2 history hardening

This branch is reserved for the P2 tool continuation hardening work.

The first implementation attempt is available on `codex/history-hardening` and contains:

- `src/codex_chat_history.rs`
- `tests/codex_chat_history_hardening.rs`

The helper module covers:

- previous response id priority
- unique call id fallback
- ambiguous call id rejection
- parallel call ids from one response
- split call ids across responses
- unknown call id handling
- duplicate tool output handling
- chat history legality checks

The module is intentionally not a full copy of cc-switch `codex_chat_history.rs`.
