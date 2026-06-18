# 可追溯日志体系补齐方案

> 状态：Draft
> 日期：2026-04-16
> 适用范围：`src/main.rs`、`src/engine/`、`src/query/`、`src/tools/`、`src/ipc/`、`src/daemon/`、`src/session/`
> 约束：不新增第三方依赖；保持 `~/.cc-rust/` 路径隔离；优先小步可回退改造

## 1. 需求摘要

当前仓库已经有日志、转录、会话持久化和审计导出，但它们不是一套统一的可追溯体系。目标不是“再多打一层日志”，而是把现有记录面收束成一个可以稳定复盘单次请求的闭环。

本方案要求最终满足以下能力：

- 任意一次用户提交都能按 `session_id + submit_id` 还原完整执行链路。
- 任意一次工具调用都能关联到对应 assistant message、tool_use、权限决策和结果。
- 任意一次异常都能定位到发生阶段、关联上下文和最终结果。
- 运行时记录必须是 append-only，进程异常退出后仍保留可读取前缀。
- 审计导出必须建立在运行时真实记录之上，而不是只依赖会话快照“事后拼装”。
- 现有 `transcript`、`session`、`audit_export`、`daemon`、`headless IPC` 继续保留，但角色要明确，不再互相越位。

## 2. 当前现状与证据

### 2.1 已有记录面

1. 进程级 `tracing` 文件日志已存在。
   入口在 `src/main.rs:230-272`，当前采用两层 `tracing_subscriber::fmt::layer()`，stderr 一层，文件一层；文件日志按日滚动到 `~/.cc-rust/logs/cc-rust.log.YYYY-MM-DD`。

2. 会话级 transcript 已存在。
   `src/session/transcript.rs:59-90` 提供 NDJSON append-only 转录，`src/engine/lifecycle/submit_message.rs:136`、`src/engine/lifecycle/submit_message.rs:332`、`src/engine/lifecycle/submit_message.rs:378`、`src/engine/lifecycle/submit_message.rs:390` 在 turn 处理中持续写入。

3. 会话快照已存在。
   `src/session/storage.rs:133-176` 负责完整 session JSON 持久化，`src/engine/lifecycle/submit_message.rs:337-343` 在 assistant turn 后自动保存。

4. 审计导出已存在。
   `src/session/audit_export.rs:121-135` 可以导出 `.audit.json`，`src/session/audit_export.rs:158-245` 可以校验 hash chain。

5. daemon 有自己的 Markdown 日志。
   `src/daemon/memory_log.rs:28-62` 追加到 `~/.cc-rust/logs/YYYY/MM/YYYY-MM-DD.md`；`src/daemon/routes.rs:161-162` 和 `src/daemon/tick.rs:58` 会写入 submit / tick 事件。

6. IPC 和 SSE 已有事件链路。
   `src/ipc/protocol.rs:115-159` 定义了 `Ready`、`ToolUse`、`ToolResult`、`PermissionRequest` 等协议消息；`src/daemon/state.rs:71-138` 提供 SSE ring buffer 和断线补发。

### 2.2 当前缺口

1. 缺少统一关联键。
   `submit_message()` 虽然记录了 `session` 字段，见 `src/engine/lifecycle/submit_message.rs:45-49`，但全链路并没有统一传播 `submit_id / turn_id / request_id / event_id`。工具执行日志目前主要只带 `tool` 或 `duration_ms`，见 `src/tools/execution/pipeline.rs:207-292`。

2. `tracing` 不是结构化运行审计日志。
   `src/main.rs:257-268` 使用的是文本 `fmt` layer，不是稳定 JSON 事件流；不同模块字段不统一，难以离线聚合。

3. transcript 不是“完整转录”。
   `src/session/transcript.rs:145-153` 对 assistant 只存 `content_summary` 和 `stop_reason`，对 progress 只存 `tool_use_id`，不能据此重建完整工具输入输出。

4. session 快照恢复会丢 assistant usage。
   保存时 assistant 只写 `content / stop_reason / cost_usd`，见 `src/session/storage.rs:283-289`；恢复时显式写成 `usage: None`，见 `src/session/storage.rs:377-385`。这会让基于 saved session 的后续统计和审计不完整。

5. 审计导出是“结果物”，不是运行时事实源。
   `src/session/audit_export.rs:268-275` 试图从 session data 读取 `usage`，但当前 session 快照并不稳定保存 usage。说明 audit export 现在更像离线拼装，而不是基于真实事件流生成。

6. daemon 日志覆盖不足。
   当前 `append_log_entry()` 只落少量字符串事件，不能重建 tool、permission、stream、retry、abort、stop reason 等关键过程。

