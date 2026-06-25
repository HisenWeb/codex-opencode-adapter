# Codex OpenCode Adapter 使用说明

这份说明用于安装、启动和排查薄 Bridge，目标是避免用真实模型反复试错。

适配器使用 Rust 编写，基于 axum + tokio 异步运行时。

完整文档入口见 [INDEX.md](INDEX.md)。真实验证手册见 [VALIDATION.zh-CN.md](VALIDATION.zh-CN.md)。

## 1. 配置分层

| 配置 | 正确位置 | 作用 |
|---|---|---|
| `model_providers.opencode_go_adapter` | `%USERPROFILE%\.codex\config.toml` | 注册 Codex 可调用的模型服务 |
| Agent TOML | 项目 `.codex\agents\*.toml` | 定义 `oss_flash`、`oss_kimi` 等子代理 |

Codex 会扫描项目级 Agent TOML，但会忽略项目 `.codex/config.toml` 中的
`model_providers`。配置放错时会出现：

- 工具描述能够看到 `oss_flash`；
- 启动时却报 `agent type is currently not available`；
- Bridge 完全收不到请求。

这不是模型故障。先检查用户级 provider 注册，不要重试子代理。

## 2. 安装用户级 Provider

打开：

```text
C:\Users\<用户名>\.codex\config.toml
```

合并以下配置，不要覆盖已有 provider：

```toml
[model_providers.opencode_go_adapter]
name = "OpenCode Go Adapter"
base_url = "http://127.0.0.1:4010/v1"
wire_api = "responses"
request_max_retries = 0
stream_max_retries = 0
stream_idle_timeout_ms = 120000

[model_providers.opencode_go_adapter.auth]
command = "cmd.exe"
args = ["/d", "/s", "/c", "echo codex-opencode-local"]
timeout_ms = 1000
```

`codex-opencode-local` 是本地 Codex → Bridge 的 token，不是 OpenCode Go
API Key。两个凭据不能相同。

Windows 中 `echo` 是 shell 内建命令，不能直接作为 auth helper 启动，因此需要
通过 `cmd.exe` 调用。

[config.toml.example](../config.toml.example) 是合并模板，不应整份复制成项目
`.codex/config.toml`。

## 3. 启动 Bridge

```powershell
cd D:\AI-Tools\codex-opencode-adapter

# 首次构建（或代码更新后）
cargo build --release

# 启动
$env:OPENCODE_GO_API_KEY = "<你的 OpenCode Go API Key>"
$env:CODEX_OPENCODE_LOCAL_TOKEN = "codex-opencode-local"
$env:CODEX_OPENCODE_PORT = "4010"
$env:CODEX_OPENCODE_MAX_CONCURRENCY = "8"
cargo run --release
```

- API Key 只放环境变量，不写入仓库。
- 默认端口为 `4010`，避免与旧 Bridge 的 `4000` 冲突。
- Bridge 必须保持运行，Codex 子代理才能连接。
- `CODEX_OPENCODE_MAX_CONCURRENCY` 是最大并发数，默认 `8`。

## 4. 免费检查

不要先启动子代理。依次检查：

```powershell
Invoke-RestMethod http://127.0.0.1:4010/health
```

期望 `status` 为 `ok`。

```powershell
$headers = @{ Authorization = "Bearer codex-opencode-local" }
(Invoke-RestMethod http://127.0.0.1:4010/v1/models -Headers $headers).data.id
```

应看到 `opencode-go/deepseek-v4-flash` 等模型。

```powershell
cargo test
```

以上任一步失败，都不要启动子代理。

## 5. 让 Desktop 加载配置

1. 完全关闭并重新打开 Codex Desktop。
2. 打开 `D:\AI-Tools\codex-opencode-adapter`。
3. 确认可用角色包含：
   - `oss_flash`
   - `oss_kimi`
   - `oss_deepseek`
   - `oss_mimo`

如果仍是 `oss_flash_support`、`oss_kimi_rapid` 或 `oss_*_investigator`，说明
打开的是旧项目或旧配置仍在生效。

## 6. 第一次低 Token 验收

只启动一次 `oss_flash`，不要并发，不要重试：

```text
只读 pyproject.toml，回答项目名。不要读取其他文件，不要修改文件。只输出项目名。
```

期望：

```text
codex-opencode-adapter
```

完成后立即关闭子代理，并运行 `git status --short`，确认没有额外修改。

## 7. Agent 定位

