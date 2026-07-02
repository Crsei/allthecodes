# allthecodes Record/Replay 完整实现计划

本文档给出 allthecodes 会话 record/replay 的落地计划。这里的 replay 首先定义为“从持久事件日志重建已发生的会话时间线”，用于恢复、审计、回滚、分支、导出和 UI 重放；不是重新请求模型、重新执行工具的确定性重跑。确定性重跑需要额外的模型响应夹具、工具沙箱、网络隔离和时间随机数控制，本文只在后续扩展中保留接口。

## 1. 目标

### 必须实现

- 为每个会话建立追加写入的 canonical record log，作为恢复和回放的事实来源。
- 现有 `sessions/<session_id>.json`、SQLite `sessions/session_messages`、`transcripts/*.ndjson` 继续存在，但退化为兼容快照、索引或派生视图。
- CLI、TUI、Web、daemon/headless 使用同一套 session replay API 恢复会话。
- 支持崩溃后恢复：已经确认写入 log 的用户消息、助手消息、工具结果、权限响应和压缩边界不能丢。
- 支持旧会话迁移：无 record log 的旧 JSON 会话仍可读取，并可懒迁移为 record log。
- 支持版本化 schema、容错读取、坏行隔离、SQLite 索引重建。
- 为后续 rollback、branch、edit、regenerate 提供基于 event sequence 的稳定基础。

### 暂不实现

- 不在第一阶段保证模型和工具的确定性重新执行。
- 不把所有 token delta 默认写入 canonical log；raw stream 只作为可选 diagnostic 记录。
- 不立即删除旧 JSON session 文件，也不立即改变用户可见的 session list 行为。
- 不把 SQLite 变成事实来源。SQLite 只做索引和快速查询。

## 2. Codex 参考结论

Codex 的 record/replay 设计可以直接借鉴以下原则：

- 使用 JSONL 作为 durable rollout log，每一行是带时间戳的结构化事件。
- writer 是异步后台任务，通过 bounded channel 接收 `AddItems`、`Persist`、`Flush`、`Shutdown` 命令。
- flush/shutdown 可重复调用，失败时保留 pending items 并重试，避免调用方直接承担磁盘 I/O。
- 持久化策略集中在 policy 函数中，不让业务路径散落“哪些事件要写盘”的判断。
- replay 读取器按行解析，容忍旧 schema 和部分坏行，首个 `SessionMeta` 是 canonical session id 来源。
- TUI 的交互 replay buffer 和 durable rollout log 分层处理；UI 事件缓存不是事实来源。
- SQLite 或其他状态数据库只作为索引和缓存，不能替代 JSONL log。

allthecodes 应采用相同边界：record log 负责事实，session JSON 负责兼容快照，SQLite 负责索引，UI event buffer 负责前端显示。

## 3. allthecodes 当前基线

| 模块 | 当前行为 | record/replay 缺口 |
| --- | --- | --- |
| `crates/allthecodes-session/src/storage/file_store.rs` | `save_session` 每次把完整 `SessionFile` 覆盖写入 JSON，并同步 SQLite 投影 | 不是追加日志，崩溃时无法精确知道最后一个确认事件，truncate 会破坏历史 |
| `crates/allthecodes-session/src/storage/sqlite_store.rs` | `sessions`、`session_messages` 是可重建投影 | 没有 rollout path、event seq、schema version、log 状态 |
| `crates/allthecodes-session/src/storage/serialization.rs` | `SerializableMessage` 把 `Message` 转成 JSON；反序列化目前对 Progress/Attachment 等不完整 | 作为 replay 事实来源会丢信息；record log 应直接存 typed `Message` 或完整 event item |
| `crates/allthecodes-session/src/transcript.rs` | 写 compact NDJSON transcript | 适合人读和 fork 辅助，不适合作为完整 replay 来源 |
| `crates/allthecodes-session/src/session_export/` | 导出分析包 | 是导出视图，不是运行时恢复链路 |
| `crates/allthecodes-engine/src/lifecycle/submit_message/` | submit 时向 `state.messages` push 消息，并记录 transcript | 缺少中心化 recorder；权限、问题、tool progress、turn boundary 没有 durable 顺序 |
| `allthecodes/src/full_init.rs` | `--resume`、`--continue` 从 `storage::load_session` 恢复 | 应优先从 record log reconstruct，缺失时 fallback legacy JSON |
| `allthecodes/src/shutdown.rs` | 退出时 flush transcript 并保存完整 session JSON | 应先 flush recorder，再写兼容快照 |
| `allthecodes-web/src/handlers/sessions.rs` | Web session detail/resume/edit/delete/regenerate 使用当前 snapshot/truncate 语义 | 应改为 replay-first，并用 event seq 实现 rollback/branch |
| TUI event path | `EngineEvent`、BackendMessage mapper 维护前端事件流 | 需要从 replay snapshot 重建 UI 状态，但不能把 UI cache 当事实来源 |