7. 有些事件只走 UI，不落 durable log。
   `src/daemon/routes.rs:128` 明确把 `SdkMessage::ToolUseSummary` 从 SSE 广播中丢弃。即便 UI 还原得到，也没有 durable runtime audit lane。

8. 权限拒绝和 progress 结构存在，但未形成闭环。
   `src/engine/lifecycle/types.rs:41-45` 定义了 `PermissionDenial`，`src/engine/lifecycle/mod.rs:258-259` 允许记录，但当前主流程未形成稳定、可查询、可导出的权限审计链。
   `src/types/tool.rs:82-85` 定义了 `ToolProgress`，但仓库内没有实际生产者。

## 3. 决策

### 3.1 总体决策

采用“单一运行时事实源 + 多个派生视图”的方案：

- `cc-rust.log.*` 保留为操作员调试日志。
- 新增一条 append-only 的结构化运行审计事件流，作为事实源。
- `transcript` 保留为会话可读转录，但降级为派生物，不再承担完整审计职责。
- `session` 保留为 resume 快照。
- `audit_export` 改为优先基于运行时事实源生成和校验。

### 3.2 备选方案

#### 方案 A：继续增强现有 `tracing` 文本日志

优点：

- 改动小。
- 不需要新增独立 writer。

缺点：

- 依然是调试日志思路，不适合作为稳定审计来源。
- 字段规范很难统一，模块间漂移大。
- 不适合做 hash chain、事件回放和离线验证。

#### 方案 B：新增结构化 append-only 事件流，并保留现有日志面

优点：

- 运行时事实源清晰。
- 方便按 `session_id / submit_id / tool_use_id` 查询和导出。
- 可以让 transcript、audit export、dashboard 都围绕同一事实源收敛。

缺点：

- 需要一轮上下文传播和 writer 接入。
- 需要控制事件量，避免每个 token 都落盘。

### 3.3 选型结论

采用方案 B。

原因：

- 当前仓库已经不是“完全没有记录”，问题在于记录面分裂。
- 新增事实源后，现有能力都可以保留并逐步归一，不需要推倒重来。
- 该方案最符合“可追溯”而不是“多打日志”的目标。

## 4. 目标架构

## 4.1 角色分层

### A. Operator Log

文件：

- `~/.cc-rust/logs/cc-rust.log.YYYY-MM-DD`

用途：

- 面向开发者和排障。
- 保留文本可读性。
- 记录模块内部调试信息、warning、panic 上下文、慢操作。

不承担：

- 完整审计。
- 精确回放。

### B. Runtime Audit Event Log

新文件：

- `~/.cc-rust/runs/<session_id>/events.ndjson`
- `~/.cc-rust/runs/<session_id>/meta.json`
- `~/.cc-rust/runs/<session_id>/artifacts/` 用于超大工具输出、导出中间件和附件索引

用途：

- 作为唯一运行时事实源。
- append-only。
- 事件级结构化 JSON。
- 支持离线校验、重放、导出和聚合分析。

### C. Session Snapshot

现有文件：

- `~/.cc-rust/sessions/<session_id>.json`

用途：

- resume。
- UI 初始恢复。

要求：

- 不是事实源。
- 必须补齐 assistant usage 等关键字段，避免和 audit log 相互矛盾。

### D. Transcript

现有文件：

- `~/.cc-rust/transcripts/<session_id>.ndjson`

用途：

- 面向人读。
- tail 友好。
- 允许压缩摘要。

要求：

- 明确标注它是“会话转录视图”，不是完整审计。

### E. Audit Export

现有文件：

- `~/.cc-rust/audits/<session_id>.audit.json`

用途：

- 对外交付。
- hash chain 校验。
- 法务 / 合规 / 问题复盘留档。

要求：

- 生成优先读 `events.ndjson`，不足时再回退 session snapshot。

## 4.2 统一关联键

全链路最少统一以下字段：

- `session_id`
  现有字段，继续沿用。

- `submit_id`
  一次用户提交或一次外部触发的边界 ID。
  headless 取 `FrontendMessage::SubmitPrompt.id`。
  daemon 取 `/api/submit` 里的 `message_id`。
  REPL/TUI 直接生成 UUID。

- `turn_id`
  query loop 每一轮迭代一个 ID，对应 `src/query/loop_impl.rs:75` 的循环边界。

- `request_id`
  一次模型 API request 的 ID。
  同一 submit 内可能有多个 request，尤其是 retry / fallback / continue。

- `message_id`
  assistant / user / system 消息的 UUID；现有 message UUID 继续沿用。

