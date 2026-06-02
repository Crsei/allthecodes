# claude-code-rs 重构审计

> 审计日期: 2026-05-07
> 范围: `crates/claude-code-rs`
> 基准: 当前工作树。审计开始时仓库已有大量未提交改动，本文件只记录当前状态，不假设这些改动都来自同一作者。

## 结论

屎山程度: **7/10，中高风险，但不是全局失控**。

核心问题不是文件多，而是几个运行时边界承担了过多职责：任务/团队协作、工具执行、IPC 子系统、Query 生命周期和命令系统。这些区域同时连接持久化、权限、hooks、TUI/headless、模型流、测试和上游兼容逻辑，继续在原地叠功能会让回归定位越来越慢。

积极信号也存在：`src/ui/` 已经有分层目录和快照测试，`src/tools/` 有架构约束文档，QueryEngine 已经把早期的多锁状态合并到 `QueryEngineState`。所以当前更像“核心边界长成了大块硬结”，不是所有模块都需要推倒重来。

## 快速量化

证据来自 `rg --files crates/claude-code-rs/src -g *.rs` 与逐文件行数统计。

| 指标 | 当前值 |
| --- | ---: |
| Rust 源文件数 | 589 |
| Rust 源码行数 | 139,833 |
| 超过 800 行的文件 | 38 |
| 超过 1000 行的文件 | 20 |
| 超过 1500 行的文件 | 5 |
| 超过 2000 行的文件 | 3 |
| `ui` + `tools` + `commands` 行数 | 79,814 |
| crate/module 级 `#![allow(unused)]` | 16 |
| crate/module 级 `#![allow(dead_code)]` | 11 |
| `allow(dead_code/unused/unused_imports)` 总出现数 | 363 |

最大目录:

| 目录 | 文件数 | 行数 |
| --- | ---: | ---: |
| `src/ui` | 313 | 28,531 |
| `src/tools` | 48 | 27,290 |
| `src/commands` | 72 | 23,993 |
| `src/engine` | 22 | 11,439 |
| `src/ipc` | 25 | 9,377 |
| `src/api` | 15 | 8,365 |

最大文件:

| 文件 | 行数 | 判断 |
| --- | ---: | --- |
| `src/tools/tasks.rs` | 4,785 | P0 |
| `src/query/loop_tests.rs` | 2,402 | P2 测试拆分 |
| `src/ipc/subsystem_handlers.rs` | 2,177 | P0/P1 |
| `src/engine/lifecycle/deps.rs` | 1,949 | P0 |
| `src/commands/mcp_cmd.rs` | 1,501 | P1 |
| `src/tools/tool_search.rs` | 1,458 | P2 |
| `src/engine/system_prompt.rs` | 1,387 | P2 |
| `src/query/loop_helpers.rs` | 1,309 | P1 |
| `src/engine/lifecycle/submit_message.rs` | 1,271 | P1 |
| `src/tools/fs/file_read.rs` | 1,269 | P2 |

## 急需重构区域

### P0: Task / Team / coordination 状态边界

受影响文件:

- `crates/claude-code-rs/src/tools/tasks.rs`
- `crates/claude-code-rs/src/tools/send_message.rs`
- `crates/claude-code-rs/src/tools/team_spawn.rs`
- `crates/claude-code-rs/src/tools/background_agents.rs`
- `crates/claude-code-rs/src/teams/*`

证据:

- `tasks.rs` 单文件 4,785 行。
- `tasks.rs:85` 起定义 `TaskStore`，同文件继续包含任务持久化、schema 迁移、高水位锁、任务列表锁、todo 状态、remote task 恢复、所有 Task/Todo 工具实现和大量测试。
- `tasks.rs:2155` 起把 `TodoWriteTool`、`TaskCreateTool`、`TaskGetTool`、`TaskUpdateTool`、`TaskListTool`、`TaskStopTool`、`TaskOutputTool` 放在同一文件。
- `tasks.rs:3203` 起内联测试，说明生产逻辑和回归夹在一个超大文件里。
- `src/tools/ARCHITECTURE.md` 已经规定 “不要让单文件超过约 800 行”，并把 `tasks.rs + send_message.rs + send_user_message.rs + background_agents.rs + orchestration.rs` 提为下一步 `coord/` 子域候选。

