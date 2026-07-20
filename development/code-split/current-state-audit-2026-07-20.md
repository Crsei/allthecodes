# 当前代码拆分与重构审计

> 审计日期：2026-07-20
> 源码快照：`35974b25`（独立 clean worktree；已包含审计期间主分支新增的 query boundary flush 回归修复）
> 权威总计划：[`codebase-optimization-plan-2026-07-03.md`](codebase-optimization-plan-2026-07-03.md)
> 总执行清单：[`codebase-optimization-execution-checklist-2026-07-20.md`](codebase-optimization-execution-checklist-2026-07-20.md)
> 编译专项：[`compile-performance-plan-2026-07-20.md`](compile-performance-plan-2026-07-20.md)
> 结论类型：只读代码/依赖/历史审计；本任务不实施产品代码重构

## 执行摘要

当前最需要处理的不是“所有超过 1,000 行的文件”，而是大文件与以下信号同时出现的区域：

- 一个函数推进多个生命周期阶段。
- handler 同时持有 transport、持久化、权限和 runtime 状态。
- 新抽象已经存在，但复杂度只是从调用方迁移到另一个大模块。
- 安全/权限语义在 UI、handler 或工具层再次推导。
- 大 crate 位于依赖关键链末端，既难维护又拖慢编译。

据此，第一优先级是 query/submit/tool execution 的残留收口、typed permission/FS capability、Group Chat/Web 领域边界和真实编译模式隔离。文件 workflow、discovery search、terminal、daemon 和 TUI 输入路由列为 P1。纯测试文件、codegen 和依赖版本微调列为 P2。

## 审计方法与口径

本轮使用普通 shell、`rg`、`wc`、`git`、`cargo metadata/tree --offline --locked` 和 Cargo timings；未调用暂停的 Boost MCP。

- “总行数”是物理 Rust 行数，含注释和同文件测试；不等同于复杂度。
- “生产边界”优先按主 `#[cfg(test)] mod tests` 的起始位置估算；嵌入式 test helper 不机械扣除。
- 函数跨度按下一个同级函数/impl item 的起始行估算，数字用于排序，不是质量门本身。
- 生成文件、测试文件和生产主路径分开评估。
- 编译结论来自一次全新 isolated target 的 release timing；不把 warm no-op 当冷构建。

仓库 `crates/` 下共有约 495,232 行、1,455 个 Rust 文件。最大的五个源码域为：

| 源码域 | Rust 文件 | 物理行数 |
|---|---:|---:|
| `crates/allthecodes/src` | 407 | 83,777 |
| `crates/allthecodes-engine/src` | 112 | 49,176 |
| `crates/allthecodes-web/src` | 96 | 46,888 |
| `crates/allthecodes-commands/src` | 107 | 38,968 |
| `crates/allthecodes-tools/src` | 77 | 37,446 |

这些数字只说明审计入口。是否拆分由 ownership、状态转换、依赖方向和测试边界决定。

## 已落地结构与真实残留

旧总计划和运行时计划原先的状态字段已经落后于代码。本轮核对结果如下：

| 领域 | 已落地证据 | 仍未闭环 |
|---|---|---|
| 工具执行 | `ToolExecutionPipeline` / `ToolExecutionPlan` 位于 `lifecycle/deps/tool_pipeline.rs`；`execute_tool_impl` 已调用 pipeline | 主函数仍约 456 行；`maybe_prompt_user` 约 518 行，stage/decision/record 仍紧耦合 |
| Query | `QueryTurnState` 位于 `query/turn_state.rs:49`，`query/recovery.rs` 已承接部分恢复策略 | `query` 主体约 1,184 行，仍负责 fallback、stream、budget、hooks、tools 和 terminal outcome |
| Submit | `SubmitTransaction` 位于 `submit_message/transaction.rs:24`；最新主分支已补 immediate boundary flush | `submit_message_with_overrides` 主体仍约 927 行，多处分支各自创建/提交 transaction |
| API | `ApiOperationRegistry` 位于 `allthecodes-web/src/api_operation_registry.rs:16` | registry 仍有 92 项手写表；`api_dispatcher.rs:342-1233` 仍是约 892 行大 match，并有 92 次泛型 processor dispatch |
| Runtime capability | `RuntimeCapabilityRegistry` 位于 `allthecodes-tools/src/runtime_capability.rs:145` | core seed 仍硬编码约 34 个名称，deferred executor 尚未统一消费 registry |
| TUI state | `ConversationStore`、`OverlayState`、`MessageListViewModel`、`RuntimeViewState` 已存在 | `App` 仍有 61 个直接字段；输入和部分 render 路径继续承担业务路由 |
| Permission UI | 未知 label 已 fail closed 到 deny | callback payload 仍暴露 `tool_name: String`/`tool_input: Value`；router 继续解析 JSON，typed payload 边界未完成 |
| Session mutation | 尚无 `SessionMutationService` | `handlers/sessions.rs:665-1089` 仍集中 mutation，引擎重建 helper 位于 `:1742-1803` |
| Session truth source | SQLite/JSON stores 均存在 | file store 仍有 SQLite 失败回 JSON、SQLite 后 JSON 双写和无 marker legacy import，CS-009 仍为 open |

