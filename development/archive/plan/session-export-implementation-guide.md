# 会话完整导出实现说明

这份文档描述如何把当前 Claude Code 项目里的一个 session 导出成”可审计的完整记录”，包括：

- 会话消息全文
- 每次工具调用与工具结果
- 压缩过程中的摘要、边界、替换记录
- 计算后的上下文视图
- 当前 API 请求快照

目标不是做一个”聊天记录导出”，而是做一个”可回放、可分析、可追踪”的 session 数据包。

## Rust 实现进度 (cc-rust)

状态日期：2026-05-17。当前导出 schema 为 v2。

### 已完成

| 功能 | 文件 | 说明 |
|------|------|------|
| transcript.messages 全量导出 | `crates/cc-session/src/session_export/*` | `build_transcript_data()` — 所有消息类型 (user/assistant/system/progress/attachment)，图片块导出为结构化 metadata + 可读占位，不输出 base64 原文 |
| rawTranscript vs apiView 基础结构 | `crates/cc-session/src/session_export/mod.rs` | `SessionExport` schema v2 包含 `raw_transcript`、`transcript`、`api_view` 和 `api_requests`；当前 `api_view` 是 request snapshot summary，不是完整 projectView 投影；snapshot log 读取失败会写入 `api_view.diagnostics` |
| API 请求快照 | `crates/cc-session/src/request_snapshot.rs`, `crates/cc-engine/src/lifecycle/deps.rs` | `cc-engine` 在 provider request 发送前记录最终 request body；导出读取同一 request boundary，image source data 会被 metadata 占位替换 |
| tool call timeline 重建 | `crates/cc-session/src/session_export/compression.rs` | `reconstruct_tool_timeline()` — tool_use ↔ tool_result 配对，含未匹配的孤立调用 |
| compact boundary 提取 | `crates/cc-session/src/session_export/compression.rs` | `extract_compression_events()` — 从 `SystemSubtype::CompactBoundary` 提取 |
| content replacement 检测 | `crates/cc-session/src/session_export/compression.rs` | `detect_content_replacement()` — 匹配 `tool_result_budget` 产生的 `”Full output saved to”` 标记 |
| microcompact 检测 | `crates/cc-session/src/session_export/compression.rs` | `detect_microcompact()` — 匹配 `compact/microcompact.rs` 产生的 `”(microcompacted)”` 标记 |
| context snapshot | `crates/cc-session/src/session_export/builders.rs` | `build_context_snapshot()` — token/cost/tool 统计、上下文窗口利用率 |
| session 元数据 | `crates/cc-session/src/session_export/mod.rs` | git branch/sha、model、project_path、时间戳、custom_title；`mode`/`permission_mode`/`tags` 字段已在 schema 中预留 |
| /session-export 命令 | `crates/cc-commands/src/session_export.rs` | export / list / export-to-path / export-by-id + summary 输出，包含 API request count |
| /audit-export 命令 | `crates/cc-session/src/audit_export.rs`, `crates/cc-commands/src/audit_export.rs` | SHA-256 哈希链、防篡改验证 |

### 仍未完整实现

以下功能在原版 TypeScript 中存在，但 cc-rust 尚未实现完整来源或投影逻辑，因此仍是 release residual：

| 功能 | 阻塞原因 | 后续路径 |
|------|----------|----------|
| **Context collapse 事件** (`contextCollapseCommits`, `contextCollapseSnapshot`) | `src/compact/pipeline.rs` 中 Step 4 明确标注 `”Not yet implemented — will fold old segments into summaries.”` (Phase 2+)。`SystemSubtype` 枚举中没有 `ContextCollapse` 变体。 | 等 `compact/pipeline.rs` 实现 context collapse 后，在 `SystemSubtype` 中增加变体，然后在 `extract_compression_events()` 中提取。 |
| **Session mode / tag / permission_mode 来源** | `SessionExport` schema 已预留 `mode`, `permission_mode`, `tags`，`custom_title` 已从 `SessionFile` 导出；但 session storage / runtime 还没有稳定 mode/tag 来源。 | 在 `crates/cc-session/src/storage.rs` 和 engine runtime state 中补持久来源，然后由 `build_session_meta()` 填充。 |
| **完整 apiView 投影** | 当前 `api_view` 是 `ApiRequestSnapshot` 摘要；还没有 `getMessagesAfterCompactBoundary()` + `projectView()` 等价 API 视图投影逻辑。 | 需要在 `crates/cc-compact/` 或 `crates/cc-engine/` 中实现 API 视图投影函数，然后在导出中输出模型上下文视图。 |
| **Microcompact boundary (系统消息)** | `microcompact_messages()` 只是修改 tool result 内容，不产生 `SystemSubtype` 边界消息。当前只能通过文本标记间接检测。 | 可选：在 `microcompact_messages()` 执行后插入一条 `SystemSubtype::MicrocompactBoundary` 系统消息，记录本轮 microcompact 的 tokens_freed 等统计。 |

