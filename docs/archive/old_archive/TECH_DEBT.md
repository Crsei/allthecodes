# cc-rust 技术债务

> 更新日期: 2026-05-17 | 当前阶段: 全量构建 / Full Build
>
> 本文件只保留仍需要执行、重评或继续验证的代码层技术债。已经实现、已勘误或已被当前代码结构超越的历史条目已迁移到 [archive/TECH_DEBT.md](archive/TECH_DEBT.md)。
>
> 功能缺口与 intentional crop 看 [IMPLEMENTATION_GAPS.md](IMPLEMENTATION_GAPS.md)，用户可感知问题和审查发现看 [KNOWN_ISSUES.md](KNOWN_ISSUES.md)。

---

## 当前优先级

| 优先级 | 范围 | 当前状态 | 下一步 |
| --- | --- | --- | --- |
| P0 | Query / lifecycle / tool execution 主流程 | `query/loop_impl.rs` 与 `engine/lifecycle/submit_message.rs` 已继续拆分，但 `engine/lifecycle/deps.rs`、Phase D 前置准备、inner stream 事件分发和 `SdkResult` 构造仍偏大 | 先补回归测试，再按“helper 提取，不改协议/行为”的方式继续拆分 |
| P1 | IPC subsystem handlers/types/events | `headless.rs` 已是薄入口，envelope version/min-compat 策略已有 roundtrip tests；subsystem 层仍把 LSP/MCP/Plugin/IDE/Skill/AgentSettings 聚合在大文件里 | 继续补 per-subsystem serialization/roundtrip contract tests，再机械拆到 per-subsystem 模块 |
| P1 | 过度防御性容错 / 静默降级 | 多处把锁失败、已有配置解析失败、认证读取失败或持久化状态损坏折成默认值/空列表/无认证 | 按边界分级：状态写入和安全/认证路径失败必须显式返回错误；可选文件缺失才允许默认值 |
| P1 | API provider 与 streaming 转换 | 已有 `StreamProvider` 抽象和 `RetryConfig`/错误分类，但 provider 内部消息转换、SSE/event 语义仍分散 | 收束 provider-local 转换边界，避免跨 provider 行为漂移 |
| P2 | 全局状态与 runtime registry | `PROCESS_STATE` 已使用 `parking_lot::RwLock` 和部分 accessor，但字段仍大量 `pub`；LSP/plugin registry 仍是全局 `LazyLock` | 逐步转 runtime-owned service / getter-setter，减少多 session 测试污染 |
| P2 | 代码卫生 | `#![allow(unused)]` / `#[allow(dead_code)]`、重复 `test_ctx()`、通配符导入、工具输入解析风格不统一仍存在 | 按模块小批量清理，保留行为测试 |
| P2 | 协议与文档一致性 | IPC envelope 已有显式版本策略；文档和注释中仍有历史 Lite/乱码残留 | 继续清理 mojibake 和过期文档结论，并把新增 wire DTO 纳入 contract tests |

---

## P1: 过度防御性容错 / 静默降级

审计日期：2026-05-07。扫描信号包括 `.ok()`、`unwrap_or_default()`、`let _ =`、`Err(_) => ...` 与 `allow(dead_code/unused)`。这些信号本身不等于问题；本节只保留人工确认后会改变用户可见行为、数据一致性或诊断能力的点。

执行计划：[`docs/plan/p1-defensive-fail-fast-execution-plan-2026-05-07.md`](plan/p1-defensive-fail-fast-execution-plan-2026-05-07.md)。该计划把本节拆成可按测试驱动执行的阶段，并先给出统一 fail-fast 判定表，减少实现时的开放式推理。

### Phase 1 status - 2026-05-07

### Phase 2-6 closure status - 2026-05-07

Detailed completion records for the fail-fast implementation phases are archived in [archive/TECH_DEBT.md](archive/TECH_DEBT.md). The active P1 fail-fast debt is no longer the completed behavior changes; it is limited to the remaining risks below and future follow-up hardening.

