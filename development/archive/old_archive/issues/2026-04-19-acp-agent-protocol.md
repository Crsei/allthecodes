# Issue: 为 `cc-rust` 实现 ACP (Agent Client Protocol) Agent 入口

Labels: `enhancement`, `protocol`, `ide-integration`
Priority: `P1`

## 背景

参考实现已经存在于 `F:\AIclassmanager\cc\claude-code-bun\docs\features\acp-zed.md` 与对应源码：

- `F:\AIclassmanager\cc\claude-code-bun\src\services\acp\entry.ts`
- `F:\AIclassmanager\cc\claude-code-bun\src\services\acp\agent.ts`
- `F:\AIclassmanager\cc\claude-code-bun\src\services\acp\bridge.ts`
- `F:\AIclassmanager\cc\claude-code-bun\src\services\acp\permissions.ts`
- `F:\AIclassmanager\cc\claude-code-bun\src\services\acp\utils.ts`

当前 `cc-rust` 已有两条相关链路，但都不是 ACP：

- `--headless`: 面向 OpenTUI 前端的自定义 JSONL IPC，位于 `src/ipc/*`
- `--web`: 面向浏览器前端的 HTTP/SSE API，位于 `src/web/*`

这意味着当前项目虽然已经有 QueryEngine、session 持久化、权限回调、slash commands 等 ACP 所需底座，但还不能被 Zed/Cursor 这类 ACP 客户端直接拉起。

## `claude-code-bun` 参考实现拆解

`claude-code-bun` 的 ACP 不是独立重写了一套 agent runtime，而是在现有内部运行时之上包了一层 ACP 适配层。

核心判断：

- 入口是 ACP stdio/NDJSON transport
- 会话执行仍然使用内部 `QueryEngine`
- ACP 只是把内部 query/message/tool/permission 生命周期翻译成标准 ACP 方法与 `SessionUpdate`

### 1. CLI 接线方式

真正的 CLI fast-path 不在 `main.tsx` 主流程里，而是在：

- `F:\AIclassmanager\cc\claude-code-bun\src\entrypoints\cli.tsx`

关键实现：

- 命中 `process.argv[2] === '--acp'`
- 动态导入 `../services/acp/entry.js`
- 直接调用 `runAcpAgent()`
- 不走常规 REPL/TUI 启动路径

这说明 ACP 在参考实现里是“并行入口”，而不是对默认交互模式的改造。

对 `cc-rust` 的含义：

- 最合理的落点也是单独的 CLI 分支
- 不应把现有 `--headless` 消息协议硬改成 ACP

### 2. ACP 入口层做什么

参考实现入口文件：

- `F:\AIclassmanager\cc\claude-code-bun\src\services\acp\entry.ts`

这个入口层只做 transport 和进程生命周期管理，不做业务逻辑：

- `enableConfigs()` 启用配置系统
- `applySafeConfigEnvironmentVariables()` 把 settings 中的认证和模型相关 env 注入当前进程
- 用 `@agentclientprotocol/sdk` 的 `ndJsonStream()` + `AgentSideConnection()` 建立 ACP 连接
- 把 `console.log/info/warn/debug` 全部重定向到 `stderr`
- 在连接关闭或收到信号时关闭所有活动 session

其中最重要的约束是：

- `stdout` 只允许输出 ACP 协议帧
- 日志必须去 `stderr`

这对 `cc-rust` 很关键，因为现有 tracing/logging 必须避开 ACP stdout。

### 3. Agent 层不是协议壳，而是 Session 管理器

参考实现主文件：

- `F:\AIclassmanager\cc\claude-code-bun\src\services\acp\agent.ts`

`AcpAgent` 同时承担两层职责：

- 实现 ACP `Agent` 接口
- 维护内存中的 session registry

其核心状态是：

- `sessions = new Map<string, AcpSession>()`

每个 `AcpSession` 里保存：

