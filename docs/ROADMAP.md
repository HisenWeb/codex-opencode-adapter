# Roadmap

## Current status

The project is in the protocol-compatibility and validation phase.

| Area | Status | Notes |
|---|---|---|
| Responses -> Chat request conversion | Implemented | Covers instructions, input history, tool calls, tool outputs, tool choice, and multimodal input blocks |
| Chat -> Responses non-stream conversion | Implemented | Covers text, reasoning extraction, usage, finish reasons, function calls, custom tool calls, tool search calls, and stored replay state |
| Chat stream -> Responses stream conversion | Implemented | Covers Responses SSE lifecycle, text deltas, reasoning deltas, streamed tool-call assembly, custom tool input finalization, tool search, terminal events, failed streams, truncated streams, and tool-enabled text buffering before tool-call adoption |
| Tool lifecycle and state replay | Implemented, narrow scope | Supports `previous_response_id`, unique pending-call fallback, stateless full-history continuation, and continuation diagnostics |
| Reasoning output compatibility | Implemented for extraction/splitting | Reads upstream reasoning fields and leading `<think>...</think>` blocks; request-side provider-specific reasoning controls still need real validation |
| Tool search compatibility | Implemented | `tool_search` has a dedicated schema and converts to/from Responses `tool_search_call` |
| Multimodal input conversion | Implemented, real validation passed | Request-side image/file/audio conversion, text-only model guard, and a real MiMo vision smoke path have been verified |
| Non-stream upstream failure shape | Implemented, real validation passed | `/v1/responses` non-stream upstream HTTP errors return a Responses `status: failed` body while preserving upstream HTTP status |
| Multimodal output generation | Planned | Current phase keeps text output reliable; image/audio/file output mapping is planned for later phases |
| Other upstream providers | Planned | Current default target is OpenCode Go |
| Mock integration tests | Implemented | L2 tests use mock upstream behavior and do not require external OpenCode Go calls |
| Real OpenCode Go upstream smoke | Implemented | Real `/v1/models`, text, stream, function-call, continuation, multimodal, custom tool, and tool-search smoke validation completed on 2026-06-25 |
| Real Codex subagent E2E | Partial | Project custom subagent smoke has been exercised, but broader end-to-end Codex task validation is still pending |

## Completed protocol work

Implemented in the current codebase:

- Responses request -> Chat request conversion.
- Chat non-stream response -> Responses response conversion.
- Chat SSE stream -> Responses stream conversion.
- Function tool, namespace tool, custom tool, and tool search conversion.
- Tool result continuation, `previous_response_id` state replay, unique pending-call fallback, and stateless full-history repair.
- Tool continuation diagnostics for stored response misses, unique fallback, ambiguity, missing call IDs, and stateless bypass.
- Tool-enabled streaming text buffering to avoid content-before-tool adoption issues.
- Non-stream upstream error regression coverage for Responses `status: failed` body shape.
- Reasoning field extraction and compatibility handling.
- Usage, finish reason, incomplete, failed, and terminal event mapping.
- SSE parsing compatibility for LF/CRLF blocks and upstream `event:error` cases.

## Compatibility hardening completed

- Tool search schema and lifecycle fixes.
- Streaming custom/tool search event separation.
- Custom tool partial argument handling.
- Upstream stream error translation into Responses failed events.
- Additional regression tests for custom tool and tool search behavior.
- Streaming tool-call delta regression tests for split `id`/`name`/`arguments`, out-of-order argument arrival, interleaved indexes, type separation, and malformed missing-name chunks.

## Multimodal compatibility completed

Implemented in mock/regression coverage and verified on the real upstream for the currently tested models:

- Mixed text/image/file/audio Responses input conversion.
- Anthropic-style base64 image source -> Chat `image_url` data URL.
- `input_file` mapping only when a usable `file_id` or `file_data` is present.
- `input_audio` mapping.
- Known text-only model guard.
- Reactive upstream multimodal unsupported error detection.
- Multimodal regression tests.

## Known gaps after initial real validation

These should stay on the watch list before expanding the adapter further:

- Some strict reasoning models may require a non-empty `reasoning_content` placeholder on assistant messages that contain `tool_calls`.
- Streaming incomplete termination must be verified against Codex's exact event expectations.
- Multimodal output mapping is not implemented until real upstream output shapes are observed.

## Next milestone

Priority next step:

- Keep real upstream smoke coverage easy to rerun through `scripts/run-real-smoke.ps1`.
- Expand the real smoke suite further into explicit reasoning-model coverage and any model-specific quirks beyond the now-covered text/stream/function-call/custom-tool/tool-search/multimodal basics.
- Decide whether some of those real checks should stay as ignored Rust tests or move into a separate manual/CI smoke layer.

Detailed validation steps are in `docs/VALIDATION.zh-CN.md`, and the latest executed results are in `docs/REAL_VALIDATION_2026-06-25.zh-CN.md`.

## Follow-up Codex validation

After the repeatable smoke layer is stable:

- Configure Codex subagent to call this adapter as `opencode-go/...` models.
- Run a real text-only subagent task.
- Run a real tool-using subagent task.
- Run a real streaming tool-using subagent task.
- Run a broader set of real Codex tasks that exercise tool choice variation and longer continuation chains.
- Record any model-specific quirks in output shape, reasoning fields, or stream behavior.

## Long-term multimodal output support

Planned after the text/tool/reasoning path is stable against real Codex subagent traffic:

- Preserve Chat response content arrays instead of flattening all non-text output to plain text.
- Map upstream image/file/audio output blocks to valid Responses output items when the upstream provides a stable shape.
- Add streaming event support for non-text output parts only after a real upstream format is observed.
- Keep generated image/audio/file artifacts out of adapter-owned state unless Codex requires protocol-level references.
- Prefer external CLI, MCP, or Codex tools for actual media generation/extraction work; the adapter should translate protocol metadata, not become a media runtime.
- Add regression tests for every verified multimodal output shape.

## Long-term additional upstream support

Planned after OpenCode Go is validated:

- Add a small provider/profile boundary for upstreams that are mostly OpenAI Chat Completions-compatible.
- Keep Codex Responses as the stable frontend contract.
- Add provider-specific request/response quirks only when a real model/API requires them.
- Start with configuration/profile differences such as base URL, headers, model IDs, reasoning fields, stream usage, tool-call shape, and error shape.
- Introduce a dedicated provider adapter trait only if a second upstream cannot be handled by profile-level differences.
- Add one upstream at a time with mock tests plus real smoke tests.
- Avoid provider aggregation features such as automatic routing, fallback, price optimization, UI management, or model marketplace behavior.

Candidate future upstream categories:

- OpenAI-compatible Chat Completions APIs.
- Chat-like APIs with small schema differences.
- Responses-compatible APIs that can bypass some conversion steps.

## Stabilization

After the initial real validation phase:

- Patch concrete upstream shape mismatches found in P3.
- Add regression tests for every real incompatibility found.
- Tighten model capability metadata only when a model has been verified.
- Improve diagnostics/logging around failed upstream streams and unsupported media.
- Keep protocol conversion code small and avoid introducing a provider registry unless a second provider becomes an explicit project goal.
