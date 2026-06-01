# Rust IPC 与 `claude-code-bun` 通信消息对照

> 范围：
> - Rust 侧对比对象：`rust/src/ipc/protocol.rs`、`rust/src/ipc/headless.rs`
> - `claude-code-bun` 侧对比对象：`src/entrypoints/sdk/coreSchemas.ts`、`src/entrypoints/sdk/controlSchemas.ts`、`src/cli/structuredIO.ts`、`src/cli/remoteIO.ts`、`src/bridge/bridgeMessaging.ts`
>
> 结论先行：
> - Rust `src/ipc` 是面向本地 UI 的 headless IPC 协议。
> - `claude-code-bun` 的通信协议是面向 SDK host / bridge / CCR 的分层控制协议。
> - 两边很多消息“语义相近”，但不在同一抽象层，所以字段经常是一对一不成立，只能做近似映射。

## 1. 对照前提

这份对照文档遵循两个原则：

1. 只把“公开出现在协议边界上的消息”拿来比较，不把内部函数参数当协议字段。
2. 遇到 `claude-code-bun` 里用 placeholder 隐去内部结构的消息，明确标注为“推断映射”。

需要特别注意的两类 placeholder：

- `SDKPartialAssistantMessageSchema.event = RawMessageStreamEventPlaceholder()`
- `SDKUserMessageSchema.message = APIUserMessagePlaceholder()`
- `SDKAssistantMessageSchema.message = APIAssistantMessagePlaceholder()`

因此：

- `stream_event.event.*` 的细字段，是根据 Anthropic 原始流事件形状和消费代码推断的，不是 `controlSchemas.ts` 直接钉死的公开字段。
- `assistant.message.content`、`user.message.content` 里的 `tool_use` / `tool_result` block 字段，也是根据内容块约定和消费代码推断的，不是这里的 Zod schema 明文展开。

## 2. 消息类型总览

### 2.1 Frontend -> Backend

| Rust `FrontendMessage` | `claude-code-bun` 最接近消息 | 关系 | 说明 |
|---|---|---|---|
| `submit_prompt` | `SDKUserMessage` | 近似对应 | 都表示“提交一条用户输入”，但 bun 侧是更通用的 user message。 |
| `abort_query` | `control_request(subtype=interrupt)` | 直接对应 | 都是中断当前执行中的 turn。 |
| `permission_response` | `control_response` | 近似对应 | Rust 为权限单独建消息；bun 用通用 control response 返回权限结果。 |
| `slash_command` | `SDKUserMessage` | 近似对应 | bun 没有独立 `slash_command` 协议消息，通常仍走普通用户输入。 |
| `resize` | 无 | Rust 独有 | Rust headless UI 需要终端尺寸同步。 |
| `quit` | 无 | Rust 独有 | bun 主要靠 transport close / process teardown。 |
| `lsp_command` | 无直接公开 peer | Rust 独有 | bun SDK 协议未公开统一 LSP 命令口。 |
| `mcp_command` | `control_request(mcp_*)` | 近似对应 | bun 只公开若干 MCP 控制请求。 |
| `plugin_command` | `control_request(reload_plugins)` | 近似对应 | bun 没有统一 plugin command 枚举。 |
| `skill_command` | 无 | Rust 独有 | bun SDK 协议不公开 skill command。 |
| `query_subsystem_status` | `control_request(mcp_status)` | 弱近似 | bun 只查 MCP，不查统一 subsystem snapshot。 |
| `agent_command` | 无直接公开 peer | Rust 独有 | bun 更常通过 task / agent config 间接表达。 |
| `team_command` | 无直接公开 peer | Rust 独有 | 同上。 |

### 2.2 Backend -> Frontend

