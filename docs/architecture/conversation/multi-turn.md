---
title: "多轮对话管理 - allthecodes 会话编排与持久化"
description: "从源码角度解析 allthecodes (Rust port of Claude Code) 多轮对话管理：会话状态机、JSON/NDJSON 持久化、Transcript 录制、会话恢复和 Fork 分支机制。"
keywords: ["多轮对话", "会话管理", "会话持久化", "transcript", "resume", "fork"]
sourceRef: "ea4e6ab2 (2026-05-28)"
---

{/* 本章目标：从源码角度揭示 allthecodes 的会话编排、持久化存储、会话恢复和 Fork 分支的完整链路 */}

## 单轮 vs 多轮：架构层面的差异

- **单轮**（一次 Agentic Loop）：`query()` 函数的一次完整执行——`crates/allthecodes-query/src/loop_impl.rs` 中的 `'query_loop` 循环，从上下文预处理到工具执行到最终终止
- **多轮**（一个 Session）：`crates/allthecodes-session/` 管理的完整会话——跨越数十次 `query()` 调用，持续数小时，存储于磁盘

allthecodes 将会话管理从查询循环中解耦，专门封装为 `allthecodes-session` crate，包含持久化、录制、恢复和导出功能：

```
allthecodes-session crate（src/lib.rs）
├── storage.rs        — 会话文件的 JSON 持久化（读/写/列表/截断）
├── transcript.rs     — 会话 Transcript 的 NDJSON 追加录制
├── resume.rs         — 会话恢复（查找最近会话 + 加载消息）
├── fork.rs           — 会话分支（从父会话 Fork 新会话）
├── export.rs         — 导出为 Markdown 格式
├── session_export/   — 结构化 SessionExport（JSON 数据包）
├── request_snapshot.rs — API 请求快照记录
└── memdir/           — 会话记忆系统（MemoryEntry、MemoryType、Recall）
```

## 会话持久化：JSON SessionFile

### 存储路径

```
~/.allthecodes/sessions/<session-uuid>.json
```

路径由 `allthecodes_config::paths::sessions_dir()` 解析。每个会话存储为一个独立的 JSON 文件。

### SessionFile 数据结构

```rust
// crates/allthecodes-session/src/storage.rs
struct SessionFile {
    session_id: String,          // UUID v4
    created_at: i64,             // 创建时间（Unix 秒）
    last_modified: i64,          // 最后修改时间
    cwd: String,                 // 工作目录（git root 规范化）
    custom_title: Option<String>, // 用户自定义标题（/rename 命令设置）
    messages: Vec<SerializableMessage>,  // 序列化消息列表
}
```

消息序列化采用 `SerializableMessage` 类型，按消息类型（user/assistant/system/progress/attachment）分别存储：

```rust
struct SerializableMessage {
    msg_type: String,            // "user" | "assistant" | "system" | ...
    uuid: String,
    timestamp: i64,
    data: serde_json::Value,     // 类型特定的数据负载
}
```

### 持久化操作

```rust
// 核心 API
save_session(session_id, messages, cwd)        → 写入/覆盖会话文件
load_session(session_id)                        → 读取并反序列化消息
list_sessions()                                 → 列出所有会话（按最后修改时间排序）
list_workspace_sessions(cwd)                    → 列出同一 workspace 的会话
truncate_session(session_id, keep)              → 截断到前 keep 条消息（含备份）
set_session_title(session_id, title)            → 设置/清除自定义标题
```

### Workspace 分组

`workspace_key(cwd)` 为同一 Git 仓库的所有子目录生成相同的 `workspace_key`：

- **Git 仓库**（含 worktree）：使用 `git common_dir` 的规范化路径，同一仓库的所有 worktree 共享 key
- **非 Git 目录**：使用规范化后的稳定工作目录路径

这使得 `list_workspace_sessions()` 能按工作目录过滤会话——无论用户在仓库的哪个子目录启动会话。

### 标题机制

会话标题按优先级确定：
1. **自定义标题**：`set_session_title()` 写入的 `custom_title`（最长 200 字符）
2. **自动派生**：第一条非 meta 用户消息的文本内容（截断至 80 字符）
3. **空标题**：以上均不可用时为空字符串

### 截断与回滚

`truncate_session(session_id, keep)` 是谨慎的：
- 先备份完整会话到 `{session_id}.rewind-{epoch}.json`
- 再截断 `keep` 之前的消息
- 当 `keep >= 当前消息数` 时为 no-op，不产生备份

## Transcript 录制：NDJSON 追加日志

与 `SessionFile`（整体覆写）不同，Transcript 采用追加写入的 NDJSON 格式，确保部分会话在崩溃时也能保留。

### 存储路径

```
~/.allthecodes/transcripts/<session-uuid>.ndjson
```

### 数据结构

每条记录是一行 JSON，格式为 `TranscriptEntry`：

```rust
struct TranscriptEntry {
    timestamp: i64,            // Unix 毫秒
    session_id: String,
    msg_type: String,          // "user" | "assistant" | "system" | ...
    uuid: String,
    payload: serde_json::Value,// 压缩后的内容摘要
}
```

消息载荷经过精简：User 消息只保留 `text` 字段，Assistant 消息只保留 `content_summary`（文本块 + tool_use 名称列表），避免占用过大空间。

### 写入流程

```rust
// crates/allthecodes-session/src/transcript.rs
record_transcript(session_id, &messages)
  → files.create(true).append(true)
  → 逐条序列化 + writeln!
  → 崩溃安全（已写入的行不丢失）
```