- `tool_use_id`
  现有工具调用 ID，继续沿用。

- `event_id`
  新增 audit 事件唯一 ID。

- `parent_event_id`
  用于表达事件链父子关系，例如 `tool.result` 指向 `tool.start`。

## 4.3 统一事件格式

建议基准 schema：

```json
{
  "event_id": "evt_01",
  "parent_event_id": null,
  "ts": "2026-04-16T08:30:12.345Z",
  "session_id": "sess_01",
  "submit_id": "sub_01",
  "turn_id": "turn_01",
  "request_id": "req_01",
  "message_id": "msg_01",
  "tool_use_id": null,
  "source": "headless",
  "kind": "tool.start",
  "stage": "tool_execution",
  "level": "info",
  "outcome": "started",
  "duration_ms": null,
  "data": {
    "tool_name": "Read",
    "input": {
      "file_path": "src/main.rs"
    }
  }
}
```

约束：

- 顶层字段稳定，便于 grep 和后续数据管道。
- 业务 payload 放 `data`。
- 超大 payload 不直接塞进 `data`，改落 `artifacts/`，事件中只保存 `artifact_path` 和 `size_hint`。
- 对 secret、token、key、cookie 走 redact。

## 4.4 事件分类

首批必须覆盖：

- `session.start`
- `session.end`
- `submit.received`
- `submit.completed`
- `submit.aborted`
- `query.turn.start`
- `query.turn.continue`
- `query.turn.stop`
- `model.request.start`
- `model.request.retry`
- `model.request.finish`
- `model.request.error`
- `assistant.message`
- `user.message`
- `system.message`
- `tool.start`
- `tool.progress`
- `tool.finish`
- `tool.error`
- `permission.requested`
- `permission.resolved`
- `compact.pre`
- `compact.post`
- `ipc.client.connected`
- `ipc.client.disconnected`
- `daemon.sse.connected`
- `daemon.sse.reattach`
- `background_agent.spawned`
- `background_agent.completed`

不建议默认落盘每一个 token delta。

建议：

- 默认只记录 `stream.start`、`stream.stop`、最终 assembled assistant message。
- 如果需要极限调试，再加开关记录 `stream.delta`。

## 5. 分阶段实施方案

## 阶段 0：建立统一事实源骨架

目标：

- 在不打散现有逻辑的前提下，先把事件模型、writer 和落盘路径建起来。

建议新增：

- `src/observability/mod.rs`
- `src/observability/event.rs`
- `src/observability/context.rs`
- `src/observability/sink.rs`

建议改动：

- `src/main.rs`
- `.env.example`

关键动作：

- 定义 `AuditEvent`、`AuditContext`、`AuditSink`。
- 在 `main.rs` 启动时初始化 session 级 writer 目录。
- 提供 `emit(event)` 的非阻塞写接口。
- 新增环境变量：
  - `CC_AUDIT_LOG=on|off`
  - `CC_AUDIT_STREAM_DELTAS=0|1`
  - `CC_AUDIT_REDACTION=strict|default|off`

完成标准：

- 进程启动后能为当前 session 建立 `events.ndjson` 和 `meta.json`。
- 即使没有任何业务打点，也先写出 `session.start`。

## 阶段 1：统一上下文传播

目标：

- 把 `session_id / submit_id / turn_id / request_id / tool_use_id` 真正串起来。

建议改动：

- `src/engine/lifecycle/submit_message.rs`
- `src/query/loop_impl.rs`
- `src/query/deps.rs`
- `src/ipc/headless.rs`
- `src/ipc/query_runner.rs`
- `src/daemon/routes.rs`
- `src/types/config.rs`

关键动作：

- 在 submit 入口创建 `submit_id`。
- 在每轮 query loop 创建 `turn_id`。
- 在每次模型调用前创建 `request_id`。
- 在 tool pipeline 里复用现有 `tool_use_id`。
- 把这些字段挂进 `AuditContext`，避免每层手工拼。

完成标准：

- 任意一条 `tool.finish` 事件都能反查回 `submit_id` 和 `turn_id`。
- headless、daemon、TUI/REPL 三条入口都使用同一 submit 语义。

## 阶段 2：补齐核心运行链路打点

目标：

- 覆盖“用户提交 -> query turn -> model call -> assistant/tool -> finish/error”最短闭环。

建议改动：

- `src/engine/lifecycle/submit_message.rs`
- `src/query/loop_impl.rs`
- `src/tools/execution/pipeline.rs`
- `src/tools/hooks/pre_tool.rs`
- `src/tools/hooks/post_tool.rs`
- `src/api/client/mod.rs`
- `src/shutdown.rs`