| Rust `BackendMessage` | `claude-code-bun` 最接近消息 | 关系 | 说明 |
|---|---|---|---|
| `ready` | `system(init)` + `control_response(initialize)` | 近似对应 | Rust 一条消息完成“就绪”；bun 拆成初始化响应和系统初始化快照。 |
| `stream_start` / `stream_delta` / `thinking_delta` / `stream_end` | `stream_event` | 近似对应 | Rust 把原始流事件拍平成 UI 友好消息。 |
| `assistant_message` | `assistant` | 直接对应 | 都是完整 assistant 消息。 |
| `tool_use` | `assistant.message.content[].tool_use` | 近似对应 | Rust 提升为顶层消息；bun 保留在 assistant 内容块里。 |
| `tool_result` | `user/userReplay.message.content[].tool_result` | 近似对应 | Rust 提升为顶层消息；bun 保留在用户侧内容块里。 |
| `permission_request` | `control_request(subtype=can_use_tool)` | 直接对应 | 都表示工具权限请求。 |
| `system_info` | 各类 `system(...)` 消息 | 一对多 | bun 没有统一 `system_info` 信封。 |
| `conversation_replaced` | replay / internal-events hydrate | 近似对应 | bun 没有单条“整体替换会话历史”的通用消息。 |
| `usage_update` | `result` | 近似对应 | Rust 单发 usage；bun 把 usage 聚合在 `result`。 |
| `suggestions` | `prompt_suggestion` | 近似对应 | Rust 一次发数组；bun 一条 suggestion 一条消息。 |
| `error` | `result(error)` / `control_response(error)` / `system(api_retry)` | 一对多 | bun 按错误来源拆开。 |
| `background_agent_complete` | `system(task_notification)` | 近似对应 | 都是后台任务完成类通知，但 identity 维度不同。 |
| `brief_message` | 无 | Rust 独有 | Rust/Kairos 扩展。 |
| `autonomous_start` | 无 | Rust 独有 | Rust/Kairos 扩展。 |
| `notification_sent` | 无直接 peer | Rust 独有 | bun 无“通知已发送确认”消息。 |
| `lsp_event` / `mcp_event` / `plugin_event` / `skill_event` | 无统一 peer | Rust 独有 | Rust 在 IPC 层暴露完整子系统事件总线。 |
| `subsystem_status` | `control_response(mcp_status)` | 弱近似 | bun 只有 MCP 维度。 |
| `agent_event` / `team_event` | `task_*` / `post_turn_summary` | 弱近似 | bun 没有对等的公开 agent/team event 总线。 |

## 3. 字段级对照表

## 3.1 Frontend -> Backend

### 3.1.1 `submit_prompt` ↔ `SDKUserMessage`

Rust:

```json
{ "type": "submit_prompt", "text": "...", "id": "..." }
```

`claude-code-bun`:

```json
{
  "type": "user",
  "message": { "role": "user", "content": "..." },
  "parent_tool_use_id": null,
  "uuid": "...",
  "session_id": "..."
}
```

| Rust 字段 | bun 字段 | 对应关系 | 说明 |
|---|---|---|---|
| `type = submit_prompt` | `type = user` | 语义近似 | 都表示“提交用户输入”，但 bun 直接使用通用消息模型。 |
| `text` | `message.content` | 直接近似 | Rust 用平铺字符串；bun 放在嵌套 `message` 内。 |
| `id` | 无直接等价字段 | 不对应 | Rust 的 `id` 是 UI 侧请求/流式关联键；bun 没有同语义字段。 |
| 无 | `uuid` | bun 独有 | bun 的 `uuid` 是消息事件 ID，不等于 Rust `id`。 |
| 无 | `session_id` | bun 独有 | bun 明确带会话范围；Rust 这条输入消息不带 session_id。 |
| 无 | `parent_tool_use_id` | bun 独有 | bun 可把用户消息挂到某个父 tool use 下。Rust 无此输入维度。 |
| 无 | `isSynthetic` / `tool_use_result` / `priority` / `timestamp` | bun 独有 | bun 的 user message 兼容更多来源和调度场景。 |

### 3.1.2 `abort_query` ↔ `control_request(subtype=interrupt)`

Rust:

```json
{ "type": "abort_query" }
```

`claude-code-bun`:

```json
{
  "type": "control_request",
  "request_id": "...",
  "request": { "subtype": "interrupt" }
}
```

