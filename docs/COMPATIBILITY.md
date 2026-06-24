# Compatibility scope

This adapter is a deliberately thin protocol bridge:

```text
Codex Responses API
  <-> codex-opencode-adapter
  <-> OpenCode Go Chat Completions-like API
```

It converts protocol shapes. It does not run another agent system.

## Intended target

The primary target is the Codex subagent -> OpenCode Go path.

The adapter should stay small and focused on protocol compatibility:

- Responses request -> Chat request conversion
- Chat response -> Responses response conversion
- Chat SSE stream -> Responses SSE event conversion
- tool-call and tool-output continuation
- reasoning field extraction and safe storage
- multimodal input conversion and text-only model guard
- upstream error shaping that keeps the Codex protocol chain intact

## Reference implementations

This project references cc-switch only for protocol-compatible behavior. It is not a cc-switch fork.

| cc-switch file | Reference scope in this adapter |
|---|---|
| `src-tauri/src/proxy/providers/transform_codex_chat.rs` | Responses request -> Chat payload conversion, including input blocks, tool calls, tool outputs, and response history replay |
| `src-tauri/src/proxy/providers/streaming_codex_chat.rs` | Chat streaming delta -> Responses SSE lifecycle for text, reasoning, tool calls, terminal events, and failed streams |
| `src-tauri/src/proxy/providers/transform_opencode_go.rs` | OpenCode Go-specific compatibility details and OpenAI-compatible upstream behavior |
| `src-tauri/src/proxy/providers/transform_responses.rs` | Responses output/event shapes and output text item compatibility |
| `src-tauri/src/proxy/providers/transform.rs` | Content block normalization, including base64 image source -> `image_url` data URL behavior |
| `src-tauri/src/proxy/media_sanitizer.rs` | Multimodal capability guard behavior and upstream unsupported-media detection |
| `src-tauri/src/proxy/forwarder.rs` | Request/response integration points and reactive multimodal error handling |
| `src-tauri/src/proxy/providers/models/openai.rs` | OpenAI-compatible Chat content block shapes such as `image_url` |
| `src-tauri/src/proxy/providers/codex_chat_common.rs` | Reasoning extraction, `<think>` splitting, tool-call item construction, `call_id` extraction, and empty-value handling |
| `src-tauri/src/proxy/providers/codex_chat_history.rs` | Tool-call history replay, `previous_response_id` restoration, unique `call_id` fallback, and omitted/re-written response-id cases |

This project also references `goldtetsola/opencode-bridge` as a real-world Codex + OpenCode Go adapter experience source.

The reference scope is limited to adapter-layer behavior observed around real OpenCode Go usage:

- SSE terminal handling and avoiding silent stream disconnects
- early `response.created` emission and stream timeout avoidance
- Chat `content + tool_calls` handling to avoid Codex tool-call adoption issues
- GPT-family or unknown model misrouting safeguards
- orphan tool-output recovery and Responses-compatible failed-result shaping
- lightweight diagnostics for tool-call continuation failures

## Local implementation map

| Adapter area | Local files |
|---|---|
| Responses -> Chat request conversion | `src/conversion/responses_to_chat.rs`, `src/conversion/tool_context.rs`, `src/conversion/multimodal_input.rs` |
| Chat -> Responses non-stream conversion | `src/conversion/chat_to_responses.rs`, `src/conversion/text.rs` |
| Chat stream -> Responses stream conversion | `src/conversion/stream_chat_to_responses.rs`, `src/upstream.rs` |
| OpenCode Go request/response integration | `src/server.rs`, `src/upstream.rs` |
| Multimodal model capability guard | `src/media_guard.rs` |
| State replay for tool continuations | `src/state.rs`, `src/codex_chat_history.rs` |

The state replay implementation is a narrow StoredResponse continuation path, not a full port of cc-switch's history repair subsystem.

## Explicit non-goals

Not planned for this project:

- full cc-switch port
- provider aggregation platform
- UI, hooks, plugins, statusLine, or OpenCode session management
- automatic model fallback/routing
- automatic multimodal retry after stripping media
- silent multimodal degradation that makes a text-only model pretend it saw media
- MissionV1, OSS agent task tiers, evidence ledgers, RunRecord, patch escrow, claim gates, or burn-in framework from opencode-bridge

## Additional upstreams

Additional upstream support is only planned after OpenCode Go is validated.

The preferred order is:

1. Keep Codex Responses as the stable frontend contract.
2. Add small profile differences for mostly OpenAI Chat Completions-compatible upstreams.
3. Add provider-specific quirks only when real traffic requires them.
4. Introduce a dedicated provider adapter trait only if profile-level differences are insufficient.
5. Avoid marketplace, routing, fallback, pricing, or UI management features.
