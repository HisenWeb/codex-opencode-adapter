# Codex OpenCode Adapter 使用说明

这份说明用于安装、启动和排查薄 Bridge，目标是避免用真实模型反复试错。

适配器使用 Rust 编写，基于 axum + tokio 异步运行时。

完整文档入口见 [INDEX.md](INDEX.md)。真实验证手册见 [VALIDATION.zh-CN.md](VALIDATION.zh-CN.md)。

## 最短自用流程

如果你只是自己日常用，先看这 3 步就够了：

1. 全局安装（仅首次，或更新后重新安装）：

```powershell
cargo install codex-opencode-adapter
```

2. 初始化配置（安装用户级 Provider + 写入项目配置 + 写入默认 OSS agent 模板）：

```powershell
codex-opencode-adapter init --api-key "<你的 OpenCode Go API Key>"
```

3. 启动 bridge：

```powershell
codex-opencode-adapter start
```

开发阶段可以直接从源码运行：

```powershell
cargo run -- init --api-key "<你的 OpenCode Go API Key>"
cargo run -- start
```

如果这 3 步都正常，就直接进入真实使用；不要为了自用项目先把流程做得太重。

## 1. 配置分层

| 配置 | 正确位置 | 作用 |
|---|---|---|
| `model_providers.opencode_go_adapter` | `%USERPROFILE%\.codex\config.toml` | 注册 Codex 可调用的模型服务 |
| `.codex-opencode-adapter.env` | 当前项目根目录 | 保存项目级 API Key、端口、本地 token、SQLite 路径 |
| Agent TOML | 项目 `.codex\agents\*.toml` | 定义 `oss_flash`、`oss_kimi` 等子代理 |

Codex 会扫描项目级 Agent TOML，但会忽略项目 `.codex/config.toml` 中的
`model_providers`。配置放错时会出现：

- 工具描述能够看到 `oss_flash`；
- 启动时却报 `agent type is currently not available`；
- Bridge 完全收不到请求。

这不是模型故障。先检查用户级 provider 注册，不要重试子代理。

## 2. 安装用户级 Provider

推荐直接使用 `init` 子命令：

```powershell
codex-opencode-adapter init --api-key "<你的 OpenCode Go API Key>"
```

它会自动：

- 更新 `%USERPROFILE%\.codex\config.toml` 中的 `model_providers.opencode_go_adapter`
- 创建或覆盖当前项目的 `.codex-opencode-adapter.env`
- 创建或覆盖默认 OSS agent 模板到 `.codex/agents/`

其中本地 token 会按项目生成并落盘到 `.codex-opencode-adapter.env`，后续可手动修改。

`init` 使用的 provider auth helper 固定为：

```toml
[model_providers.opencode_go_adapter.auth]
command = "codex-opencode-adapter"
args = ["auth", "print-local-token"]
timeout_ms = 1000
```

[config.toml.example](../config.toml.example) 是合并模板，不应整份复制成项目
`.codex/config.toml`。

`scripts/install-user-provider.ps1` 现在只是遗留兼容包装，不再是正式入口。

## 3. 启动 Bridge

```powershell
codex-opencode-adapter start
```

开发阶段直接从源码运行：

```powershell
cd D:\AI-Tools\codex-opencode-adapter

cargo build --release

cargo run -- init --api-key "<你的 OpenCode Go API Key>"
cargo run -- start
```

- 默认配置保存在当前项目的 `.codex-opencode-adapter.env`。
- 默认端口为 `4010`。
- Bridge 必须保持运行，Codex 子代理才能连接。
- `CODEX_OPENCODE_MAX_CONCURRENCY` 是最大并发数，默认 `8`，可手动改 `.codex-opencode-adapter.env`。

## 4. 免费检查

不要先启动子代理。推荐直接运行：

```powershell
codex-opencode-adapter check
```

旧脚本 `./scripts/check-local-adapter.ps1` 仍可作为手动辅助，但正式入口应优先使用 `check` 子命令。

它等价于依次检查：

```powershell
Invoke-RestMethod http://127.0.0.1:4010/health
```

期望 `status` 为 `ok`。

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
   - `oss_glm`
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

## 7. OSS 子代理列表

在当前项目下使用 Codex Desktop 时，可用角色包含：

| 角色 | 上游模型 | Sandbox | 适用场景 |
|---|---|---|---|
| `oss_flash` | DeepSeek V4 Flash | workspace-write | 简单文本任务与轻量修改 |
| `oss_kimi` | Kimi K2.7 Code | workspace-write | 复杂文本任务与综合实现 |
| `oss_glm` | GLM 5.2 | workspace-write | 复杂文本分析、审查与实现 |
| `oss_mimo` | MiMo V2.5 | workspace-write | 简单多模态任务与轻量实现 |

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

Codex provider auth 的 token 必须与项目 `.codex-opencode-adapter.env` 里的
`CODEX_OPENCODE_LOCAL_TOKEN` 相同。不要把 OpenCode Go API Key 配成本地 token。

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

## 10. 上游模型更新时怎么处理

如果 OpenCode Go 的模型列表有更新，不要先改代码，先看：

```powershell
$headers = @{ Authorization = "Bearer codex-opencode-local" }
(Invoke-RestMethod http://127.0.0.1:4010/v1/models -Headers $headers).data.id
```

处理顺序保持简单：

1. 如果只是新增模型：
   直接把 `.codex/agents/*.toml` 里的 `model` 改成新的 `opencode-go/<model-id>`。
2. 如果旧模型下线或改名：
   先以 `/v1/models` 的真实返回为准，再改 agent 配置。
3. 如果模型能力变了：
   改完后跑一次最小真实 smoke：

```powershell
./scripts/run-real-smoke.ps1 -ApiKey "<你的 OpenCode Go API Key>"
```

对自用项目，不建议维护一份手写固定模型表。最稳的做法就是：

- 以 `/v1/models` 为准
- 改 agent TOML
- 跑一次最小 smoke
- 能用就继续

## 11. Token 控制纪律

排障顺序固定为：

1. 查看配置文件。
2. 检查 `/health`。
3. 检查 `/v1/models`。
4. 运行 mock 测试。
5. 查看 Bridge 是否收到请求。
6. 最后才做一次最短真实子代理调用。

禁止并行启动多个代理、失败后自动重试、跑全模型矩阵、或使用旧 Mission 代理测试
新 Bridge。

## 12. 真实验证入口

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
- [resources/templates/](../resources/templates/) — Canonical OSS agent template files