核心问题：当前系统有“最终快照”和“轻量 transcript”，但没有一个 append-only、版本化、可重放的 canonical history。

## 4. 目标架构

```text
QueryEngine
  |
  | canonical events
  v
SessionRecorder  ------> rollouts/YYYY/MM/DD/rollout-<ts>-<session_id>.jsonl
  |                               |
  | derived projections           | replay
  v                               v
SQLite index              ReplayReader -> SessionReconstructor
Session JSON snapshot       |              |
Transcript NDJSON           |              v
Export package              |        Vec<Message> + metadata + UI snapshot
                             |
                             v
                  CLI / TUI / Web / daemon resume
```

设计边界：

- `SessionRecorder` 只负责 durable append，不直接驱动 UI。
- `ReplayReader` 只负责读取和解析 log，不做业务裁剪。
- `SessionReconstructor` 负责从 log 重建 `Vec<Message>`、session metadata、pending interaction 状态和可选 UI replay snapshot。
- `storage::save_session` 在过渡期继续写旧 JSON snapshot，但不再是新会话的唯一事实来源。
- Web/TUI 的现有事件流保持不变，只在 session load、switch、rollback、branch 时接入 replay API。

## 5. 持久化布局

新增路径放在 `allthecodes_config::paths` 体系下，遵守 `ALLTHECODES_HOME`：

```text
~/.allthecodes/
  rollouts/
    2026/
      07/
        02/
          rollout-20260702T131455Z-<session_id>.jsonl
  sessions/
    <session_id>.json
  transcripts/
    <session_id>.ndjson
  state/
    state_5.sqlite
```

推荐新增 helper：

- `allthecodes_config::paths::rollouts_dir()`
- `allthecodes_session::record_replay::paths::rollout_day_dir(created_at)`
- `allthecodes_session::record_replay::paths::new_rollout_file(session_id, created_at)`
- `allthecodes_session::record_replay::index::lookup_rollout(session_id)`

SQLite 新增索引表，不直接依赖 `sessions` 表 schema 改动：

```sql
CREATE TABLE IF NOT EXISTS session_rollouts (
  session_id TEXT NOT NULL,
  rollout_path TEXT NOT NULL,
  schema_version INTEGER NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  first_seq INTEGER NOT NULL DEFAULT 0,
  last_seq INTEGER NOT NULL DEFAULT 0,
  event_count INTEGER NOT NULL DEFAULT 0,
  status TEXT NOT NULL DEFAULT 'active',
  parent_session_id TEXT,
  branch_from_seq INTEGER,
  workspace_key TEXT,
  workspace_root TEXT,
  workspace_name TEXT,
  PRIMARY KEY (session_id, rollout_path)
);

CREATE INDEX IF NOT EXISTS idx_session_rollouts_session_id
  ON session_rollouts(session_id, updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_session_rollouts_workspace
  ON session_rollouts(workspace_key, updated_at DESC);
```

`sessions` 和 `session_messages` 继续作为快速列表和兼容读取投影。新逻辑必须能从 JSONL 全量重建 SQLite 投影。

## 6. Record Log Schema

每行是一条独立 JSON，按 `seq` 单调递增：

```json
{
  "schema_version": 1,
  "seq": 42,
  "timestamp": "2026-07-02T13:14:55.123Z",
  "session_id": "b4b6...",
  "turn_id": "turn-0007",
  "item": {
    "type": "message",
    "message": {}
  }
}
```