因此应把这些项目标为 `partial` 或 `done / residual`，不能重新从零实现，也不能因类型已经存在就宣告验收完成。

## P0：主路径与安全边界

### P0-1 `query` 主循环

**证据**

- `crates/allthecodes-engine/src/query/loop_impl.rs`：1,661 行。
- `query` 从 `:89` 到约 `:1272`，主体约 1,184 行。
- 同一函数协调 model/fallback、stream timeout、token budget、stop hooks、tool round、verification、steering、recording 和 terminal result。

**问题**

`QueryTurnState` 已经存在，但它主要记录状态；真正的转换和 side effect 仍散落在巨型 async stream 中。继续往主循环加恢复分支会扩大交叉组合测试矩阵。

**建议边界**

```text
query/orchestrator        只推进 typed turn event，目标 < 250 行
query/model_attempt       单次模型请求、stream 与 timeout
query/recovery            扩展现有模块：prompt-too-long / max-output / fallback retry
query/tool_round          tool scheduling、result folding、continuation
query/terminal            stop/abort/verification/final outcome
query/recording           typed record projection
```

所有模块继续留在唯一 `allthecodes-engine` query implementation 中。不得重建平行 `allthecodes-query` crate。

**验收**

- orchestrator 不直接拼装每类 record/UI message。
- 每个 recovery transition 有 table-driven test。
- abort、stream read error、partial response、fallback、max token、tool error 和 stop hook 各有独立终态断言。

### P0-2 Submit transaction 真正收口

**证据**

- `crates/allthecodes-engine/src/lifecycle/submit_message/mod.rs`：1,385 行。
- `submit_message_with_overrides` 从 `:412` 到约 `:1338`，主体约 927 行。
- `SubmitTransaction::new()` 在主函数和 `stream_handler.rs` 中多次分支性创建。

**问题**

transaction type 已落地，但调用方仍决定哪些分支 append/persist/record/flush/finalize。这样 transaction 更像 helper，而不是 side-effect ownership boundary。

**建议边界**

- `SubmitCoordinator` 只构造 request context 并消费 query events。
- `SubmitTransaction` 统一拥有 append、usage/cost、session persistence、record、flush 和 terminal result 的提交顺序。
- stream handler 只把 provider/query event 转成 transaction input，不直接完成相同 side effect。
- telemetry、hook 和 report generation 通过 typed completion outcome 接入。

**验收**

- 每个 submit 只有一个明确 commit/abort terminal path。
- 重复 result、partial stream、abort 和 recorder failure 不产生重复 session mutation。
- 主函数目标低于 250 行，transaction-level fault injection 可测试。

### P0-3 Tool execution pipeline 的二次拆分

**证据**

- `crates/allthecodes-engine/src/lifecycle/deps/execute.rs`：1,634 行。
- `execute_tool_impl` 从 `:280` 到约 `:735`，约 456 行。
- `crates/allthecodes-engine/src/lifecycle/deps/tool_pipeline.rs`：1,351 行。

**问题**

第一轮 pipeline 已经消除部分巨型函数，但 validation、pre-hook、permission/approval、sandbox/security、tool call、post-hook、audit/runtime records 的数据所有权仍交错。`tool_pipeline.rs:505-1022` 的 `maybe_prompt_user` 本身约 518 行；若继续向单个 pipeline object 增加字段，复杂度只是换了文件。

**建议边界**

- immutable `ToolExecutionPlan`：规范化输入、capabilities、permission subject、sandbox requirement。
- typed stage result：每阶段返回 `Continue / Deny / Fail / Execute / Complete`，禁止靠多个 bool 组合。
- `ExecutionDecisionLedger`：permission/security/hook decision id 单点记录。
- executor 只接收已批准 plan；recorder 只消费 terminal outcome。

