# Agent Runtime 执行记录字段补齐计划

Status: 已完成；执行记录字段、输出消费者和回归覆盖已落地
Date: 2026-07-02
Review Date: 2026-07-03
Scope: agent runtime 事件、工具执行结果、权限决策、dashboard/headless 输出

## 目标

为 agent runtime 增加一条稳定的结构化“工具执行记录”，覆盖以下字段：

```json
{
  "session_id": "...",
  "agent_role": "build-agent",
  "tool": "shell",
  "command": "npm test",
  "cwd": "...",
  "exit_code": 1,
  "stdout_digest": "...",
  "stderr_digest": "...",
  "retry_count": 0,
  "model": "...",
  "fallback_used": false,
  "permission_decision": "allowed_by_policy"
}
```

新记录用于审计、复现和外部消费，不替代现有 UI 展示事件。现有 `AgentEvent::ToolUse`、`AgentEvent::ToolResult`、dashboard NDJSON 和 headless JSONL 需要保持兼容。

## 当前状态

当前 agent runtime 已稳定产生 agent 生命周期、流式输出、工具调用、权限队列、树快照事件和结构化 `execution_record`。本次实现已把前 3 个阶段的核心数据路径补齐，并按任务要求把 `permission_decision` 提前并入工具执行记录。

截至 2026-07-03 实施完成的部分：

- `crates/allthecodes-types/src/agent_runtime_record.rs` 中的 `AgentRuntimeExecutionRecord` 已稳定输出目标字段；nullable 字段现在序列化为 `null`，不再省略键。
- `crates/allthecodes-types/src/bash_result.rs` 新增 `ShellExecutionOutput`，并通过 `ToolResult.shell` 在通用工具结果层携带 shell 原始执行元数据；`BashResult` 保留为兼容别名。
- Bash 和 PowerShell 执行路径已回填 `command`、`cwd`、`stdout`、`stderr`、`exit_code`、`interrupted`、`termination`、`error`，包括成功、失败、超时、取消、spawn/preflight 错误等路径。
- `ToolExecResult` 已携带 `effective_input`、`duration_ms`、`permission_decision`；`QueryDeps` / `AgentContext` 已贯通 `parent_agent_id`。
- `crates/allthecodes-engine/src/query/loop_impl.rs` 已使用真实 turn 上下文构造记录：`model`、`fallback_used`、`retry_count`、`duration_ms`、`parent_agent_id`、`permission_decision` 都从 runtime 路径传入。
- shell 类工具已规范化为 `tool = "shell"`；常见非 shell 工具也会输出稳定小写 id。
- `stdout_digest` / `stderr_digest` 使用 shell 原始 stdout/stderr 计算 SHA-256 hex digest，不从展示文本或截断 JSON 反解析。
- 权限路径已提前接入核心记录：policy、hook、user prompt、无需权限等结果会映射为 `allowed_by_policy`、`allowed_by_hook`、`allowed_by_user`、`denied_by_policy`、`denied_by_hook`、`denied_by_user`、`not_required`。
- headless legacy `BackendMessage::AgentEvent` 路径可以携带新的 `AgentEvent::ExecutionRecord`；Rust TUI 继续忽略该事件，不影响现有工具展示。
- dashboard NDJSON 已新增 `execution_record` 事件和落盘测试，normalized IPC 已把 legacy `AgentEvent::ExecutionRecord` 映射为 `agent_runtime/execution_record` payload。
- web/API IPC session hub 已接入 agent runtime event channel，root engine 产生的 `ExecutionRecord` 会进入 runtime queue、event log、WebSocket replay 和 normalized consumer 路径。
- 已新增/更新类型 serde、shell runtime record、exec 工具、dashboard、normalized IPC、web IPC bridge 相关测试。

本计划验收项当前已完成：