| Agent | 模型 | Sandbox | 适合 |
|---|---|---|---|
| `oss_flash` | DeepSeek V4 Flash | read-only | 简单查询、文档整理 |
| `oss_kimi` | Kimi K2.6 | read-only | 代码导航、只读分析 |
| `oss_deepseek` | DeepSeek V4 Pro | workspace-write | 边界明确的实现 |
| `oss_mimo` | MiMo V2.5 Pro | workspace-write | 边界明确的实现或复核 |

调查、实现、审查等职责由父 Codex 每次派工决定。Bridge 不派工、不执行工具，
也不判断任务是否完成。

## 8. Reasoning 行为

当前适配器重点保证输出侧 reasoning 兼容：

- 提取上游 `reasoning_content`、`thinking`、`reasoning`、`reasoning_details` 字段。
- 当没有显式 reasoning 字段时，拆分开头的 `<think>...</think>` 块。
- 保存 reasoning 到内部 chat history，保证工具续传上下文有效。
- 不向用户可见输出隐藏 reasoning 正文。

请求侧 provider-specific reasoning 参数仍需真实 OpenCode Go 验证。不要通过增加复杂提示词来猜测 reasoning 是否生效。

## 9. 常见故障

### `agent type is currently not available`

1. 检查 provider 是否位于 `%USERPROFILE%\.codex\config.toml`。
2. 检查名称是否精确为 `opencode_go_adapter`。
3. 重启 Desktop。
4. 确认打开的是新项目。
5. 此时不要重试模型调用。

### `401 Unauthorized`

Codex provider auth 的 token 必须与 `CODEX_OPENCODE_LOCAL_TOKEN` 相同。不要把
OpenCode Go API Key 配成本地 token。

### 无法连接 `127.0.0.1:4010`

```powershell
Get-NetTCPConnection -State Listen -LocalPort 4010
```

没有监听时重新启动 Bridge，不要重试子代理。

### Bridge 有请求，但上游返回 401

检查启动 Bridge 的进程是否设置了 `OPENCODE_GO_API_KEY`。不要在日志或聊天中粘贴
API Key。

### 工具结果无法续传

保持同一个 Bridge 进程及状态数据库。状态过期或数据库被删除后，应开始新任务，
不要无限重试旧调用。

需要更细的续传诊断时，用 debug 日志启动：

```powershell
$env:RUST_LOG = "codex_opencode_adapter=debug"
cargo run --release
```

重点看这些事件：

```text
stored_response_not_found
tool_history_unique_fallback_hit
tool_history_call_id_ambiguous
tool_history_response_ambiguous
tool_history_call_id_not_found
stateless_tool_history_bypass_state_lookup
```

完整说明见 [DIAGNOSTICS.md](DIAGNOSTICS.md)。

### 非流式上游错误

当前 `/v1/responses` 非流式上游 HTTP 错误会返回 Responses `status: failed` body，同时保留上游 HTTP status。真实 Codex subagent 验证时需要确认非 2xx status 是否会断链。

## 10. Token 控制纪律

排障顺序固定为：

1. 查看配置文件。
2. 检查 `/health`。
3. 检查 `/v1/models`。
4. 运行 mock 测试。
5. 查看 Bridge 是否收到请求。
6. 最后才做一次最短真实子代理调用。

禁止并行启动多个代理、失败后自动重试、跑全模型矩阵、或使用旧 Mission 代理测试
新 Bridge。

## 11. 真实验证入口

完成本地 mock 测试后，再进入真实 OpenCode Go / Codex subagent 验证。

完整步骤见 [VALIDATION.zh-CN.md](VALIDATION.zh-CN.md)。

真实验证应按顺序执行：

1. 启动 adapter，并设置 `RUST_LOG=codex_opencode_adapter=debug`。
2. 检查 `/health`。
3. 检查 `/v1/models`。
4. 验证 `/v1/responses` 非流式文本请求。
5. 验证 `/v1/responses` streaming 文本请求。
6. 验证 function call 与 tool output continuation。
7. 验证 stateless continuation fallback。
8. 验证 streamed tool-call。
9. 记录真实模型 ID、响应 shape、stream terminal events 和 adapter diagnostics。

不要一开始就跑完整子代理任务。先完成真实验证手册，再进入完整 Codex subagent 验证。

## 参考

- [Documentation index](INDEX.md)
- [Codex Subagents](https://developers.openai.com/codex/subagents)
- [Codex Configuration Reference](https://developers.openai.com/codex/config-reference)