- `queryEngine`
- `cancelled`
- `cwd`
- `sessionFingerprint`
- `modes`
- `models`
- `configOptions`
- `promptRunning`
- `pendingMessages`
- `toolUseCache`
- `clientCapabilities`
- `appState`
- `commands`

这里最值得抄的几个设计：

- 会话不是轻量 ID 映射，而是完整 runtime 容器
- `toolUseCache` 不挂在全局，挂在 session 级别
- `commands` 在 session 创建时就加载，后续直接发 `available_commands_update`
- `appState` 是 session 私有的可变状态，而不是全局共享

### 4. Session 创建的真实流程

参考实现里，`newSession()` 最终走到 `createSession()`。

`createSession()` 做了这些事：

- 设置 `cwd`
- `process.chdir(cwd)` 尽量切过去
- 构造工具集 `getTools(permissionContext)`
- 构造 ACP 权限桥接函数 `createAcpCanUseTool(...)`
- 加载 slash commands / skills：`getCommands(cwd)`
- 构造 `AppState`
- 组装 `QueryEngineConfig`
- 实例化 `QueryEngine`
- 构造 ACP 可见的 `modes / models / configOptions`
- 把 session 放入 `sessions` map
- 异步下发 `available_commands_update`

结论：

- ACP 在参考实现里并没有单独的执行引擎
- 它直接复用现有 QueryEngine，只是换了 transport 和状态容器

对 `cc-rust` 的含义：

- 不需要为 ACP 再造一套 query loop
- 需要的是 `QueryEngine -> ACP agent session` 的装配层

### 5. Prompt 是如何跑起来的

参考实现里 `prompt()` 的关键路径是：

1. 找到 `sessionId` 对应的 `AcpSession`
2. 把 ACP `ContentBlock[]` 转成内部文本输入
3. 如果当前已有 prompt 在跑，则进入队列
4. 重置 `QueryEngine` 的 abort controller
5. 调用 `queryEngine.submitMessage(promptInput)`
6. 把返回的 `SDKMessage` async generator 交给 `forwardSessionUpdates()`
7. 根据最终 stop reason / usage 回填 ACP `PromptResponse`

这里的两个关键设计值得保留：

- prompt queueing：ACP 客户端连续发 prompt 时不会把 session 打坏
- cancel 与 queue 共用一套 session 状态机

### 6. 历史恢复不是只读文件，而是 replay

`resumeSession()` / `loadSession()` 最终会走到 `getOrCreateSession()`。

这个函数的关键逻辑：

- 如果该 `sessionId` 已经在内存里，先比较 `sessionFingerprint`
- 如果 fingerprint 没变，直接复用现有 session
- 如果 fingerprint 变了，先 teardown 再重建
- 如果 session 只存在于磁盘，加载 log/messages
- 调用 `createSession(..., { sessionId, initialMessages })`
- 然后调用 `replayHistoryMessages()` 把历史消息重新发给 ACP client

这点非常重要：

- 恢复会话不只是“让 engine 有历史”
- 还必须把历史重新 replay 成 ACP `SessionUpdate`，否则 IDE UI 看不到上下文

对 `cc-rust` 的含义：

- `session::resume::resume_session()` 只解决“读出历史”
- 还需要单独的 ACP replay 层解决“把历史重新渲染给客户端”

### 7. Bridge 是 ACP 成败的核心

参考实现最关键的文件其实是：

- `F:\AIclassmanager\cc\claude-code-bun\src\services\acp\bridge.ts`

它负责把内部 `SDKMessage` 翻译成 ACP `SessionUpdate`。

主要职责包括：

- `toolInfoFromToolUse()`
  - 将 `Read / Edit / Write / Bash / Glob / Grep / WebSearch / WebFetch / TodoWrite / ExitPlanMode` 映射到 ACP `ToolKind`
  - 补 title、location、diff、terminal 等展示信息
- `toolUpdateFromToolResult()`
  - 将工具结果映射为 `tool_call_update`
  - 特别处理 `Bash` 输出、`Read` 文本、错误块、terminal metadata