为什么急:

任务系统已经不只是一个工具，它是团队、后台任务、remote task、输出保留、重启恢复、权限协作的共享状态层。这个文件越大，后续修 Team、daemon、background agent 或 task output 都会被迫同时理解持久化和工具协议。

建议切法:

1. 先把 `tasks.rs` 内部切成 `tools/tasks/{store,repository,migration,locking,todo,output,tools,tests}.rs`，保持公开工具名和 JSON schema 不变。
2. 再建立 `tools/coord/`，吸收 `send_message.rs`、`send_user_message.rs`、`team_spawn.rs`、`background_agents.rs`、`orchestration.rs`。
3. 行为锁定先跑 `cargo test -p claude-code-rs tools::tasks`、`cargo test -p claude-code-rs tools::send_message`、`cargo test -p claude-code-rs tools::team_spawn`，再做机械移动。

### P0: 工具执行边界存在双轨和半收束状态

受影响文件:

- `crates/claude-code-rs/src/engine/lifecycle/deps.rs`
- `crates/claude-code-rs/src/query/loop_helpers.rs`
- `crates/claude-code-rs/src/tools/execution/*`

证据:

- `engine/lifecycle/deps.rs:671` 的 `QueryEngineDeps::execute_tool` 同时组装 `ToolUseContext`、跑 validation/security、pre/post hooks、权限询问、工具调用、result size enforcement 和审计。
- `tools/execution/pipeline.rs:55` 仍导出 `run_tool_use()`，注释称主 Query loop 已经 canonicalized 到 `QueryDeps::execute_tool`，但这里还保留一份完整 orchestration。
- `tools/execution/coordinator.rs:32` 也定义了一个 `StreamingToolExecutor`，注释在 `coordinator.rs:40` 明确说上线前必须改走 `QueryDeps::execute_tool`，否则权限、progress、audit、abort、结果保真会和主路径分叉。
- 当前 live query loop 使用的是 `query/loop_helpers.rs:55` 的另一个 `StreamingToolExecutor`，它已经走 `deps.execute_tool()`；`tools/execution/coordinator.rs` 主要只被自身测试引用。

为什么急:

工具执行是安全边界。只要存在两个“完整工具执行管线”，后续修权限、hooks、sandbox、progress、Langfuse 或 result normalization 时就很容易只改一边。这里不是单纯大文件问题，而是架构事实源不唯一。

建议切法:

1. 明确唯一执行入口：保留 `QueryDeps::execute_tool` 或保留 `run_tool_use()`，但不能两个都拥有完整管线。
2. 把 `ToolUseContext` 构造、permission resolution、hook dispatch、security validation 抽成小型 stage 函数，供唯一入口组合。
3. 删除或降级 `tools/execution/coordinator.rs` 的旧 `StreamingToolExecutor`，避免和 `query/loop_helpers.rs` 同名概念继续并存。
4. 行为锁定先跑 `cargo test -p claude-code-rs query::loop_helpers`、`cargo test -p claude-code-rs engine::lifecycle::deps`、`cargo test -p claude-code-rs tools::execution`。

### P1: IPC subsystem handlers/types/events 仍是聚合大球

受影响文件:

- `crates/claude-code-rs/src/ipc/subsystem_handlers.rs`
- `crates/claude-code-rs/src/ipc/subsystem_events.rs`
- `crates/claude-code-rs/src/ipc/subsystem_types.rs`
- `crates/claude-code-rs/src/ipc/agent_settings.rs`

证据:

- `subsystem_handlers.rs` 2,177 行，LSP、MCP、Plugin、IDE、Skill、AgentSettings 的命令处理和 status snapshot builder 都在一个文件中。
- `subsystem_events.rs` 1,240 行，同一文件同时定义多个 subsystem 的 event enum、command enum 和 event bus。
- `subsystem_types.rs` 932 行，且 `subsystem_events.rs:12`、`subsystem_types.rs:13` 都用 `#![allow(dead_code)]` 表示预定义未来扩展类型。
- `ipc/headless.rs` 已经变成 27 行薄入口，但 subsystem 层没有完成同等拆分。
- `docs/ipc-refactor-plan.md` 已有计划，但内容明显早于当前结构，不能直接当完成状态。

