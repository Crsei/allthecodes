# 代码拆分与复杂度优化计划

> 生成日期: 2026-07-03
> 覆盖范围: 当前 `allthecodes` worktree
> 输入来源: 4 个只读 subagent 分区审计 + 本地粗复杂度扫描
> 当前阶段: Full Build。不得再以 Lite 缩减为理由保留行为缺口。
> 最新校准: 2026-07-20。此前若干结构已落地，但原跟踪表仍停留在 `todo`；本次按当前 HEAD、实际行数和一次隔离冷构建重新分级。
> 当前证据: [`current-state-audit-2026-07-20.md`](current-state-audit-2026-07-20.md)
> 编译专项: [`compile-performance-plan-2026-07-20.md`](compile-performance-plan-2026-07-20.md)

---

## 结论摘要

当前最高风险不是单纯文件行数过大，而是几条关键路径被多层重复实现：

1. 工具执行、权限、hook、沙箱、审计混在同一执行函数中。
2. query/submit 生命周期由多个巨型状态机共同推进。
3. Web/API/session/IPC 迁移层有多套手写分发表和 legacy adapter。
4. Rust TUI 在 UI 层重新解释权限、工具参数、按钮语义。
5. session 存储长期保留 JSONL、legacy JSON、SQLite index 多真相源。

优化目标不是删功能，而是把行为收敛到单一权威路径，降低 full build 补齐上游行为时的回归概率。

## 2026-07-20 现状校准

本计划继续作为代码拆分与复杂度治理的唯一总跟踪入口。状态含义如下：

- `open`：尚未形成计划要求的权威结构。
- `partial`：结构已经落地，但主函数、重复判定或验收目标仍未收口。
- `done / residual`：原任务已完成；后续问题属于新的残留治理，不应把历史实现改回 `todo`。

当前最重要的变化不是“所有旧任务都没做”，而是部分抽象已经存在、复杂度仍停留在调用方或迁移到新模块：

| 区域 | 当前证据 | 判断 |
|---|---|---|
| 工具执行 | `execute_tool_impl` 已接入 `ToolExecutionPipeline`，但函数仍约 456 行，`tool_pipeline.rs` 本身 1,351 行 | `partial`；继续按 stage/decision/record 边界收口 |
| Query loop | `QueryTurnState` 已存在，但 `loop_impl.rs` 1,661 行，`query` 主体约 1,184 行 | `partial`；保留唯一 engine 内实现，不重建平行 crate |
| Submit | `SubmitTransaction` 已存在，但 `submit_message_with_overrides` 主体约 922 行 | `partial`；把分支 side effect 真正交给 transaction/handler |
| Web | `allthecodes-web/src` 共 46,888 行，`handlers/group_chat.rs` 2,695 行 | 新 P0；按领域和编译单元同时拆，不做纯文件搬家 |
| Rust TUI | domain stores / overlay dispatcher / view-model 已落地；`App` 仍有 61 个直接字段，`app/input.rs` 1,446 行 | 原任务 `done / residual`；继续拆 typed route/store |
| 编译 | 42 个 workspace member；Linux 默认 normal+build 闭包 410 个包；隔离冷 release 为 401.7 秒 | 独立执行编译性能专项，不把 crate 数量当 KPI |

详细文件排序、函数证据和“已完成结构/仍未闭环验收”的区分见当前证据报告。历史运行时边界任务的实现提交则保留在
[`runtime-boundary-refactor-plan-2026-07-03.md`](runtime-boundary-refactor-plan-2026-07-03.md)；该文档是实施记录，不再是九项全部未开始的待办表。

---

## 风险分级

| 级别 | 定义 | 处理原则 |
|---|---|---|
| P0 | 安全边界、生命周期主路径、协议主入口，当前有行为漂移或 stub 风险 | 先补测试护栏，再拆分；禁止顺手改行为 |
| P1 | 高复杂度共享模块，改动频繁且容易产生状态漂移 | 以服务层/状态机/typed DTO 收敛 |
| P2 | 局部 UI/registry 状态设计差，扩展时容易出错 | 在相关功能触及时顺带结构化 |