当前回归命令：

```bash
cargo test -p cc-session request_snapshot
cargo test -p cc-session session_export
cargo test -p cc-commands session_export
cargo test -p cc-engine lifecycle
```

---

## 完成状态（2026-05-21）

**判定：⚠️ 部分完成（11/15 项完成，4 项 release residual）**

已完成功能（11 项）：
1. `transcript.messages` 全量导出 — `build_transcript_data()` 支持所有消息类型
2. `rawTranscript` vs `apiView` 基础结构 — `SessionExport` schema v2
3. API 请求快照 — `cc-session::request_snapshot` + `cc-engine::lifecycle::deps`
4. tool call timeline 重建 — `reconstruct_tool_timeline()`
5. compact boundary 提取 — `extract_compression_events()`
6. content replacement 检测 — `detect_content_replacement()`
7. microcompact 检测 — `detect_microcompact()`
8. context snapshot — `build_context_snapshot()`
9. session 元数据 — git branch/sha/model/project_path/时间戳
10. `/session-export` 命令 — export/list/export-to-path/export-by-id + summary
11. `/audit-export` 命令 — SHA-256 哈希链、防篡改验证

仍未实现的 release residual（4 项）：
1. **Context collapse 事件** — 阻塞于 `compact/pipeline.rs` Step 4 未实现
2. **Session mode/tag/permission_mode 来源** — schema 已预留但 runtime 无稳定来源
3. **完整 apiView 投影** — 当前是 `ApiRequestSnapshot` 摘要，不是 `projectView()` 等价
4. **Microcompact boundary 系统消息** — 当前只通过文本标记间接检测

## 原版 TypeScript 架构参考

以下内容描述原版 TypeScript 实现的架构，供 Rust 版对齐参考。

## 结论先行

如果要实现这件事，优先从下面三个地方出发：

1. `src/utils/sessionStorage.ts`
2. `src/query.ts`
3. `src/utils/toolResultStorage.ts`

原因很直接：

- `sessionStorage.ts` 是 transcript 的真实落盘层，负责写入、恢复、读取。
- `query.ts` 是每轮上下文变换的主入口，负责 `microcompact`、`autocompact`、`contextCollapse` 和工具结果预算。
- `toolResultStorage.ts` 负责”工具结果压缩/替换”的持久化状态，是完整导出里最容易漏掉的一层。

## 现有架构

### 1. 会话落盘层

核心写入链路如下：

```text
useLogMessages / QueryEngine
  -> recordTranscript()
  -> insertMessageChain()
  -> appendEntry()
  -> .jsonl transcript
```

对应代码：

- [`src/hooks/useLogMessages.ts`](../src/hooks/useLogMessages.ts)
- [`src/QueryEngine.ts`](../src/QueryEngine.ts)
- [`src/utils/sessionStorage.ts`](../src/utils/sessionStorage.ts)

读取链路：

- [`loadTranscriptFile()`](../src/utils/sessionStorage.ts)
- [`loadAllLogsFromSessionFile()`](../src/utils/sessionStorage.ts)

### 2. 工具调用层

每次工具调用本质上是：

- assistant 消息里的 `tool_use`
- user 消息里的 `tool_result`

工具执行和结果生成在：

- [`src/services/tools/toolExecution.ts`](../src/services/tools/toolExecution.ts)
- [`src/services/tools/toolOrchestration.ts`](../src/services/tools/toolOrchestration.ts)
- [`src/services/tools/StreamingToolExecutor.ts`](../src/services/tools/StreamingToolExecutor.ts)

### 3. 压缩层

项目里有三类压缩/裁剪：

- `tool_result` 预算压缩
- `microcompact`
- `compact` / `autocompact` / `contextCollapse`

对应代码：

