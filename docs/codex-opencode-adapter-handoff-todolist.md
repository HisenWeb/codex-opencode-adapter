# codex-opencode-adapter 本地 Claude Code 接手 TODO v6

> 用途：给新窗口 / 新执行上下文接手。  
> 核心原则：**新窗口先对齐，不要马上执行。**  
> 状态：P0-1 远程已初步实现，用户反馈本地 `cargo test` 通过；本地 Claude Code 尚未完成对齐/复核；**所有 checklist 默认未勾，必须由本地 Claude Code 亲自阅读代码、运行验证后再勾。**

---

## 0. 本地 Claude Code 接手规则

本地 Claude Code 收到本文档后，第一步不是写代码，而是先对齐、读本地代码、确认本地工作区状态。本文档中的历史记录只说明曾经发生过什么，不等于当前 agent 已完成。

### 必须先做

- [ ] 复述项目目标
- [ ] 复述执行策略
- [ ] 复述本轮只做 P0-1
- [ ] 说明准备修改哪些文件
- [ ] 说明不会修改哪些文件
- [ ] 说明会先复核哪些源码
- [ ] 说明可能的风险点
- [ ] 等用户明确说“开始实现 / 开始改 / 执行 P0-1”后，再写代码

### 明确禁止

- [ ] 不要一上来就改代码
- [ ] 不要一上来就重构
- [ ] 不要一上来就跑大范围改动
- [ ] 不要自动扩大到 P0-3 / P1 / P2
- [ ] 不要把 cc-switch 整套架构搬过来
- [ ] 不要把本文档里的背景判断直接当成已完成代码项

---

## 1. 一句话目标

做一个面向 **Codex subagent** 的**双向协议适配层**，让 Codex 子代理可以使用 **OpenCode Go** 套餐里的模型。

请求方向：

```txt
Codex subagent
  ↓ Responses API request
codex-opencode-adapter
  ↓ Chat Completions-like request
OpenCode Go model
```

响应方向：

```txt
OpenCode Go model
  ↑ Chat Completions-like response / stream
codex-opencode-adapter
  ↑ Responses API response / stream
Codex subagent
```

这不是做大平台，也不是完整搬 cc-switch。本轮 P0-1 只处理响应方向中的 `Chat Completions-like streaming tool_calls -> Responses API streaming events`。

---

## 2. 执行策略

采用：

```txt
功能移植为主，局部增量落地
```

含义：

- cc-switch 是 **行为参考实现**
- 当前 Rust adapter 是 **承载位置**
- adapter 最终定位是双向协议适配：请求方向做 `Responses -> Chat Completions-like`，响应方向做 `Chat Completions-like -> Responses`
- 不复制 cc-switch 的 UI / CLI / provider / 配置系统
- 不做 provider 聚合平台
- 不在当前代码上零散堆 if
- P0-1 只对响应方向里不符合 cc-switch 行为的 streaming tool_call 逻辑做局部替换
- 每轮修改后更新本文档进度

---

## 3. 重要说明：哪些状态不能直接打勾

新窗口接手时，不能因为本文档写了“上一个窗口已讨论过”就直接把执行项打勾。

本文档中的状态分三类：

### A. 背景事实

用于理解目标，不代表代码已完成。

### B. 开工复核

新窗口应快速重新查看仓库源码后再勾选。

### C. 执行 TODO

只有实际改代码、跑测试、确认结果后才能勾选。

---

## 4. 背景事实，供新窗口理解

### 4.1 目标仓库

```txt
https://github.com/HisenWeb/codex-opencode-adapter
```

本地路径：

```txt
D:\AI-Tools\codex-opencode-adapter
```

第一轮主要目标文件：

```txt
src/conversion/stream_chat_to_responses.rs
```

### 4.2 cc-switch 参考仓库

远程仓库：

```txt
https://github.com/farion1231/cc-switch
```

本地路径，优先读取本地源码：

```txt
D:\AI-Tools\cc-switch
```

重点参考文件：

```txt
D:\AI-Tools\cc-switch\src-tauri\src\proxy\providers\streaming_codex_chat.rs
D:\AI-Tools\cc-switch\src-tauri\src\proxy\providers\transform_codex_chat.rs
```

辅助参考：

```txt
D:\AI-Tools\cc-switch\src-tauri\src\proxy\providers\streaming.rs
```

### 4.3 上一窗口已经形成的判断