---

## P0 工作流

### 1. 统一工具执行边界

核心文件:

- `crates/allthecodes-engine/src/lifecycle/deps/execute.rs:9`
- `crates/allthecodes-engine/src/lifecycle/deps/permission.rs:36`
- `crates/allthecodes-engine/src/tools/exec/bash.rs:154`
- `crates/allthecodes-engine/src/tool_runtime/execution/security.rs:61`

问题:

- `ToolExecutionPipeline` 已落地，但 `execute_tool_impl` 仍约 456 行，`deps/tool_pipeline.rs` 达 1,351 行；旧复杂度被拆开但尚未形成足够小的权威 stage。
- validation、PreToolUse hook、central permission、用户审批、auto review、tool call、PostToolUse hook、audit、Langfuse、runtime record 仍跨主函数和 pipeline 紧密耦合。
- shell 命令同时被 read-only、dangerous、sandbox preflight、mode fallback、hook 多层判断。

计划:

1. 添加 `ToolExecutionPlan`:
   - 原始输入
   - hook 后输入
   - permission subject
   - sandbox requirement
   - audit labels
   - expected side effects
2. 添加 `ToolExecutionPipeline`:
   - `validate`
   - `run_pre_hooks`
   - `resolve_permission`
   - `execute`
   - `run_post_hooks`
   - `emit_records`
3. 把 shell 命令分类收敛成单一 `ShellPolicyDecision`:
   - read-only
   - build
   - mutate
   - destructive
   - deploy
   - secret
   - parser failure
4. 所有 UI、permission prompt、sandbox preflight 只消费这个统一 decision。

验收:

- `execute_tool_impl` 降到只编排 pipeline，目标小于 180 行。
- shell policy golden tests 覆盖 bash/powershell/read-only/destructive/deploy/secret/parse-fail。
- 任意工具执行 record 能追溯同一个 permission decision id。

### 2. 补齐并收敛 hook 系统

核心文件:

- `crates/allthecodes-engine/src/hooks/agent_hook.rs:5`
- `crates/allthecodes-engine/src/hooks/prompt_hook.rs:5`
- `crates/allthecodes-engine/src/hooks/file_watcher.rs:77`

问题:

- 多个 hook 文件仍标注 full implementation placeholder/stub。
- agent/prompt hook 存在直接 success 的结构性缺口。

计划:

1. 对照上游 TypeScript hook 行为列出事件矩阵:
   - SessionStart
   - UserPromptSubmit
   - PreToolUse
   - PostToolUse
   - Stop
   - StopFailure
   - Agent hooks
   - file watcher hooks
2. 每个事件明确:
   - 输入 payload schema
   - 可修改字段
   - 可阻断行为
   - timeout/error 行为
   - audit event
3. 删除无行为 stub；不能实现的项写入 intentional gap，而不是静默 success。

验收:

- 每个 hook type 至少有 schema test 和 error/timeout test。
- placeholder/stub 文案从生产路径移除。
- full build 未实现项必须落到文档的 explicit gap。

### 3. 拆分 query/submit 生命周期

The canonical query loop currently lives in `crates/allthecodes-engine/src/query/`.
The previous independent `allthecodes-query` crate was removed because it duplicated
behavior. Do not recreate `allthecodes-query` as a parallel implementation. If the
query loop is extracted again, it must be a single canonical crate depending on typed
abstractions, with no duplicate engine-internal implementation. The detailed decision
record is `development/code-split/query-loop-boundary-decision-2026-07-03.md`.

核心文件:

- `crates/allthecodes-engine/src/query/loop_impl.rs:71`
- `crates/allthecodes-engine/src/query/loop_helpers.rs:203`
- `crates/allthecodes-engine/src/lifecycle/submit_message/mod.rs:277`
- `crates/allthecodes-engine/src/lifecycle/submit_message/stream_handler.rs:42`

问题:

- `QueryTurnState` 已落地，但 `query()` 主体当前约 1,184 行，仍承担 model fallback、stream timeout、stop hooks、token budget、tool execution。
- `SubmitTransaction` 已落地，但 `submit_message_with_overrides` 主体当前约 922 行，仍和 stream handler 共同推进 QueryEngine 全局状态。
- `loop_helpers.rs` 实际是第二状态机。

计划:

1. 定义 `QueryTurnState`:
   - preparing
   - streaming
   - assistant_received
   - executing_tools
   - terminal_check
   - continuing
   - finished
2. 把恢复策略移入独立模块:
   - prompt too long recovery
   - max output token recovery
   - fallback model retry
   - stream timeout recovery
3. 把 submit side effects 收敛到 `SubmitTransaction`:
   - append message
   - persist session
   - update usage/cost
   - emit event
   - flush stream
4. `query` 只产出 typed turn events；`submit` 只消费事件并提交 side effects。

验收:

- 主循环函数小于 250 行。
- submit lifecycle 有 transaction-level tests。
- abort、stop hook、max token、tool error、fallback retry 都有独立测试。

### 4. 修复 UI 权限重解释

核心文件:

- `crates/allthecodes/src/ui/permissions/permission_request_router.rs:122`
- `crates/allthecodes/src/ui/permissions/dialog_overlay.rs:321`
- `crates/allthecodes/src/ui/permissions/utils.rs:321`
- `crates/allthecodes/src/ui/permissions/shell_permission_helpers.rs:23`

问题:

- UI 层靠字符串和 JSON heuristic 识别工具、apply_patch、shell 风险和按钮语义。
- 未知按钮标签路径存在退到 allow 的风险。
- 用户看到的风险文案可能和后端实际 allow/deny/ask 不一致。

计划:

1. 后端 permission request 改为 typed payload:
   - `PermissionSubject`
   - `RiskClassification`
   - `AllowedResponses`
   - `DefaultResponse`
   - `DenyReason`
2. UI 只渲染 typed payload，不再解析 tool JSON。
3. 未知按钮、未知 response、未知 permission kind 全部 fail closed。
4. route/render/input 共用同一个 resolved view model，避免每帧重复推导。

验收:

- UI permission 层不再调用 shell heuristic。
- `choice_for_label` 未知标签测试必须 deny/cancel。
- 权限弹窗 snapshot 覆盖 Bash、ApplyPatch、MCP、dynamic workflow。

---

## P1 工作流

### 5. Web/API/session mutation 服务化

核心文件:

- `crates/allthecodes-web/src/api_dispatcher.rs:250`
- `crates/allthecodes-web/src/api_dispatcher.rs:387`
- `crates/allthecodes-web/src/ws/ipc.rs:121`
- `crates/allthecodes-web/src/handlers/sessions.rs:392`

问题:

- API metadata、migration state、dispatch match 三套手写表。
- WS handler 同时管 transport、engine 生命周期、permission callbacks、event conversion。
- session REST handler 直接替换 active engine、fork rollout、rollback、同步 transcript。

计划:

1. 建 `ApiOperationRegistry`，由一个注册点生成:
   - method metadata
   - processor binding
   - transport kind
   - migration state
2. `ws/ipc.rs` 拆为:
   - socket transport
   - session runtime controller
   - protocol adapter
   - permission callback bridge
3. `sessions.rs` mutation 下沉到 `SessionMutationService`:
   - resume
   - branch
   - feedback
   - delete
   - regenerate prepare
   - edit prepare
   - rollback preview
   - rollback apply

验收:

- 新增 API operation 只需要一个注册点。
- session mutation 测试不依赖 HTTP handler。
- WS 断连/重连/abort/pending permission 有集成测试。

### 6. 收敛 session/record-replay 真相源

核心文件:

- `crates/allthecodes-session/src/resume.rs:61`
- `crates/allthecodes-session/src/record_replay/reconstruct.rs:49`
- `crates/allthecodes-session/src/record_replay/migration.rs:121`
- `crates/allthecodes-session/src/record_replay/recorder.rs:246`
- `crates/allthecodes-session/src/storage.rs:1`
- `crates/allthecodes-session/src/storage/file_store.rs:339`
- `crates/allthecodes-session/src/storage/sqlite_store.rs:104`