- 对外 JSON 稳定包含目标字段；不可取得的 shell 专属字段按 nullable 策略输出 `null`。
- 权限矩阵测试覆盖 policy allow、user allow/deny、policy deny、hook allow/deny、无需权限。
- shell digest 回归测试覆盖原始 stdout/stderr、空输出和超长输出裁剪边界。
- headless/protocol、dashboard、normalized IPC、web IPC replay/bridge 均有 `execution_record` fixture 或回归测试。

按实施阶段的当前完成度：

| Phase | 状态 | 说明 |
| --- | --- | --- |
| Phase 1: 类型和序列化 | 已完成 | `AgentRuntimeExecutionRecord`、`AgentEvent::ExecutionRecord`、nullable/default serde 策略和 digest 类型测试已落地。 |
| Phase 2: 上下文打通 | 已完成 | `session_id`、`agent_role`、`model`、`fallback_used`、`retry_count`、`parent_agent_id`、`duration_ms` 已从 runtime/tool 上下文进入记录。 |
| Phase 3: shell 结果提取和 digest | 已完成 | Bash/PowerShell 已暴露结构化 shell 元数据；digest 来自原始 stdout/stderr；核心 runtime 测试已覆盖。 |
| Phase 4: 权限决策整合 | 已完成 | 核心执行路径已回填最终 decision；权限矩阵测试覆盖 policy、hook、user 和 not_required 路径。 |
| Phase 5: 事件输出和消费者兼容 | 已完成 | headless/protocol、dashboard NDJSON、normalized IPC、web/API IPC bridge 均显式支持 `execution_record`；Rust TUI 继续兼容忽略。 |
| Phase 6: 验证和回归 | 已完成 | 类型、runtime record、exec 工具、权限矩阵、长输出 digest、dashboard、normalized IPC、web bridge 回归测试已落地。 |

当前字段来源：

- `model`: query loop 记录本轮实际请求模型；fallback 后记录最终模型。
- `agent_role`: `AgentContext.agent_type` 映射到执行记录的 `agent_role`。
- `tool`: `ToolExecResult.tool_name` 经 runtime 规范化；Bash/PowerShell 统一为 `shell`。
- `permission_decision`: lifecycle tool execution 边界把最终权限结果写入 `ToolExecResult.permission_decision`。
- `exit_code`: Bash/PowerShell 的 `ToolResult.shell.exit_code`。
- `retry_count`: query loop 的本轮 fallback/retry 计数。
- `fallback_used`: query loop 的本轮 fallback 状态。
- `session_id`、`cwd`: 分别来自 `QueryDeps.session_id()` 和 `ToolResult.shell.cwd`。

当前目标固定字段已进入记录构造路径：

- `session_id`
- `agent_role`
- `tool`
- `command`
- `cwd`
- `exit_code`
- `stdout_digest`
- `stderr_digest`
- `retry_count`
- `model`
- `fallback_used`
- `permission_decision`

其中 shell 专属字段在非 shell 工具上按 nullable 策略输出 `null`；`retry_count` 和 `fallback_used` 按模型 turn 上下文输出。

## 目标记录

新增记录类型建议命名为 `AgentRuntimeExecutionRecord`，固定输出这些键。不可取得的字段用 `null` 或默认值表达，避免消费者按事件类型猜字段。

建议字段：

```rust
pub struct AgentRuntimeExecutionRecord {
    pub session_id: String,
    pub agent_id: String,
    pub parent_agent_id: Option<String>,
    pub agent_role: Option<String>,
    pub tool: String,
    pub tool_use_id: Option<String>,
    pub command: Option<String>,
    pub cwd: Option<PathBuf>,
    pub exit_code: Option<i32>,
    pub stdout_digest: Option<String>,
    pub stderr_digest: Option<String>,
    pub retry_count: u32,
    pub model: Option<String>,
    pub fallback_used: bool,
    pub permission_decision: Option<String>,
    pub duration_ms: Option<u64>,
    pub had_error: bool,
    pub schema_version: u32,
}
```