> 注意：这些是交接判断，不是新窗口的执行完成项。新窗口可以快速复核。

- cc-switch 的实际实现是 Rust/Tauri，不是交接里误写的 TS 路径。
- 当前最应参考的是 `streaming_codex_chat.rs`。
- `streaming.rs` 更偏 OpenAI SSE → Anthropic SSE，不是当前主参考。
- 当前项目基础协议转换已有壳。
- 当前第一风险点不是普通文本，而是 `streaming tool_calls`。
- 当前 `StreamAssembler` 有轻量状态雏形，但 tool_call 生命周期不完全对齐 cc-switch。
- 第一轮不应扩大到 provider / config / server 大改。

---

## 5. 本地 Claude Code 第一轮对齐清单

> 本地 Claude Code 应先回复这一节内容，而不是直接改代码。

### 5.1 需要向用户确认的理解

- [ ] 目标是给 Codex subagent 使用 OpenCode Go 模型
- [ ] 方式是做 Responses API ↔ Chat Completions-like API 的双向协议转换层
- [ ] 请求方向是 `Responses -> Chat Completions-like`，响应方向是 `Chat Completions-like -> Responses`
- [ ] cc-switch 是行为参考，不是完整复制对象
- [ ] 当前只做 P0-1：响应方向中的 streaming tool_call 生命周期移植
- [ ] 第一轮默认只改 `src/conversion/stream_chat_to_responses.rs`
- [ ] 测试可以补，但不要为了测试大改项目结构

### 5.2 需要向用户说明的执行边界

本轮不做：

- [ ] 不做 provider 平台
- [ ] 不做配置系统重构
- [ ] 不做 OpenCode Go 真实接入测试
- [ ] 不做 P0-3 stream truncated 收口
- [ ] 不做 P1 request transform
- [ ] 不做 P2 non-stream response
- [ ] 不重写整个 adapter

### 5.3 需要向用户说明的实现策略

- [ ] 不是零散补丁
- [ ] 不是整文件复制 cc-switch
- [ ] 是把 cc-switch 的 tool_call 生命周期移植到当前 `StreamAssembler`
- [ ] 主要是替换当前 `tool_calls: BTreeMap<usize, Value>` 的不稳定实现
- [ ] 用显式 `StreamingToolCall` 状态承载 index / call_id / name / arguments / added / done

---

## 6. 本地 Claude Code 开工复核清单

> 只有用户明确同意开始后，才进入本节。  
> 这些项目必须由本地 Claude Code 自己复核后才能打勾。

### 6.1 复核目标仓库

- [ ] 打开目标仓库 `HisenWeb/codex-opencode-adapter`
- [ ] 确认当前分支和工作区状态
- [ ] 打开 `src/conversion/stream_chat_to_responses.rs`
- [ ] 确认当前 `StreamAssembler` 仍包含 `tool_calls: BTreeMap<usize, Value>`
- [ ] 确认 `accept_tool_delta` 仍存在 `unwrap_or(0)` 或等价默认 index=0 行为
- [ ] 确认 `ensure_tool_started` 当前 start 条件仍没有同时要求 `call_id + name`
- [ ] 确认 `finalize()` 当前是否缺少开头 `terminal_emitted` 保护

说明：

- 已通过 GitHub 远程仓库复核默认分支内容；当前执行环境无法访问用户本机 `D:\AI-Tools\codex-opencode-adapter` 工作区，因此“当前分支和工作区状态”暂不打勾。

### 6.2 复核本地 cc-switch 参考行为

- [ ] 打开 cc-switch 的 `streaming_codex_chat.rs`
- [ ] 确认 cc-switch 按 `index -> ToolCallState` 绑定 tool_call
- [ ] 确认 cc-switch 是 `call_id + name` 齐全后才 start
- [ ] 确认 cc-switch arguments 是 append/cache，并在 start 后补发 pending delta
- [ ] 确认 cc-switch finalize 会补齐 arguments done / output item done
- [ ] 确认 cc-switch terminal event 有 completed/failed 幂等保护

### 6.3 复核实现范围

- [ ] 第一轮只做 P0-1
- [ ] 第一轮优先只改 `src/conversion/stream_chat_to_responses.rs`
- [ ] 不混入 P0-3 stream truncated 收口
- [ ] 不混入 P1 request transform
- [ ] 不混入 P2 non-stream response
- [ ] 不做 provider 平台
- [ ] 不做 OpenCode Go 实测