| Phase | Status | Changed files | Verification recorded |
| --- | --- | --- | --- |
| Phase 2 - Hook criticality and fail-closed policy | Completed | `crates/cc-types/src/hooks.rs`; `crates/claude-code-rs/src/tools/hooks/{mod.rs,pre_tool.rs,post_tool.rs,execution.rs}`; `crates/claude-code-rs/src/engine/lifecycle/deps.rs`; `crates/claude-code-rs/src/query/loop_tests.rs` | `cargo test -p claude-code-rs tools::hooks -- --nocapture`; `cargo test -p claude-code-rs engine::lifecycle -- --nocapture`; `cargo check -p claude-code-rs --message-format short` |
| Phase 3 - Single canonical tool execution path | Completed | `crates/claude-code-rs/src/tools/mod.rs`; deleted `crates/claude-code-rs/src/tools/orchestration.rs`; `crates/claude-code-rs/src/tools/hooks/{mod.rs,post_tool.rs}` | `cargo test -p claude-code-rs tools::execution -- --nocapture`; `cargo test -p claude-code-rs query::loop_helpers -- --nocapture`; `cargo test -p claude-code-rs tools::hooks -- --nocapture`; `cargo check -p claude-code-rs --message-format short` |
| Phase 4 - Existing config/data must not masquerade as missing | Completed | `crates/cc-mcp/src/discovery.rs`; `crates/cc-auth/src/{lib.rs,codex_cli.rs}`; `crates/cc-types/src/permissions.rs`; `crates/claude-code-rs/src/{main.rs,engine/system_prompt.rs,ipc/subsystem_handlers.rs,plugins/loader.rs,plugins/mod.rs,plugins/refresh.rs,commands/memory.rs,commands/model_add.rs,commands/doctor.rs,commands/login.rs,commands/logout.rs,commands/voice_cmd.rs,startup/mod.rs,startup/runtime_config.rs,api/client/mod.rs}` | `cargo test -p cc-mcp discovery -- --nocapture`; `cargo test -p claude-code-rs plugins -- --nocapture`; `cargo test -p claude-code-rs commands::memory -- --nocapture`; `cargo test -p claude-code-rs commands::model_add -- --nocapture`; `cargo test -p claude-code-rs startup -- --nocapture`; `cargo test -p cc-types permissions -- --nocapture`; `cargo test -p cc-auth --lib`; `cargo check -p claude-code-rs --message-format short` |
| Phase 5 - Protocol and runtime result cardinality | Completed | `crates/claude-code-rs/src/query/loop_helpers.rs`; `crates/claude-code-rs/src/api/streaming.rs`; `crates/claude-code-rs/src/engine/output_style.rs`; `crates/claude-code-rs/src/engine/system_prompt.rs`; `crates/claude-code-rs/src/commands/config_cmd.rs` | `cargo test -p claude-code-rs query::loop_helpers -- --nocapture`; `cargo test -p claude-code-rs api::streaming -- --nocapture`; `cargo test -p claude-code-rs engine::output_style -- --nocapture`; `cargo check -p claude-code-rs --message-format short` |
| Phase 6 - Team coordination and worktree isolation | Completed | `crates/claude-code-rs/src/teams/runner.rs`; `crates/claude-code-rs/src/engine/agent/supervisor.rs` | `cargo test -p claude-code-rs teams::runner -- --nocapture`; `cargo test -p claude-code-rs engine::agent -- --nocapture`; `cargo check -p claude-code-rs --message-format short` |

### Remaining P1 follow-ups

- 2026-05-08 follow-up closure sessions resolved and archived the MCP startup diagnostics, auth command diagnostic surfaces, hook IO public diagnostics, and team smoke coverage follow-ups. Evidence is recorded in `target/codex-runs/session-01-mcp-startup-diagnostics/task-01.last-message.txt` through `target/codex-runs/session-05-team-e2e-coverage/task-01.last-message.txt`, with Review B in `target/codex-runs/review-b-final-integration/task-01.last-message.txt`.
- 2026-05-08 residual Review B closure sessions resolved and archived the critical post-tool/post-failure hook propagation gap and the `/reload_plugins` global diagnostics visibility gap. Evidence is recorded in `target/codex-runs/p1-review-b-residual-session-01-hook-propagation/task-01.last-message.txt`, `target/codex-runs/p1-review-b-residual-session-02-plugin-reload-diagnostics/task-01.last-message.txt`, and `target/codex-runs/p1-review-b-residual-review-a/task-01.last-message.txt`.
- Auth compatibility wrappers now call the diagnostic `try_*` APIs and log warnings for compatibility; external callers that need actionable errors should call `try_resolve_auth()` / `try_resolve_codex_auth_token()` directly.
- Plugin filtered tests passed in Session 4 and the residual reload-output visibility gap was later closed; active risk is limited to global registry/env isolation in plugin tests if that path is touched again.
- Session 5 added filtered/unit-smoke coverage for worktree isolation fallback visibility, but a full multi-process team E2E run is still not recorded.
- Broad lint allows remain in modules outside the completed phase touch set and should be handled by the general code hygiene track, not this fail-fast closure.