问题:

- JSONL、legacy JSON、SQLite index 三套读写路径长期并存。
- 读取失败多走 warn/fallback，可能掩盖数据损坏。
- SQLite 保存时删插全部 messages，JSON fallback 和 index 可能漂移。

计划:

1. 定义权威源:
   - record-replay JSONL 作为 transcript truth
   - SQLite 作为 index/query cache
   - legacy JSON 只做一次性 migration input
2. migration 完成后写 marker，避免每次 resume 重扫。
3. 对 SQLite drift 做一致性检测:
   - session count
   - message count
   - last seq
   - root rollout id
4. fallback 从静默 warn 改为 typed recoverable error，并暴露诊断。

验收:

- resume 路径有 corrupt JSONL、missing index、legacy-only、mixed storage tests。
- storage drift 会被显式报告。
- 新 session 不再写 legacy JSON，除非打开兼容开关。

### 7. Command/deferred/MCP registry 统一

核心文件:

- `crates/allthecodes-commands/src/lib.rs:630`
- `crates/allthecodes-commands/src/dynamic_registry.rs:37`
- `crates/allthecodes-tools/src/deferred_tools.rs:651`
- `crates/allthecodes-mcp/src/manager.rs:283`
- `crates/allthecodes-commands/src/mcp/config.rs:16`

问题:

- 静态命令、动态命令、hidden command、runtime provider 都集中在 `commands/lib.rs`。
- `ExecuteExtraTool` 隐藏执行路径依赖 hard-coded `CORE_TOOLS` 和 session discovery state。
- MCP server/tool 身份靠名称约定串起来，权限 subject 分散。

计划:

1. 统一 `RuntimeCapabilityRegistry`:
   - command
   - tool
   - deferred tool
   - MCP server tool
   - dynamic workflow
2. 每个 capability 带:
   - visibility
   - permission subject
   - provider
   - source scope
   - discoverability
3. `ExecuteExtraTool` 不再绕 registry，改为执行 registry 中已授权 capability。
4. dynamic command parser 只消费 registry metadata。

验收:

- hidden/deferred/MCP tool 都能输出同一种 permission subject。
- 命令展示、命令解析、命令执行来自同一 metadata snapshot。
- 删除 hard-coded `CORE_TOOLS` 边界或只保留为 registry seed。

### 8. Web files 和 FS 写入能力令牌

核心文件:

- `crates/allthecodes-web/src/handlers/files.rs:66`
- `crates/allthecodes-tools/src/fs/file_write.rs:80`
- `crates/allthecodes-tools/src/fs/file_edit.rs:377`
- `crates/allthecodes-tools/src/fs/apply_patch.rs:491`
- `crates/allthecodes-tools/src/fs/safe_write.rs:52`

问题:

- Web file handler 同时处理路径安全、文件业务、response shape、processor glue。
- FS 写入工具各自做 path gate、safe write、apply patch 校验。
- 新写入工具容易漏掉同一套授权和路径边界。

计划:

1. 抽 `WorkspaceFileService`:
   - resolve path
   - authorize read/write/delete
   - hash precondition
   - atomic write
   - copy/move/delete
2. 引入授权后的 `FsCapability`:
   - root
   - normalized path
   - operation
   - precondition
   - permission decision id
3. Web handler 和 tool handler 都只调用 service。

验收:

- `handlers/files.rs` 只保留 DTO/HTTP 映射。
- 所有写入路径必须持有 `FsCapability`。
- path traversal、symlink、missing parent、hash conflict 共享测试。

### 9. Rust TUI 状态拆分

核心文件:

- `crates/allthecodes/src/ui/app.rs:193`
- `crates/allthecodes/src/ui/app/input.rs:124`
- `crates/allthecodes/src/ui/app/render.rs:71`
- `crates/allthecodes/src/ui/messages/render/mod.rs:219`
- `crates/allthecodes/src/ui/messages/render/preprocessing.rs:10`
- `crates/allthecodes/src/ui/command_surface/surfaces/tasks.rs:150`