Rust 类型建议放在 `crates/allthecodes-session/src/record_replay/types.rs`：

```rust
pub const RECORD_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordLine {
    pub schema_version: u32,
    pub seq: u64,
    pub timestamp: DateTime<Utc>,
    pub session_id: String,
    pub turn_id: Option<String>,
    pub item: RecordItem,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RecordItem {
    SessionMeta(SessionMetaRecord),
    SessionState(SessionStateRecord),
    TurnStarted(TurnStartedRecord),
    TurnFinished(TurnFinishedRecord),
    Message(MessageRecord),
    QueryEvent(QueryEventRecord),
    ToolProgress(ToolProgressRecord),
    PermissionRequest(PermissionRequestRecord),
    PermissionResponse(PermissionResponseRecord),
    QuestionRequest(QuestionRequestRecord),
    QuestionResponse(QuestionResponseRecord),
    CompactionBoundary(CompactionBoundaryRecord),
    Snapshot(SessionSnapshotRecord),
    Rollback(RollbackRecord),
    Branch(BranchRecord),
    LegacyMessage(LegacyMessageRecord),
}
```

### 默认 canonical events

默认必须写入：

- `SessionMeta`：session id、created_at、cwd、workspace、model/config 摘要、父 session 信息。
- `SessionState`：cwd 或重要运行配置变化。
- `TurnStarted` / `TurnFinished`：用户提交边界、abort reason、usage、错误摘要。
- `Message`：完整 `allthecodes_types::message::Message`，包括 system、progress、attachment、tool use/result。
- `QueryEvent::RequestStart`：一次模型请求开始，便于定位 partial turn。
- `PermissionRequest` / `PermissionResponse`：工具权限决策。
- `QuestionRequest` / `QuestionResponse`：`ask_user` 交互决策。
- `CompactionBoundary`：compact/microcompact 后的边界和摘要消息。
- `Snapshot`：可选 checkpoint，帮助快速恢复。
- `Rollback` / `Branch`：用户编辑、删除、回滚、分支的历史操作。

### 可选 diagnostic events

默认不写，配置开启后写入：

- raw `StreamEvent` token delta。
- 高频 `ToolProgress` 增量。
- provider request/response metadata。
- UI backend event snapshot。

diagnostic events 可以设置为 droppable：队列满时允许丢弃；canonical events 不能丢弃。

## 7. Record Policy

新增 `record_replay::policy`，所有持久化判断集中在这里：

```rust
pub enum RecordClass {
    Canonical,
    Diagnostic,
    Ephemeral,
}

pub fn classify_record_item(item: &RecordItem, config: &RecordReplayConfig) -> RecordClass;
pub fn should_persist(item: &RecordItem, config: &RecordReplayConfig) -> bool;
pub fn may_drop_under_pressure(item: &RecordItem, config: &RecordReplayConfig) -> bool;
```

原则：

- 可以恢复语义的事件是 canonical，必须写。
- 只影响动画、流式细节、debug 可观测性的事件是 diagnostic。
- 纯 UI 临时状态是 ephemeral，不进入 record log。
- 含密钥、cookie、认证 token、大型二进制内容的 payload 必须经过 redaction 或 sidecar 策略。

## 8. 新模块划分

在 `crates/allthecodes-session/src/record_replay/` 下新增：

```text
record_replay/
  mod.rs
  types.rs
  config.rs
  paths.rs
  policy.rs
  recorder.rs
  reader.rs
  reconstruct.rs
  index.rs
  migration.rs
  redaction.rs
  fixtures.rs
```

职责：

- `types.rs`：record line、record item、schema version、错误类型。
- `config.rs`：`RecordReplayConfig`、feature flags、默认值。
- `paths.rs`：rollout path 生成和合法性校验。
- `policy.rs`：canonical/diagnostic/ephemeral 分类。
- `recorder.rs`：异步 writer、flush、shutdown、retry。
- `reader.rs`：按行读取、容错解析、schema upgrade hook。
- `reconstruct.rs`：从 `RecordLine` 重建 `Vec<Message>`、metadata、pending interactions。
- `index.rs`：SQLite `session_rollouts` 读写、reindex。
- `migration.rs`：legacy JSON session 到 rollout JSONL 的迁移。
- `redaction.rs`：敏感字段过滤和大型 payload 策略。
- `fixtures.rs`：测试夹具生成。

