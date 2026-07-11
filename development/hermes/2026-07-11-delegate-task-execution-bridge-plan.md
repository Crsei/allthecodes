# DelegateTask 执行桥修订计划

> 修订日期：2026-07-11  
> 修订原因：旧方案把 delegated task 回落到全局 task store，使用 daemon 进程 cwd，预计算并持久化尚未创建的 worktree，同时没有把真实 child transcript、runtime activity 和 restart recovery 绑定到同一 child session。本文原位替代旧方案，不创建 V2 文件。

## 基线与目标

当前基线中的 `DelegateTask` 只创建 metadata envelope：父 session task list 中有一个 pending task，并预存一条 bootstrap child session，但没有启动真实 `QueryEngine` child。

目标是复用现有 `Agent`/`QueryEngine` runtime，让 `DelegateTask` 启动可取消、可追踪、可恢复的后台 child。普通 `Agent` 行为保持兼容；`verification_policy` 仅公开、持久化和转发，策略执行由 runtime verification plan 负责。

## 强制不变量

1. 一个 delegation 只有一个 task、child session、agent identity 和 worktree。
2. delegated runtime 的 `agent_id` 等于 `child_session_id`；普通 Agent 继续自行生成 UUID。
3. `TaskList`、`TaskGet`、`TaskOutput`、`TaskStop` 与 supervisor 的状态、输出和取消必须使用同一个 `{task_list_id, task_id}`。
4. delegated task 禁止回落到 `global_store()`；普通 Agent 使用 default task list。
5. `DelegateTask.cwd` 为空时使用真实 engine cwd，禁止使用 daemon 进程目录。
6. worktree 路径和分支只能由 supervisor 在成功创建后写入 task/session record；DelegateTask 不得持久化预测值。
7. 显式 worktree slug 的路径或分支碰撞必须 fail closed；禁止 `git worktree add -B`。
8. launch failure 已持久化为 `Failed` 后，后台 completion 不得覆盖该终态。
9. 重启不自动恢复本地 delegated child；状态变为 `Interrupted + needs_manual_recovery`，保留 session/worktree 并只追加一次 `/resume <child_session_id>`。

## 数据契约

### ToolUseContext

增加 engine `cwd`，由 `QueryDeps` 的 `QueryEngineConfig.cwd` 填充。所有测试构造器显式提供 cwd。

### Agent hidden envelope

`AgentInput` 增加公开 `verification_policy`，以及仅供内部 deferred dispatch 使用的字段：

- `_delegate_task_id`
- `_delegate_task_list_id`
- `_delegate_session_id`
- `_delegate_cwd`
- `_delegate_worktree_slug`

隐藏字段可反序列化，但不得出现在模型可见 schema。

### Background launch result

```json
{
  "status": "running",
  "agent_id": "<child_session_id>",
  "task_id": "<scoped task id>",
  "child_session_id": "<child session id>",
  "worktree_path": null,
  "worktree_branch": null,
  "message": "..."
}
```

`DelegateTask` 保留既有字段并增加 `agent_id`、`worktree_branch`；成功状态固定为 `running`。

### Runtime activity

`TaskEntry.runtime_activity` 使用 serde default，并在 JSON 与 SQLite 中持久化：

- phase：`queued`、`running`、`waiting_for_permission`、`stalled`、`needs_manual_recovery`、`completed`、`failed`、`cancelled`
- `last_heartbeat_at_ms`、`last_progress_at_ms`
- `task_id`、`agent_id`、`child_session_id`
- `partial_output_bytes`

SQLite migration 增加 `runtime_activity_json`，旧 JSON/SQLite task 必须保持可读。

## 执行架构

```text
DelegateTask
  ├─ resolve effective cwd from input or ToolUseContext.cwd
  ├─ create bootstrap child session
  ├─ create Pending task in parent session task list
  └─ execute_deferred_tool("Agent", hidden scoped envelope)
       └─ Agent supervisor
            ├─ validate envelope and canonicalize cwd
            ├─ prepare optional collision-safe worktree
            ├─ atomically adopt Pending scoped task
            ├─ bind QueryEngine to child_session_id
            ├─ stream output/activity into the same scoped task
            └─ complete/cancel/fail only from InProgress
```

Supervisor 维护运行期 `agent_id -> scoped task reference` 索引。完成后允许调用方读取完整输出；terminal lookup 后清理索引。

## 实施切片

### 1. 文档与基础契约