---

## 7. P0-1：移植 cc-switch 的 streaming tool_call 生命周期

目标文件：

```txt
src/conversion/stream_chat_to_responses.rs
```

### 7.1 P0-1 总原则

P0-1 不是在当前 `Value` 状态上继续补 if，而是把 tool_call 生命周期整体替换成显式状态模型。

核心链路：

```txt
index -> tool_call state
call_id/name ready -> response.output_item.added
arguments append/cache
pending arguments replay
arguments done
output_item done
terminal once
```

---

## 8. P0-1 执行 TODO

### 8.1 引入显式 StreamingToolCall 状态

当前目标：

```rust
tool_calls: BTreeMap<usize, StreamingToolCall>
```

建议结构：

```rust
struct StreamingToolCall {
    output_index: Option<u32>,
    item_id: String,
    call_id: String,
    name: String,
    arguments: String,
    added: bool,
    done: bool,
}
```

TODO：

- [ ] 新增 `StreamingToolCall` struct
- [ ] 将 `tool_calls` 从 `BTreeMap<usize, Value>` 改为 `BTreeMap<usize, StreamingToolCall>`
- [ ] 移除 tool_call 内部用 `serde_json::Value` 存状态的写法
- [ ] 保留 `BTreeMap`，确保 finalize 顺序 deterministic

验收：

- [ ] 每个 tool_call 的状态字段可直接读写
- [ ] 不再依赖 JSON path 读写内部状态
- [ ] finalize 时遍历顺序稳定

---

### 8.2 index 绑定

当前风险：

```rust
unwrap_or(0)
```

第一版策略：

```txt
缺失 index 的 tool_call delta 直接忽略并 warning，不归入 index=0。
```

TODO：

- [ ] `index` 必须存在
- [ ] 缺失 index 时不再默认归到 0
- [ ] 缺失 index 的 delta 不参与 merge
- [ ] 缺失 index 时记录 warning
- [ ] 不污染 index=0

验收：

- [ ] 多 tool_call 交错时不会串线
- [ ] 缺失 index 不会污染 index=0
- [ ] `index -> tool state` 是唯一归属锚点

---

### 8.3 call_id 生命周期稳定

规则：

- 新建 state 时 `call_id` 为空
- delta 带 `id` 时写入
- 未 added 前允许补真实 id
- 已 added 后禁止覆盖 id
- finalize 时仍无 id，fallback 为 `call_{index}`
- 禁止用随机 UUID 生成 call_id

TODO：

- [ ] 新 state 的 `call_id` 初始为空
- [ ] delta 带 id 时写入 `call_id`
- [ ] 未 added 前允许更新 `call_id`
- [ ] added 后收到不同 id 时 warning，不覆盖
- [ ] finalize 时缺失 `call_id` fallback 为 `call_{index}`
- [ ] 删除随机 UUID call_id fallback

验收：

- [ ] 同一 index 的 call_id 生命周期稳定
- [ ] start 后 call_id 不再变化
- [ ] final output / pending_call_ids / replay tool_calls 使用同一 call_id

---

### 8.4 name merge 行为

规则：

- name delta 覆盖，不 append
- name 为空不覆盖
- added 后不再改 name
- added 后收到不同 name，warning，不修改

TODO：

- [ ] name delta 改为覆盖
- [ ] name 为空时不覆盖
- [ ] added 后不再改 name
- [ ] added 后收到不同 name 时记录 warning

验收：

- [ ] name 不会被拼接坏
- [ ] name 一旦用于 start 就保持稳定
- [ ] 不生成错误工具名

---

### 8.5 tool_call start 条件

必须满足：

```txt
!added && !call_id.is_empty() && !name.is_empty()
```

TODO：

- [ ] 修改 `ensure_tool_started(index)`
- [ ] id 未到时不 start
- [ ] name 未到时不 start
- [ ] start 前 finish text item
- [ ] start 前 finish reasoning item
- [ ] 分配 `output_index`
- [ ] 设置 `added = true`
- [ ] 发 `response.output_item.added`

验收：

- [ ] start 时 call_id/name 都是稳定值
- [ ] start 后 item 状态为 `in_progress`
- [ ] 不会提前生成不完整 function_call item

---

### 8.6 pending arguments 补发

规则：