`crates/allthecodes-session/src/lib.rs` 新增：

```rust
pub mod record_replay;
```

## 9. Recorder 设计

`SessionRecorder` API 建议：

```rust
pub enum RecorderOpenMode {
    Create {
        session_id: String,
        cwd: PathBuf,
        created_at: DateTime<Utc>,
        metadata: SessionMetaRecord,
    },
    Resume {
        session_id: String,
        rollout_path: PathBuf,
        next_seq: u64,
    },
}

pub struct SessionRecorderHandle {
    session_id: String,
    rollout_path: PathBuf,
    tx: mpsc::Sender<RecorderCommand>,
}

pub enum RecorderCommand {
    Add(Vec<RecordItem>),
    Persist,
    Flush(oneshot::Sender<Result<RecorderStats>>),
    Shutdown(oneshot::Sender<Result<RecorderStats>>),
}
```

writer 行为：

- `Create` 使用 `OpenOptions::create_new(true)`，避免覆盖旧 log。
- `Resume` 使用 append，并从 reader/index 得到 `next_seq`。
- 内部维护 `next_seq`，调用方只提交 `RecordItem`。
- 每次写入完整一行 JSON 加换行，flush 时刷新 `BufWriter`。
- canonical item 写失败时保留 pending queue，下一轮重试。
- `Flush` 和 `Shutdown` 幂等；多次调用返回同一最终状态。
- channel 建议容量 256。队列压力下只允许丢弃 diagnostic item。
- 可选 `fsync_on_flush`，默认在 shutdown 和关键 turn boundary 开启。
- 每次成功写入后更新 SQLite `session_rollouts.last_seq/event_count/updated_at`，但 index 更新失败不能阻止 JSONL 写入；失败应记录告警并允许后续 reindex。

## 10. Reader 与 Reconstructor

`ReplayReader`：

- 从 `session_rollouts` 查找最新 active rollout。
- 找不到时检查 legacy JSON session。
- 按行读取 JSONL，返回 `Vec<RecordLine>` 或 streaming iterator。
- 坏行不 panic，生成 `ReplayReadWarning`，并继续读取后续完整行。
- schema version 不支持时返回明确错误；兼容旧 schema 时走 upgrade hook。
- 首个 `SessionMeta` 定义 canonical session id；后续 session id 不一致视为 warning/error，按严格模式决定。

`SessionReconstructor`：

- 应用 `SessionMeta` 初始化 metadata。
- 顺序应用 `Message` 事件，得到 `Vec<Message>`。
- 应用 `Rollback` 时把可见历史移动到目标 `seq` 或目标 snapshot。
- 应用 `Branch` 时记录 parent/branch metadata。
- 应用 `Snapshot` 时可快速跳过前序事件，但 debug 模式仍可全量验证 hash。
- 从未完成的 `PermissionRequest`、`QuestionRequest` 重建 pending interaction 状态；默认 resume 时不自动重新询问，先显示 interrupted 状态。
- 返回：

```rust
pub struct ReconstructedSession {
    pub session_id: String,
    pub metadata: SessionMetaRecord,
    pub messages: Vec<Message>,
    pub last_seq: u64,
    pub warnings: Vec<ReplayWarning>,
    pub pending_interactions: Vec<PendingInteraction>,
}
```

## 11. Engine 集成

### 配置

在 `allthecodes-engine` 的 config 中增加：

```rust
pub struct RecordReplayConfig {
    pub enabled: bool,
    pub read_prefer_replay: bool,
    pub include_raw_stream: bool,
    pub include_tool_progress: bool,
    pub fsync_on_turn_finish: bool,
    pub redaction_enabled: bool,
}
```

默认：

- CLI/Web 正常运行：`enabled = true`。
- 单元测试：可保持 `enabled = false`，需要新增专门测试开启。
- `read_prefer_replay = true` 在迁移阶段可以通过 env/config 回退。