- `forwardSessionUpdates()`
  - 消费 `QueryEngine.submitMessage()` 的 async generator
  - 将流式事件、assistant message、tool use/result、result、usage 逐条转成 ACP 更新
- `replayHistoryMessages()`
  - 将历史消息重新发为 ACP 更新

桥接层里几个特别重要的细节：

- `toolUseCache`
  - 用于把 `tool_result` 匹配回之前的 `tool_use`
- `TodoWrite -> plan`
  - 不是普通 tool update，而是翻译成 ACP `plan`
- `Bash`
  - 客户端支持 terminal output 时，下发 `terminal` 内容与 `_meta.terminal_output`
  - 不支持时退化为普通文本块
- `usage_update`
  - 参考实现会跟踪最近一次顶层 assistant usage 和 model，再用 `modelUsage` 做 context window 匹配

对 `cc-rust` 的直接建议：

- 优先复用已有 `src/ipc/sdk_mapper.rs` 对 `SdkMessage` 的理解
- 但不要直接复用协议结构
- 应新建 `acp/bridge.rs`，专门输出 ACP 事件

### 8. 权限桥接不是简单 yes/no

参考实现权限桥接文件：

- `F:\AIclassmanager\cc\claude-code-bun\src\services\acp\permissions.ts`

`createAcpCanUseTool()` 的工作方式是：

- 接管内部 `canUseTool`
- 当工具需要询问权限时，向 ACP client 发 `requestPermission()`
- 根据客户端选择返回内部 allow/deny decision

普通工具选项是：

- `Always Allow`
- `Allow`
- `Reject`

但 `ExitPlanMode` 是特例，它会给出 mode 级选项：

- `auto`
- `acceptEdits`
- `default`
- `plan`
- 在允许时还可能暴露 `bypassPermissions`

而且一旦客户端选中了某个 mode，它不仅返回 allow，还会主动发：

- `current_mode_update`

这意味着在参考实现里：

- 权限桥接和 mode 切换是联动的
- `ExitPlanMode` 不是普通工具权限弹窗

### 9. 工具模式和内部权限模式需要映射层

参考实现 `utils.ts` 中有 `resolvePermissionMode()`，它支持：

- `auto`
- `default`
- `acceptEdits`
- `dontAsk`
- `plan`
- `bypassPermissions`

注意这里的 ACP / CCB 命名并不完全等同于所有内部实现命名。

对 `cc-rust` 的含义：

- 需要显式的 mode 映射层
- 不能让 ACP mode 字符串直接渗透到内部每一层
- 建议在 `acp/permissions.rs` 或 `acp/session.rs` 集中处理

### 10. 命令列表是真实命令，不是假数据

参考实现会在 session 创建和恢复后发：

- `available_commands_update`

数据源是 `getCommands(cwd)` 的真实结果，再过滤出：

- `cmd.type === 'prompt'`
- `!cmd.isHidden`
- `cmd.userInvocable !== false`

如果命令定义里有 `argumentHint`，还会映射成 ACP 里的：

- `input: { hint: ... }`

对 `cc-rust` 的含义：

- 命令暴露层最好直接从 `src/commands/mod.rs` 的真实注册表生成
- 如果缺少 `hidden/user_invocable/argument_hint` 元数据，需要在 Rust 侧补齐

### 11. 参考实现已经有较完整测试面

测试文件在：

- `F:\AIclassmanager\cc\claude-code-bun\src\services\acp\__tests__\agent.test.ts`
- `F:\AIclassmanager\cc\claude-code-bun\src\services\acp\__tests__\bridge.test.ts`
- `F:\AIclassmanager\cc\claude-code-bun\src\services\acp\__tests__\permissions.test.ts`

覆盖面包括：

- ACP initialize/newSession/prompt/cancel/load/resume/fork/close
- prompt queueing
- model/mode/config option 更新
- command exposure
- `toolInfoFromToolUse()` 映射
- `toolUpdateFromToolResult()` 映射
- `forwardSessionUpdates()` 行为
- permission request round-trip

