# 真实验证手册

## 目标

这份手册用于在完整 Codex subagent 验证前，先用真实 OpenCode Go API Key 验证 adapter 的基础协议链路。

按顺序执行。遇到第一个失败点就停止，记录请求、响应、模型 ID 和 adapter 日志。

## 0. 验证前检查

```powershell
cd D:\AI-Tools\codex-opencode-adapter
git status --short
cargo fmt --check
cargo test
```

期望：

- 工作区没有非预期修改。
- `cargo fmt --check` 通过。
- `cargo test` 通过。

## 1. 启动 adapter

单独开一个终端运行：

```powershell
$env:OPENCODE_GO_API_KEY = "<你的 OpenCode Go API Key>"
$env:CODEX_OPENCODE_LOCAL_TOKEN = "codex-opencode-local"
$env:CODEX_OPENCODE_PORT = "4010"
$env:CODEX_OPENCODE_MAX_CONCURRENCY = "1"
$env:RUST_LOG = "codex_opencode_adapter=debug"
cargo run --release
```

第一轮真实验证建议把并发设为 `1`。串行链路稳定后再提高并发。

## 2. 免费本地检查

另开一个终端：

```powershell
Invoke-RestMethod http://127.0.0.1:4010/health
```

期望：

```text
status = ok
```

检查模型列表：

```powershell
$headers = @{ Authorization = "Bearer codex-opencode-local" }
(Invoke-RestMethod http://127.0.0.1:4010/v1/models -Headers $headers).data.id
```

记录准备测试的真实模型 ID。

## 3. 非流式文本 smoke

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

期望：

- `object = response`
- `status = completed`
- `model` 仍带 `opencode-go/` 前缀
- 输出文本包含 `adapter-ok`
- 如果上游返回 usage，adapter 响应中也保留 usage shape

## 4. 流式文本 smoke

```powershell
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

期望事件形状：

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

记录 token 是否逐步返回，还是接近结束时一次性返回。

## 5. function call 往返

验证非流式 tool call 能被 Codex 采纳，并能续传 tool output。

请求：

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

期望：

- output 中出现 `function_call`
- `call_id` 非空
- name 是 `run`
- arguments 包含 `cmd`
- adapter 已保存 response state，供下一步续传

续传请求：

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

期望：

- 不出现 `invalid_tool_history`
- 模型能使用 tool output
- 如果 `previous_response_id` 失败，查看 `stored_response_not_found`

## 6. stateless continuation fallback

重复上一节续传，但不传 `previous_response_id`。在 `input` 中同时带上原始 tool call 和 tool output。

期望诊断：

```text
stateless_tool_history_bypass_state_lookup
```

期望行为：

- 不因缺少 stored state 失败
- `build_chat_payload()` 能修复 self-contained history
- 响应仍是合法 Responses shape

## 7. 流式 tool-call 往返

重复 function-call 请求，但设为：

```text
stream = true
```

期望：

- 如果上游先输出文本、后输出 tool call，早期文本不应成为最终 assistant output
- 最终 stream 中出现 tool-call output item
- 终止事件为 `response.completed` 和 `[DONE]`

如果 stream 在正常 finish reason 前结束：

- 记录是否出现 `response.incomplete`
- 记录上游最后一个 chunk
- 记录 adapter 是否有 stream truncation 相关日志

## 8. custom tool smoke

如果测试客户端能请求 custom tool，再验证：

- output item type 是 `custom_tool_call`
- custom tool input 只 finalize 一次
- 续传使用 `custom_tool_call_output`

## 9. tool search smoke

如果测试客户端能请求 `tool_search`，再验证：

- output item type 是 `tool_search_call`
- tool search arguments 保持 JSON shape
- 续传使用 `tool_search_output`

## 10. 文本模型 multimodal failure smoke

向一个已知或疑似文本模型发送小型 image/file/audio 输入。

期望：

- 非流式请求返回 Responses object，`status = failed`
- `error.code = unsupported_multimodal_input`
- 流式请求发出 `response.failed` 和 `[DONE]`
- 父 agent 收到协议合法 failure，而不是 provider error 断链

## 11. 非流式上游错误策略检查

临时使用错误的上游 key 或错误的上游 model 调用 `/v1/responses` 非流式。

当前期望行为：

- HTTP status 可能是非 2xx
- body 仍是 Responses object，`status = failed`
- `error.type` 和 `error.code` 是 `upstream_error`

决策点：

如果真实 Codex subagent 把非 2xx 当作断链，即使 body 是 Responses failed，也需要把 `/v1/responses` 的上游错误改成 HTTP 200 + `response.status = failed`。不要同时改 `/v1/models`，除非另有验证证据。

## 12. 重点 diagnostics

用：

```powershell
$env:RUST_LOG = "codex_opencode_adapter=debug"
```

重点观察：

```text
stored_response_not_found
tool_history_unique_fallback_hit
tool_history_call_id_ambiguous
tool_history_response_ambiguous
tool_history_call_id_not_found
stateless_tool_history_bypass_state_lookup
```

解释见 `docs/DIAGNOSTICS.md`。

## 13. 记录模板

每轮真实验证复制一份：

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

## 14. 停止条件

出现以下任一情况，停止真实验证，先补 regression test 再修 adapter：

- `/v1/responses` 返回 plain `{error: ...}`
- stream 没有任何 terminal Responses event 就结束
- 有效 `previous_response_id` 下 tool output continuation 失败
- tool output continuation 被错误匹配到其他 stored response
- 文本模型 multimodal failure 导致协议断链
- 上游出现当前 tests 未覆盖的新 content/tool shape