**验收**

- `execute_tool_impl` 只负责 stage orchestration，目标低于 180 行。
- 同一 permission decision id 能贯穿 prompt、execution 和 audit record。
- shell policy、sandbox 和 UI 不各自重复分类同一 command。

### P0-4 Typed permission 与 FS capability

**证据**

- `crates/allthecodes/src/ui/permissions/permission_request_router.rs`：1,416 行；`:247-758` 仍从 JSON 推导 command/path/action。
- `crates/allthecodes-types/src/callbacks.rs:35-47` 的 payload 仍包含 `tool_name: String`、`tool_input: Value` 和字符串 message/options。
- `crates/allthecodes/src/ui/permissions/utils.rs:321` 仍以 `rm`/`curl`/`sudo` 等字符串生成 shell risk hint。
- `dialog_overlay.rs:406-415` 对未知 label 已 fail closed 到 deny，这是应保留的改进。
- files/workflow/tool 路径各自仍有 path normalization、authorization 或 mutation guard。

**问题**

UI 展示 risk 与后端实际 permission/security decision 仍可能来自不同推导。文件写入能力散落时，symlink、workspace boundary、archive/extract 和动态 workflow 容易出现不一致。

**建议边界**

- 后端发出 `PermissionSubject`、`RiskClassification`、`AllowedResponses`、`DefaultResponse`、`DecisionId`。
- UI 只渲染，不解析 tool JSON 或 shell command。
- 建立单一 `FsCapabilityService`，统一 resolve/canonicalize/workspace/symlink/mutation policy；handler/tool 只消费 capability。

**验收**

- UI 不再包含 shell risk heuristic。
- 未知 permission kind/response 保持 fail closed。
- Read/Edit/Write/ApplyPatch/Web files/workflow/archive 使用同一 FS decision 结构和 decision id。

### P0-5 Group Chat 与 Web 编译边界

**证据**

- `crates/allthecodes-web/src/handlers/group_chat.rs`：2,695 总行，主 test module 从 `:2309` 开始。
- 文件内同时包含 HTTP handler、room/invite/message store mutation、delegation dispatch、runtime monitor 和 SSE projection。
- `allthecodes-web/src` 46,888 行；本轮冷 release 中该 crate 单元耗时 185.3 秒（frontend 90.9 秒、codegen 94.4 秒）。

**建议领域边界**

```text
group_chat/routes               HTTP 参数与 response mapping
group_chat/room_service         room/member lifecycle
group_chat/invite_service       invite/request policy
group_chat/message_store        durable message/dispatch state
group_chat/delegation_runtime   launch/observe/cancel/monitor
group_chat/sse_projection       replay/reset/live event projection
```

先在领域层形成无 Axum 依赖的 service contracts，再按编译性能计划决定哪些边界成为独立 crate。若只拆成同 crate 文件，可维护性会改善，但 185.3 秒 rustc 单元不会被并行化。

**验收**

- handler 不直接持久化或推进 delegation runtime。
- store mutation 与 SSE projection 使用同一 event sequence/terminal state。
- domain crate 之间不互相依赖；薄 composition crate 统一组装 router。
- 编译专项要求最大 Web 单元不高于 90 秒。

### P0-6 Full Build hook structural stubs

**证据**

- `engine/src/hooks/agent_hook.rs:5,96` 明确标注 structural stub。
- `engine/src/hooks/prompt_hook.rs:5,31` 明确标注 structural stub。
- `engine/src/hooks/file_watcher.rs:8,79` 明确标注 structural stub/TODO。
- `engine/src/hooks/api_query_helper.rs:5,71` 与 `skill_improvement.rs:9,47` 仍保留结构性缺口。

这是功能完整性缺口，不应通过“拆文件”掩盖。先补事件矩阵、typed input/output、timeout/error/blocking 行为和测试，再决定共用 runner/adapter 的结构。

## P1：高收益领域拆分