这说明实现时不该只做 happy path：

- 桥接层和权限层必须单独可测
- session replay 和 queueing 也必须有测试

## 对 `cc-rust` 的直接落地结论

参考 `claude-code-bun` 的真实实现后，`cc-rust` 最合理的实现路径是：

1. 新增 `--acp` fast-path
   直接并行于 `--headless` / `--web`

2. 新增 `src/acp/entry.rs`
   只负责 stdio transport、日志重定向、session 生命周期

3. 新增 `src/acp/agent.rs`
   负责 ACP `Agent` 方法与 session registry

4. 新增 `src/acp/bridge.rs`
   负责 `SdkMessage -> ACP SessionUpdate`

5. 新增 `src/acp/permissions.rs`
   负责 `PermissionCallback` / `CanUseTool` 与 ACP permission 的桥接

6. 复用现有 `QueryEngine`
   不重写 query loop

7. 复用现有 session 存储
   但新增 ACP replay 层

8. 复用现有 commands
   但需要补 ACP 可见性元数据

## 目标

为 `cc-rust` 增加一个独立的 `--acp` 入口，使其可以作为 ACP agent 被 IDE 通过 `stdin/stdout` 驱动。

交付目标：

- 支持 `claude --acp` 启动 ACP agent
- 支持 Zed / Cursor 等 ACP 客户端建立新会话并发送 prompt
- 支持会话恢复、会话列表、会话关闭、prompt cancel
- 支持工具权限桥接
- 支持 slash commands / skills 列表下发
- 支持运行时切换 mode / model

## 非目标

- 不替换现有 `--headless` 协议
- 不重写 `src/ipc/*` 为 ACP
- 不新增 ACP 专用 UI
- 不顺带实现 Chrome/browser 集成
- 默认不引入新的 ACP Rust 依赖；优先使用本地 `serde` 类型建模

## 为什么不能直接复用 `src/ipc/*`

`src/ipc/*` 是当前 Rust 项目自定义的前后端协议，目标是 OpenTUI/Web 自家前端，消息模型是：

- `FrontendMessage` / `BackendMessage`
- `QuestionRequest`
- `ConversationReplaced`
- `StatusLineUpdate`
- subsystem / agent / team 扩展事件

ACP 的目标是面向标准 IDE agent client，协议面是另一套：

- agent/session 方法集
- `SessionUpdate` 增量事件
- `requestPermission()`
- mode/model/config option 管理

结论：ACP 应该作为与 `--headless` 并行的入口存在，而不是把现有 `src/ipc/protocol/*` 强行改造成 ACP。

## 现有可复用能力

当前仓库已经具备 ACP 所需的大部分底座：

- QueryEngine 生命周期与流式响应：`src/engine/lifecycle/mod.rs`
- 当前 headless 的 `SdkMessage -> UI 消息` 映射：`src/ipc/sdk_mapper.rs`
- 权限回调桥接：`src/ipc/callbacks.rs`
- 权限模式定义：`src/types/tool.rs`
- session 持久化 / 列表 / 恢复：`src/session/storage.rs`, `src/session/resume.rs`
- slash commands 注册表：`src/commands/mod.rs`
- mode/model/session 的 web 侧读写样例：`src/web/handlers.rs`

也就是说，这个功能不是“从零开始做 ACP”，而是“在现有 runtime 上新开一个 ACP transport + agent surface”。

## 建议架构

新增独立模块：

```text
src/acp/
  mod.rs
  entry.rs
  protocol.rs
  agent.rs
  bridge.rs
  permissions.rs
  session.rs
```

### 1. CLI 与入口

- 在 `src/cli.rs` 增加 `--acp`
- 在 `src/main.rs` 中于 `--headless` / `--web` 旁新增 ACP 分支
- `--acp` 模式下：
  - `stdout` 只输出 ACP 协议帧
  - tracing / 日志重定向到 `stderr`
  - 不启动 TUI / Web UI

### 2. Session Registry

