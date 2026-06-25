# 真实验证记录（2026-06-25）

## 概要

这份文档记录 2026-06-25 对 `codex-opencode-adapter` 进行的真实 OpenCode Go 链路验证结果。

目标不是替代 `docs/VALIDATION.zh-CN.md` 的操作手册，而是沉淀：

- 这次实际验证了哪些链路
- 哪些行为已经从 mock/regression 升级为真实验证通过
- 哪些现象是已知限制、测试方法问题，或后续仍需继续确认

## 验证环境

- 日期：`2026-06-25`
- adapter 仓库：`D:\AI-Tools\codex-opencode-adapter`
- 上游 base URL：`https://opencode.ai/zen/go/v1`
- adapter 本地地址：`http://127.0.0.1:4010`
- 本地鉴权：`Bearer codex-opencode-local`
- 并发限制：`CODEX_OPENCODE_MAX_CONCURRENCY=1`
- 日志级别：`RUST_LOG=codex_opencode_adapter=debug`

## 本次使用模型

- 纯文本主测模型：`opencode-go/deepseek-v4-flash`
- 多模态主测模型：`opencode-go/mimo-v2.5`

本次实际从 `/v1/models` 读取到的相关模型还包括：

- `opencode-go/deepseek-v4-pro`
- `opencode-go/glm-5`
- `opencode-go/glm-5.1`
- `opencode-go/glm-5.2`
- `opencode-go/kimi-k2.5`
- `opencode-go/kimi-k2.6`
- `opencode-go/kimi-k2.7-code`
- `opencode-go/minimax-m2.5`
- `opencode-go/minimax-m2.7`
- `opencode-go/minimax-m3`
- `opencode-go/mimo-v2-pro`
- `opencode-go/mimo-v2-omni`
- `opencode-go/mimo-v2.5-pro`
- `opencode-go/qwen3.5-plus`
- `opencode-go/qwen3.6-plus`
- `opencode-go/qwen3.7-plus`
- `opencode-go/qwen3.7-max`

## 已通过的真实验证

### 1. `/v1/models`

结果：通过

观察：

- adapter 可以正常代理上游模型列表
- 返回模型名保留 `opencode-go/` 前缀
- `deepseek-v4-flash` 与 `mimo-v2.5` 均在真实返回列表中

### 2. 非流式文本请求

模型：`opencode-go/deepseek-v4-flash`

结果：通过

观察：

- `/v1/responses` 返回 `object = response`
- `status = completed`
- 输出文本正确返回 `adapter-ok`
- `usage` 字段有真实上游 token 数据
- 输出中包含 reasoning item 与 message item

### 3. 流式文本请求

模型：`opencode-go/deepseek-v4-flash`

结果：通过

观察：

- 流式事件链完整
- 实际看到：
  - `response.created`
  - `response.in_progress`
  - reasoning 增量事件
  - `response.output_item.added`
  - `response.output_text.delta`
  - `response.output_text.done`
  - `response.output_item.done`
  - `response.completed`
  - `[DONE]`
- 流式返回中 reasoning 与正文都能正确组装

### 4. 纯文本模型的多模态失败

模型：`opencode-go/deepseek-v4-flash`

结果：通过

观察：

- 当请求包含 `input_image` 时，adapter 在本地直接返回协议级失败
- 返回：
  - `status = failed`
  - `error.type = unsupported_multimodal_input`
  - `error.code = unsupported_multimodal_input`
- 没有退化成上游 provider 原始错误

结论：

- 文本模型的多模态保护链路已经真实验证通过

### 5. 多模态输入链路

模型：`opencode-go/mimo-v2.5`

结果：通过

观察：

- 非流式图片输入请求能完成
- `usage.input_tokens_details.image_tokens` 出现非零值，说明图片输入已被上游消费

补充验证：

- 使用 `1x1` 极小占位 PNG 时，模型回答倾向于“看不到图片/请重新发送”
- 改用明确可识别测试图后，模型能够正确识别：
  - 中间单词：`CAT`
  - 背景四象限颜色：红、蓝、绿、黄

结论：

- `mimo-v2.5` 真实视觉输入链路可用
- 先前“看不到图片”的现象更像测试图片无信息量，而不是 adapter 丢图

### 6. 非流式 function call

模型：`opencode-go/deepseek-v4-flash`

结果：通过

测试意图：

- 请求模型调用 `run` 工具
- 约束参数为 `cmd`

观察：

- 输出中正确出现 `function_call`
- `call_id` 非空
- `name = run`
- `arguments = {"cmd":"echo tool-ok"}`

结论：

- 非流式工具调用主链路已真实验证通过