### QueryEngine ownership

`QueryEngine` 持有当前 session recorder：

```rust
pub struct QueryEngine {
    ...
    session_recorder: Option<SessionRecorderHandle>,
}
```

新增方法：

- `ensure_session_recorder(&mut self) -> Result<()>`
- `record_items(&self, items: Vec<RecordItem>)`
- `flush_session_record(&self) -> Result<()>`
- `shutdown_session_record(&mut self) -> Result<()>`
- `switch_session_record(&mut self, session_id, cwd, initial_messages) -> Result<()>`

### submit_message 写入点

在 `crates/allthecodes-engine/src/lifecycle/submit_message/mod.rs`：

1. 处理用户输入后，在 push 到 `state.messages` 同一逻辑附近写入：
   - `TurnStarted`
   - processed user/local command messages
2. 发起 provider 请求前写入 `QueryEvent::RequestStart`。
3. `process_stream_item` 产出完整 `Message` 时写入 `Message`。
4. abort、error、usage 汇总时写入 `TurnFinished`。
5. compact/microcompact 发生时写入 `CompactionBoundary` 和 compact 后可见消息。

注意：record 写入必须不阻塞 token streaming。调用方只 enqueue canonical event；flush 只在 turn finish、shutdown、显式保存时等待。

### callbacks

在权限和问题回调处记录：

- permission prompt 发出：`PermissionRequest`
- 用户或 policy 决策完成：`PermissionResponse`
- ask_user prompt 发出：`QuestionRequest`
- 用户回答：`QuestionResponse`
- tool progress：默认只进入 UI；`include_tool_progress` 开启时写 `ToolProgress`

### session switch

以下操作必须先 flush 当前 recorder，再切换 session id：

- `start_new_session`
- `/resume` 或 `CommandResult::SwitchSession`
- Web `rebuild_engine_with_session_id_and_cwd`
- fork/branch

如果 flush 失败，应阻止无提示切换，避免事件写入错误 session。

## 12. CLI/TUI/Web/Daemon 接入

### CLI full init

在 `allthecodes/src/full_init.rs`：

- `--resume`、`--continue` 优先调用 `record_replay::resume_session(session_id)`。
- 找不到 rollout 时 fallback `storage::load_session`。
- fallback 成功后触发 lazy migration，生成 synthetic rollout。
- `QueryEngineConfig.initial_messages` 使用 reconstructed messages。
- 创建 engine 后以 `Resume` 模式打开 recorder。

### Shutdown

在 `allthecodes/src/shutdown.rs`：

1. `engine.flush_session_record()`。
2. `transcript::flush_transcript(session_id)`。
3. `storage::save_session(session_id, &messages, cwd)` 写兼容 snapshot。

如果第 1 步失败，需要明确 log error，并在 UI/CLI 退出提示中暴露。

### TUI

- 启动或切换 session 时用 `ReconstructedSession.messages` 重建 conversation view。
- pending permission/question 显示为 interrupted 状态，不自动重放弹窗。
- 后续可把 diagnostic raw stream 转成 UI replay animation，但不影响恢复正确性。

### Web

在 `allthecodes-web/src/handlers/sessions.rs`：

- `load_session_messages` 改为 replay-first。
- `session_detail_from_messages` 保持原有输出结构。
- `rollback preview` 从 `RecordLine.seq` 和 snapshot 计算变更。
- `rollback` 写入 `Rollback` event，而不是只 truncate JSON。
- `edit/delete/regenerate` 写入 `Branch` 或 `Rollback + TurnStarted` 组合，保留原始历史。

在 `chat_handler`：

- SSE 行为不变。
- submit 结束后等待 turn-level flush 或由 engine 自动 flush。

### Daemon/headless

- daemon 创建 engine 时使用相同 `RecordReplayConfig`。
- 增加内部 API：
  - `GET /sessions/:id/replay`
  - `GET /sessions/:id/events?from_seq=`
  - `POST /sessions/:id/reindex`
- 初期可只实现 crate API，HTTP 暴露放到 Phase 6。

## 13. Transcript、Export 与 Audit

`transcript.rs` 不要立即删除。过渡策略：