| Rust 字段 | bun 字段 | 对应关系 | 说明 |
|---|---|---|---|
| `type = abort_query` | `type = control_request` + `request.subtype = interrupt` | 直接对应 | Rust 用独立消息种类；bun 放进 control request 框架。 |
| 无 | `request_id` | bun 独有 | bun 对控制交互统一做 request/response 关联。 |

### 3.1.3 `permission_response` ↔ `control_response` for `can_use_tool`

Rust:

```json
{
  "type": "permission_response",
  "tool_use_id": "...",
  "decision": "allow|deny|always_allow"
}
```

`claude-code-bun` 成功响应的形状通常是：

```json
{
  "type": "control_response",
  "response": {
    "subtype": "success",
    "request_id": "...",
    "response": {
      "behavior": "allow|deny",
      "toolUseID": "...",
      "updatedInput": {},
      "updatedPermissions": [],
      "decisionClassification": "user_temporary|user_permanent|user_reject"
    }
  }
}
```

| Rust 字段 | bun 字段 | 对应关系 | 说明 |
|---|---|---|---|
| `type = permission_response` | `type = control_response` | 语义近似 | bun 不为权限单独建 response 类型。 |
| `tool_use_id` | `response.response.toolUseID` | 近似对应 | bun 的权限结果里可回带 `toolUseID`，但真正的控制关联主键是 `request_id`。 |
| `decision = allow` | `response.response.behavior = allow` | 直接近似 | 二者都表示允许执行。 |
| `decision = deny` | `response.response.behavior = deny` | 直接近似 | 二者都表示拒绝执行。 |
| `decision = always_allow` | `updatedPermissions` + `decisionClassification = user_permanent` | 语义拆分 | bun 没有 `always_allow` 单字面量，而是把“本次允许”和“持久化规则更新”拆开表达。 |
| 无 | `response.request_id` | bun 独有 | bun 优先按控制请求 ID 关联，不按 `tool_use_id`。 |
| 无 | `updatedInput` | bun 独有 | SDK host 可在放行时修改输入；Rust 这条协议不支持。 |
| 无 | `decisionClassification` | bun 独有 | 用于区分 allow-once / always allow / reject 的遥测含义。 |

### 3.1.4 `slash_command` ↔ `SDKUserMessage`

Rust:

```json
{ "type": "slash_command", "raw": "/help" }
```

`claude-code-bun` 没有独立的 slash command 输入消息，通常仍以 user message 进入。

| Rust 字段 | bun 字段 | 对应关系 | 说明 |
|---|---|---|---|
| `type = slash_command` | 无单独类型 | 不对应 | bun 协议层通常不区分“普通文本”与“slash 命令文本”。 |
| `raw` | `message.content` | 近似对应 | slash 命令文本通常仍放进普通 user message 内容。 |

补充：

- Rust 是“传输层就知道这是一条 slash command”。
- bun 更像“上层输入处理器再识别这是不是 slash command”，输出时常出现 `system/local_command_output`。

### 3.1.5 `resize`

Rust:

```json
{ "type": "resize", "cols": 120, "rows": 40 }
```

| Rust 字段 | bun 对应字段 | 对应关系 | 说明 |
|---|---|---|---|
| `type = resize` | 无 | 无 | bun 公共 SDK/bridge 协议没有通用 terminal resize 消息。 |
| `cols` | 无 | 无 | Rust headless UI 直接依赖终端尺寸。 |
| `rows` | 无 | 无 | 同上。 |

### 3.1.6 `quit`

Rust:

```json
{ "type": "quit" }
```

| Rust 字段 | bun 对应字段 | 对应关系 | 说明 |
|---|---|---|---|
| `type = quit` | 无 | 无 | bun 主要依赖连接关闭、进程退出、transport teardown。 |

### 3.1.7 Rust 独有命令类消息

以下 Rust 输入消息在 bun 的公开通信协议中没有对等顶层消息：

| Rust 消息 | 说明 |
|---|---|
| `lsp_command` | Rust IPC 直接暴露 LSP 生命周期控制。 |
| `mcp_command` | bun 只有分散的 `mcp_* control_request`。 |
| `plugin_command` | bun 只有 `reload_plugins` 一类公开控制请求。 |
| `skill_command` | bun 无公开 skill command。 |
| `query_subsystem_status` | bun 只有 `mcp_status`。 |
| `agent_command` | bun 无公开 agent command 总线。 |
| `team_command` | bun 无公开 team command 总线。 |