### 7. `previous_response_id` continuation

模型：`opencode-go/deepseek-v4-flash`

结果：通过

测试方式：

1. 先请求模型发起 `run` 工具调用
2. 再带 `previous_response_id`
3. 发送 `function_call_output`

观察：

- 续传请求没有出现 `invalid_tool_history`
- adapter 能正确接上前一轮 response
- 返回仍是合法 Responses shape

结论：

- `previous_response_id` continuation 已真实验证通过

### 8. Stateless full-history continuation

模型：`opencode-go/deepseek-v4-flash`

结果：通过

测试方式：

- 不传 `previous_response_id`
- 在 `input` 中完整带上：
  - 原始用户消息
  - `function_call`
  - `function_call_output`
  - 后续用户消息

观察：

- 当历史带得足够完整时，adapter 能正确走 stateless repair
- 最终模型可基于 tool result 返回 `tool-ok`

注意：

- 如果只带 tool output 和残缺上下文，模型行为会发散，不能据此判断 stateless repair 失败
- 这类测试必须尽量贴近真实 Codex 提交的完整 history

结论：

- stateless full-history continuation 已真实验证通过

### 9. 流式 function call

模型：`opencode-go/deepseek-v4-flash`

结果：通过

观察：

- 流中先出现 reasoning 事件
- 之后出现工具调用生命周期事件
- 实际看到：
  - `response.output_item.added`
  - 多次 `response.function_call_arguments.delta`
  - `response.function_call_arguments.done`
  - `response.output_item.done`
  - `response.completed`
  - `[DONE]`
- 最终输出中的 `function_call.arguments` 正确组装为：
  - `{"cmd":"echo stream-tool-ok"}`

结论：

- 流式工具调用组装链路已真实验证通过

## 本次验证中确认的行为

### 1. 并发限制会主动生效

由于本次按手册设置：

- `CODEX_OPENCODE_MAX_CONCURRENCY=1`

当并行发起多个真实请求时，adapter 会返回：

- `rate_limit_error`

这说明：

- adapter 并发上限逻辑是生效的
- 流式失败场景下也能返回协议正确的 `response.failed`

### 2. 测试图片质量会直接影响多模态判断

已确认：

- `1x1` 占位图不适合作为视觉能力 smoke
- 应使用可识别、信息量明确的图片进行多模态验证

建议最小测试图应满足：

- 有明显文字
- 有高对比颜色块
- 分辨率至少足够让模型稳定提取结构

### 3. 当前上游会返回 reasoning

在 `deepseek-v4-flash` 与 `mimo-v2.5` 的真实返回中，都观察到了 reasoning 输出。

这说明：

- adapter 当前的 reasoning item / SSE reasoning 处理不是只在 mock 情况下工作
- 至少对这两条模型路径，真实链路里 reasoning 映射是活的

## 当前可以提升的项目状态判断

结合这次真实验证，可以把项目状态从“主要停留在 mock/regression 覆盖”上调为：

- 文本主链路：已真实验证
- 工具调用主链路：已真实验证
- continuation 主链路：已真实验证
- 多模态输入主链路：已真实验证
- 文本模型多模态失败保护：已真实验证

这意味着：

- 该 adapter 已经不是仅靠本地测试推断可用
- 至少在 `deepseek-v4-flash` 与 `mimo-v2.5` 这两条低成本模型路径上，具备真实使用基础

## 仍待继续验证

本次还没有完成的真实验证项：

- `custom_tool_call`
- `tool_search_call`
- 真实 Codex subagent 端到端接入
- 上游故意报错时，真实 Codex 客户端是否会被非 2xx `/v1/responses` 断链
- 更广模型范围下的 reasoning / multimodal 兼容性差异

## 建议的下一步

建议按这个顺序继续：

1. 真实验证 `custom_tool_call`
2. 真实验证 `tool_search_call`
3. 用真实 Codex subagent 直接接入本 adapter
4. 故意制造上游错误，判断 `/v1/responses` 是否需要改成 HTTP 200 + `status: failed`

## 结论

截至 `2026-06-25`，`codex-opencode-adapter` 已在真实 OpenCode Go 上游上验证通过以下关键能力：

- 模型列表代理
- 文本非流式与流式响应
- reasoning 输出映射
- function call 非流式与流式
- `previous_response_id` continuation
- stateless full-history continuation
- 多模态输入传递
- 文本模型多模态失败保护

当前最合理的项目判断是：

- 该 adapter 已具备进入真实 Codex subagent 接入测试阶段的条件
- 后续重点不再是证明“基础协议是否成立”，而是补齐更少见工具类型和真实客户端容错行为