- Phase 1-3：继续按现有路径写 transcript。
- Phase 4：新增从 record log 生成 transcript 的函数，验证两者一致。
- Phase 5：把 transcript 作为派生视图，必要时从 replay 重建。

`session_export`：

- 新增 `rollout_path`、`record_schema_version`、`last_seq`。
- export 优先读取 replay，再 fallback 旧 JSON。
- audit export 可以包含 read warnings 和 redaction summary。

## 14. 旧会话迁移

### 懒迁移

当 resume/session detail 找不到 rollout：

1. 读取 `sessions/<session_id>.json`。
2. 生成 synthetic rollout：
   - `SessionMeta { migrated_from: "legacy_json" }`
   - 每条 legacy message 生成 `Message`。
   - 无法完整恢复 typed message 时生成 `LegacyMessage`，保留原始 `SerializableMessage.data`。
   - 末尾写 `Snapshot`。
3. 写入 `session_rollouts`。
4. 保留旧 JSON 文件不动。

### 批量迁移命令

后续新增：

```text
allthecodes session migrate --record-replay
allthecodes session reindex --record-replay
allthecodes session verify --record-replay <session_id>
```

迁移命令必须支持 dry-run：

```text
allthecodes session migrate --record-replay --dry-run
```

### 回退

配置或环境变量：

```text
ALLTHECODES_RECORD_REPLAY=0
ALLTHECODES_RECORD_REPLAY_READ_PREFER_LEGACY=1
```

回退时仍可写旧 JSON snapshot；不要删除 rollout。

## 15. 安全与隐私

record log 会比旧 snapshot 更完整，因此必须有 redaction 策略：

- 默认沿用现有消息持久化范围，不额外写 raw provider payload。
- diagnostic raw stream 默认关闭。
- 对 tool input/output 做可配置 redaction：
  - API key、Authorization、Cookie、private key。
  - 大型 base64/blob 内容。
  - 本地绝对路径是否脱敏按现有配置决定。
- `RecordItem` 增加 `redaction_summary` 或在 line metadata 中保留 redaction 计数。
- 权限 request/response 记录 decision、tool name、必要上下文，不记录完整敏感环境变量。

## 16. 分阶段实施

### Phase 0：基线固定和夹具

改动：

- 增加当前 session JSON、transcript、SQLite projection 的 fixture。
- 为 `storage::load_session`、`resume_session`、Web session detail 建立行为基线测试。
- 记录 `SerializableMessage` 反序列化缺口。

验收：

- `cargo test -p allthecodes-session`
- fixture 覆盖 user/assistant/system/progress/attachment/tool use/tool result。
- 明确哪些 legacy message 只能以 `LegacyMessage` 迁移。

### Phase 1：record_replay crate module

改动：

- 新增 `allthecodes-session::record_replay` 模块。
- 实现 `RecordLine`、`RecordItem`、`RecordReplayConfig`。
- 实现 path helper、policy、redaction skeleton。
- 实现 `SessionRecorder` create/resume/flush/shutdown。
- 实现 `ReplayReader` 基础按行读取。

验收：

- recorder 可以写出合法 JSONL。
- flush/shutdown 幂等。
- 坏行读取不 panic。
- diagnostic item 在配置关闭时不写入。

### Phase 2：engine 写入 canonical events

改动：

- `QueryEngine` 持有 recorder handle。
- `submit_message` 写入 turn/message/request/finish events。
- 权限和 ask_user callback 写入 request/response events。
- shutdown 前 flush record。
- 测试中默认关闭，新增 record-replay integration tests 开启。

验收：

- 新会话产生 rollout JSONL。
- 正常 turn 完成后 log 包含 user message、assistant message、turn finished。
- abort/error turn 也有明确 `TurnFinished`。
- record 写入失败不会污染 `state.messages`，但会返回可观测错误。

### Phase 3：replay-first resume

改动：

- 实现 `SessionReconstructor`。
- `resume_session` 新增 replay-first API。
- CLI `--resume`、`--continue` 使用 replay-first。
- Web `load_session_messages` 使用 replay-first。
- fallback legacy JSON 并触发 lazy migration。

验收：