| 文件 | 总行数 / 生产边界 | 主要混合职责 | 推荐边界 |
|---|---:|---|---|
| `tools/src/workflow/file_workflow.rs` | 2,523 / test `:2214` | parser、authorization、persistence、locking、run state、task projection | `definition_parser`, `run_store`, `transition`, `projection`, thin service/tool adapter |
| `tools/src/discovery_search.rs` | 2,087 / test `:1692` | runtime provider、DTO builder、query/filter/rank、MCP/plugin aggregation | contracts、provider adapters、index/query、ranking、presentation DTO |
| `web/src/handlers/files.rs` | 1,727 / test `:1725` | path policy、metadata/read、mutation、archive | 先接统一 FS capability，再拆 read/metadata、mutation、archive service |
| `web/src/ws/terminal.rs` | 1,864 / test `:1626` | manager、session、process、reader、I/O、resize、cleanup、health | transport、session state machine、process backend、I/O buffer、cleanup monitor |
| `web/src/handlers/sessions.rs` | 1,874 | REST mapping、active engine mutation、branch/resume/feedback | thin handler + `SessionMutationService` + transcript adapter |
| `web/src/api_dispatcher.rs` | 1,802 / test `:1359` | registry、legacy mapping、dispatch、error projection | registry source of truth + transport adapter + legacy adapter |
| `daemon/src/routes.rs` | 1,804 / test `:1122` | HTTP routes 与 daemon control/state operations | route DTO、controller service、process/status projection |
| `daemon/src/gateway_bridge.rs` | 2,059 / test `:946` | production bridge 与大量 scenario tests | production state machine 保持集中；测试按 handshake/recovery/shutdown 拆文件 |
| `permissions/src/command_risk.rs` | 1,842 / test `:1083` | Bash/PowerShell 分类与大测试矩阵 | 保留单一公共 decision；按 dialect rule tables 拆内部模块，不复制 classifier |
| `engine/src/agent/supervisor.rs` | 1,713 / test `:1508` | `AgentRuntime::run` 约 411 行，混 process lifecycle、event/output、shutdown | runtime state machine、output projection、forced shutdown |
| `engine/src/agent/worktree.rs` | `run_in_worktree` 约 551 行 | worktree prepare、agent run、finalize/cleanup | prepare lease、execution、finalization result |

### TUI 残留

- `crates/allthecodes/src/ui/app.rs` 2,182 行，`App` 在 `:247` 开始并有 61 个直接字段。
- `crates/allthecodes/src/ui/app/input.rs` 1,446 行；`handle_key_event` 从 `:126` 到约 `:465`，约 340 行。
- `dispatch_bound_action` 约 314 行；`ui/tui.rs:271-923` 的 `run_tui` 约 653 行。
- `ui/app/render.rs:56-445` 的主 render 约 390 行，同一 frame 在 `:171-182` 与 `:223-234` 两次构造 `MessageListViewModel`。
- `command_surface/surfaces/mcp.rs:23-24` 用 `100/101` 表示 server detail/settings，`:31` 仍是 `action_index: usize`。
- `agents/new_agent_creation/create_agent_wizard.rs:28` 用 `current_step: usize` 表示状态机步骤。

建议把 `handle_key_event` 拆成 typed priority router：global guard → active overlay → selection → command/prompt mode → fallback。MCP action 与 agent wizard step 改为 enum；已有 stores 继续作为 facade 字段的收口点，不再新建平行状态。

### Commands、protocol 与 IPC

- `allthecodes-commands/src/lib.rs` 1,739 行，生产边界约 `:1533`；先抽稳定 metadata/dispatch contract，再按 command domain 分组实现。
- `allthecodes-protocol/src/codegen.rs` 1,834 行，生产边界约 `:1476`；`request.rs:40-1269` 的一次宏输入定义约 275 operations，宏又生成大 enum/match/metadata arrays。分 runtime wire schema、API catalog 和 renderer，但保持一个生成真相源；必须用 timings 验证，不能改成另一套手写表。
- `allthecodes-ipc/src/agent_handlers.rs` 1,610 行，生产边界约 `:1089`；按 request validation、runtime controller、event projection 拆，避免 handler 直接管理 engine lifecycle。

另外两个容易继续增长的函数是 `engine/tools/exec/bash.rs:269-694` 的 `call`（约 426 行）和 `permissions/read_only_shell/commands/git.rs:77-639` 的 Git 规则构造（约 563 行）。前者按 process/sandbox/output/cleanup 拆；后者更适合 declarative per-subcommand rule table，而不是把声明表误拆成多个相互覆盖的 classifier。

这些项重要，但不应抢在 query/tool/permission/Web 安全主路径之前。

## P2：不要和生产主路径混在一起治理

### 大测试文件

以下文件很大，但主要问题是测试发现/维护，不是生产架构：

- `engine/src/lifecycle/tests.rs`：2,840 行。
- `allthecodes/src/ui/app/tests.rs`：2,510 行。
- `config/src/settings/tests.rs`：1,803 行。
- `tools/src/semantic_tool_tests.rs`：1,654 行。
- `mcp/src/client/client_tests.rs`：1,549 行。