问题:

- `App` 同时持有 chat、prompt、scroll、permission、agent nav、task surface、theme、session、terminal、voice、completion。
- input handler 按顺序让 overlay/command/history/agent/message selection 抢事件。
- render 层承担 message normalization、copy text、raw JSON、tool result grouping。
- tasks surface 重建 agent/team 状态。

计划:

1. 拆 domain stores:
   - `ChatStore`
   - `OverlayStore`
   - `AgentTaskStore`
   - `InputStore`
   - `LayoutStore`
2. 建 overlay dispatcher:
   - explicit focus owner
   - typed overlay action
   - z-order/render/input 同源
3. 建 message view-model:
   - normalize once
   - render consumes `RenderableMessage`
   - copy/history/virtual scroll 使用同一 projection
4. tasks surface 只读 `AgentTaskStore` projection。

验收:

- `App` 字段数降低，目标小于 40 个直接字段。
- `handle_key_event` 小于 160 行。
- overlay focus 和 permission dialog 有 narrow terminal snapshot tests。

---

## P2 工作流

### 10. Agent/MCP UI 局部状态机

核心文件:

- `crates/allthecodes/src/ui/command_surface/surfaces/agents.rs:32`
- `crates/allthecodes/src/ui/agents/new_agent_creation/create_agent_wizard.rs:5`
- `crates/allthecodes/src/ui/command_surface/surfaces/mcp.rs:22`

问题:

- agent wizard step 动态插入，但 handler 用硬编码 step 判断。
- MCP surface 用 `usize action_index` 编码 list/detail/settings/tool detail 等模式。

计划:

1. agent wizard 改为 `AgentWizardState` + `AgentWizardStep` enum。
2. MCP surface 改为:
   - `McpView::List`
   - `McpView::ServerDetail`
   - `McpView::Settings`
   - `McpView::ToolList`
   - `McpView::ToolDetail`
3. 删除魔法 offset。

验收:

- 无 100/101/1000/2000 这种状态 offset。
- wizard step 插入只改 step definition。

---

## 推荐执行顺序

### Phase 0: 护栏先行

目标: 不改行为，先锁定现状。

任务:

1. 工具执行 golden tests:
   - bash allow/ask/deny
   - hook allow/deny/modify input
   - sandbox preflight
   - permission prompt response
2. query lifecycle tests:
   - abort before stream
   - abort during tool
   - prompt too long
   - max tokens
   - fallback model
3. session replay consistency tests:
   - JSONL only
   - legacy JSON only
   - SQLite index missing
   - corrupt replay
4. UI permission fail-closed tests.

退出条件:

- 所有 P0 现有行为都有 regression coverage。
- 未实现 upstream 行为记录为 explicit gap。

### Phase 1: 安全边界收敛

目标: 权限、工具、shell、FS 不再多处解释。

任务:

1. `ShellPolicyDecision`
2. `ToolExecutionPlan`
3. typed permission request payload
4. `FsCapability`

退出条件:

- UI 不再解析 shell 风险。
- FS 写入不再绕过统一 capability。
- 工具执行审计可追溯同一 decision id。

### Phase 2: 生命周期拆分

目标: query/submit/web session 不再互相直接改状态。

任务:

1. `QueryTurnState`
2. recovery modules
3. `SubmitTransaction`
4. `SessionMutationService`
5. WS runtime controller

退出条件:

- query 主循环、submit 主函数、WS socket handler 都小于当前一半。
- abort/reconnect/rollback/resume 有 service-level tests。

### Phase 3: Registry 和协议治理

目标: API、command、tool、MCP 都有单一注册与 dispatch 元数据。

任务:

1. `ApiOperationRegistry`
2. `RuntimeCapabilityRegistry`
3. canonical IPC/API envelope
4. legacy adapter 单点化

退出条件:

- 新增 API/command/tool 不需要改多套手写表。
- legacy 转换集中在一个 adapter 层。

### Phase 4: TUI 状态解耦

目标: UI 只消费 view-model，不重新推导业务语义。