- arguments delta 永远 append 到 `state.arguments`
- added=true 时立即发 `response.function_call_arguments.delta`
- added=false 时只缓存
- start 后如果已有 pending arguments，立刻补发一次 delta
- 不重复发同一段

TODO：

- [ ] arguments delta append 到 `state.arguments`
- [ ] added=true 时立即发 arguments delta
- [ ] added=false 时缓存 arguments
- [ ] start 后补发 pending arguments
- [ ] 防止重复补发

验收：

- [ ] arguments 先到不会丢
- [ ] id/name 后到时 start 后立刻补发 pending arguments
- [ ] final done arguments 与 delta 累计一致
- [ ] 不重复发同一段 arguments

---

### 8.7 function_call done

TODO：

- [ ] finalize 遍历 `BTreeMap<usize, StreamingToolCall>`
- [ ] 已 done 的跳过
- [ ] name 缺失的 tool_call 跳过并 warning
- [ ] call_id 缺失时 fallback 为 `call_{index}`
- [ ] 未 added 但 name 有效时，finalize 阶段补发 `response.output_item.added`
- [ ] arguments 用 `canonicalize_json_string_if_parseable`
- [ ] 发 `response.function_call_arguments.done`
- [ ] 发 `response.output_item.done`
- [ ] 设置 `done = true`
- [ ] 加入 final response output
- [ ] 加入 stored assistant.tool_calls replay
- [ ] 加入 pending_call_ids

验收：

- [ ] done 只发一次
- [ ] final output 中 function_call 完整
- [ ] pending_call_ids 与 function_call.call_id 一致
- [ ] stored history 可被下一轮 function_call_output 正确接上

---

### 8.8 terminal 幂等

当前目标：

```rust
if self.terminal_emitted {
    return Ok(json!({}));
}
```

TODO：

- [ ] finalize 开头增加 `terminal_emitted` 检查
- [ ] `response.completed` 只发一次
- [ ] `response.incomplete` 只发一次
- [ ] `response.failed` 只发一次
- [ ] `[DONE]` 后不再被自然结束重复 finalize

验收：

- [ ] 连续调用 finalize 两次不会重复 terminal event
- [ ] fail 后再 finalize 不会重复 terminal event
- [ ] finalize 后再 fail 不会重复 terminal event

---

## 9. P0-2：基础测试补齐

目标：

```txt
用最小测试覆盖 P0-1 的协议行为，不追求大测试框架。
```

TODO：

- [ ] 找出现有测试结构
- [ ] 如果已有 Rust tests，在现有结构补测试
- [ ] 如果没有测试结构，先加最小单元测试
- [ ] 不引入复杂 mock server
- [ ] 不为测试大改生产代码

测试用例：

### Case 1：arguments 先到，id/name 后到

- [ ] 输入 arguments delta
- [ ] 再输入 id/name
- [ ] 再输入 arguments delta
- [ ] 期望 start 后补发 pending arguments
- [ ] 期望 done arguments 完整

### Case 2：多 tool_call 交错

- [ ] index=0 start
- [ ] index=1 start
- [ ] index=1 args
- [ ] index=0 args
- [ ] 期望 arguments 不串线

### Case 3：call_id 后到

- [ ] name 先到
- [ ] args 先到
- [ ] id 后到
- [ ] 期望 id 到之前不 start
- [ ] 期望 id 到之后 start

### Case 4：name 缺失

- [ ] id 有
- [ ] args 有
- [ ] name 缺失
- [ ] 期望不生成假 tool item
- [ ] 期望不污染 pending_call_ids

### Case 5：finalize 幂等

- [ ] 连续 finalize 两次
- [ ] 期望 terminal event 只发一次

### Case 6：fail / finalize 互斥

- [ ] fail 后 finalize
- [ ] finalize 后 fail
- [ ] 期望 terminal event 只发一次

验收：

- [ ] `cargo test` 通过
- [ ] 新增测试能覆盖核心 tool_call 生命周期
- [ ] 测试不依赖真实 OpenCode Go

---

## 10. P0-3：stream 收口补强，P0-1 完成后再做

目标文件：

```txt
src/server.rs
```

说明：

```txt
本项不混入 P0-1，除非测试发现 terminal 幂等必须同步调整 server.rs。
```

TODO：

