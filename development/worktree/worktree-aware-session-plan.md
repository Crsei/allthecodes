# Worktree-aware Session 实现计划

Status: 已实现
Date: 2026-07-02
Completed: 2026-07-03
Scope: worktree runtime、session storage、task metadata、agent isolation、Web/API/TUI 展示

当前实现状态：

- `WorktreeSessionRecord`、SQLite store、migration、repo/session/goal 查询和 orphan reconciliation 已落地。
- `EnterWorktree` / `ExitWorktree` 和 Agent worktree isolation 会写入并更新 worktree session records。
- Web/API 已提供 `/api/worktree-sessions`、`/api/worktree-sessions/current`、`/api/worktree-sessions/{session_id}`。
- 启动时会执行 best-effort reconciliation；API 查询也会对返回记录做 orphan reconciliation。
- UI 侧当前保持只读/状态展示，不自动 resume/chdir 到历史 worktree。

## 目标

为 allthecodes 增加稳定的 worktree-aware session 记录，使一个 session、一个仓库、一个 worktree、一个目标或任务之间的关系可以被恢复、查询、展示和审计。

目标对外语义接近：

```ts
type WorktreeSession = {
  sessionId: string;
  repoId: string;
  worktreePath: string;
  branch: string;
  baseCommit: string;
  linkedGoalId?: string;
  createdBy: "human" | "agent";
};
```

Rust 侧建议扩展为版本化记录，避免后续 schema 迁移困难：

```rust
pub struct WorktreeSessionRecord {
    pub schema_version: u32,
    pub session_id: String,
    pub repo_id: String,
    pub worktree_path: PathBuf,
    pub branch: String,
    pub base_commit: Option<String>,
    pub linked_goal_id: Option<String>,
    pub created_by: WorktreeSessionCreator,
    pub git_root: PathBuf,
    pub original_cwd: Option<PathBuf>,
    pub status: WorktreeSessionStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub kept_at: Option<DateTime<Utc>>,
    pub removed_at: Option<DateTime<Utc>>,
    pub source: WorktreeSessionSource,
}
```

`repo_id` 第一版使用现有 `allthecodes_session::storage::workspace_key()`，因为它已经把同一 git repo 的多个 worktree 归到同一个 key。后续如果需要远端仓库 ID，可在 `repo_id` 之外新增 `remote_repo_id`，不要改变已持久化语义。

## 非目标

- 不重写现有 `EnterWorktree` / `ExitWorktree` 工具行为。
- 不要求第一版实现跨机器同步 worktree 状态。
- 不把 task store 的 `worktree_path` / `worktree_branch` 当成唯一事实来源。
- 不把 Git worktree 列表 API 当成 session 记录。Git 列表只能说明磁盘状态，不能说明 session 归属、目标归属和创建者。

## 当前基线

已有实现：

- `crates/allthecodes-worktree/src/tool.rs` 有进程内 `WorktreeSession`，字段为 `worktree_path`、`branch_name`、`original_cwd`、`git_root`、`original_head_commit`。
- `EnterWorktree` 会创建 worktree 并写入进程全局 `CURRENT_SESSION`，`ExitWorktree` 会 keep/remove 并清空该状态。
- `crates/allthecodes-engine/src/agent/worktree.rs` 支持 Agent 工具 `isolation: "worktree"`，会为子 agent 创建临时 worktree，结束后按是否有变更决定清理或保留。
- `crates/allthecodes-tasks/src/types.rs`、`sqlite.rs` 已持久化 `isolation`、`worktree_path`、`worktree_branch`。
- `crates/allthecodes-session/src/storage.rs` 的 `workspace_key()` 已对 git worktree 使用 git common directory，使同一 repo 的多个 worktree 共享 workspace key。
- Web Git API 已有 `/api/git/worktrees`，可列出当前 repo 的 worktree path、branch、head_sha、dirty 状态。