在 `src/acp/agent.rs` 维护一个 `HashMap<session_id, AcpSession>`：

- `engine: Arc<QueryEngine>`
- `cwd: String`
- `session_fingerprint: String`
- `prompt_running: bool`
- `pending_prompts: queue`
- `tool_use_cache`
- `modes / models / config_options`

`session_fingerprint` 建议由以下信息组成：

- `cwd`
- MCP server 配置摘要
- 未来如有必要，再加入 sandbox / permissions 相关配置

这样 `loadSession` / `resumeSession` 可以在会话定义参数变化时安全重建底层 engine。

### 3. ACP Bridge

在 `src/acp/bridge.rs` 实现：

- `SdkMessage -> ACP SessionUpdate`
- 历史消息回放
- ToolUse / ToolResult 缓存与匹配
- usage/context window 映射

这里应优先复用 `src/ipc/sdk_mapper.rs` 的既有理解，而不是再发明一套事件解释逻辑。

### 4. Permission Bridge

在 `src/acp/permissions.rs` 中，把现有 `PermissionCallback` 适配到 ACP 的权限请求：

- 普通工具：
  - `Allow`
  - `Reject`
  - `Always Allow`
- `ExitPlanMode` 特殊处理：
  - `auto`
  - `acceptEdits`
  - `default`
  - `plan`
  - 视内部策略决定是否暴露 `bypassPermissions`

Rust 内部 mode 与 ACP mode 需要映射：

- ACP `default` -> Rust `default`
- ACP `auto` -> Rust `auto`
- ACP `acceptEdits` -> Rust `acceptEdits`
- ACP `dontAsk` -> Rust `dontAsk`
- ACP `plan` -> Rust `plan`
- ACP `bypassPermissions` -> Rust `bypass`

### 5. Session 与历史

ACP 会话能力直接复用现有 session 存储：

- `listSessions` -> `src/session/storage.rs::list_sessions`
- `resumeSession` / `loadSession` -> `src/session/resume.rs::resume_session`
- 历史回放 -> 将持久化消息转换为 ACP `user_message_chunk` / `agent_message_chunk` / tool updates

注意：

- 需要过滤 meta/synthetic message
- 需要在 replay 阶段保留工具调用顺序，否则 IDE 面板会丢失 tool timeline

### 6. Slash Commands / Skills 暴露

ACP 里应向客户端下发可用命令列表，数据源来自 `src/commands/mod.rs`。

建议先只暴露“用户可直接调用”的命令：

- `name`
- `description`
- `argument hint`（如果未来补）

如果当前 Rust 命令系统缺少“是否适合 ACP 暴露”的元数据，先全部暴露公开命令，后续再补 `user_invocable` / `hidden` 字段。

## 协议支持范围

### 第一阶段必须支持

| ACP 方法 / 事件 | 要求 |
|---|---|
| `initialize` | 返回 agent info / capabilities |
| `newSession` | 创建新会话 |
| `prompt` | 发送 prompt 并流式推送更新 |
| `cancel` | 取消当前 prompt |
| `agent_message_chunk` | 文本增量 |
| `agent_thought_chunk` | thinking 增量 |
| `tool_call` | 工具开始 |
| `tool_call_update` | 工具结束/失败/输出 |
| `usage_update` | token/cost 更新 |
| `available_commands_update` | slash commands / skills 列表 |

### 第二阶段必须支持

| ACP 方法 / 事件 | 要求 |
|---|---|
| `loadSession` | 加载指定 session |
| `resumeSession` | 恢复已有 session |
| `listSessions` | 列表返回 title/cwd/更新时间 |
| `closeSession` | 释放内存中的 session |
| `setSessionMode` | mode 切换 |
| `setSessionModel` | model 切换 |
| `setSessionConfigOption` | 动态配置写回 |
| `current_mode_update` | mode 变化通知 |
| `config_option_update` | config 变化通知 |
| `user_message_chunk` | 历史回放时回放用户消息 |

### 第三阶段建议支持