关键动作：

- 在 `submit_message: starting` 附近新增 `submit.received`。
- 在 `query loop iteration start` 附近新增 `query.turn.start`。
- 在模型调用开始 / retry / 错误 / 完成处新增 `model.request.*` 事件。
- 在工具执行开始 / 成功 / 失败处新增 `tool.*` 事件。
- 在 shutdown 时落 `session.end` 和 `submit.completed`。

完成标准：

- 一次正常请求至少能在 `events.ndjson` 中看到：
  - `submit.received`
  - `query.turn.start`
  - `model.request.start`
  - `assistant.message`
  - `submit.completed`
- 一次带工具调用的请求至少能额外看到：
  - `tool.start`
  - `tool.finish` 或 `tool.error`

## 阶段 3：补齐权限、进度、daemon、IPC 边界

目标：

- 把当前最容易断链的边界补完整。

建议改动：

- `src/types/tool.rs`
- `src/engine/lifecycle/mod.rs`
- `src/ipc/sdk_mapper.rs`
- `src/ipc/protocol.rs`
- `src/daemon/routes.rs`
- `src/daemon/state.rs`
- `src/daemon/sse.rs`
- `src/dashboard.rs`

关键动作：

- 让 `PermissionDenial` 真正落地到 audit event，而不是只留 struct。
- 给 `ToolProgress` 增加统一生产入口，并在 transcript / audit 两边都有记录。
- daemon submit / attach / detach / SSE connect / reattach 记事件，不再只记字符串 Markdown。
- `ToolUseSummary` 即便不走 SSE，也要进入 runtime audit log。
- dashboard 的 `subagent-events.ndjson` 保留为专用视图，但建议用同一事件 schema 子集。

完成标准：

- 一次权限拒绝能看到：
  - `permission.requested`
  - `permission.resolved`
  - `tool.error` 或 `permission.denied`
- SSE 断线重连后，可以通过 durable audit log 对照 ring buffer 结果。

## 阶段 4：收敛 transcript / session / audit export

目标：

- 让“运行时事实源”和“派生视图”关系稳定下来。

建议改动：

- `src/session/transcript.rs`
- `src/session/storage.rs`
- `src/session/audit_export.rs`
- `src/session/export.rs`

关键动作：

- transcript 头部文档和命名改为“conversation transcript view”。
- session snapshot 保存 assistant `usage`，消除 `usage: None` 恢复缺口。
- audit export 优先从 `events.ndjson` 构造；session snapshot 只作补全和兼容。
- 对于工具大结果、压缩替换、microcompact、artifact 外链，在导出里显式保留引用关系。

完成标准：

- 通过 saved session 再导出 audit 时，token 和 usage 不再丢失。
- transcript、session、audit export 三者角色清晰且不互相冒充事实源。

## 阶段 5：把 `tracing` 调整为真正可聚合的 operator log

目标：

- 保留现有调试体验，但让 operator log 也更好用。

建议改动：

- `src/main.rs`
- 关键高频模块的 `tracing` 调用点

关键动作：

- 保留 stderr 文本层。
- 文件层改为 JSON 或至少 key-value 稳定输出。
- 统一高频字段命名：
  - `session_id`
  - `submit_id`
  - `turn_id`
  - `request_id`
  - `message_id`
  - `tool_use_id`
  - `kind`
  - `duration_ms`
  - `outcome`

完成标准：

- `cc-rust.log.*` 能直接被 `jq` / log shipper / grep 脚本消费。
- operator log 与 runtime audit log 共享核心 ID 字段。

## 6. 文件触点建议

核心建议如下：

- 启动与 writer 初始化：
  - `src/main.rs`

- 运行时上下文与事件模型：
  - `src/observability/event.rs`（new）
  - `src/observability/context.rs`（new）
  - `src/observability/sink.rs`（new）
  - `src/observability/mod.rs`（new）

- submit / query / model：
  - `src/engine/lifecycle/submit_message.rs`
  - `src/query/loop_impl.rs`
  - `src/query/deps.rs`
  - `src/api/client/mod.rs`

- tool / permission / progress：
  - `src/tools/execution/pipeline.rs`
  - `src/tools/hooks/pre_tool.rs`
  - `src/tools/hooks/post_tool.rs`
  - `src/types/tool.rs`
  - `src/engine/lifecycle/mod.rs`
  - `src/engine/lifecycle/types.rs`