- 关闭进程后重新打开同一 session，消息与退出前一致。
- 删除或损坏最后半行时可恢复到最后完整行。
- 找不到 rollout 的旧 session 仍可 resume。
- replay warnings 可被 CLI/Web log 到 debug 输出。

### Phase 4：SQLite index、snapshot 和迁移

改动：

- 新增 `session_rollouts` migration。
- recorder 成功写入后更新 index。
- 提供 `reindex_rollouts`。
- `save_session` 保持旧 snapshot，但 metadata 增加 rollout 关联信息。
- `session_export` 优先从 replay 读取。

验收：

- 删除 SQLite 后可从 JSONL reindex。
- 删除 legacy JSON 后，新 session 仍可从 rollout 恢复。
- session list 性能不明显退化。
- export 中包含 `last_seq`、schema version、rollout path。

### Phase 5：TUI/Web replay 状态

改动：

- TUI session switch 从 `ReconstructedSession` 初始化。
- Web session detail 返回 replay warnings 和 last_seq。
- pending permission/question 显示 interrupted 状态。
- transcript 可从 replay 生成。

验收：

- `/resume` 和 Web resume 显示一致。
- 有未完成 permission/question 的会话 resume 后不会误触发工具。
- transcript 重建结果与原写入结果语义一致。

### Phase 6：rollback、branch、edit、regenerate

改动：

- Web rollback preview 基于 seq 实现。
- rollback 写入 `Rollback` event，不破坏旧历史。
- edit/delete/regenerate 写入 `Branch` 或 rollback 后新 turn。
- fork 记录 `Branch { parent_session_id, branch_from_seq }`。

验收：

- rollback 后可见消息变化正确，原历史仍可审计。
- branch session 可追溯 parent 和 seq。
- regenerate 不覆盖原 assistant response。

### Phase 7：性能、压缩和诊断

改动：

- snapshot checkpoint 策略：每 N 条消息或 N MB 写一次 `Snapshot`。
- 可选 gzip/zstd 压缩旧 rollout。
- 大型 payload sidecar。
- diagnostic raw stream 开关。
- `allthecodes session verify --record-replay` 做 hash 和投影校验。

验收：

- 大会话 resume 时间满足目标阈值。
- 压缩后的旧 rollout 仍可读取。
- verify 能发现 seq gap、session id mismatch、坏行和 snapshot hash mismatch。

## 17. 测试矩阵

### Unit tests

- `record_replay::types` serde roundtrip。
- `record_replay::policy` canonical/diagnostic/ephemeral 分类。
- `record_replay::recorder` create、resume、flush、shutdown、retry。
- `record_replay::reader` empty file、bad line、partial last line、schema mismatch。
- `record_replay::reconstruct` message append、rollback、branch、snapshot。
- `migration` legacy JSON 到 synthetic rollout。
- `redaction` secret patterns 和 large blob。

### Integration tests

- CLI new session -> write rollout -> shutdown -> resume。
- `--continue` 指定 session id。
- Web session detail replay-first。
- Web rollback preview/rollback。
- TUI switch session。
- permission request/response 中断恢复。
- compact/microcompact 边界恢复。
- SQLite 删除后 reindex。
- legacy-only session lazy migration。

### 建议命令

```bash
cargo test -p allthecodes-session
cargo test -p allthecodes-engine
cargo test -p allthecodes-web sessions
cargo test -p allthecodes --lib
cargo check -p allthecodes --bin allthecodes
```

如果新增 CLI migration 子命令，还需要补充：

```bash
cargo test -p allthecodes session_migrate
cargo test -p allthecodes session_reindex
```

## 18. 总体验收标准

- 新 session 默认生成 rollout JSONL，并可独立于 legacy JSON 恢复。
- legacy JSON session 仍可读取，并可懒迁移。
- SQLite 只作为索引；删除 SQLite 后可重建。
- replay reader 对坏行、半行、旧 schema 有明确行为。
- CLI、TUI、Web 使用同一 reconstruct API，恢复结果一致。
- shutdown、session switch、rollback 前会 flush canonical events。
- edit/delete/regenerate 不再不可逆破坏原历史。
- diagnostic raw stream 关闭时 log 体积可控。
- record log 中敏感信息写入范围不大于现有 session snapshot，额外 diagnostic 需显式开启。