| ACP 方法 / 事件 | 要求 |
|---|---|
| `forkSession` | 基于现有上下文分叉 |
| `plan` | 将 Todo/Plan 更新映射到 ACP plan entries |
| prompt queueing | 连续 prompt 自动排队 |
| diff 型 tool output | Edit/Write 显示更细粒度 diff |
| `authenticate` | no-op 即可，但保持协议兼容 |

## 实施步骤

- [ ] 在 `src/cli.rs` 增加 `--acp`
- [ ] 在 `src/main.rs` 接入 `acp::entry::run_acp()`
- [ ] 新建 `src/acp/` 模块骨架
- [ ] 实现 ACP transport 与请求分发
- [ ] 实现 `AcpSession` 注册表
- [ ] 实现 `prompt` / `cancel`
- [ ] 实现 `SdkMessage -> SessionUpdate` bridge
- [ ] 实现 permission bridge
- [ ] 接通 `list/load/resume/close` session 能力
- [ ] 接通 mode/model/config option
- [ ] 接通 slash commands/skills 更新
- [ ] 为 ACP 增加单元测试与集成测试
- [ ] 补充用户文档（Zed/Cursor 配置示例）

## 验收标准

- `claude --acp` 可启动，且 `stdout` 不混入 tracing 日志
- Zed 可以通过：

```json
{
  "agent_servers": {
    "cc-rust": {
      "type": "custom",
      "command": "claude",
      "args": ["--acp"]
    }
  }
}
```

成功建立对话

- 新会话、prompt、cancel 工作正常
- 工具权限弹窗可往返
- 会话列表/恢复与 `.cc-rust/sessions/` 数据一致
- 命令列表可见，且至少能看到 `/help`、`/model`、`/session` 等公开命令
- mode/model 切换真正作用到底层 `QueryEngine`
- `--headless` 和 `--web` 行为不回归

## 测试建议

新增测试至少覆盖：

- ACP request/response serde
- `SdkMessage -> SessionUpdate` 映射
- tool call / tool result 配对
- 历史消息 replay
- permission request round-trip
- `claude --acp` 子进程级集成测试

建议新增文件：

```text
tests/acp_protocol.rs
tests/acp_integration.rs
src/acp/tests/*.rs
```

如果测试框架更适合复用现有 `tests/message_suite/`，也可以为 ACP 增加一套最小 NDJSON harness。

## 风险与注意点

- 当前仓库没有现成 Rust ACP SDK，第一版最好自己定义最小协议面，避免引入新依赖
- 现有 `src/ipc/sdk_mapper.rs` 针对自定义前端协议，抽取复用时要避免把 ACP 和 headless 语义搅在一起
- slash command 当前没有“ACP 可见性”元数据，可能需要补字段
- ACP mode 名称与内部 `PermissionMode` 名称不完全一致，需要明确映射层
- session replay 时如果只回放纯文本，会丢 IDE 里的 tool timeline，需要保留结构化 block
- `stdout` 被 ACP 占用后，所有日志都必须走 `stderr`

## 参考文件

参考实现：

- `F:\AIclassmanager\cc\claude-code-bun\docs\features\acp-zed.md`
- `F:\AIclassmanager\cc\claude-code-bun\src\services\acp\entry.ts`
- `F:\AIclassmanager\cc\claude-code-bun\src\services\acp\agent.ts`
- `F:\AIclassmanager\cc\claude-code-bun\src\services\acp\bridge.ts`
- `F:\AIclassmanager\cc\claude-code-bun\src\services\acp\permissions.ts`
- `F:\AIclassmanager\cc\claude-code-bun\src\services\acp\utils.ts`

当前仓库落点：

- `src/cli.rs`
- `src/main.rs`
- `src/ipc/sdk_mapper.rs`
- `src/ipc/callbacks.rs`
- `src/engine/lifecycle/mod.rs`
- `src/types/tool.rs`
- `src/session/storage.rs`
- `src/session/resume.rs`
- `src/commands/mod.rs`
- `src/web/handlers.rs`