- [ ] `[DONE]` 继续作为正常 finalize 入口
- [ ] 网络错误转 `response.failed`
- [ ] upstream error chunk 转 `response.failed`
- [ ] JSON parse error 至少 warning
- [ ] stream 自然结束时检查是否已有 finish_reason
- [ ] 无 finish_reason 但有实质输出时，按 incomplete/length 收口
- [ ] 无 finish_reason 且无实质输出时，按 failed/truncated 收口
- [ ] 避免 `[DONE]` 后自然结束二次 finalize

---

## 11. P0-4：function_call_output / previous_response_id 回链确认

目标文件：

```txt
src/conversion/responses_to_chat.rs
src/state.rs
src/conversion/tool_context.rs
```

TODO：

- [ ] 确认 function_call_output 能根据 call_id 找回上一轮 tool_call
- [ ] 确认 pending_call_ids 与 stored output 一致
- [ ] 确认 previous_response_id 能读到 stored response
- [ ] 确认 repair_history 不会破坏 tool_call / tool output 顺序
- [ ] 确认 subagent 多轮工具调用可持续

---

## 12. 暂不执行的后续阶段

### P1：请求转换对齐

- [ ] 对齐 system / instructions 合并
- [ ] 对齐 tools 转换
- [ ] 对齐 tool name sanitize / reverse mapping
- [ ] 对齐 function_call_output 转 tool message
- [ ] 对齐 previous_response_id 历史拼接

### P2：非流式响应转换

- [ ] 对齐普通文本 response
- [ ] 对齐 reasoning_content
- [ ] 对齐 tool_calls
- [ ] 对齐 usage
- [ ] 对齐 finish_reason

### P3：OpenCode Go 实测

- [ ] 配置 OpenCode Go upstream
- [ ] Codex subagent 指向 adapter
- [ ] 测试纯文本回答
- [ ] 测试一次工具调用
- [ ] 测试连续工具调用

### P4：收敛与清理

- [ ] 删除临时 debug 输出
- [ ] 更新 README 定位说明
- [ ] 写明不支持范围
- [ ] 整理最终测试命令

---

## 13. 每次对齐规则

每次完成一轮修改后，只更新以下内容：

1. 勾选已完成 TODO
2. 在“进度记录”追加一条记录
3. 写清楚改了哪些文件
4. 写清楚验证了什么
5. 写清楚还没做什么
6. 不重写整份文档
7. 不新增无关目标

---

## 14. 进度记录

### 2026-06-23：交接版 v4

状态：

- 本文档用于新窗口接手。
- 尚未开始写代码。
- 尚未实现 P0-1。
- 尚未运行测试。
- 旧窗口已完成目标澄清、cc-switch 审计、目标仓库初步审计，但新窗口仍应先对齐，再按“开工复核清单”自行确认。
- v4 相比 v3 的关键变化：新窗口启动后先对齐，不要马上执行。

下一步：

```txt
新窗口先完成第 5 节对齐，等用户明确确认后，再进入第 6 节开工复核和第 8 节 P0-1 实现。
```


### 2026-06-23：第一轮对齐确认与双向定位修正

状态：

- 已按交接文档完成第一轮对齐。
- 用户已确认可以继续。
- 已修正文档定位：`codex-opencode-adapter` 是双向协议适配层。
- 本轮 P0-1 范围保持不变：只处理响应方向中的 `streaming tool_call` 生命周期。
- 尚未开始 P0-1 代码修改。
- 尚未复核目标仓库源码，故第 6 节仍不能打勾。
- 尚未运行测试。

改动文件：

```txt
docs/codex-opencode-adapter-handoff-todolist.md
```

验证：

- 用户已确认第一轮对齐。
- 确认原始 v4 文档需要先修正“双向协议适配层”表述后再写入仓库。

还没做：

- 尚未复核 `src/conversion/stream_chat_to_responses.rs`。
- 尚未复核 cc-switch 参考文件。
- 尚未实现 P0-1。
- 尚未补测试或运行 `cargo test`。

### 2026-06-23：开工复核完成

状态：

- 已复核 GitHub 远程仓库 `HisenWeb/codex-opencode-adapter` 默认分支。
- 已复核 `src/conversion/stream_chat_to_responses.rs`。
- 已复核 cc-switch 的 `streaming_codex_chat.rs`、`transform_codex_chat.rs`、`streaming.rs`。
- 确认当前 P0-1 风险点仍存在：`BTreeMap<usize, Value>`、缺失 index 默认归 0、name append、start 未同时要求 `call_id + name`、`finalize()` 开头缺少 terminal 幂等保护。
- 本地 Windows 工作区状态无法在当前执行环境读取，故未勾选“确认当前分支和工作区状态”。