### Phase 1 status detail - 2026-05-07

The original scan tables after this status note are retained only as source context for completed Phase 1-6 work. They should not be treated as the current active fail-fast TODO list; completed records live in [archive/TECH_DEBT.md](archive/TECH_DEBT.md), and the remaining active follow-ups are listed above.

- Status: implemented strict persistence/lock boundaries for TaskStore write operations and mailbox append.
- Changed files: `crates/claude-code-rs/src/tools/tasks/store.rs`, `crates/claude-code-rs/src/tools/tasks/task_tools.rs`, `crates/claude-code-rs/src/tools/tasks/tests.rs`, `crates/claude-code-rs/src/teams/mailbox.rs`, `crates/claude-code-rs/src/engine/agent/supervisor.rs`, `crates/claude-code-rs/src/commands/tasks_cmd.rs`.
- Evidence: `cargo test -p claude-code-rs tools::tasks:: -- --nocapture`; `cargo test -p claude-code-rs teams::mailbox -- --nocapture`; `cargo check -p claude-code-rs --message-format short`.
- Remaining risk: global teammate unassign still returns the legacy summary shape, so caller-level propagation for that coordination path should be handled with the later team coordination phase.

### 判定标准

- 可选文件不存在时返回默认值是合理容错。
- 已存在的配置/凭据/状态文件读取或解析失败后直接按默认值继续，是过度防御。
- 清理临时文件、向已关闭 channel 发送通知这类 best-effort 操作不列入本节。
- 状态写入、锁、认证、安全和协议边界失败不应静默降级。

### 已确认问题

| 优先级 | 范围 | 证据 | 风险 | 建议 |
| --- | --- | --- | --- | --- |
| P1 | TaskStore 写入路径拿不到任务锁后继续写 | `crates/claude-code-rs/src/tools/tasks/store.rs:314`、`:378`、`:401`、`:581`、`:615`、`:653` 都把 `acquire_task_list_lock(...).ok()` 转为可选 guard；而 `claim_task()` 在 `:478` 已经把锁失败显式映射为 `LockUnavailable` | 锁超时或创建失败时，create/update/delete/stop 会退回内存快照继续写，绕过 repository refresh，可能覆盖并发 teammate/agent 的任务状态 | 写操作统一返回 `Result` 或工具错误；只允许显式 stale-lock recovery，不允许持久化写入退回内存态 |
| P1 | MCP 配置发现把已有配置错误折成“没有服务器” | `crates/cc-mcp/src/discovery.rs:175` 和 `:188` 对 user/project settings 使用 `if let Ok(configs)`；`:211` 解析单个 server 失败直接跳过；调用端如 `crates/claude-code-rs/src/main.rs:438`、`startup/fast_paths.rs:68`、`ipc/subsystem_handlers.rs:1016`、`:1027`、`:1109`、`engine/system_prompt.rs:755` 再用 `unwrap_or_default()` | `settings.json` 存在但 JSON 或某个 server 配置损坏时，MCP server 会从启动、状态面板和系统提示中静默消失，用户只看到“没配置” | 缺失文件返回空；已存在文件的读/解析错误必须产生日志和 UI/IPC 诊断；单个 server 无效时保留 name/scope/error |
| P1 | 配置写入前的 load-or-default 可能覆盖坏配置 | `crates/claude-code-rs/src/commands/memory.rs:508` 在 `/memory auto` 写入前对 `load_global_config()` 使用 `unwrap_or_default()`；`crates/claude-code-rs/src/commands/model_add.rs:122` 读取 `.env` 失败时按空文件继续保留/重写 | 全局 settings 或 `.env` 已存在但不可读/非法时，后续写入会按空配置重建，丢失原字段、注释或用户手写内容 | 区分“文件不存在”和“存在但读/解析失败”；后者应中止 mutation 并返回可操作错误 |
| P2 | Auth resolution 把凭据错误折成无认证 | `crates/cc-auth/src/lib.rs:132`、`:146` 只接受 `Ok(Some(...))`；`:181` 对 `load_token()` 使用 `.ok().flatten()`；`:320`-`:323` 在刷新失败时清除 credentials 并返回 `Ok(None)` | 凭据文件损坏、keychain 读取失败或临时网络/服务错误看起来都像未登录；refresh 临时失败可能删除仍可诊断的 token | 引入带 warnings 的 auth resolution 结果；只在确认 revoked/invalid_grant 时清凭据，其他错误应保留凭据并提示 |
| P2 | Team mailbox 写入路径会把坏 mailbox 重置为空 | `crates/claude-code-rs/src/teams/mailbox.rs:115`-`:118` 写入前读/解析失败时使用 `"[]"` 或 `unwrap_or_default()`；同文件 `read_mailbox()` 在 `:76`-`:87` 已经是严格错误返回 | mailbox 文件一旦损坏，下次写入会覆盖为只含新消息，历史消息丢失；严格读和宽松写行为不一致 | 写入路径复用严格读取；解析失败时保留原文件，写入 `.corrupt`/backup 或直接返回错误 |
| P2 | Hook 执行 IO 过度 best-effort | `crates/claude-code-rs/src/tools/hooks/execution.rs:37`-`:40` 忽略 stdin 写入/flush 错误，`:58`、`:65` 忽略 stdout/stderr 读取错误，`:102` 超时 kill 也忽略结果 | hook 进程未收到输入、输出被截断或 kill 失败时，调用方只能看到不完整 hook 结果；权限/安全类 hook 难以 fail closed | 保留“进程提前退出”的宽容分支，但把 IO 错误记录到 hook 结果或 tracing；安全相关 hook 失败应可配置为 fail closed |
| P2 | 大范围 lint allow 掩盖未完成边界 | 本次扫描命中约 211 处 `allow(dead_code/unused/unused_imports)`，例如 `crates/claude-code-rs/src/ipc/subsystem_events.rs:12`、`subsystem_types.rs:13`、`plugins/mod.rs:14`、`teams/runner.rs:8`、`tools/orchestration.rs:1` | 编译器无法帮助发现旧 facade、未接线类型和已经失效的兼容分支；Full Build 阶段容易把“未来会用”误当已实现 | 模块级 allow 改成最小作用域；每个保留项附 issue/计划；每清一个边界先跑对应模块测试 |