- [`src/utils/toolResultStorage.ts`](../src/utils/toolResultStorage.ts)
- [`src/services/compact/microCompact.ts`](../src/services/compact/microCompact.ts)
- [`src/services/compact/compact.ts`](../src/services/compact/compact.ts)
- [`src/services/compact/autoCompact.ts`](../src/services/compact/autoCompact.ts)
- [`src/services/contextCollapse/index.ts`](../src/services/contextCollapse/index.ts)

### 4. 上下文层

如果你想导出“模型实际看到的上下文”，而不是 UI 原始历史，要复用：

- [`src/commands/context/context-noninteractive.ts`](../src/commands/context/context-noninteractive.ts)
- [`src/utils/analyzeContext.ts`](../src/utils/analyzeContext.ts)
- [`src/utils/messages.ts`](../src/utils/messages.ts)

这条链路已经做了：

- 过滤 compact boundary
- projectView 投影
- microcompact
- token 统计
- tool call / tool result / attachment 分类

## 设计目标

导出的文件建议是一个 JSON 文档，而不是纯文本。

原因：

- 纯文本适合阅读，不适合恢复和比对。
- JSON 才能同时保存原始消息、压缩痕迹、上下文统计和请求快照。

建议导出结构：

```ts
type SessionExport = {
  schemaVersion: number
  exportedAt: string
  session: {
    sessionId: string
    projectPath?: string
    gitBranch?: string
    mode?: 'normal' | 'coordinator'
    customTitle?: string
    tag?: string
  }
  transcript: {
    messages: unknown[]
    entries: unknown[]
  }
  toolCalls: Array<{
    assistantUuid: string
    toolUseId: string
    toolName: string
    input: unknown
    result?: unknown
    isError?: boolean
    timestamp?: string
  }>
  compression: {
    contentReplacements: unknown[]
    compactBoundaries: unknown[]
    contextCollapseCommits: unknown[]
    contextCollapseSnapshot?: unknown
    microcompact?: unknown
  }
  context: {
    current?: unknown
    apiUsage?: unknown
    messageBreakdown?: unknown
  }
  apiRequest?: {
    params: unknown
    messages?: unknown[]
  }
}
```

## 数据来源映射

### transcript / entry 原始记录

来源：

- [`loadTranscriptFile()`](../src/utils/sessionStorage.ts)

包含：

- `messages`
- `summaries`
- `customTitles`
- `tags`
- `agentNames`
- `agentColors`
- `agentSettings`
- `prNumbers`
- `worktreeStates`
- `contentReplacements`
- `contextCollapseCommits`
- `contextCollapseSnapshot`
- `leafUuids`

### tool 调用明细

建议从 transcript 里重建，而不是从 UI state 里猜。

规则：

- 扫描 assistant 消息中的 `tool_use`
- 用 `tool_use.id` 去匹配后续 user 消息里的 `tool_result.tool_use_id`
- 记录同一轮里的输入、输出、错误态、是否被持久化替换

工具调用生成点在：

- [`runToolUse()`](../src/services/tools/toolExecution.ts)

### 工具结果压缩

工具结果预算替换和持久化记录在：

- [`enforceToolResultBudget()`](../src/utils/toolResultStorage.ts)
- [`applyToolResultBudget()`](../src/utils/toolResultStorage.ts)
- [`recordContentReplacement()`](../src/utils/sessionStorage.ts)

这是完整导出必须包含的内容，否则导出的“上下文”会比真实模型输入更完整，回放时会失真。

### 逻辑压缩边界

需要导出的边界类型：

- `compact_boundary`
- `isCompactSummary`
- `microcompact_boundary`
- `marble-origami-commit`
- `marble-origami-snapshot`

来源：

- [`compact.ts`](../src/services/compact/compact.ts)
- [`microCompact.ts`](../src/services/compact/microCompact.ts)
- [`sessionStorage.ts`](../src/utils/sessionStorage.ts)

### 当前上下文快照

如果导出时想附带“当前上下文视图”，建议直接复用 `/context` 的非交互逻辑：

- [`collectContextData()`](../src/commands/context/context-noninteractive.ts)

它已经聚合了：

- token 分布
- tool call / tool result 统计
- attachment 统计
- `apiUsage`
- system prompt / memory / tools / skills 统计

## 推荐实现路径

### 第一步：增加一个导出服务

新增一个独立模块，比如：

- `src/utils/sessionExport.ts`

职责：

- 读取 session transcript
- 重建 tool call timeline
- 收集 compaction / replacement 元数据
- 可选附加 context snapshot
- 生成 JSON 对象

