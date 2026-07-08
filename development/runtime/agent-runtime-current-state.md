# Agent Runtime 当前实现情况

日期：2026-07-04

范围：Agent/Task 工具、同步/后台 subagent、Agent Teams teammate spawn、worktree 隔离、agent tree、IPC/dashboard 事件、工具执行记录和用户交互面。

## 功能概述

Agent runtime 是运行时委派系统。主会话可以通过 `Agent` 或上游兼容别名 `Task` 启动子 `QueryEngine`，让子 agent 用独立上下文执行复杂任务，再把结果返回给父会话或后台任务队列。

当前能力包括：

- 同步 subagent：父会话等待子 agent 完成，工具结果直接回到下一轮模型上下文。
- 后台 subagent：父会话立即得到启动确认，子 agent 继续运行，完成后在父会话 turn boundary 注入系统消息。
- Agent Teams teammate：传入 `name` 时不走普通 subagent，而是通过 in-process teammate/team runtime 创建可被 `SendMessage` 路由的成员。
- worktree 隔离：传入 `isolation = "worktree"` 时为 agent 创建 Git worktree，并在完成时按变更情况保留或清理。
- agent tree：维护 agent 节点、父子关系、状态、深度、模型、结果预览和耗时。
- 流式事件转发：子 engine 的文本、thinking、工具使用、工具结果、权限排队/完成、树快照等事件会转成 `AgentEvent` 发给 IPC/Web/TUI 消费方。
- 结构化执行记录：每个工具执行完成后输出 `AgentRuntimeExecutionRecord`，供审计、dashboard、headless/API 消费。

Agent runtime 不替代普通 query loop。它复用 `QueryEngine`、工具注册、权限治理、hook、MCP binding、record/replay、usage/cost 等主路径，只是在 agent 上下文、生命周期管理和事件输出上加了一层运行时封装。

## 用户交互

模型侧主要通过工具调用进入 runtime：

- `Agent`：allthecodes 历史工具名。
- `Task`：上游兼容别名，执行同一套 `AgentTool` runtime。

用户可见的交互表面包括：

- 普通同步调用：用户看到主 assistant 后续总结；子 agent 的完整结果作为工具结果回到父模型，不直接作为最终用户消息展示。
- 后台调用：工具先返回 “launched in background” 文本，完成后 query loop 在下一轮开始前注入后台 agent 完成/失败系统消息。
- 权限交互：后台 agent 的权限请求会发出 `PermissionQueued`，用户响应后发出 `PermissionResolved`；同步 agent 复用普通工具权限回调。
- 取消与查询：IPC `AgentCommand` 支持 `AbortAgent`、`QueryActiveAgents`、`QueryAgentOutput`。
- 输出读取：后台任务输出进入 task store，支持按 `after_seq` 和 `limit_bytes` 批量读取。
- 树状态展示：`Spawned`、`Completed`、`TreeSnapshot` 让 Web/TUI 可以展示正在运行、完成、失败、取消的 agent 树。

普通用户不需要直接操作执行记录；`ExecutionRecord` 是机器消费事件，和 `ToolUse`/`ToolResult` 并行存在，主要用于审计、复现、dashboard 和外部集成。

## 实现架构

核心分层如下：

1. 工具入口

   `crates/allthecodes-engine/src/agent/tool_impl.rs` 实现 `AgentTool` 和 `TaskAgentTool`。入口负责解析参数、检查递归深度、解析模型、应用 agent definition 默认值、判断 teammate spawn、同步/后台/worktree 路径分发。

2. 子 engine 配置

   `crates/allthecodes-engine/src/agent/mod.rs` 的 `build_child_config()` 创建子 `QueryEngineConfig`。它设置子 cwd、工具集合、模型、fallback model、max turns、预算、agent context、权限上下文、MCP binding context 和 team context。

3. 同步执行

   `crates/allthecodes-engine/src/agent/dispatch.rs` 创建子 `QueryEngine`，调用 `submit_message(QuerySource::Agent(agent_id))`，通过 `collect_stream_result()` 收集文本结果，同时把子流式消息映射为 `AgentEvent`。

