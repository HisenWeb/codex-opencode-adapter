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
| Multimodal input conversion | Implemented | Request-side image/file/audio conversion and text-only model guard are covered by mock/regression tests |
| Non-stream upstream failure shape | Implemented, pending real validation | `/v1/responses` non-stream upstream HTTP errors return a Responses `status: failed` body while currently preserving upstream HTTP status |
| Multimodal output generation | Planned | Current phase keeps text output reliable; image/audio/file output mapping is planned for later phases |
| Other upstream providers | Planned | Current default target is OpenCode Go |
| Mock integration tests | Implemented | L2 tests use mock upstream behavior and do not require external OpenCode Go calls |
| Real OpenCode Go / Codex subagent validation | Pending | Must be verified with a real OpenCode Go API key and a real Codex subagent workflow |

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

Implemented in mock/regression coverage; real upstream validation is still pending:

- Mixed text/image/file/audio Responses input conversion.
- Anthropic-style base64 image source -> Chat `image_url` data URL.
- `input_file` mapping only when a usable `file_id` or `file_data` is present.
- `input_audio` mapping.
- Known text-only model guard.
- Reactive upstream multimodal unsupported error detection.
- Multimodal regression tests.

## Known gaps before real validation

These should be verified against real Codex subagent traffic before expanding the adapter:

- Some strict reasoning models may require a non-empty `reasoning_content` placeholder on assistant messages that contain `tool_calls`.
- Non-stream upstream errors already return a Responses `status: failed` body, but the adapter currently preserves upstream HTTP status; real Codex validation must confirm whether non-2xx status breaks the subagent chain.
- Streaming incomplete termination must be verified against Codex's exact event expectations.
- Multimodal output mapping is not implemented until real upstream output shapes are observed.

## P3-lite real validation

Next planned milestone:

- Start the adapter locally with a real OpenCode Go API key.
- Verify `/v1/models` and basic `/v1/responses` non-stream requests.
- Verify `/v1/responses` streaming requests.
- Verify reasoning output with known DeepSeek/MiMo models.
- Verify normal function-call round trip.
- Verify streamed function-call round trip.
- Verify custom tool-call round trip.
- Verify tool-search call round trip.
- Verify `function_call_output` / `custom_tool_call_output` / `tool_search_output` continuation through stored state and stateless full-history input.
- Verify non-stream upstream failures do not break Codex subagent control flow. If they do, switch `/v1/responses` upstream failures to HTTP 200 with `response.status = failed`.

Detailed steps are in `docs/VALIDATION.zh-CN.md`.

## P3-full Codex subagent validation

After P3-lite passes:

- Configure Codex subagent to call this adapter as `opencode-go/...` models.
- Run a real text-only subagent task.
- Run a real tool-using subagent task.
- Run a real streaming tool-using subagent task.
- Run a real multimodal input smoke test against a model believed to support vision.
- Run a real text-only-model multimodal failure test and confirm the parent agent receives a protocol-valid Responses `failed` result rather than a broken provider error.
- Record exact model IDs and observed OpenCode Go response shapes.

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

Only after real validation:

- Patch concrete upstream shape mismatches found in P3.
- Add regression tests for every real incompatibility found.
- Tighten model capability metadata only when a model has been verified.
- Improve diagnostics/logging around failed upstream streams and unsupported media.
- Keep protocol conversion code small and avoid introducing a provider registry unless a second provider becomes an explicit project goal.