### Subagent 补充：过度设计 / fail-fast 债务

2026-05-07 追加。按用户要求构建 3 个 subagent 分别检查 config/auth/MCP/startup、task/team/mailbox/coordination、query/tool execution/hooks/API streaming。以下条目与上表重叠时不重复展开，只补充更明确的 fail-fast 收敛点。

| 优先级 | 范围 | 证据 | 风险 | 建议 |
| --- | --- | --- | --- | --- |
| P1 | Hook 策略整体 fail-open | `crates/claude-code-rs/src/tools/hooks/pre_tool.rs:92`-`:99` 和 `engine/lifecycle/deps.rs:786`-`:814` 对 pre-tool hook 错误只 warn 后继续；`tools/hooks/post_tool.rs:141`-`:148`、`:179`-`:197`、`:239`-`:246` 对 post/failure/stop/notification hook 错误也继续 | Hook 很可能承载权限、安全、审计或团队策略；配置错误、脚本不可执行、运行时错误会被包装成“策略未触发”，问题不会在第一现场暴露 | 给 hook 配置增加 critical/optional 语义；critical hook 失败时中止当前 step/tool，optional hook 才允许 warn-only |
| P1 | 旧工具执行管线与 canonical 管线并存 | `crates/claude-code-rs/src/tools/orchestration.rs:1` 使用 `#![allow(unused)]`；`tools/mod.rs:25` 仍导出该模块；而 `tools/execution/mod.rs:1`-`:6` 已声明 `QueryDeps::execute_tool` 是完整执行入口；`tools/orchestration.rs:113`-`:240` 仍重复 validation、hook、permission、tool-call、post-hook 逻辑 | 两套执行路径会漂移：hook 错误、权限错误、结果截断、审计记录可能出现不一致；未使用代码被 allow 掩盖后难以及时删除 | 删除旧 orchestration，或改成明确的 test-only fixture；生产路径只保留 `QueryDeps::execute_tool` |
| P1 | 并发工具 JoinError 丢失 tool result | `crates/claude-code-rs/src/query/loop_helpers.rs:431`-`:464` 中 spawned task panic/JoinError 只记录 `tool task panicked`，没有为原始 `tool_use_id` 生成失败结果；`Ok(Err(e))` 分支还使用 `unknown` tool id/name | 模型发出的 tool_use 数量和返回的 tool_result 数量可能不匹配；panic 被日志吞掉后，下一轮对话上下文缺失失败结果 | spawn 前把 `tool_use_id`、`tool_name` 带入 join context；任何 JoinError 都合成对应失败 `ToolExecResult` |
| P1 | Startup/env/auth/permission 配置错误被当作默认状态 | `crates/claude-code-rs/src/startup/mod.rs:22`-`:30` 对 `.env` 加载使用 `let _ =`；`crates/cc-types/src/permissions.rs:39`-`:46` 把未知 permission mode 解析为 `Default`；`crates/cc-auth/src/lib.rs:118`-`:119`、`:350` 将无效 key 或缺 Tokio runtime 继续折成无认证/无 refresh | 用户明明配置了值，但拼写、权限或运行时问题会表现为“没配置”或默认权限，排查成本高，安全边界也不清晰 | 对“文件存在但不可读/非法”“环境变量存在但非法”“枚举值未知”统一返回诊断；只有真正缺失才使用默认值 |
| P1 | Plugin loader 把元数据损坏折成无插件 | `crates/claude-code-rs/src/plugins/loader.rs:31`-`:60` 读取或解析 `installed_plugins.json` 失败时返回空列表；`:99`-`:133` 扫 cache 目录时对遍历和 manifest 错误继续；`plugins/mod.rs:488`-`:494` 只注册 loader 返回值；`plugins/refresh.rs:53`-`:56` 还备注 init 会吞错误 | 插件元数据损坏、磁盘权限错误或 manifest 破损会让插件静默消失；刷新逻辑看到的是“没有插件”，不是“插件状态不可用” | loader 返回 `Result<LoadedPlugins, Diagnostics>`；startup/IPC 状态面板展示损坏项，避免自动覆盖或静默清空 |
| P1 | Team 协调路径过度宽松 | `crates/claude-code-rs/src/teams/mailbox.rs:71`-`:76` 明确 no locking/best-effort 读取；`:105`-`:118` 写入前把坏 mailbox 当空列表；`:190`-`:258` 重试后会强制移除 lock；`teams/runner.rs:224` mailbox 错误只 warn；`:423`-`:459` 对 `ShutdownRequest` 自动批准；`engine/agent/supervisor.rs:575`-`:604` worktree 隔离失败后退回 normal cwd | 团队协作、shutdown 和 worktree isolation 都是高风险边界；失败后继续可能让多个 agent 在错误状态下协同写入或失去隔离 | mailbox 读写统一加锁和严格解析；runner 对协调状态错误 fail task；worktree fallback 需要显式 opt-in 和用户可见 warning |
| P2 | API streaming parser 对 required fields 使用默认值 | `crates/claude-code-rs/src/api/streaming.rs:21`-`:33`、`:39`-`:50` 对 usage、index、content_block、delta 等字段使用 `unwrap_or_default()` / `unwrap_or(0)` | 上游 SSE 协议漂移或响应损坏时会被映射到 index 0/default block，后续状态机可能在错误上下文里继续 | required 字段解析失败应返回 stream parse error；只对协议明确 optional 的字段保留默认值 |
| P2 | Output style 配置错误静默回到 Default | `crates/claude-code-rs/src/engine/output_style.rs:45`-`:66` 中未知 style 会尝试磁盘查找，找不到自定义文件时回到 `Default`；`crates/cc-config/src/validation.rs:277`-`:299` 对未知 style 仅给 Info | 用户拼错 style 或自定义文件丢失时，系统提示词悄悄变回默认风格，实际行为和配置界面不一致 | `resolve` 返回带 warning/error 的结果；prompt/status 面板显示当前 style 是否由 fallback 得到 |