4. 后台执行

   `crates/allthecodes-engine/src/agent/supervisor.rs` 的 `BackgroundSupervisor` 持有活跃后台 job、取消 token、task id、join handle 和 worktree runtime。`AgentRuntime::run()` 拥有后台子 engine 的完整生命周期。

5. 全局 runtime adapter

   `crates/allthecodes-engine/src/agent_runtime.rs` 提供全局 adapter：dashboard emitter、agent tree runtime、builtin agent registry、agent tool registry、teammate spawner、task store。默认实现是 no-op/in-memory，启动层会安装真实实现。

6. 工具执行边界

   `crates/allthecodes-engine/src/lifecycle/deps/execute.rs` 是 query loop 的统一工具执行边界。它在真正调用工具前后统一处理输入校验、安全校验、pre/post hook、权限决策、审计、record/replay、duration 和 runtime metadata。

7. 执行记录输出

   `crates/allthecodes-engine/src/query/loop_impl.rs` 在工具结果转成 user tool_result 后构造 `AgentRuntimeExecutionRecord`，再发送到 dashboard emitter 和 `AgentEvent::ExecutionRecord`。

## 核心参数

`Agent`/`Task` 工具输入参数：

| 参数 | 类型 | 作用 |
| --- | --- | --- |
| `prompt` | string | 子 agent 要执行的任务；必填。 |
| `description` | string | 3-5 个词的任务描述；必填，用于 UI、task、agent tree、事件。 |
| `subagent_type` | string | agent 类型/角色；默认 `general-purpose`，可匹配内置或插件 agent definition。 |
| `model` | string | 子 agent 模型覆盖；支持 `SOTA`、`MOTA`、`FOTA`、`inherit` 和完整模型 ID。 |
| `run_in_background` | bool | 是否后台运行；默认 `false`。 |
| `name` | string | 存在时走 Agent Teams teammate spawn，而不是普通 subagent。 |
| `team_name` | string | teammate 所属 team；未设置时使用 active team 或隐式 session team。 |
| `mode` | string | teammate permission mode，可选 `default`、`auto`、`bypass`、`plan`、`acceptEdits`、`dontAsk`。 |
| `isolation` | string | 当前只支持 `worktree`，用于 Git worktree 隔离。 |

运行时关键参数和默认值：

- `MAX_AGENT_DEPTH = 5`：限制嵌套 agent 深度，避免无限递归。
- `agent_id`：每次普通/后台 subagent 启动时生成 UUID。
- `parent_agent_id`：来自父 `ToolUseContext.agent_id`，用于树关系和执行记录。
- `chain_id`：同一 agent 链路共享的跟踪 id；没有父链路时新建。
- `agent_model`：优先级为显式 `model`、环境 `CLAUDE_MODEL`、父模型；agent definition 默认值会先写回输入参数。
- `fallback_model`：子 engine 配置中设为父模型，用于模型请求 fallback。
- `persist_session = false`、`auto_save_session = false`：子 agent 不作为独立普通会话自动持久化。
- `max_result_size_chars = 200000`：Agent 工具结果最大字符数。
- `SHUTDOWN_WAIT_PER_AGENT = 5s`：进程关闭时等待后台 agent 结束的单 agent 超时。
- `ALLTHECODES_ALLOW_WORKTREE_FALLBACK` / `CC_RUST_ALLOW_WORKTREE_FALLBACK`：worktree 创建失败时是否允许降级到普通 cwd。

执行记录核心字段见 `AgentRuntimeExecutionRecord`：

- session/agent：`session_id`、`agent_id`、`parent_agent_id`、`agent_role`。
- 工具：`tool`、`tool_use_id`、`command`、`cwd`、`exit_code`。
- 输出摘要：`stdout_digest`、`stderr_digest`，算法为 SHA-256 hex。
- 模型上下文：`retry_count`、`model`、`fallback_used`。
- 权限和耗时：`permission_decision`、`duration_ms`、`had_error`、`schema_version`。

## 数据流

同步 agent 数据流：