- 原位重写本文。
- 为 `ToolUseContext` 增加 cwd 并从 engine config 填充。
- 增加 Agent public/hidden 字段和结构化 launch result。
- 测试 hidden schema exclusion、serde round-trip、public verification policy 和 message 兼容字段。

### 2. Scoped task adoption

- 引入 scoped task reference `{task_list_id, task_id}`。
- startup adapter 按 task list ID 解析 store；普通 Agent 使用 default scope。
- `TaskStore` 增加原子 adoption，仅允许 `Pending`。
- 同一临界区写 `InProgress`、agent/supervisor/session identity、实际 worktree、初始 activity。
- cancelled、running、terminal 和重复 adoption 全部拒绝。

### 3. cwd、worktree 与 session

- 非 worktree child 使用 canonicalized delegated cwd。
- worktree 从 delegated cwd 查找 Git root，路径和分支统一为 `agent-worktree-{slug}`。
- 显式 slug 碰撞 fail closed；Git fallback 使用 `worktree add -b`。
- child 首次 submit 前设置 `child_session_id`，启用 `persist_session` 与 `auto_save_session`。
- bootstrap session 仅作为 launch-failure fallback；真实 transcript 在同一 session ID 下成为事实来源。

### 4. Canonical deferred dispatch

- `DelegateTask` 创建 envelope 后调用 `execute_deferred_tool("Agent")`。
- 校验嵌套 result 的 task/session/agent identity 和 `running` 状态。
- executor 缺失、tool error、invalid cwd、worktree failure、malformed result 或 identity mismatch 时：取消 runtime handle，追加可操作错误，task 设为 `Failed`，保留 child session。

### 5. Activity、输出与恢复

- heartbeat 最多每 5 秒持久化一次。
- 5 分钟没有 SDK message、tool activity 或输出进展且不在权限等待时标记 `stalled`；后续进展恢复 `running`。
- permission queue 标记 `waiting_for_permission`，最后一个请求解决后恢复。
- text delta 实时写 task output；没有 delta 的 assistant turn 在完整 message 到达时写一次。
- 终态不重复追加完整答案，只补尚未持久化的诊断/worktree suffix。
- restart recovery 写 `Interrupted + needs_manual_recovery`，保留 worktree/session。
- task JSON、`/tasks` detail 和 dashboard 显示 activity；使用 `AgentEvent::RuntimeActivity`，不新增无消费者 subsystem DTO。

### 6. E2E 与状态收口

- 增加 deterministic no-provider `delegate_task_e2e`，覆盖 success、cancel、launch failure、malformed/mismatch、permission、cwd、worktree cleanup/retention、restart recovery。
- 全部门禁通过后再关闭 HERMES-002；scheduled cwd 和 Web task-list scope 等其他 Hermes 缺口继续开放。

## 失败矩阵

| 失败点 | task | session | worktree | 返回/诊断 |
|---|---|---|---|---|
| executor 缺失 | Failed | 保留 | 无 | canonical runtime unavailable |
| invalid cwd | Failed | 保留 | 无 | 包含无效路径 |
| worktree path/branch collision | Failed | 保留 | 不覆盖 | collision + slug |
| hook/Git 创建失败 | Failed | 保留 | 无法确认时保留 | hook/Git 诊断 |
| adoption 被拒绝 | 原终态不覆盖 | 保留 | 安全清理或保留诊断 | pending-only error |
| malformed/mismatched result | Failed | 保留 | 取消 handle；不确定时保留 | expected/actual identity |
| runtime cancellation | Cancelled | 保留 | changed/不确定则保留 | partial output 可读 |
| process restart | Interrupted | 保留 | 保留 | needs_manual_recovery + `/resume` |

## 验收

- delegated task 只存在于父 session task list；global store 无重复。
- adoption、output、TaskStop 和 supervisor completion 命中同一 store。
- explicit/default cwd 真实进入 child config；child transcript 使用指定 session。
- max turns 与父 permission callback 生效；普通 Agent 行为不变。
- worktree slug 只产生一个路径/分支，碰撞 fail closed。
- partial output 不重复；activity phase、heartbeat、permission 和 restart 状态可序列化并在 UI 可见。
- launch failure 不被异步 completion 覆盖。

最终门禁：

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p allthecodes-tools tasks::tests -- --nocapture
cargo test -p allthecodes-engine agent:: -- --nocapture
cargo test -p allthecodes-tasks -- --nocapture
cargo test -p allthecodes --test delegate_task_e2e -- --nocapture
cargo build --workspace --release
```

完成全部门禁前不推送、不关闭 HERMES-002。
