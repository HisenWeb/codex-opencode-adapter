# P3-lite real validation checklist

## Scope

This checklist verifies the adapter against a real OpenCode Go API key before full Codex subagent validation.

Run these checks in order. Stop at the first failure and record the exact request, response, model ID, and adapter log excerpt.

## 0. Pre-flight

```powershell
cd D:\AI-Tools\codex-opencode-adapter
git status --short
cargo fmt --check
cargo test
```

Expected:

- clean working tree before validation, or only intentional local notes
- formatting passes
- tests pass

## 1. Start adapter

Use one terminal for the adapter process:

```powershell
$env:OPENCODE_GO_API_KEY = "<your OpenCode Go API key>"
$env:CODEX_OPENCODE_LOCAL_TOKEN = "codex-opencode-local"
$env:CODEX_OPENCODE_PORT = "4010"
$env:CODEX_OPENCODE_MAX_CONCURRENCY = "1"
$env:RUST_LOG = "codex_opencode_adapter=debug"
cargo run --release
```

Keep concurrency at `1` for the first real validation pass. Increase it only after the serial path is stable.

## 2. Free local checks

In a second terminal:

```powershell
Invoke-RestMethod http://127.0.0.1:4010/health
```

Expected:

```text
status = ok
```

Then check models:

```powershell
$headers = @{ Authorization = "Bearer codex-opencode-local" }
(Invoke-RestMethod http://127.0.0.1:4010/v1/models -Headers $headers).data.id
```

Record at least the exact model IDs you plan to test.

## 3. Non-stream text smoke

```powershell
$headers = @{ Authorization = "Bearer codex-opencode-local" }
$body = @{
  model = "opencode-go/deepseek-v4-flash"
  input = "Reply with exactly: adapter-ok"
  stream = $false
} | ConvertTo-Json -Depth 20

Invoke-RestMethod http://127.0.0.1:4010/v1/responses `
  -Method Post `
  -Headers $headers `
  -ContentType "application/json" `
  -Body $body | ConvertTo-Json -Depth 50
```

Expected:

- `object = response`
- `status = completed`
- model remains prefixed as `opencode-go/...`
- output text contains `adapter-ok`
- usage shape is present if upstream supplied usage

## 4. Streaming text smoke

```powershell
$headers = @{ Authorization = "Bearer codex-opencode-local" }
$body = @{
  model = "opencode-go/deepseek-v4-flash"
  input = "Reply with exactly: stream-ok"
  stream = $true
} | ConvertTo-Json -Depth 20

curl.exe -N `
  -H "Authorization: Bearer codex-opencode-local" `
  -H "Content-Type: application/json" `
  -d $body `
  http://127.0.0.1:4010/v1/responses
```

Expected event shape:

```text
response.created
response.in_progress
response.output_item.added
response.output_text.delta
response.output_text.done
response.output_item.done
response.completed
[DONE]
```

Record whether deltas arrive incrementally or only near the end for the tested model.

## 5. Function-call round trip

This phase verifies that a non-stream tool call can be adopted and continued.

Request:

```powershell
$headers = @{ Authorization = "Bearer codex-opencode-local" }
$body = @{
  model = "opencode-go/deepseek-v4-flash"
  input = "Call the run tool with cmd set to echo tool-ok. Do not answer directly."
  stream = $false
  tools = @(
    @{
      type = "function"
      name = "run"
      description = "Run a shell command"
      parameters = @{
        type = "object"
        properties = @{
          cmd = @{ type = "string" }
        }
        required = @("cmd")
      }
    }
  )
} | ConvertTo-Json -Depth 50

$response = Invoke-RestMethod http://127.0.0.1:4010/v1/responses `
  -Method Post `
  -Headers $headers `
  -ContentType "application/json" `
  -Body $body
$response | ConvertTo-Json -Depth 50
```

Expected:

- response output contains a `function_call`
- call has a non-empty `call_id`
- name is `run`
- arguments include `cmd`
- adapter stores the response for continuation

Continuation request:

```powershell
$call = $response.output | Where-Object { $_.type -eq "function_call" } | Select-Object -First 1
$continueBody = @{
  model = "opencode-go/deepseek-v4-flash"
  previous_response_id = $response.id
  input = @(
    @{
      type = "function_call_output"
      call_id = $call.call_id
      output = "tool-ok"
    }
  )
  stream = $false
} | ConvertTo-Json -Depth 50

Invoke-RestMethod http://127.0.0.1:4010/v1/responses `
  -Method Post `
  -Headers $headers `
  -ContentType "application/json" `
  -Body $continueBody | ConvertTo-Json -Depth 50
```

Expected:

- response does not fail with `invalid_tool_history`
- model can use the tool output
- if `previous_response_id` fails, check `stored_response_not_found`

## 6. Stateless continuation fallback

Repeat the function-call continuation without `previous_response_id`, but include the original tool call and the tool output in `input`.

Expected diagnostics:

```text
stateless_tool_history_bypass_state_lookup
```

Expected behavior:

- no stored-state lookup failure
- `build_chat_payload()` repairs the self-contained history
- response remains protocol-valid

## 7. Streamed tool-call round trip

Repeat the tool-call request with `stream = true`.

Expected:

- early text does not become final assistant output if a tool call is later adopted
- final stream contains a tool-call output item
- terminal events end with `response.completed` and `[DONE]`

If the stream ends before a finish reason:

- check whether `response.incomplete` is emitted
- record the upstream terminal shape
- record whether the adapter logged stream truncation

## 8. Custom tool smoke

Use a `custom` tool if Codex or your test client can request one.

Expected:

- output item type is `custom_tool_call`
- custom tool input is finalized once
- continuation uses `custom_tool_call_output`

## 9. Tool-search smoke

Use a `tool_search` tool if the client can request one.

Expected:

- output item type is `tool_search_call`
- tool search arguments remain JSON-shaped
- continuation uses `tool_search_output`

## 10. Multimodal failure smoke for text-only model

Send a small image/file/audio input to a model believed to be text-only.

Expected:

- non-stream request returns a Responses object with `status = failed`
- `error.code = unsupported_multimodal_input`
- streaming request emits `response.failed` and `[DONE]`
- parent agent should receive a protocol-valid failure rather than a broken provider error

## 11. Non-stream upstream failure policy check

Temporarily use an invalid upstream key or invalid upstream model and call `/v1/responses` non-stream.

Expected current behavior:

- HTTP status may be non-2xx
- body is still a Responses object with `status = failed`
- `error.type` and `error.code` are `upstream_error`

Decision point:

If real Codex subagent treats non-2xx as a broken chain despite the Responses body, change `/v1/responses` upstream failures to HTTP 200 with `response.status = failed`. Do not change `/v1/models` behavior unless separately justified.

## 12. Diagnostics to watch

Use `RUST_LOG=codex_opencode_adapter=debug` and watch these events:

```text
stored_response_not_found
tool_history_unique_fallback_hit
tool_history_call_id_ambiguous
tool_history_response_ambiguous
tool_history_call_id_not_found
stateless_tool_history_bypass_state_lookup
```

Interpretation is documented in `docs/p1-continuation-diagnostics.md`.

## 13. Observation template

Copy this block for each real validation run:

```text
Date:
Adapter commit:
OS / shell:
Codex client:
OpenCode Go base URL:
Model alias used:
Upstream model ID after prefix stripping:
Request type: non-stream | stream
Tool type: none | function | custom | tool_search
Multimodal input: none | image | file | audio
HTTP status:
Responses status:
Terminal stream events:
Output item types:
Usage shape:
Continuation mode: previous_response_id | unique call_id fallback | stateless full history | none
Adapter diagnostics:
Unexpected upstream fields:
Result: pass | fail | unclear
Notes:
```

## 14. Stop conditions

Stop real validation and patch the adapter if any of these occur:

- adapter returns plain `{error: ...}` for `/v1/responses`
- stream ends without any terminal Responses event
- tool output continuation fails despite a valid `previous_response_id`
- tool output continuation is accepted for the wrong stored response
- multimodal text-only failure breaks the protocol chain
- upstream emits a new content/tool shape not represented by current tests

Add a regression test before patching any discovered mismatch.