对外 JSON 必须至少包含用户指定的 12 个键；额外字段用于 agent 关联和版本演进。

## 字段来源

`session_id`

- 来源：当前 session / QueryEngine 生命周期上下文。
- 要求：agent spawn、tool execution、dashboard/headless 输出使用同一个 session id。

`agent_role`

- 来源：优先使用 agent spawn 时的 `agent_type` 或角色配置。
- 命名：对外字段保持 `agent_role`，内部可继续使用 `agent_type`。
- 主 agent 无显式 role 时可输出 `null`，不要伪造。

`tool`

- 来源：工具调用名。
- 规范化：shell 类工具统一输出 `shell`；非 shell 工具输出稳定小写工具 id。

`command`

- 来源：shell 工具输入中的 `command` 或等价字段。
- 非 shell 工具输出 `null`。
- 不从展示文本反解析命令。

`cwd`

- 来源：工具执行上下文最终工作目录。
- 要记录实际执行目录，不是用户输入的相对目录。

`exit_code`

- 来源：Bash/PowerShell 等进程工具的结构化结果。
- 非进程工具输出 `null`。

`stdout_digest` / `stderr_digest`

- 来源：进程工具原始 stdout/stderr。
- 只记录 digest，不写入完整 stdout/stderr。
- digest 算法需要稳定并写入文档。优先复用现有 hash/audit 组件；若必须新增依赖，使用 workspace 统一依赖并说明原因。

`retry_count`

- 来源：模型调用恢复路径的 retry 计数。
- 对纯工具执行记录，若没有发生模型级 retry，输出 `0`。
- 不把 shell 命令内部 retry 或用户脚本 retry 混入该字段。

`model`

- 来源：agent spawn/runtime 使用的模型。
- fallback 后若模型发生变化，记录最终实际执行该轮的模型，并通过 `fallback_used` 表示发生过 fallback。

`fallback_used`

- 来源：模型选择/请求 fallback 路径。
- 默认 `false`。
- 只有 runtime 实际切换模型或 provider 时置为 `true`。

`permission_decision`

- 来源：权限系统最终有效决策。
- 需要区分 policy、hook、user prompt 等来源，例如 `allowed_by_policy`、`allowed_by_user`、`denied_by_policy`。
- 如果工具无需权限检查，输出 `null` 或明确的 `not_required`，最终取值需要在类型文档中固定。

## 落地设计

1. 在共享类型层增加记录类型

   位置优先级：

   - `crates/allthecodes-types/src/agent_events.rs`
   - 或新增 `crates/allthecodes-types/src/agent_runtime_record.rs`

   同时为 headless/API 需要的协议类型补齐 serde 序列化测试。

2. 扩展 agent runtime 事件

   在 `AgentEvent` 中增加新 variant：

   ```rust
   ExecutionRecord {
       record: AgentRuntimeExecutionRecord,
   }
   ```

   老客户端可以忽略未知 kind；新客户端按 `kind = "execution_record"` 消费。

3. 在工具执行完成点构造记录

   重点检查：

   - `crates/allthecodes-engine/src/agent_runtime.rs`
   - `crates/allthecodes-engine/src/agent/mod.rs`
   - `crates/allthecodes-engine/src/agent/supervisor.rs`
   - shell 工具执行模块
   - lifecycle/tool callback 里携带的 `ToolUseContext`

   构造记录必须发生在工具有最终结果之后，这样可以同时拿到 command、cwd、exit_code、digest、permission decision 和 duration。

4. shell 工具结果结构化

   Bash/PowerShell 的 stdout/stderr/exit code 需要以类型化结果传递给 runtime，不允许 runtime 从渲染文本或截断摘要中反解析。

5. 权限决策回填

   当前 permission queued/resolved 是独立事件。需要按 `agent_id + tool_use_id` 在执行记录构造时回填最终 decision。

   如果工具执行发生在无需审批路径，也要产出明确的默认值，避免 consumer 误判为数据丢失。