1. 父模型发出 `Agent`/`Task` 工具调用。
2. `execute_tool_impl()` 完成普通工具前置流程：校验、hook、权限、安全检查。
3. `AgentTool::call()` 解析参数，生成 `agent_id`，决定普通、worktree、teammate 或后台路径。
4. 同步路径创建子 `QueryEngineConfig`，设置 `AgentContext` 和子工具集合。
5. 子 `QueryEngine` 用 `QuerySource::Agent(agent_id)` 执行 prompt。
6. 子流式消息经 `sdk_to_agent_event()` 转成 `StreamDelta`、`ThinkingDelta`、`ToolUse`、`ToolResult` 等事件。
7. 子 agent 完成后更新 agent tree，发出 `Completed` 和 `TreeSnapshot`。
8. 父工具调用返回 `ToolResult`，query loop 把它转成 user tool_result 进入下一轮模型上下文。
9. query loop 构造并发送 `ExecutionRecord`。

后台 agent 数据流：

1. `run_in_background = true` 且存在 `bg_agent_tx` 时进入 supervisor。
2. `spawn_background_agent()` 准备 runtime、可选 worktree、task store entry、取消 token、agent tree 节点。
3. 父工具调用立即返回启动确认文本。
4. tokio task 中的 `AgentRuntime::run()` 创建子 engine 并执行 prompt。
5. 子 engine 中间事件持续写入 agent IPC channel；权限请求会额外产生 `PermissionQueued` / `PermissionResolved`。
6. 完成、失败或取消后写 task output/status，更新 agent tree，发 `background_complete` dashboard event、`Completed` 和 `TreeSnapshot`。
7. 父 query loop 后续 turn 开始时调用 `drain_background_results()`，把结果注入为 system informational message。

执行记录数据流：

1. 工具调用完成后形成 `ToolExecResult`。
2. `ToolExecResult` 携带 `effective_input`、`duration_ms`、`permission_decision` 和 `ToolResult.shell`。
3. query loop 的 `build_execution_record()` 从 deps、turn context、tool result 和 shell output 组装记录。
4. 记录同时进入 dashboard emitter 和 `AgentEvent::ExecutionRecord`。
5. dashboard NDJSON、normalized IPC、WebSocket replay/headless consumer 可以按结构化事件消费。

## 关键设计决策

- 子 agent 复用 `QueryEngine`，不实现第二套模型调用循环。这样工具、权限、MCP、hook、压缩、usage、fallback 行为与主循环保持一致。
- `Agent` 和 `Task` 是同一 runtime 的两个名字。`Task` 用于上游兼容，不引入分叉行为。
- 工具执行记录在 query loop 里统一生成，而不是每个工具自己发。这样 shell、文件、MCP、插件、agent 工具都走同一个 schema。
- `ToolUse`/`ToolResult` 继续服务 UI，`ExecutionRecord` 服务审计和外部消费；两者并行，避免破坏老客户端。
- 后台 agent 必须由 supervisor 管理。它集中拥有取消、shutdown、task output、worktree 清理和 active job registry，避免 detached task 无法收尾。
- 权限决策在工具执行边界回填到 `ToolExecResult`。执行记录不重新推断权限来源，避免 hook/user/policy 标签不一致。
- shell stdout/stderr 只记录 digest，不进入执行记录正文。完整输出仍走既有工具结果和 task output 通道，执行记录保持可审计但不扩大泄漏面。
- worktree 隔离失败默认是硬失败；只有显式环境变量允许降级。这样不会静默把需要隔离的后台写操作落到主工作区。
- 子 agent 默认不持久化成普通会话。它继承父 session id 用于审计和事件关联，但不污染普通会话列表。
- agent definition 可以限制工具和 MCP server。内置 definition 默认不会自动开放所有 MCP server，除非显式 allowlist 或工具 spec 允许。
- agent 嵌套深度固定限制为 5。深度来自 `QueryChainTracking`，不是只看当前调用栈。

## 文件索引

主要入口：