## 3.2 Backend -> Frontend

### 3.2.1 `ready` ↔ `system(init)` + `control_response(initialize)`

Rust:

```json
{ "type": "ready", "session_id": "...", "model": "...", "cwd": "..." }
```

`claude-code-bun` 对应信息分散在两处：

- `system/init`
- `control_response` to `initialize`

| Rust 字段 | bun 字段 | 对应关系 | 说明 |
|---|---|---|---|
| `type = ready` | `system.subtype = init` | 语义近似 | Rust 用一条轻量消息表明“就绪”。 |
| `session_id` | `system.session_id` | 直接对应 | 二者都显式携带会话 ID。 |
| `model` | `system.model` | 直接对应 | 二者都显式携带当前模型。 |
| `cwd` | `system.cwd` | 直接对应 | 二者都显式携带当前目录。 |
| 无 | `initialize.response.commands/models/account/agents/...` | bun 独有 | bun 初始化响应除了“ready”之外，还顺带返回能力清单。 |

### 3.2.2 `stream_start` / `stream_delta` / `thinking_delta` / `stream_end` ↔ `stream_event`

Rust 会把原始流事件拍平成 UI 友好消息：

| Rust 消息 | bun 对应 | 对应关系 | 说明 |
|---|---|---|---|
| `stream_start { message_id }` | `stream_event(event=message_start)` | 近似对应 | bun 保留原始流事件，不单独拆 `stream_start`。 |
| `stream_delta { message_id, text }` | `stream_event(event=content_block_delta, delta.type=text_delta)` | 近似对应 | Rust 把 `delta.text` 提纯出来。 |
| `thinking_delta { message_id, thinking }` | `stream_event(event=content_block_delta, delta.type=thinking_delta)` | 近似对应 | Rust 把 thinking 单独抬平。 |
| `stream_end { message_id }` | `stream_event(event=message_stop)` | 近似对应 | Rust 有显式结束消息。 |

字段级对照：

| Rust 字段 | bun 字段 | 对应关系 | 说明 |
|---|---|---|---|
| `message_id` | 嵌在 `event` 内部 | 语义近似 | bun 顶层只有 `uuid/session_id/parent_tool_use_id`，原始消息 ID 在 `RawMessageStreamEvent` 内部。 |
| `text` | `event.delta.text` | 直接近似 | 这是从 `content_block_delta/text_delta` 抽出来的字段。 |
| `thinking` | `event.delta.thinking` | 直接近似 | 同上。 |

注：

- 这里的 `event.delta.*` 是基于原始流事件形状的推断映射，不是 `SDKPartialAssistantMessageSchema` 明文展开的字段。

### 3.2.3 `assistant_message` ↔ `assistant`

Rust:

```json
{
  "type": "assistant_message",
  "id": "...",
  "content": [...],
  "cost_usd": 0.001
}
```

`claude-code-bun`:

```json
{
  "type": "assistant",
  "uuid": "...",
  "session_id": "...",
  "message": { ... }
}
```

| Rust 字段 | bun 字段 | 对应关系 | 说明 |
|---|---|---|---|
| `type = assistant_message` | `type = assistant` | 直接对应 | 都是完整 assistant 消息。 |
| `id` | `uuid` | 直接近似 | Rust 这里实际就是把 assistant 的 UUID 作为消息 ID 发给前端。 |
| `content` | `message.content` | 直接近似 | Rust 把内容提到顶层；bun 保持嵌套在 `message` 中。 |
| `cost_usd` | 无稳定顶层字段 | 不完全对应 | bun 的稳定成本字段在 `result.total_cost_usd`；assistant 消息本身不保证有顶层成本字段。 |
| 无 | `session_id` | bun 独有 | Rust 这条输出消息不重复带 session_id。 |
| 无 | `error` | bun 独有 | bun assistant 可附带 `assistant.error`。 |

### 3.2.4 `tool_use` ↔ `assistant.message.content[].tool_use`

Rust:

```json
{
  "type": "tool_use",
  "id": "...",
  "name": "Bash",
  "input": {...}
}
```

`claude-code-bun` 中没有等价顶层消息，它通常是 assistant 内容块的一部分。

| Rust 字段 | bun 字段 | 对应关系 | 说明 |
|---|---|---|---|
| `type = tool_use` | `assistant.message.content[].type = tool_use` | 语义近似 | Rust 预先从 assistant 内容中拆出来单独发送。 |
| `id` | `assistant.message.content[].id` | 直接近似 | 工具调用 ID。 |
| `name` | `assistant.message.content[].name` | 直接近似 | 工具名。 |
| `input` | `assistant.message.content[].input` | 直接近似 | 工具输入。 |

### 3.2.5 `tool_result` ↔ `user/userReplay.message.content[].tool_result`

Rust:

```json
{
  "type": "tool_result",
  "tool_use_id": "...",
  "output": "...",
  "is_error": false,
  "content_blocks": [...]
}
```

`claude-code-bun` 没有稳定的顶层 `tool_result` 消息，通常保留在 user / replay message 的内容块里。

| Rust 字段 | bun 字段 | 对应关系 | 说明 |
|---|---|---|---|
| `type = tool_result` | `user.message.content[].type = tool_result` | 推断近似 | bun 更常把 tool result 保留在内容块中。 |
| `tool_use_id` | `user.message.content[].tool_use_id` | 推断近似 | 具体字段藏在 `APIUserMessagePlaceholder()` 内。 |
| `output` | `user.message.content[].content` | 推断近似 | Rust 统一提炼成字符串输出。 |
| `is_error` | `user.message.content[].is_error` | 推断近似 | Rust 提升为顶层布尔值。 |
| `content_blocks` | 无稳定公开顶层字段 | 不完全对应 | bun 的公开 SDK schema 没有为 tool result 的非文本块单独抬平。 |

### 3.2.6 `permission_request` ↔ `control_request(subtype=can_use_tool)`

Rust:

```json
{
  "type": "permission_request",
  "tool_use_id": "...",
  "tool": "Bash",
  "command": "git status",
  "options": ["allow", "deny", "always_allow"]
}
```

`claude-code-bun`:

```json
{
  "type": "control_request",
  "request_id": "...",
  "request": {
    "subtype": "can_use_tool",
    "tool_name": "Bash",
    "input": {...},
    "permission_suggestions": [...],
    "blocked_path": "...",
    "decision_reason": "...",
    "title": "...",
    "display_name": "...",
    "tool_use_id": "..."
  }
}
```

| Rust 字段 | bun 字段 | 对应关系 | 说明 |
|---|---|---|---|
| `type = permission_request` | `type = control_request` + `request.subtype = can_use_tool` | 直接近似 | Rust 对权限单独建消息，bun 复用 control request。 |
| `tool_use_id` | `request.tool_use_id` | 直接对应 | 工具调用实例 ID。 |
| `tool` | `request.tool_name` | 直接对应 | 工具名。 |
| `command` | `request.description` | 近似对应 | bun 的 `description` 是可选字段；更稳定的信息通常在 `request.input`。 |
| `options` | `request.permission_suggestions` | 语义拆分 | Rust 给 UI 一组动作选项；bun 更像建议更新哪些 permission rules。 |
| 无 | `request.input` | bun 独有 | bun 强调“原始工具输入”而不是预格式化的 command 字符串。 |
| 无 | `request.blocked_path` | bun 独有 | 便于 UI 定位被阻止的路径。 |
| 无 | `request.decision_reason` | bun 独有 | 带上权限决策原因。 |
| 无 | `request_id` | bun 独有 | control request/response 的关联主键。 |

### 3.2.7 `system_info` ↔ 各类 `system(...)`

Rust:

```json
{ "type": "system_info", "text": "...", "level": "info|warning|error" }
```

`claude-code-bun` 没有统一 `system_info` 信封，而是按 subtype 分拆：

- `system/init`
- `system/status`
- `system/api_retry`
- `system/local_command_output`
- `system/compact_boundary`
- `system/task_notification`
- `system/task_started`
- `system/task_progress`
- `system/session_state_changed`
- `system/post_turn_summary`