### 收敛顺序

1. 先修 TaskStore 锁失败继续写：这是状态一致性风险，且 `claim_task()` 已提供显式失败模式。
2. 紧接收束 Hook 策略和旧 `tools/orchestration.rs`：一个关系到策略 fail-closed，一个关系到执行路径漂移。
3. 再修 startup/MCP/plugin/config/auth 的“已有文件或已有值错误被当成缺失”：这些是用户最难自查的静默失败。
4. 然后修 mailbox/team/worktree isolation：协调和隔离失败不能默认继续协同写入。
5. 最后处理 API streaming、output style 和 lint allow：先加诊断，再按模块收窄。

---

## P0: Query / lifecycle / tool execution 主流程

### 已完成的本轮拆分

- `query/loop_impl.rs`：已抽出 `QueryRunContext` 与 `prepare_model_request()`，把每轮模型请求前的 microcompact、PreCompact/PostCompact hook、tool refresh、autocompact 与 `ModelCallParams` 组装移出主循环。
- `engine/lifecycle/submit_message.rs`：已抽出 `SubmitTurnState`、本地 slash command 分发、`/clear` 会话切换、系统提示词构建、memory recall 分支与 `InstructionsLoaded` hook 触发。

### 仍需继续拆分

1. **Lifecycle Phase D 前置准备**：继续从 `submit_message()` 抽出 API client / Langfuse trace 初始化。建议 helper 返回 `Result<ApiSetup, SdkMessage>`，保持“缺失 API provider 时立即 yield Result 并返回”的现有行为。
2. **Query inner stream 事件处理**：将 `while let Some(item) = inner_stream.next().await` 中的 `QueryYield` 分发拆成效果处理器。建议先引入小枚举表达 `Emit` / `Finish` / `Continue`，避免 helper 直接拥有 async stream 的 `yield` 语义。
3. **Result 构造去重**：在 `submit_message.rs` 中提取本地命令结果、最大轮次、预算耗尽、缺失 provider 等 `SdkResult` 构造 helper。保持 helper 只承载字段一致性，不提前抽象业务分支。
4. **边界文件再收敛**：如果 `submit_message.rs` 继续增长，将本地命令处理迁入独立 `lifecycle/local_command.rs`，系统提示词构建迁入 `lifecycle/system_prompt_build.rs`。移动前先确保测试覆盖 slash command fast path、`/clear` session rotation、memory recall 和 hook 顺序。
5. **Tool execution / deps 边界**：`engine/lifecycle/deps.rs` 仍承载 ToolUseContext 组装、validation/security、pre/post hooks、权限询问、工具调用、result size enforcement 和审计。继续拆分前先锁定 `query::loop_helpers`、`engine::lifecycle::deps` 与 `tools::execution` 回归。