不要把这些逻辑塞进命令文件里。命令层只负责触发和保存。

### 第二步：增加一个命令入口

建议做一个本地命令，比如：

- `export-session`

它可以有两种模式：

1. 导出当前 session 到 `.json`
2. 导出到指定路径

命令层只做：

- 获取当前 session id
- 调用导出服务
- 写文件

### 第三步：补一个纯文本预览

JSON 是机器可读的，建议额外提供一个人类可读的 `.txt` 或 `.md` 预览文件，方便快速查看：

- 会话标题
- 会话时长
- tool 调用数量
- 压缩次数
- 最后一次 API 视图 token 数

这个预览可以复用：

- [`renderMessagesToPlainText()`](../src/utils/exportRenderer.tsx)
- [`formatContextAsMarkdownTable()`](../src/commands/context/context-noninteractive.ts)

## 实现细节

### 1. 从 transcript 重建 tool timeline

做法：

- 先遍历 `messages`
- 对每个 assistant 消息，提取 `tool_use` block
- 为每个 `tool_use.id` 建索引
- 再遍历后续 user 消息，匹配 `tool_result.tool_use_id`

这样能保留：

- 单个 assistant 里多个 tool_use
- 并发工具调用
- tool 失败和 user interrupt
- 结构化输出和附件消息

### 2. 把压缩事件按时间顺序串起来

建议把下面这些一起输出：

- `content-replacement`
- `compact_boundary`
- `microcompact_boundary`
- `marble-origami-commit`
- `marble-origami-snapshot`

其中：

- `content-replacement` 反映工具结果被替换成 preview
- `compact_boundary` 反映对话级摘要
- `microcompact_boundary` 反映短周期工具结果裁剪
- `marble-origami-*` 反映 context collapse 状态

### 3. 区分“原始消息”和“API 视图”

导出时建议同时保留两份视图：

- `rawTranscript`：`.jsonl` 里的原始消息
- `apiView`：`getMessagesAfterCompactBoundary()` + `projectView()` + `microcompactMessages()`

这样用户能同时看到：

- 真实历史
- 模型实际输入
- 哪些内容在压缩时被吞掉或替换

### 4. 当前 API 请求快照

如果要导出“最近一次模型请求”：

- 用 [`getLastAPIRequest()`](../src/bootstrap/state.ts)
- `messages` 用 [`getLastAPIRequestMessages()`](../src/bootstrap/state.ts)

注意：

- 这只是“当前进程内最近一次请求”的快照
- 不是历史所有请求
- 如果进程重启，它不会完整保留

## 建议的最小落地版本

如果你想先做一个能用的版本，优先级建议是：

1. 导出 `transcript.messages`
2. 导出 `toolCalls`
3. 导出 `contentReplacements`
4. 导出 `compact_boundary` 和 `summary`
5. 导出 `context` 快照
6. 最后再补 `lastAPIRequestMessages`

这样能先得到一个可分析、可回放的结果，再逐步补全“当前请求级别”的细节。

## 不建议一开始就做的事

- 不要直接从 UI state 导出，因为它会混入展示态和临时态。
- 不要只导出 `/share` 风格的纯文本，因为会丢失结构和压缩痕迹。
- 不要只导出最后一次 API payload，因为那只能看到“当前窗口”，看不到完整 session。
- 不要把所有逻辑塞进 command 文件，后续很难测试。

## 关键入口清单

按实现优先级排序：

1. [`src/utils/sessionStorage.ts`](../src/utils/sessionStorage.ts)
2. [`src/utils/toolResultStorage.ts`](../src/utils/toolResultStorage.ts)
3. [`src/query.ts`](../src/query.ts)
4. [`src/services/compact/compact.ts`](../src/services/compact/compact.ts)
5. [`src/services/compact/microCompact.ts`](../src/services/compact/microCompact.ts)
6. [`src/commands/context/context-noninteractive.ts`](../src/commands/context/context-noninteractive.ts)
7. [`src/utils/analyzeContext.ts`](../src/utils/analyzeContext.ts)
8. [`src/utils/log.ts`](../src/utils/log.ts)
9. [`src/bootstrap/state.ts`](../src/bootstrap/state.ts)

## 实现建议

如果要继续，我建议下一步直接做这两件事：

1. 先落一个 `SessionExport` 的 JSON schema 和导出函数
2. 再加一个命令入口，把当前 session 导出成 `.json`

这样可以先把“完整记录”打通，再考虑 UI 预览和格式美化。