字段级只能做一对多对照：

| Rust 字段 | bun 字段 | 对应关系 | 说明 |
|---|---|---|---|
| `text` | `content` / `summary` / `description` / `status_detail` / `error.*` | 一对多 | bun 不把所有系统文本塞进同一个字段。 |
| `level` | 无统一字段 | 不对应 | bun 的“严重级别”主要由 subtype 和错误分支表达，而不是统一 `level`。 |

### 3.2.8 `conversation_replaced`

Rust:

```json
{ "type": "conversation_replaced", "messages": [...] }
```

| Rust 字段 | bun 对应字段 | 对应关系 | 说明 |
|---|---|---|---|
| `type = conversation_replaced` | 无 | 无 | bun 没有“整段会话直接替换”的公开协议消息。 |
| `messages[]` | replay / internal-events hydrate | 语义近似 | bun 更常用逐条 replay 或 internal event 恢复。 |

### 3.2.9 `usage_update` ↔ `result`

Rust:

```json
{
  "type": "usage_update",
  "input_tokens": 123,
  "output_tokens": 45,
  "cost_usd": 0.001
}
```

`claude-code-bun`:

```json
{
  "type": "result",
  "usage": {
    "input_tokens": 123,
    "output_tokens": 45,
    "cache_creation_input_tokens": 0,
    "cache_read_input_tokens": 0
  },
  "total_cost_usd": 0.001
}
```

| Rust 字段 | bun 字段 | 对应关系 | 说明 |
|---|---|---|---|
| `type = usage_update` | `type = result` | 语义近似 | Rust 单发 usage；bun 把 usage 放在 turn 结果里。 |
| `input_tokens` | `usage.input_tokens` | 直接对应 | bun 还会额外区分 cache token。 |
| `output_tokens` | `usage.output_tokens` | 直接对应 | 一致。 |
| `cost_usd` | `total_cost_usd` | 直接近似 | bun 成本字段挂在 result 顶层。 |
| 无 | `usage.cache_creation_input_tokens` | bun 独有 | Rust `UsageUpdate` 没细分 cache token。 |
| 无 | `usage.cache_read_input_tokens` | bun 独有 | 同上。 |

### 3.2.10 `suggestions` ↔ `prompt_suggestion`

Rust:

```json
{ "type": "suggestions", "items": ["...", "..."] }
```

`claude-code-bun`:

```json
{ "type": "prompt_suggestion", "suggestion": "...", "uuid": "...", "session_id": "..." }
```

| Rust 字段 | bun 字段 | 对应关系 | 说明 |
|---|---|---|---|
| `type = suggestions` | `type = prompt_suggestion` | 语义近似 | Rust 批量发送；bun 单条发送。 |
| `items[]` | 多条 `suggestion` | 一对多 | Rust 一条消息里装数组，bun 一条消息只装一条 suggestion。 |

### 3.2.11 `error` ↔ `result(error)` / `control_response(error)` / `system(api_retry)`

Rust:

```json
{ "type": "error", "message": "...", "recoverable": true }
```

字段级对照只能做一对多：

| Rust 字段 | bun 字段 | 对应关系 | 说明 |
|---|---|---|---|
| `message` | `control_response.response.error` / `result.errors[]` / `system(api_retry).error` | 一对多 | bun 按错误来源拆分。 |
| `recoverable` | 无统一字段 | 不对应 | bun 由 subtype 语义表达“是否可恢复”，没有统一布尔值。 |

### 3.2.12 `background_agent_complete` ↔ `system(task_notification)`

Rust:

```json
{
  "type": "background_agent_complete",
  "agent_id": "...",
  "description": "...",
  "result_preview": "...",
  "had_error": false,
  "duration_ms": 1234
}
```

`claude-code-bun`:

```json
{
  "type": "system",
  "subtype": "task_notification",
  "task_id": "...",
  "status": "completed|failed|stopped",
  "summary": "...",
  "output_file": "...",
  "usage": { "duration_ms": 1234, ... }
}
```