---

## P1: IPC subsystem 聚合

来源：`docs/tmp/claude-code-rs-refactor-audit-2026-05-07.md`

### 受影响文件

- `crates/claude-code-rs/src/ipc/subsystem_handlers.rs`
- `crates/claude-code-rs/src/ipc/subsystem_events.rs`
- `crates/claude-code-rs/src/ipc/subsystem_types.rs`
- `crates/claude-code-rs/src/ipc/agent_settings.rs`

### 证据

- `subsystem_handlers.rs` 同时处理 LSP、MCP、Plugin、IDE、Skill、AgentSettings 命令和 status snapshot builder。
- `subsystem_events.rs` 同时定义多个 subsystem 的 event enum、command enum 和 event bus。
- `subsystem_types.rs` 与 `subsystem_events.rs` 仍用 `#![allow(dead_code)]` 预定义未来扩展类型。
- `ipc/headless.rs` 已经是薄入口，但 subsystem 层没有完成同等拆分。
- `docs/ipc-refactor-plan.md` 仍是活跃计划，但内容早于当前结构，不能当作完成状态。

### 建议切法

1. 建立 `ipc/subsystems/{lsp,mcp,plugin,ide,skill,agent_settings}/`。
2. 每个子域自带 `types.rs`、`events.rs`、`handlers.rs`，顶层只保留统一 `SubsystemEvent` 和 event bus。
3. 先补 serialization/roundtrip contract tests，再机械迁移。不要同时修改 JSON 协议。

---

## P1: API provider 与 streaming 转换

### 当前状态

- `api/stream_provider.rs` 已提供 `StreamProvider` trait，解决了早期 provider routing 大量 match 的问题。
- `api/retry.rs` 已有 `RetryConfig`、`ApiErrorCategory` 与 `categorize_stream_start_error()`，早期“重试循环重复 ~90 行”的结论已归档。
- Provider 内部的消息转换、SSE event 语义和错误字符串解析仍分散在 `api/google_provider.rs`、`api/openai_compat.rs`、`api/bedrock.rs`、`api/vertex.rs` 等文件中。