关键特性：
- **追加写入**：使用 `OpenOptions::append(true)` 打开文件
- **崩溃安全**：每条消息独立一行，之前已写入的行不会丢失
- **文件在首次使用时创建**：`create(true)` 确保目录存在

### Session Header

Transcript 文件的首行可以是 `session_header` 记录：

```rust
struct SessionHeader {
    timestamp: i64,
    session_id: String,
    msg_type: "session_header",
    forked_from: Option<String>,      // 父会话 ID（Fork 时写入）
    forked_at_uuid: Option<String>,   // Fork 点消息 UUID
    title: Option<String>,            // 分支标题
}
```

`write_session_header()` 使用 `create_new(true)` 创建，防止覆写已有转录文件。

### Transcript 刷新

`flush_transcript(session_id)` 提供显式同步点，调用 `file.sync_all()` 确保数据落盘。这对 Fork 操作后的持久性保证很重要。

### 录制范围

当前 `record_transcript` 支持的消息类型：
- **user**：文本内容（或 block 计数摘要）
- **assistant**：文本块 + tool_use 名称 + stop_reason
- **system**：原始 content 文本
- **progress**：tool_use_id
- **attachment**：attachment 类型字段

## 会话恢复（Resume）

`crates/allthecodes-session/src/resume.rs` 提供简洁的恢复操作：

```rust
// 查找最近会话
fn get_last_session(cwd: &Path) -> Result<Option<SessionInfo>>
  → storage::list_workspace_sessions(cwd)
  → 返回第一个（最近修改的）会话

// 恢复会话消息
fn resume_session(session_id: &str) -> Result<Vec<Message>>
  → storage::load_session(session_id)
```

恢复流程（命令行入口）：
1. 解析 resume 参数（UUID → 直接加载，boolean → 最近会话 picker）
2. `load_session(session_id)` 重建 `Message[]` 数组
3. 创建新的 `query()` 调用，传入恢复的消息作为初始上下文

## 会话分支（Fork）

`crates/allthecodes-session/src/fork.rs` 实现 Transcript 级分支：

```rust
fn fork_session(
    parent_session_id: &str,
    new_session_id: &str,
    messages: &[Message],
    cwd: &str,
    cursor_uuid: Option<&str>,
) -> Result<ForkOutcome>
```

### Fork 流程

```
1. 确定 Fork 点：
   ├── cursor_uuid（显式指定）
   ├── messages 最后一条消息的 UUID
   └── 无合适 cursor → 复制所有
2. 写入 session_header（fork_provenance）：
   ├── forked_from: parent_session_id
   ├── forked_at_uuid: cursor
   └── title: "ParentTitle (fork @ abcd1234)"
3. 复制父会话 Transcript 条目：
   ├── 跳过 session_header
   ├── 重写 session_id 为目标会话 ID
   └── 到 cursor_uuid 为止（含）
4. 写入 Fork 的 SessionFile：
   ├── 消息截断到 cursor
   └── 设置自定义标题
5. flush_transcript() 确保持久化
```

Fork 是**非破坏性**操作——父会话的所有文件（transcript、session file、title）不被修改。

## 会话导出格式

### Markdown 导出（export.rs）
人类可读格式，包含会话元数据、消息时间戳、工具调用代码块、费用和 token 用量汇总。

### 结构化导出（session_export/mod.rs）
`SessionExport` 数据包（schema v2），包含：
- **raw_transcript** / **transcript**：完整消息 JSON + 数量统计
- **api_view**：API 请求统计、Provider 列表、诊断信息
- **api_requests**：API 请求快照记录（sanitized，不含认证信息）
- **tool_calls**：工具调用时间线（tool_use ↔ tool_result 配对）
- **compression**：压缩事件记录（compact boundaries、content replacements）
- **context**：token 估算、费用分解、工具使用统计

```rust
struct SessionExport {
    schema_version: u32,
    session: SessionMeta,
    transcript: TranscriptData,
    api_view: ApiViewData,
    api_requests: Vec<ApiRequestSnapshot>,
    tool_calls: Vec<ToolCallRecord>,
    compression: CompressionData,
    context: ContextSnapshot,
}
```

## API 请求快照

`crates/allthecodes-session/src/request_snapshot.rs` 记录每次 API 请求的完整结构（不含凭证）：

```
~/.allthecodes/sessions/<session-id>.requests.ndjson
```

`ApiRequestSnapshot` 包含 schema 版本、provider、model、消息/系统/工具计数、max_tokens、stream 标志、thinking 配置、tool_choice 等。图像数据被 sanitize 为结构化元数据（`[image omitted: image/png, base64 length 12345]`）。

## 记忆系统（memdir）

`crates/allthecodes-session/src/memdir/` 实现类 Bun 的闭包记忆分类：

- **MemoryType**：User、Feedback、Project、Reference
- **MemoryScope**：Global、Project、Team、Auto
- **RelevantMemory**：带相关性分数和匹配关键词的记忆条目
- 支持 CRUD 操作、全文索引和模型辅助召回

记忆入口点文件为 `MEMORY.md`，最大 200 行/25KB。

## 整体数据流

```
用户输入
  ↓
query() Agentic Loop（allthecodes-query）
  ↓  yield 每个事件
UI 层（TUI/REPL）
  ↓  turn 结束
save_session() → session.json（覆写）
record_transcript() → transcript.ndjson（追加）
  ↓ 会话结束
export_session() → session.json（结构化导出）
```

这种分层设计确保：
- **实时安全**：Transcript 追加写入，崩溃不丢已记录行
- **查询效率**：SessionFile 整体读取，适合恢复
- **分析友好**：结构化导出包含工具调用时间线和压缩事件
- **分支透明**：Fork 在 Transcript 级别复制，不侵入父会话