关键缺口：

- 没有以 `session_id` 为主键的 worktree session 持久记录。
- 没有 `repo_id` 字段；当前只能从 `workspace_key()` 推导。
- 没有 `linked_goal_id`，尽管 goal runtime 已有 `goal_id`。
- 没有规范化 `created_by = human | agent`；当前工具结果只有展示字符串 `created_by: "git worktree"` 或 `"WorktreeCreate hook"`。
- Agent worktree isolation 的 kept/cleaned 结果只通过文本和 subagent event 暴露，没有进入 session storage。
- 当前 root-owned `allthecodes-worktree` 仍依赖进程全局状态，不能恢复已存在 session 的 active worktree。

## 目标模型

### 枚举

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorktreeSessionCreator {
    Human,
    Agent,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorktreeSessionStatus {
    Active,
    Kept,
    Removed,
    CleanupFailed,
    Orphaned,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorktreeSessionSource {
    EnterWorktree,
    AgentIsolation,
    WorktreeCreateHook,
    Imported,
}
```

### 字段来源

| 字段 | 来源 |
| --- | --- |
| `session_id` | 当前 QueryEngine / audit context session id |
| `repo_id` | `allthecodes_session::storage::workspace_key(&git_root)` |
| `worktree_path` | `EnterWorktree` 或 Agent isolation 创建出的 path |
| `branch` | `branch_name` |
| `base_commit` | 当前 `original_head_commit` |
| `linked_goal_id` | 当前 active goal 的 `goal_id`，没有则 `None` |
| `created_by` | `EnterWorktree` 为 `Human`；Agent isolation 为 `Agent` |
| `git_root` | 创建时解析出的 repo root |
| `original_cwd` | 交互式 `EnterWorktree` 进入前 cwd；Agent isolation 可为 parent cwd |
| `status` | create/keep/remove/cleanup failure 更新 |

## 持久化设计

第一版放在 `allthecodes-session`，因为该记录以 `session_id`、workspace/repo、恢复和列表查询为核心，不应只属于工具 crate。

建议新增：

- `crates/allthecodes-session/src/worktree_sessions.rs`
- `crates/allthecodes-session/src/worktree_sessions/types.rs`
- `crates/allthecodes-session/src/worktree_sessions/store.rs`

SQLite 表建议放在 session/state DB，而不是 task DB：

```sql
CREATE TABLE IF NOT EXISTS worktree_sessions (
  session_id TEXT NOT NULL,
  repo_id TEXT NOT NULL,
  worktree_path TEXT NOT NULL,
  branch TEXT NOT NULL,
  base_commit TEXT,
  linked_goal_id TEXT,
  created_by TEXT NOT NULL,
  git_root TEXT NOT NULL,
  original_cwd TEXT,
  status TEXT NOT NULL,
  source TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  kept_at TEXT,
  removed_at TEXT,
  schema_version INTEGER NOT NULL,
  PRIMARY KEY (session_id, worktree_path)
);

CREATE INDEX IF NOT EXISTS idx_worktree_sessions_repo
  ON worktree_sessions(repo_id, updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_worktree_sessions_goal
  ON worktree_sessions(linked_goal_id, updated_at DESC);
```

如果 record/replay canonical log 先完成，应同步写入 `RecordItem::WorktreeSessionStarted` / `WorktreeSessionUpdated`。如果还未完成，先用 SQLite + JSON snapshot，后续迁移为 record log 派生索引。

## 运行时接入

### Phase 1: 类型和存储

1. 新增 `WorktreeSessionRecord` 类型和 serde 测试。
2. 新增 store API：
   - `upsert_worktree_session(record)`
   - `mark_worktree_session_kept(session_id, worktree_path)`
   - `mark_worktree_session_removed(session_id, worktree_path)`
   - `mark_worktree_session_cleanup_failed(session_id, worktree_path)`
   - `list_worktree_sessions_for_repo(repo_id)`
   - `get_active_worktree_session(session_id)`
3. 给 session SQLite 增加 migration 和重建逻辑。
4. 明确 `repo_id` 使用 `workspace_key()`，并加 worktree/common-dir 单元测试。

验收：

- 类型 JSON 能输出 `sessionId`/`repoId` camelCase 或协议层可转换。
- 同一 repo 的 main worktree 和 linked worktree 生成相同 `repo_id`。
- 表 migration 可重复运行。

### Phase 2: EnterWorktree/ExitWorktree 写入记录

1. 在 `EnterWorktreeTool::call()` 成功创建后写入 `WorktreeSessionRecord`。
2. `created_by = Human`。
3. `source = WorktreeCreateHook` 或 `EnterWorktree`，其中 hook 只表示创建机制，不改变 `created_by`。
4. `ExitWorktree` 的 keep/remove/失败路径更新记录 status。
5. 现有进程全局 `CURRENT_SESSION` 保留，但从“唯一状态”降级为当前进程快速状态。

验收：

- `EnterWorktree` 后能通过 store 查到 active record。
- `ExitWorktree { action: "keep" }` 后 status 变 `kept`。
- `ExitWorktree { action: "remove" }` 成功后 status 变 `removed`。
- remove 失败或无法验证时 status 不误标为 removed。

### Phase 3: Agent isolation 写入记录

1. 在 `AgentTool::run_in_worktree()` 创建 worktree 后写入 record。
2. `created_by = Agent`。
3. `session_id` 使用父 session id；如果未来子 agent 有独立 persisted session，再加 `agent_session_id`，不要复用 `session_id` 语义。
4. worktree 自动清理成功时标 `removed`。
5. worktree 因存在变更而保留时标 `kept`，并把 path/branch 写入 task or agent event metadata。

验收：

- `Agent` 输入 `isolation: "worktree"` 后产生 worktree session record。
- 无变更清理成功后 record 为 `removed`。
- 有变更保留后 record 为 `kept`，并能按 `repo_id` 列出。

### Phase 4: linked goal 接入

1. 增加 helper 从当前 session goal runtime 读取 active `goal_id`。
2. `EnterWorktree` 创建时，如果 active goal 存在，写入 `linked_goal_id`。
3. Agent isolation 创建时同样继承 active goal。
4. 不强制 goal 存在；无 goal 时保持 `None`。

验收：

- 有 active goal 时创建 worktree record 带 `linked_goal_id`。
- goal 完成/暂停不自动改写 worktree status。
- 查询 goal 能找到相关 worktree sessions。

### Phase 5: API 和 UI

建议新增 protocol 类型：

- `crates/allthecodes-protocol/src/v1/worktree_sessions.rs`

建议新增 Web endpoints：

- `GET /api/worktree-sessions?repo_id=...`
- `GET /api/worktree-sessions/current`
- `GET /api/worktree-sessions/{session_id}`

响应字段使用 camelCase：

```json
{
  "sessionId": "...",
  "repoId": "...",
  "worktreePath": "...",
  "branch": "...",
  "baseCommit": "...",
  "linkedGoalId": "...",
  "createdBy": "agent",
  "status": "kept"
}
```

UI 第一版只做只读展示：

- status line 继续显示当前 active worktree。
- Web workspace/session detail 展示相关 worktree sessions。
- Task detail 若已有 `worktree_path`，链接到对应 record。

验收：

- Web 能列出当前 repo 的 worktree session records。
- 只读 API 不依赖当前进程全局状态，重启后仍可查询。
- 旧 `/api/git/worktrees` 行为不变。

### Phase 6: 恢复和孤儿检测

1. 启动时扫描 active/kept worktree records。
2. 如果 path 不存在，标 `orphaned` 或 `removed`，具体规则：
   - status `active` 但 path 缺失：`orphaned`
   - status `kept` 但 path 缺失：`orphaned`
3. 如果 path 存在但 git worktree list 已无记录，标 `orphaned`。
4. 恢复 session 时，如果 active record 存在，可提示/展示当前 session 曾进入 worktree，但不要自动 `chdir`，除非后续明确实现 resume-worktree 行为。

验收：

- 重启后可以查询历史 worktree sessions。
- 磁盘 worktree 被手动删除后不会误判为 active。
- Resume session 不会悄悄切 cwd 到旧 worktree。

## 与现有模块的关系

| 模块 | 改动 |
| --- | --- |
| `allthecodes-worktree` | 保留工具 runtime，创建/退出时调用 session store |
| `allthecodes-engine/src/agent/worktree.rs` | agent isolation 创建/清理时写入 record |
| `allthecodes-session` | 新增持久类型、store、migration、查询 API |
| `allthecodes-tasks` | 继续保留 task 级 `worktree_path`/`worktree_branch`；可选关联 record |
| `allthecodes-web` | 新增只读 worktree session handlers |
| `allthecodes-protocol` | 新增 v1 DTO，使用 camelCase |
| `allthecodes/src/ui/status` | 继续当前 active 状态展示，后续可读取 persisted current |

## 迁移策略

第一版不 backfill 历史会话。原因是现有历史数据无法可靠区分：

- worktree 是 human 创建还是 agent 创建；
- branch 的 base commit；
- worktree 是否仍与某个 session 有归属关系；
- 是否关联 goal。

可做轻量 backfill：

- 对当前进程 active `CURRENT_SESSION`，首次写入新 store。
- 对 Task 里已有 `worktree_path`/`worktree_branch` 的记录，只在 UI 中显示“task-level worktree metadata”，不伪造 `WorktreeSessionRecord`。

## 测试计划

单元测试：

- `workspace_key()` 对 main repo 与 linked worktree 返回相同 repo id。
- `WorktreeSessionRecord` serde round-trip。
- status transition：active -> kept、active -> removed、active -> cleanup_failed。
- `created_by` 只接受 `human` / `agent`。

集成测试：

- `EnterWorktree` 创建 record。
- `ExitWorktree keep/remove` 更新 record。
- Agent `isolation: "worktree"` 创建并更新 record。
- cleanup failure 不清空 persisted active 状态。
- Web API 重启后仍能列出 record。

命令：

```bash
cargo test -p allthecodes-session worktree
cargo test -p allthecodes-worktree worktree
cargo test -p allthecodes-engine agent::worktree
cargo test -p allthecodes-web worktree
cargo build --workspace --release
```

## 风险

- 当前 `allthecodes-worktree` 仍是 root-owned runtime，直接依赖 `CURRENT_SESSION`。持久 store 不应反向依赖 root crate，否则 crate 边界会恶化。
- Agent isolation 自动清理路径必须 fail-closed。不能因为记录更新失败而误删或误标。
- `repo_id = workspace_key()` 是本地身份，不等于远端 GitHub repo id。对外文档必须说清楚。
- `linked_goal_id` 只能表示创建时关联，不表示 worktree 完成了 goal。
- `session_id` 对 Agent isolation 第一版建议使用父 session id，避免引入未持久化的子 session 概念。

## 完成标准

- `WorktreeSessionRecord` 类型、store、migration、API 和测试落地。
- `EnterWorktree` / `ExitWorktree` 和 Agent isolation 都会写入并更新记录。
- 重启后仍能按 `repo_id` 和 `session_id` 查询 worktree sessions。
- Web/API 输出包含 `sessionId`、`repoId`、`worktreePath`、`branch`、`baseCommit`、`linkedGoalId`、`createdBy`。
- 现有 `EnterWorktree`、`ExitWorktree`、Task worktree metadata、`/api/git/worktrees` 行为不回退。
- `cargo build --workspace --release` 通过且无新增 warning。