| Rust 字段 | bun 字段 | 对应关系 | 说明 |
|---|---|---|---|
| `type = background_agent_complete` | `system.subtype = task_notification` | 语义近似 | 都是后台任务结束通知。 |
| `agent_id` | `task_id` | 弱近似 | 二者都标识后台执行单元，但命名语义不同。 |
| `description` | 无稳定对应，接近 `summary` | 弱近似 | Rust 保留启动时描述；bun 更偏总结。 |
| `result_preview` | `summary` | 近似对应 | 都是预览/摘要文本。 |
| `had_error` | `status = failed` | 直接近似 | bun 用枚举而非布尔值。 |
| `duration_ms` | `usage.duration_ms` | 直接近似 | bun 的 duration 挂在 usage 下且可选。 |
| 无 | `output_file` | bun 独有 | bun 会给出输出文件位置。 |

## 4. Rust 独有消息扩展

这些消息在 Rust IPC 中是第一类协议项，但 `claude-code-bun` 的公开 SDK/bridge 协议没有对等 peer：

| Rust 消息 | 说明 |
|---|---|
| `brief_message` | Rust/Kairos 的 BriefTool 协议输出。 |
| `autonomous_start` | Rust/Kairos 的主动 tick 起始消息。 |
| `notification_sent` | 通知发送确认。 |
| `lsp_event` | LSP 子系统事件流。 |
| `mcp_event` | MCP 子系统事件流。 |
| `plugin_event` | 插件子系统事件流。 |
| `skill_event` | 技能子系统事件流。 |
| `subsystem_status` | 统一子系统状态快照。 |
| `agent_event` | agent 生命周期与流式事件。 |
| `team_event` | team 生命周期与事件。 |

这说明 Rust 的 `src/ipc` 已经不只是“UI 收发层”，而是“本地前端 + 子系统控制总线”。

## 5. `claude-code-bun` 独有消息扩展

这些消息在 `claude-code-bun` 的公开通信协议里是第一类协议项，但 Rust `src/ipc/protocol.rs` 没有对等项：

| bun 消息 | 说明 |
|---|---|
| `keep_alive` | 维持 WS/SSE/bridge 会话存活。 |
| `update_environment_variables` | 运行时刷新环境变量，常用于 token 更新。 |
| `system/status` | 明确同步 session state。 |
| `system/post_turn_summary` | 后台 turn 摘要。 |
| `system/api_retry` | API 重试事件。 |
| `system/local_command_output` | 本地 slash command 输出。 |
| `system/task_started` / `system/task_progress` / `system/task_notification` | 完整任务生命周期。 |
| `system/session_state_changed` | `idle/running/requires_action` 状态转移。 |
| `rate_limit_event` | 速率限制状态变化。 |
| `auth_status` | 认证状态流。 |
| `files_persisted` | 文件持久化事件。 |
| `tool_progress` | 长时工具执行中的进度事件。 |
| `hook_started` / `hook_progress` / `hook_response` | Hook 生命周期事件。 |
| `prompt_suggestion` | 单条 prompt suggestion。 |
| `control_cancel_request` | 取消一个未决 control request。 |

这说明 `claude-code-bun` 的协议更偏“远端 worker / SDK host / bridge 生命周期协议”，不是单纯 UI 协议。

## 6. 最终归纳

如果把两边压缩成一句话：

- Rust `src/ipc`：把 `QueryEngine` 输出拍平成一个本地 UI 友好的 JSONL 协议。
- `claude-code-bun` 通信层：把 CLI、SDK host、bridge、CCR worker 之间的控制与会话生命周期编码成一个可重连、可恢复、可扩展的控制协议。

因此：

1. Rust 的 `submit_prompt / tool_use / tool_result / usage_update / system_info` 更像“渲染层协议”。
2. bun 的 `user / assistant / stream_event / result / control_request / control_response` 更像“传输层协议”。
3. 两边最稳定的一对一映射只有几类：
   - `abort_query` ↔ `interrupt`
   - `permission_request` ↔ `can_use_tool`
   - `assistant_message` ↔ `assistant`
   - `usage_update` ↔ `result`
4. 其余很多消息都只能做“语义近似映射”，不能机械地按字段名对齐。