按 lifecycle phase、feature、failure mode 或 protocol scenario 分测试模块；共享 fixture 只保留 construction/helper，不创建巨型“万能测试上下文”。拆测试文件可以改善并行 ownership 和定位，但未必降低单 crate 编译时间。

### 依赖版本与微型 crate

当前闭包同时包含 `tokio-tungstenite` 0.26/0.29、`rand` 0.8/0.9 等重复版本。它们可以在主边界稳定后评估，收益预计远小于 mode isolation 和 Web split。

反向操作——为了减少 workspace member 数而合并小 crate——只有在 timings 证明调度/链接成本大于增量隔离收益时才做。crate 数量不是 KPI。

## 编译过慢的直接行动

本轮实测 cold release 为 401.7 秒、517 个 dirty units；最慢三个单元是 Web 185.3 秒、vendored OpenSSL 73.7 秒、bundled SQLite 64.9 秒。根包有 102 个 normal direct dependencies，Linux 默认 normal+build 闭包 410 个包；`--no-default-features` 仍有 391 个包。`allthecodes-tools` 默认开启 `full`，而 `allthecodes-types` 默认开启 `runtime-types`；contract/schema-only 消费方若未显式关闭默认 feature，会把实现依赖继续传播到高扇出闭包。

立即顺序：

1. 固化 cold/warm/touched-Web 三类基准，每类三次取 median。
2. 保留每 worktree 独立 target，A/B `sccache` 的跨 target 命中；建立精确 target 生命周期。
3. 让 `tui/web/daemon/acp/storage` 成为真实 Cargo feature，minimal TUI 不拉 Web/SQLite/daemon/ACP。
4. 按本报告 Group Chat/Web service 边界拆 2–4 个可并行编译单元。
5. 抽 tool/engine/config contracts，降低基础变更的反向依赖范围。
6. 让 PTY 四个 shard 消费一次构建产物，而不是四次 cold compile。

完整命令、量化门槛、缓存/target 清理安全边界和 CI 方案见编译专项计划。

## 推荐执行波次

| 波次 | 内容 | 依赖/理由 |
|---|---|---|
| Wave 0 | 更新 tracker、保存 baseline、补 characterization tests | 已完成文档校准；后续重构先有行为护栏 |
| Wave 1A | Query + submit residual | 同一生命周期，分开 commit、共同 event contract |
| Wave 1B | Tool pipeline + typed permission + FS capability | 安全决策必须先单源，再迁移 UI/handler |
| Wave 1C | Cargo mode isolation | 低风险地缩短日常反馈，不依赖 Web 代码拆分 |
| Wave 2A | Group Chat/Web domains + compile units | 同时解决最高生产文件和 185.3 秒单元 |
| Wave 2B | file workflow/discovery/terminal/session/daemon | 复用 capability/service/controller 边界 |
| Wave 3 | TUI typed router、commands/protocol/IPC、测试分组 | 在上游 typed contracts 稳定后收口消费方 |
| Wave 4 | 依赖去重、profile/linker A/B、可证明的 micro-crate 合并 | 避免用小收益实验干扰主路径 |

每个波次必须按任务建立独立计划/worktree，不应一次性修改所有热点。

## 禁止的“重构”方式

1. 不按 1,000 行阈值机械切文件；先确定 state/permission/protocol ownership。
2. 不重建平行 `allthecodes-query`，不复制 query loop 做迁移垫片。
3. 不新增 process-wide mutable registry/callback 绕过依赖注入。
4. 不把 permission、shell risk 或 path capability 下放到 UI/handler 再解析。
5. 不以 Full Build 代码太大为理由删除 recovery、sandbox、error 或跨平台分支。
6. 不以同 crate 文件拆分宣称已解决编译瓶颈。
7. 不删除 vendored OpenSSL 或让 worktree 共用 target 来换取表面速度。

## 统一验收模板

每个后续重构任务至少提交以下证据：

- 行为：现有 targeted tests + 新 characterization/fault-injection tests。
- 结构：主函数/直接字段/反向依赖/编译单元的 before/after 数字。
- 安全：permission/path/sandbox 未迁移到非权威层，未知输入 fail closed。
- 编译：cold、warm、典型 touched-crate timing；不能只报 no-op。
- Full Build：上游分支、恢复和错误语义无静默删减。
- 流程：worktree 内不运行 Rust；fast-forward 后只在主分支按分层 SOP 验证。