改动文件：

```txt
docs/codex-opencode-adapter-handoff-todolist.md
```

验证：

- 通过 GitHub 读取目标仓库源码。
- 通过 GitHub 读取 cc-switch 参考源码。

还没做：

- 尚未运行 `cargo test`。
- 尚未做 OpenCode Go 实测。
- 尚未执行 P0-2 测试补齐。

### 2026-06-23：P0-1 streaming tool_call 生命周期初步实现

状态：

- 已在 `src/conversion/stream_chat_to_responses.rs` 引入显式 `StreamingToolCall` 状态。
- 已将 tool_call 状态从 `BTreeMap<usize, Value>` 替换为 `BTreeMap<usize, StreamingToolCall>`。
- 已移除缺失 index 默认归 0 的行为，改为 warning 后忽略。
- 已实现 `call_id + name` 齐全后才 start。
- 已实现 arguments append/cache，start 后补发 pending arguments，start 后增量立即转发。
- 已实现 finalize 阶段补齐 function_call arguments done / output item done / final output / replay tool_calls / pending_call_ids。
- 已给 finalize 开头增加 terminal 幂等保护。
- 已做一次静态复查并修正 `emit_tool_arguments` 中从借用状态移动 `String` 的风险。

改动文件：

```txt
src/conversion/stream_chat_to_responses.rs
docs/codex-opencode-adapter-handoff-todolist.md
```

验证：

- 已通过源码复查确认 P0-1 关键逻辑落在目标文件内。
- 已复查 `emit_tool_arguments` 的 `item_id` / `call_id` 使用 clone，避免从借用状态移动 `String`。
- 当前执行环境无法 clone GitHub 仓库：`Could not resolve host: github.com`，因此尚未运行 `cargo test`。

还没做：

- 尚未运行 `cargo test`。
- 尚未补 P0-2 测试用例。
- 尚未做 OpenCode Go 真实接入测试。
- 尚未处理 P0-3 stream 收口。
- 尚未处理 P0-4 / P1 请求方向和 history 回链。

### 2026-06-23：修复 TODO 文档误覆盖

状态：

- 曾误将 TODO 文档临时覆盖为占位内容。
- 已立即恢复为包含 P0-1 进度记录的完整文档。
- 代码文件未受该误操作影响。

改动文件：

```txt
docs/codex-opencode-adapter-handoff-todolist.md
```

验证：

- 已重新写入完整 TODO 文档内容。

还没做：

- 尚未运行 `cargo test`。
- 尚未补 P0-2 测试用例。

### 2026-06-23：生成本地 Claude Code 接手版

状态：

- 用户已说明 cc-switch 已 clone 到 `D:\AI-Tools\cc-switch`。
- 本版本将所有 checklist 重置为未勾选，避免本地 Claude Code 误以为自己已经完成对齐、复核或验证。
- 历史完成情况只保留在“进度记录”中；后续勾选必须来自本地 Claude Code 的实际阅读、修改和验证。

改动文件：

```txt
docs/codex-opencode-adapter-handoff-todolist.md
```

验证：

- 确认 checklist 中没有 `[x]`。
- 确认文档包含本地 cc-switch 路径。

还没做：

- 本地 Claude Code 尚未完成接手对齐。
- 本地 Claude Code 尚未阅读目标代码。
- 本地 Claude Code 尚未重新运行 `cargo test`。

---

## 15. 本地 Claude Code 启动提示词

把本文档发给本地 Claude Code 后，可以直接说：

```txt
先不要写代码。

请先读取 docs/codex-opencode-adapter-handoff-todolist.md，并完成接手对齐：

1. 复述你理解的项目目标和当前阶段。
2. 说明你会先读取哪些本地文件。
3. 说明你不会直接进入 P0-2/P0-3/P1。
4. 运行并汇报 git status、git log --oneline -8、cargo test。
5. 阅读 src/conversion/stream_chat_to_responses.rs。
6. 阅读 D:\AI-Tools\cc-switch 中的参考文件。
7. 对照 TODO 说明哪些项可以确认，哪些还不能确认。

我确认后，你再更新 TODO 或进入 P0-2。
```