- IPC / daemon / dashboard：
  - `src/ipc/headless.rs`
  - `src/ipc/query_runner.rs`
  - `src/ipc/sdk_mapper.rs`
  - `src/ipc/protocol.rs`
  - `src/daemon/routes.rs`
  - `src/daemon/state.rs`
  - `src/daemon/sse.rs`
  - `src/daemon/memory_log.rs`
  - `src/dashboard.rs`

- 派生视图与导出：
  - `src/session/transcript.rs`
  - `src/session/storage.rs`
  - `src/session/audit_export.rs`
  - `src/session/export.rs`
  - `src/shutdown.rs`

## 7. 具体验收标准

以下标准必须可测试、可脚本验证：

1. 一次普通 submit 结束后，`events.ndjson` 中存在同一 `submit_id` 下的完整链路。

2. 任意 `tool.finish` 事件都带有：
   - `session_id`
   - `submit_id`
   - `turn_id`
   - `tool_use_id`
   - `duration_ms`
   - `outcome`

3. 任意权限交互都能关联：
   - permission request
   - 用户决策
   - 后续 tool outcome

4. session snapshot roundtrip 后 assistant usage 不丢失。

5. crash 发生后，`events.ndjson` 保持 NDJSON 前缀可读，不出现整文件损坏。

6. `audit_export verify` 对基于 runtime event log 导出的文件校验通过。

7. transcript 和 session snapshot 即使裁剪，也不再宣称自己是完整审计来源。

8. headless、daemon、TUI 三条入口生成的 submit 事件字段语义一致。

## 8. 验证计划

### 单元测试

- `AuditEvent` 序列化 / 反序列化。
- `AuditSink` append-only 写入和 crash-safe 前缀读取。
- `AuditContext` 生成与字段继承。
- session snapshot 对 assistant usage 的保存和恢复。
- audit export 从 event log 构建 hash chain。

### 集成测试

- `submit_message()` 一次普通问答产生完整事件序列。
- 一次 tool call 产生 `tool.start -> tool.finish`。
- 一次 permission deny 产生 request / resolved / denied 链。
- daemon `/api/submit`、SSE attach / reattach 事件持久化。

### E2E / smoke

- headless JSONL 路径跑一轮请求，检查 `events.ndjson`。
- daemon 模式跑一轮 `/api/submit` + `/events`，检查 `events.ndjson`。
- `/audit-export verify` 基于真实事件输出通过。

### 可观测性人工检查

- `jq 'select(.submit_id == "...")' events.ndjson`
- `rg '"tool_use_id":"...'" ~/.cc-rust/runs`
- 断电 / 强制 kill 后前缀是否仍可解析

## 9. 风险与缓解

### 风险 1：事件量爆炸

来源：

- stream delta
- 大型 tool 输出
- 高频 progress

缓解：

- 默认不记录每个 token delta。
- 大 payload 落 `artifacts/`，事件只保留引用。
- progress 做节流或阶段性快照。

### 风险 2：双写不一致

来源：

- transcript、session、audit event 三套 writer 并存阶段

缓解：

- 先引入 runtime audit log，再逐步让 transcript / export 依赖它。
- rollout 初期保留 dual write，但只允许一个事实源。

### 风险 3：性能回退

来源：

- 每个事件都同步写磁盘

缓解：

- 使用 non-blocking writer + flush on boundary。
- submit 结束、shutdown、critical error 时显式 sync。

### 风险 4：敏感信息泄漏

来源：

- prompt
- tool input
- token / credential
- filesystem 内容

缓解：

- 在 `AuditSink` 前统一做 redact。
- 对 auth、cookie、token、API key、OAuth code 等字段硬编码屏蔽。
- 为高风险工具保留 `input_summary` 和 `artifact_ref` 两级模式。

## 10. 建议执行顺序

建议按以下提交切片推进：

1. `docs: add traceable logging plan`
2. `feat(observability): add runtime audit event schema and sink`
3. `feat(observability): propagate submit/turn/request context`
4. `feat(observability): instrument query and tool pipeline`
5. `feat(observability): instrument daemon/ipc boundaries`
6. `fix(session): persist assistant usage in session snapshot`
7. `refactor(audit): build audit export from runtime event log`
8. `refactor(logging): normalize operator log fields`

## 11. 最终判定标准

当以下判断同时成立时，才可以认为“项目具有完整的可追溯日志体系”：

- 对任意一次请求，存在单一事实源。
- 该事实源是 append-only 的 durable event log。
- 它能串起入口、turn、模型调用、工具调用、权限决策、异常和结束状态。
- transcript、session、audit export 与该事实源不冲突。
- 审计导出可验证，且不依赖“事后猜测”缺失字段。

在达到这些条件之前，项目只能算“有多套记录能力”，不能算“完整可追溯”。