- `crates/allthecodes-engine/src/agent/mod.rs`：agent 模块总入口、`AgentInput`、深度限制、子 engine 配置、agent definition 过滤、SDK 到 agent event 映射。
- `crates/allthecodes-engine/src/agent/tool_impl.rs`：`AgentTool` / `TaskAgentTool` 工具实现、参数 schema、模型解析、teammate 分流、同步/后台分发。
- `crates/allthecodes-engine/src/agent/dispatch.rs`：同步 agent 执行、agent tree 注册、SubagentStart/SubagentStop hook、子 engine 流收集。
- `crates/allthecodes-engine/src/agent/supervisor.rs`：后台 agent supervisor、取消、shutdown、task store、后台权限事件、worktree runtime 准备和收尾。
- `crates/allthecodes-engine/src/agent/worktree.rs`：同步 worktree agent 路径和 worktree 结果处理。
- `crates/allthecodes-engine/src/agent/builtin_agents.rs`：内置 agent definition。
- `crates/allthecodes-engine/src/agent_runtime.rs`：全局 runtime adapters、agent tree、dashboard emitter、task store、teammate spawner facade。

工具执行与记录：

- `crates/allthecodes-engine/src/lifecycle/deps/execute.rs`：统一工具执行边界、权限/hook/security、runtime metadata 回填。
- `crates/allthecodes-engine/src/lifecycle/deps/permission.rs`：权限决策到 runtime execution record 标签的映射。
- `crates/allthecodes-engine/src/query/deps.rs`：`ToolExecRequest`、`ToolExecResult`、`QueryDeps` agent/runtime 扩展方法。
- `crates/allthecodes-engine/src/query/loop_impl.rs`：主 query loop、后台结果注入、工具执行记录构造和事件发送。
- `crates/allthecodes-engine/src/query/loop_helpers.rs`：工具并发/串行调度、streaming-safe tool 执行、tool result message 标准化。

共享类型和 IPC：

- `crates/allthecodes-types/src/agent_events.rs`：`AgentEvent` / `AgentCommand` / `TeamEvent` / `TeamCommand`。
- `crates/allthecodes-types/src/agent_runtime_record.rs`：`AgentRuntimeExecutionRecord`、`AgentRuntimePermissionDecision`、digest helper。
- `crates/allthecodes-types/src/agent_channel.rs`：agent IPC channel 事件封装。
- `crates/allthecodes-types/src/agent_types.rs`：agent tree node 和 team 相关展示类型。
- `crates/allthecodes-ipc-protocol/src/protocol/agent.rs`：headless/IPC agent protocol。
- `crates/allthecodes-ipc-protocol/src/normalized.rs`：legacy agent event 到 normalized IPC payload 的映射。
- `crates/allthecodes-web/src/ipc_streams.rs`：Web/API IPC stream 和 replay 输出。

启动与外部表面：

- `crates/allthecodes-startup/src/tool_registry.rs`：root-owned tool registry 中注册 `AgentTool`、`TaskAgentTool`、multi-agent/team 工具。
- `crates/allthecodes-startup/src/engine_runtime.rs`：启动时安装 engine runtime adapters。
- `crates/allthecodes/src/app_runtime_adapters/mod.rs`：应用层 runtime adapter 实现。
- `crates/allthecodes/src/dashboard.rs`：dashboard NDJSON 事件输出，包括 subagent event 和 execution record。
- `crates/allthecodes/src/command_runtime_bridge.rs`：命令 runtime 和 agent/team 工具桥接。
- `crates/allthecodes/src/ui/command_surface/surfaces/tasks.rs`：TUI/command surface 中任务和 agent 输出展示入口。

相关文档：

- `development/runtime/agent-runtime-execution-record-fields-plan.md`：执行记录字段补齐计划和完成状态。
- `development/runtime/tool-discovery-current-state.md`：agent 可用工具集合、runtime provider 和 deferred tool 发现。
- `development/runtime/permission-governance-current-state.md`：agent 工具执行前后的权限治理。
- `development/runtime/session-management-current-state.md`：session、record/replay、后台结果注入和导出相关上下文。
- `development/runtime/cost-observability-current-state.md`：usage/cost 观测边界和未完成的 per-agent/per-tool 归因。