### 剩余风险

- 每接一个 provider 或修一个 stream event 语义，都容易引入跨 provider 行为漂移。
- `api/retry.rs` 仍有字符串匹配错误分类，例如 prompt-too-long / max-tokens 等分支依赖响应文本。

---

## P2: 全局状态与 registry

### `PROCESS_STATE`

`crates/cc-bootstrap/src/state.rs` 已从 `std::sync::RwLock` 迁到 `parking_lot::RwLock`，并有部分 convenience accessors；但以下债务仍存在：

- `ProcessState` 仍有大量 `pub` 字段，跨模块可直接修改。
- `total_cost_usd` 仍是 `f64` 字段，写入需要持有全局写锁。
- 代码注释仍强调 “DO NOT ADD MORE STATE HERE”，说明该单例应继续保持收敛。

建议继续增加小粒度 accessor/mutator，并把频繁更新字段迁出全局写锁路径。

### LSP / plugin registry

- `lsp_service/mod.rs` 仍维护 `LSP_CLIENTS`、`DIAGNOSTICS`、`DELIVERED_DIAGNOSTICS`、`EVENT_TX` 等全局 `LazyLock`。
- `plugins/mod.rs` 仍维护全局 `REGISTRY` 和 `EVENT_TX`。

这些全局状态让单测和多 session 隔离变难。短期可以接受，长期应改为 runtime-owned service，并通过 headless/TUI runtime 注入。

---

## P2: 代码卫生

### `#![allow(unused)]` / `#[allow(dead_code)]`

当前仍能在 plugins、teams、engine、ipc、daemon、tools 等模块中看到 allow 指令。清理策略：

1. 先分模块确认是未来扩展、测试可见性还是已死代码。
2. 已死代码优先删除。
3. 未来扩展保留时需要写明调用计划或 issue/plan 引用。

### 重复测试样板

多个 command 测试仍各自定义 `test_ctx()`。建议收敛到 command test helper，避免以后新增字段时重复改几十处。

### 通配符导入

`use crate::types::tool::*` 仍出现在 agent、MCP、web、task、fs 等路径中。建议按文件逐步改为显式导入，降低依赖面噪音。

### 工具输入解析方式不一致

当前仍混用手写 `parse_input()`、`call()` 内联解析和 serde 结构体反序列化。新工具优先使用 serde 结构体；旧工具迁移时按风险和测试覆盖分批处理。

### 模型元数据 registry 未完全统一

模型别名已收敛到 `model_registry.rs`，但 marketing name / knowledge cutoff 仍在 `cc-config::constants` 中独立维护。后续可以把 alias、展示名、cutoff 等合并为单一 `ModelInfo` 表，减少漂移。

---

## P2: 协议与文档一致性

### IPC 协议版本策略

`cc-ipc-protocol` 的 `IpcEnvelope` 已声明 `IPC_ENVELOPE_VERSION` / `IPC_ENVELOPE_MIN_COMPAT_VERSION`，缺省 version 会按当前 wire contract 解码，future version 可被检测为 incompatible。剩余风险是 subsystem DTO 中仍有泛型 `serde_json::Value` 载荷，前后端可能在无感知情况下漂移；后续新增或拆分 subsystem 时必须补 serialization/roundtrip contract tests。

### UI facade 清理

Rust TUI 已有更清晰的目录职责，早期“大文件问题”大多转为 facade 和 `dead_code` 清理问题。优先级低于 Query/tool execution 与 IPC，但后续改 UI 时应顺手减少 path facade。

### 编码和文档债

多个代码注释和文档仍有历史 mojibake、Lite wording 或过期行数。后续整理规则：

- 活跃 TODO 只保留在 `TECH_DEBT.md`、`IMPLEMENTATION_GAPS.md`、`KNOWN_ISSUES.md` 等入口。
- 已完成内容迁入 `docs/archive/`。
- 触及历史 Lite 结论时按 Full Build 语义重评。

---

## 归档规则

完成任何本文件条目后：

1. 在同一 PR 中把完成记录移到 [archive/TECH_DEBT.md](archive/TECH_DEBT.md)。
2. 写清楚完成证据：关键文件、测试或构建命令、剩余风险。
3. 如果条目只是误报或被新的架构超越，也归档，但标记为“已勘误”或“已过期”，不要继续留在活跃债务入口。