为什么急:

IPC 是 TUI/headless/Web/未来前端的协议边界。现在协议、运行时状态查询、配置持久化、异步任务发起混在一组文件里，新增一个 subsystem 或改一个命令字段时，影响面会扩大到无关 subsystem。

建议切法:

1. 建立 `ipc/subsystems/{lsp,mcp,plugin,ide,skill,agent_settings}/`。
2. 每个子域自带 `types.rs`、`events.rs`、`handlers.rs`，顶层只保留统一 `SubsystemEvent` 和 event bus。
3. 先补 serialization/roundtrip contract tests，再机械迁移。不要同时改 JSON 协议。

### P1: Query turn / lifecycle 主流程仍是长闭包

受影响文件:

- `crates/claude-code-rs/src/engine/lifecycle/submit_message.rs`
- `crates/claude-code-rs/src/query/loop_impl.rs`
- `crates/claude-code-rs/src/query/loop_helpers.rs`
- `crates/claude-code-rs/src/query/loop_tests.rs`

证据:

- `submit_message.rs:199` 起的 `QueryEngine::submit_message()` 包含一个大型 `async_stream::stream!` 闭包，顺序处理 UserPromptSubmit hook、slash command、transcript、SystemInit、memory recall、system prompt、API client、Langfuse trace、inner query loop、result assembly。
- `query/loop_impl.rs:57` 的 `query()` 是核心 generator，虽然有 phase 注释，但仍在一个 882 行函数内处理模型流、streaming tool start、stop hooks、token budget、tool results 和 continuation。
- `query/loop_impl.rs:826` 仍有 `STEP 7: ATTACHMENTS (placeholder)`，说明流程骨架和实际能力还没有完全收束。
- `query/loop_tests.rs` 2,402 行，测试覆盖不少，但测试本身也变成定位成本。

为什么急:

这里是每一轮会话都经过的主路径。它现在靠注释维持阶段边界，真正的类型边界不足。任何新增生命周期能力都会继续塞进闭包或 helper，导致行为改动很难做窄。

建议切法:

1. 保留 `submit_message()` 的 stream 外壳，把 Phase A/B/C/D/E 抽成返回明确 enum/effect 的小函数。
2. 给 Query loop 引入 `QueryTurnContext` 和 `ModelAttemptContext`，把 fallback、stream timeout、tool execution、continuation 分开。
3. 拆 `loop_tests.rs` 为 `loop_tests/{model_recovery,tool_execution,token_budget,stop_hooks,streaming}.rs`。

### P1: 命令系统 registry 和复杂命令处理器过重

受影响文件:

- `crates/claude-code-rs/src/commands/mod.rs`
- `crates/claude-code-rs/src/commands/mcp_cmd.rs`
- `crates/claude-code-rs/src/commands/*`

证据:

- `commands/mod.rs:191` 的 `get_all_commands()` 手写构造完整命令列表，命令 metadata 和 handler 注册耦合在同一个函数里。
- `commands/mcp_cmd.rs` 1,501 行，一个文件承担 help、list/status、add/edit/remove、approve/reject、connect/disconnect/reconnect、OAuth start/complete/status/clear、flag parsing、settings JSON 读写。
- `commands` 目录共 72 个 Rust 文件、23,993 行，随着命令数增长，重复的测试 context、Output 断言和手写解析会持续扩大。

为什么急:

命令系统是用户直接入口，和 `submit_message()` 的 local-command fast path 直接相连。复杂命令继续各自手写 parser 和 persistence，会让行为不一致，尤其是 MCP、plugin、agent/settings 这类同时有 slash command 和 IPC UI 的功能。

建议切法:

1. 把 command metadata 做成小型 declarative builder，`get_all_commands()` 只聚合。
2. `mcp_cmd.rs` 拆成 `commands/mcp/{mod,list,status,config,auth,flags,tests}.rs`。
3. MCP slash command 和 IPC MCP handler 共用 settings mutation helper，避免两套配置写入逻辑。

## 次级但应该排期的区域

### API provider 与 streaming 转换

证据:

- `api/client/mod.rs` 1,050 行，provider enum、配置校验、client construction、retry/count-token/messages 路径集中。
- `api/google_provider.rs` 1,072 行，`api/openai_compat.rs` 940 行，`api/bedrock.rs` 796 行，`api/vertex.rs` 887 行。
- 已有 `api/stream_provider.rs` trait，但 provider 内部的消息转换和 SSE 处理仍分散。

判断:

这块目前不是最大屎山，因为已经有 provider 抽象雏形；但每接一个 provider 或修一个 stream event 语义，都需要警惕跨 provider 行为漂移。

### LSP / plugin 全局 registry

证据:

- `lsp_service/mod.rs:415` 起定义 `LSP_CLIENTS`、`DIAGNOSTICS`、`DELIVERED_DIAGNOSTICS`、`EVENT_TX` 等全局 `LazyLock`。
- `plugins/mod.rs:163` 起定义全局 `REGISTRY` 和 `EVENT_TX`。

判断:

这些全局状态让单测和多 session 隔离变难。短期可以接受，长期应改为 runtime-owned service，并通过 headless/TUI runtime 注入。

### UI facade 清理

证据:

- `src/ui/README.md` 已定义清晰目录职责。
- `src/ui/REFACTOR_EXECUTION.md` 中的 command_surface/app/tui/command_palette 拆分目标大多已经落地。
- 但 `ui/mod.rs`、`ui/messages.rs`、`ui/permissions.rs`、`ui/app.rs` 仍大量使用 `#[allow(dead_code)]` path facade。

判断:

UI 不是当前最急的区域。它已经从“大文件问题”转为“facade 和 dead_code 清理问题”。优先级低于工具执行和 IPC。

### 编码和文档债

证据:

- 多个代码注释和文档中出现 `鈥?`、`鈹€` 等 mojibake。
- `crates/claude-code-rs/Cargo.toml:5` 仍写着 `Rust implementation (lite)`，但根 `AGENTS.md` 已明确当前阶段是 Full Build。
- `docs/TECH_DEBT.md` 存在大量乱码且部分结论已被当前代码结构超越。

判断:

这不一定导致运行时 bug，但会降低新贡献者理解速度。建议把当前文件作为新的审计入口，后续再归档或重写旧 `TECH_DEBT.md`。

## 建议重构顺序

1. **先收敛工具执行事实源**：解决 `QueryDeps::execute_tool` 与 `tools/execution/run_tool_use` 双轨问题。安全边界优先。
2. **拆 `tools/tasks.rs`**：先机械拆生产逻辑和测试，再考虑 `coord/` 子域。
3. **拆 IPC subsystem 聚合文件**：保持 JSON 协议不变，先按 subsystem 分文件。
4. **拆 Query lifecycle 长闭包和测试**：降低每次主路径改动的认知负担。
5. **拆 `commands/mcp_cmd.rs` 并抽共享 parser/helper**：减少 slash command 与 IPC 配置逻辑漂移。
6. **清理 `allow(unused/dead_code)` 和 mojibake**：在前面边界稳定后做，避免和结构改动互相污染 diff。

## 重构守则

- 每次只拆一个边界，不顺手改行为。
- 先补或确认 regression tests，再做机械移动。
- 不新增依赖。
- 优先删除旧双轨代码，而不是再包一层 facade。
- 每个阶段结束至少跑:

```powershell
cargo fmt --all
cargo check -p claude-code-rs
cargo test -p claude-code-rs <targeted-module>
```

## 本次审计边界

已做:

- 统计 `crates/claude-code-rs/src` 文件数、行数、超大文件和目录分布。
- 抽查 P0/P1 热点文件的职责范围和现有注释。
- 对照现有 `src/tools/ARCHITECTURE.md`、`src/ui/README.md`、`src/ui/REFACTOR_EXECUTION.md`、`docs/ipc-refactor-plan.md`、`docs/TECH_DEBT.md`。

未做:

- 未运行完整构建或测试，因为本次只新增审计文档，且当前工作树已有大量非本次改动。
- 未对上游 TypeScript/Bun 实现逐项做行为 diff。
- 未做复杂度工具或 call graph 自动分析；本文件基于源码结构、行数、职责耦合和现有测试/文档证据。