## 19. 主要风险和处理

| 风险 | 影响 | 处理 |
| --- | --- | --- |
| `SerializableMessage` 反序列化不完整 | legacy 迁移丢 Progress/Attachment 等 | 新 record log 直接存 typed `Message`；legacy 迁移保留 `LegacyMessage.raw_data` |
| 高频事件导致磁盘膨胀 | 大会话性能下降 | 默认只写 canonical；diagnostic 可关闭；checkpoint 和压缩放后续阶段 |
| recorder 异步写失败 | 用户以为保存成功但 log 丢失 | turn finish/shutdown 显式 flush；错误进入 UI/日志；pending retry |
| session switch 写错 session | 历史串线 | switch 前强制 flush/shutdown 旧 recorder，再 open 新 recorder |
| SQLite 与 JSONL 不一致 | session list 错乱 | JSONL 是事实来源；提供 reindex；index 更新失败不阻塞 log |
| 权限/问题恢复误执行 | resume 后重复工具调用 | pending interaction 只显示 interrupted，不自动重放 side effect |
| replay 与 deterministic rerun 混淆 | 需求边界不清 | API 和文档命名区分 `reconstruct`、`replay_timeline`、`rerun` |

## 20. 推荐 PR 切分

1. PR1：新增 `record_replay` types/config/policy/path/reader/recorder 单元测试，不接业务路径。
2. PR2：engine 写入 canonical events，shutdown flush，新会话生成 rollout。
3. PR3：reconstruct 和 replay-first resume，legacy fallback。
4. PR4：SQLite `session_rollouts`、lazy migration、reindex。
5. PR5：Web/TUI session load 接入 replay warnings 和 last_seq。
6. PR6：rollback/branch/edit/regenerate 改为 event-based。
7. PR7：diagnostic stream、checkpoint、压缩、verify 命令。

## 21. 第一批具体文件清单

新增：

- `crates/allthecodes-session/src/record_replay/mod.rs`
- `crates/allthecodes-session/src/record_replay/types.rs`
- `crates/allthecodes-session/src/record_replay/config.rs`
- `crates/allthecodes-session/src/record_replay/paths.rs`
- `crates/allthecodes-session/src/record_replay/policy.rs`
- `crates/allthecodes-session/src/record_replay/recorder.rs`
- `crates/allthecodes-session/src/record_replay/reader.rs`
- `crates/allthecodes-session/src/record_replay/reconstruct.rs`
- `crates/allthecodes-session/src/record_replay/index.rs`
- `crates/allthecodes-session/src/record_replay/migration.rs`
- `crates/allthecodes-session/src/record_replay/redaction.rs`

修改：

- `crates/allthecodes-session/src/lib.rs`
- `crates/allthecodes-session/src/storage.rs`
- `crates/allthecodes-session/src/storage/sqlite_store.rs`
- `crates/allthecodes-session/src/storage/file_store.rs`
- `crates/allthecodes-session/src/resume.rs`
- `crates/allthecodes-session/src/session_export/mod.rs`
- `crates/allthecodes-engine/src/lifecycle/submit_message/mod.rs`
- `crates/allthecodes-engine/src/lifecycle/submit_message/stream_handler.rs`
- `crates/allthecodes-engine/src/lifecycle/types.rs`
- `allthecodes/src/full_init.rs`
- `allthecodes/src/shutdown.rs`
- `allthecodes/src/app_runtime_adapters/ingress.rs`
- `allthecodes-web/src/handlers/sessions.rs`
- `allthecodes-web/src/handlers/chat.rs`

## 22. 最小可交付版本

最小可交付不需要实现 rollback/branch，也不需要 raw stream diagnostic。必须包含：

- JSONL record log 写入。
- replay-first resume。
- legacy JSON fallback。
- shutdown flush。
- SQLite rollout index。
- 单元测试和一个 CLI resume integration test。

达到最小可交付后，record/replay 就已经能替代“只靠完整 JSON snapshot 恢复”的核心链路。后续 rollback、branch、diagnostic replay 都是在同一事实日志上的增量能力。
