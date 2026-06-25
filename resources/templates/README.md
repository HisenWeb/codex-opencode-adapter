# resources/templates — Canonical OSS agent templates

This directory holds the canonical copy of every OSS subagent configuration template shipped with codex-opencode-adapter.

## Purpose

- The Rust binary `include_str!()`s (or otherwise embeds) files from this directory to produce default agent configs during `codex-opencode-adapter init`.
- `.codex/agents/*.toml` at the project root are the runtime/user-facing copies; this directory is the source of truth.
- Keep these files in sync with `.codex/agents/` when updating agent models or instructions.

## Files

| File | Agent | Model | Reasoning effort |
|---|---|---|---|
| oss-flash.toml | oss_flash | opencode-go/deepseek-v4-flash | low |
| oss-glm.toml | oss_glm | opencode-go/glm-5.2 | high |
| oss-kimi.toml | oss_kimi | opencode-go/kimi-k2.7-code | high |
| oss-mimo.toml | oss_mimo | opencode-go/mimo-v2.5 | medium |

## Usage from Rust

```rust
// Embed a template at compile time:
pub const OSS_FLASH_TOML: &str = include_str!("templates/oss-flash.toml");
```

All files use UTF-8 encoding.