6. retry/fallback 上下文传递

   retry/fallback 状态来自模型请求层，不在工具本身。需要在每轮 query/agent context 中保存当前值，工具记录生成时读取。

   注意不要把多个 agent 的 fallback 状态混用；agent id 必须参与隔离。

7. 输出通道接入

   需要覆盖：

   - headless JSONL
   - dashboard NDJSON
   - web/API 事件流
   - Rust TUI 可选展示

   UI 不一定马上展示全部字段，但数据通道必须完整。

## 实施阶段

### Phase 1: 类型和序列化

- 增加 `AgentRuntimeExecutionRecord`。
- 增加 `AgentEvent::ExecutionRecord`。
- 写 serde round-trip 测试，确认 12 个目标键稳定存在。
- 文档记录 nullable/default 策略。

### Phase 2: 上下文打通

- 将 `session_id`、`agent_role`、`model`、`fallback_used`、`retry_count` 放入 agent/tool 执行上下文。
- 确认 parent agent 和 background agent 不丢上下文。
- 不改变现有 spawn/completed 事件行为。

### Phase 3: shell 结果提取和 digest

- 为 Bash/PowerShell 工具暴露结构化 stdout/stderr/exit code。
- 对 stdout/stderr 计算稳定 digest。
- 添加成功、失败、空 stdout、空 stderr、超长输出测试。

### Phase 4: 权限决策整合

- 统一 permission decision 枚举到对外字符串。
- 在工具记录中回填最终 decision。
- 测试 policy allow、user allow、deny、无需权限四类路径。

### Phase 5: 事件输出和消费者兼容

- headless JSONL 输出 `execution_record`。
- dashboard NDJSON 增加对应 event kind 或 payload。
- web/API 协议支持新事件。
- Rust TUI 忽略或轻量展示该事件，不能破坏现有工具展示。

### Phase 6: 验证和回归

- 单元测试覆盖类型、权限映射、shell digest。
- IPC/headless fixture 覆盖新事件。
- agent runtime 集成测试覆盖一次失败的 shell 命令，例如 `npm test` exit code 1。
- 最后执行 workspace build，处理新增 warning。

## 验收标准

- agent 执行 shell 命令后，runtime 能产生一条包含以下键的记录：
  `session_id`、`agent_role`、`tool`、`command`、`cwd`、`exit_code`、`stdout_digest`、`stderr_digest`、`retry_count`、`model`、`fallback_used`、`permission_decision`。
- shell 命令失败时，`exit_code` 保留真实退出码，`had_error` 为 true，stdout/stderr digest 仍可用。
- 非 shell 工具也能产生记录，无法取得的 shell 专属字段为 `null`。
- 权限拒绝时也有记录，且 `permission_decision` 能表达拒绝来源。
- 老的 `ToolUse`、`ToolResult`、permission 事件和 agent tree 事件保持兼容。
- 新记录不泄漏完整 stdout/stderr，只写 digest 和必要元数据。

## 风险

- 现有工具结果可能只有展示文本，必须先补结构化结果，不能靠字符串解析。
- retry/fallback 属于模型层状态，和工具执行不是同一层，需要小心上下文生命周期。
- permission decision 目前分散在不同路径，直接拼接字符串容易产生不一致取值。
- digest 算法如果未来切换，会影响审计可复现性，因此第一版要固定算法和格式。

## 待确认问题

- 这条记录是否需要进入持久 session transcript，还是只进入 runtime/dashboard 审计流。
- `agent_role` 是否应完全替代对外的 `agent_type`，还是仅作为执行记录别名。

已确认并落地的决策：

- `permission_decision` 对“无需权限”使用 `not_required`。
- digest 算法固定为 SHA-256 hex。
- shell 原始 stdout/stderr 放在通用 `ToolResult.shell`，不放在 `ToolExecResult`。