任务:

1. domain stores
2. overlay dispatcher
3. message view-model
4. tasks/agents/mcp typed view state

退出条件:

- TUI render/input 状态可以局部测试。
- 权限、任务、agent nav 只有一个 runtime store。

执行结果（2026-07-03）:

- 已新增 `ConversationStore`、`PromptQueueStore`、`SessionUiStore`、`RenderLayoutStore`，`App` 继续作为 facade 暴露原有 TUI runner 接口。
- 已新增 `OverlayState` / `OverlayOutcome`，overlay render/input/focus 使用同一优先级来源；guardrail 覆盖 `BypassPermissions > Question > Permission > AgentTree > HistorySearch > CommandSurface`。
- 已新增 `MessageListViewModel` 并让主 render 路径通过 `MessageRenderContext` projection 消费消息状态；final cleanup 已移除旧 render-context helper anchor。
- 已新增 `RuntimeViewState`，agent navigation、当前 agent thread、live task snapshot 和 backend task events 由同一 runtime view state 合并输出；`/tasks` 打开时会刷新 live snapshot 并保留 backend-only task events。
- 验证证据: `cargo fmt --all -- --check`、`cargo test -p allthecodes ui::app`、`cargo check --workspace`、`cargo build --workspace --release` 均在最终提交后通过且无新增 warning。旧 snapshot baseline 问题未在本阶段改动，仍受 `**/snapshots/` ignore 影响。

---

## 禁止事项

1. 不要以 Lite/简化版为理由删除上游行为。
2. 不要在 UI 层重新实现权限、安全、shell 分类。
3. 不要新增全局 `OnceLock/RwLock` provider 绕过依赖注入。
4. 不要新增第三套 protocol/session representation。
5. 不要在 handler 中直接操纵 active engine，必须经 service/controller。
6. 不要只按文件行数机械拆分；拆分边界必须对应状态、权限或协议边界。

---

## 跟踪清单

| ID | 模块 | 优先级 | 状态 | 目标 |
|---|---|---:|---|---|
| CS-001 | Tool execution pipeline | P0 | partial | pipeline 已存在；`execute_tool_impl` 继续编排化 |
| CS-002 | Hook full implementation matrix | P0 | todo | 移除 structural stub |
| CS-003 | Shell policy decision | P0 | partial | metadata/policy 已有基础；后端/UI/sandbox 仍需单一分类 |
| CS-004 | Query turn state machine | P0 | partial | type 已存在；主循环仍需缩小并可局部测试 |
| CS-005 | Submit transaction | P0 | partial | transaction 已存在；side effects 仍需单点提交 |
| CS-006 | Typed permission UI payload | P0 | open | UI fail closed |
| CS-007 | API operation registry | P1 | partial | registry 已存在；继续消除残留手写表 |
| CS-008 | Session mutation service | P1 | todo | handler 不直接改 engine |
| CS-009 | Record-replay truth source | P1 | todo | 收敛多真相源 |
| CS-010 | Runtime capability registry | P1 | partial | registry 已存在；command/tool/MCP/deferred 继续统一 |
| CS-011 | FS capability service | P1 | open | 写入路径统一授权 |
| CS-012 | TUI domain stores | P1 | done / residual | stores 已落地；继续缩小 61-field `App` facade |
| CS-013 | Overlay dispatcher | P1 | done / residual | dispatcher 已落地；继续收口输入路由 |
| CS-014 | Message view-model | P1 | done / residual | view-model 已落地；继续移除 render 数据变形 |
| CS-015 | Agent/MCP typed surface state | P2 | open | 删除 100/101 魔法索引与 `usize` step |
| CS-016 | Web domain and compile boundaries | P0 | open | 拆 group chat/files/terminal，降低 185.3 秒 Web 单元 |
| CS-017 | Build-mode feature isolation | P0 | open | TUI 构建不再无条件拉入 Web/SQLite/daemon/ACP |

CS-016/017 的执行顺序、基准和量化验收由编译性能专项计划维护；此表只负责总优先级，避免在两个计划中复制详细步骤。
