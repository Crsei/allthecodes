# 已完成模块 — 完整实现

> 最后更新: 2026-05-21
> 此文档记录与 TypeScript 原版功能对等或接近完整的已完成模块。
> 大幅简化的模块见 [`COMPLETED_SIMPLIFIED.md`](COMPLETED_SIMPLIFIED.md)。
> 剩余工作见 [`REWRITE_PLAN.md`](../REWRITE_PLAN.md)。

---

## 2026-05-18 Anthropic-compatible coding closeout

| 模块 | 文件 | 验证 | 说明 |
|------|------|------|------|
| Provider smoke matrix and Anthropic coding reference | `scripts/provider_smoke_matrix.py`, `docs/reference/anthropic_coding.md`, `docs/IMPLEMENTATION_GAPS.md`, `docs/WORK_STATUS.md` | `python3 -m py_compile scripts/provider_smoke_matrix.py`; `scripts/provider_smoke_matrix.py list`; `scripts/provider_smoke_matrix.py mock`; 2026-05-21 `scripts/provider_smoke_matrix.py real` partial credential run | Provider smoke matrix 覆盖 direct Anthropic API key、direct bearer、compatible bearer + custom base URL、Bedrock/Vertex model mapping、prompt cache disabled/enabled gates；真实 smoke 只在所需 env 存在时运行，并 redacts tokens/auth headers。2026-05-21 real run 通过 direct bearer、compatible custom base URL bearer、prompt-cache default 与 TTL/global gates；direct API key、Bedrock、Vertex 因缺少对应凭据跳过。Reference 文档补齐 compatible coding mode、SOTA/MOTA/FOTA、legacy fallback、prompt cache knobs 与 authority refs。 |

---

## 2026-05-17 P1 release closeout updates

| 模块 | 文件 | 验证 | 说明 |
|------|------|------|------|
| Provider validation DTO and Foundry fail-early | `crates/cc-api/src/api/providers.rs`, `crates/cc-api/src/api/client/mod.rs`, `crates/cc-commands/src/login.rs` | `cargo test -p cc-api provider_validation`; `cargo test -p cc-api test_from_env_result_errors_for_explicit_foundry`; `cargo test -p cc-commands login` | Provider capability matrix 可序列化为 validation DTO；Azure OpenAI alias 归一到 `azure`，Foundry/Azure Foundry 归一到 unsupported `azure-foundry` 并在显式 env selection 时 fail early；`/login status` 会显示 Foundry unsupported diagnostic，Bedrock/Vertex session shortcuts 会清理 stale Foundry flag。 |
| Session export schema v2 request snapshots | `crates/cc-session/src/request_snapshot.rs`, `crates/cc-session/src/session_export/*`, `crates/cc-engine/src/lifecycle/deps.rs`, `crates/cc-commands/src/session_export.rs` | `cargo test -p cc-session request_snapshot`; `cargo test -p cc-session session_export`; `cargo test -p cc-commands session_export`; `cargo test -p cc-engine lifecycle` | `cc-engine` 在 provider request 发送前记录最终 sanitized request snapshot；session export schema v2 增加 raw transcript、api view summary、api request snapshots、custom title 和图片块 metadata 占位。 |
| IPC envelope version policy | `crates/cc-ipc-protocol/src/envelope.rs`, `crates/cc-ipc-protocol/src/lib.rs` | `cargo test -p cc-ipc-protocol envelope` | `IpcEnvelope` 暴露 current/min-compatible version，缺失 version 默认按 v1 解码，future version 可被检测为 incompatible。 |
| WebFetch HTTP-only release boundary | `docs/IMPLEMENTATION_GAPS.md`, `docs/FINAL_RELEASE_PLAN.md` | 文档门禁；既有 WebFetch HTTP tests 见 2026-05-05 rows | WebFetch 发布支持面固定为 HTTP fetch：redirect/MIME/proxy/NO_PROXY/credential boundary；browser-grade JS rendering 登记为 intentional crop。 |

---

## 2026-05-05 Full Build parity 更新

| 模块 | 文件 | 验证 | 说明 |
|------|------|------|------|
| FileReadTool | `tools/fs/file_read.rs` | `cargo test -p claude-code-rs file_read` | 补齐 symlink canonicalize/metadata、UTF-8/UTF-16/BOM 编码检测、UTF-8 lossy fallback、大文件默认分页与 `next_offset` |
| BashTool heredoc validation | `tools/exec/bash.rs`, `crates/cc-utils/src/bash.rs` | `cargo test -p cc-utils bash`; `cargo test -p claude-code-rs bash` | 执行前拒绝未闭合或畸形 heredoc，覆盖 quoted delimiter、`<<-`、多 heredoc 与 arithmetic shift/quoted text 规避 |
| BashTool Git operation tracking | `tools/exec/bash.rs`, `tools/exec/powershell.rs`, `crates/cc-utils/src/git_operation_tracking.rs` | `cargo test -p cc-utils git_operation_tracking`; `cargo test -p claude-code-rs bash`; `cargo test -p claude-code-rs powershell` | 成功 shell 结果附带 `git_operations` 元数据，覆盖 commit/amend/cherry-pick、push branch、merge/rebase、`gh pr`、`glab mr create` 与 curl PR endpoint |
| BashTool process-tree termination | `tools/exec/process_control.rs`, `tools/exec/bash.rs`, `tools/exec/powershell.rs` | `cargo test -p claude-code-rs process_control`; `cargo test -p claude-code-rs bash`; `cargo test -p claude-code-rs powershell` | Bash/PowerShell 统一使用 process group / process-tree termination；超时与 abort signal 会杀进程树并返回 `termination` 元数据 |
| Bash/PowerShell destructive denylist | `crates/cc-permissions/src/dangerous.rs`, `tools/execution/security.rs` | `cargo test -p cc-permissions dangerous`; `cargo test -p claude-code-rs bash`; `cargo test -p claude-code-rs powershell` | 补齐上游 destructive warning 覆盖面：force-with-lease、`git clean` dry-run 例外、stash drop/clear、SQL drop/truncate、PowerShell `Remove-Item`/`Clear-Content`/磁盘与系统 cmdlet |
| PowerShell security validator high-risk batch | `crates/cc-permissions/src/dangerous.rs`, `tools/execution/tests.rs` | `cargo test -p cc-permissions dangerous`; `cargo test -p claude-code-rs powershell` | 覆盖 `Invoke-Expression`、嵌套 PowerShell、download cradle、`Add-Type`、COM object、`Start-Process` 提权/再拉 PowerShell；WMI/CIM 对 `Invoke-WmiMethod`/`iwmi`/`Invoke-CimMethod` 任意调用 fail-closed，避免 `$class`/`$method` 动态参数绕过 |
| PowerShell security validator high-risk follow-up | `crates/cc-permissions/src/dangerous.rs`, `tools/execution/tests.rs` | `cargo test -p cc-permissions dangerous`; `cargo test -p claude-code-rs test_powershell_dangerous_command_blocked` | 覆盖 standalone download utilities、script file execution、`ForEach-Object -MemberName`、`Invoke-Item`、scheduled task persistence、env mutation、module/script loading、alias/variable runtime-state mutation 与 alternative parameter prefixes |
| PowerShell security validator targeted syntax batch | `crates/cc-permissions/src/dangerous.rs` | `cargo test -p cc-permissions dangerous` | 覆盖动态调用 `Invoke-Expression`、危险 cmdlet script block、`ForEach-Object` script block、stop-parsing `--%` 与明显危险的 .NET static method 调用 |
| PowerShell security validator AST heuristic batch | `crates/cc-permissions/src/dangerous.rs` | `cargo test -p cc-permissions dangerous`; `cargo test -p claude-code-rs powershell`; `cargo test -p claude-code-rs test_powershell_dangerous_command_blocked` | quote-aware 执行前硬拦覆盖一般 dynamic command name、dot-sourced dynamic command、subexpression、expandable string、splatting、member/static member invocation 与非 CLM allowlist type literal |
| PowerShell nameType application command guard subset | `crates/cc-permissions/src/dangerous.rs` | `cargo test -p cc-permissions test_powershell_security_validator_patterns`; `cargo test -p cc-permissions` | 执行安全门拒绝直接执行本地脚本/应用形式的 command name：`.\payload.ps1`、`scripts\Out-Null.ps1`、dot-source profile 脚本、路径限定 `.exe` 等，避免路径命令名被 strip 后伪装成安全 cmdlet |
| PowerShell parser-invalid fail-closed | `tools/exec/powershell.rs`, `crates/cc-permissions/src/dangerous.rs`, `tools/execution/tests.rs` | `cargo test -p claude-code-rs powershell_native_parser`; `cargo test -p claude-code-rs powershell`; `cargo test -p cc-permissions powershell_obvious_parse_errors` | `PowerShellTool::validate_input()` 调用 PowerShell 原生 `Parser.ParseInput()` 并在 parser errors 时执行前拒绝；权限库保留明显未闭合 quote/paren/brace/type literal 与 mismatched delimiter 的轻量 fallback |
| PowerShell native AST metadata security gate | `tools/exec/powershell_parser.rs`, `tools/exec/powershell.rs` | `cargo fmt -p claude-code-rs --check`; `cargo test -p claude-code-rs powershell_native_parser` | `PowerShellTool::validate_input()` 现在解析原生 AST metadata，复核 `elementTypes`、colon-bound `children`、raw command name / `nameType` 语义与 statement `securityPatterns`，拒绝动态 command name、路径式 command name、colon-bound 表达式、splatting、stop-parsing、`using` / `#Requires` 与非安全 script block；保留 Where/Sort/Select/Group/Format 过滤/输出 script block |
| PowerShell parameter-binding security subset | `crates/cc-permissions/src/dangerous.rs` | `cargo test -p cc-permissions powershell_security_validator_patterns`; `cargo test -p cc-permissions` | 覆盖上游参数绑定语义的执行前硬拦子集：`Start-Process -Verb:RunAs` 冒号/quote/backtick 形式、`Start-Job` / `Start-ThreadJob` 位置脚本文件参数、`ForEach-Object` / `%` 位置 `MemberName` 参数 |
| PowerShell New-Object TypeName CLM subset | `crates/cc-permissions/src/dangerous.rs` | `cargo test -p cc-permissions powershell_new_object_typename_clm_boundary`; `cargo test -p cc-permissions` | `New-Object` 的 `-TypeName` / `-t:` / 位置 TypeName 参数会按 ConstrainedLanguage allowlist 校验，拒绝 `System.Net.WebClient`、`System.Diagnostics.Process`、`System.IO.FileInfo` 等非 allowlist .NET 类型 |
| PowerShell script-block securityPatterns subset | `crates/cc-permissions/src/dangerous.rs`, `tools/execution/tests.rs` | `cargo test -p cc-permissions powershell_script_blocks`; `cargo test -p cc-permissions dangerous`; `cargo test -p claude-code-rs test_powershell_dangerous_command_blocked`; `cargo build -p claude-code-rs --release` | 以 quote-aware 扫描模拟上游 `securityPatterns.hasScriptBlocks` 的关键安全门：非安全消费者的 script block fail-closed，仅允许 Where/Sort/Select/Group/Format 过滤与输出消费者 |
| Bash/PowerShell sandbox filesystem preflight | `crates/cc-sandbox/src/runner.rs` | `cargo test -p cc-sandbox preflight_shell_command`; `cargo test -p claude-code-rs bash`; `cargo test -p claude-code-rs powershell` | `preflight_shell_command()` 在网络检查外增加显式写目标检查，覆盖 redirection、常见 Bash 写命令、PowerShell 写 cmdlet，并执行 read-only/workspace/allowWrite/denyWrite 策略 |
| Bash/PowerShell sandbox fail-closed control | `commands/sandbox_cmd.rs`, `crates/cc-sandbox/src/runner.rs` | `cargo test -p claude-code-rs sandbox_cmd`; `cargo test -p cc-sandbox unsupported_runner_hard_fails_when_configured` | `/sandbox require` / `/sandbox optional` 在会话内切换 `sandbox.failIfUnavailable`；缺少 OS-level primitive 时可明确硬失败或退回 Rust-level preflight |
| BashTool Windows sandbox parity decision | `crates/cc-sandbox/src/availability.rs`, `crates/cc-sandbox/src/lib.rs`, `crates/cc-sandbox/src/filesystem.rs` | `cargo test -p cc-sandbox`; `cargo fmt --all --check` | Windows Restricted Token / Job Object OS-level primitive 按上游平台边界移入 Intentional 裁剪：上游 sandbox-runtime/PowerShell UI 当前不支持 Windows sandbox，cc-rust 保留 Rust-level FS/network preflight、`/sandbox require` fail-closed 与 unavailable 诊断 |
| FileEditTool stale-read conflict detection | `crates/cc-engine/src/types/tool.rs`, `tools/fs/file_read.rs`, `tools/fs/file_edit.rs` | `cargo test -p claude-code-rs file_edit`; `cargo test -p claude-code-rs file_read`; `cargo test -p cc-engine`; `cargo build -p claude-code-rs --release` | `Read` 完整文本读取登记共享文件快照；`Edit` 拒绝未读或读后被外部修改的文件，成功编辑后刷新缓存以支持连续编辑 |
| FileEditTool lock/readonly preflight | `tools/fs/file_edit.rs` | `cargo test -p claude-code-rs file_edit`; `cargo build -p claude-code-rs --release` | `Edit` 在 validate/call 阶段以读写句柄探测目标文件，提前拒绝 readonly、PermissionDenied、WouldBlock 与 Windows sharing violation 等锁定或不可写状态 |
| FileEditTool edit history backups | `tools/fs/file_edit.rs`, `tools/fs/safe_write.rs` | `cargo test -p claude-code-rs file_edit`; `cargo build -p claude-code-rs --release` | `Edit` 写入走 `safe_write_text()`，覆盖前创建恢复备份并在结果与 FileChanged hook payload 中暴露 `edit_history.backup_path` |
| FileEditTool auto-indentation repair | `tools/fs/file_edit.rs` | `cargo test -p claude-code-rs file_edit`; `cargo build -p claude-code-rs --release` | `Edit` 精确匹配失败时尝试唯一缩进等价匹配，并把 `new_string` leading whitespace 映射到文件中的实际缩进；歧义候选保持拒绝 |
| FileEditTool live transcript dispatch | `tools/fs/file_edit.rs`, `engine/sdk_types.rs`, `engine/lifecycle/submit_message.rs`, `ipc/sdk_mapper.rs`, `ui/tui/engine_events.rs`, `ui/messages/render.rs`, `ui/runtime/transcript.rs` | `cargo test -p claude-code-rs file_edit`; `cargo test -p claude-code-rs renders_file_edit_tool_preview_from_tool_use_result`; `cargo test -p claude-code-rs tui_user_replay_preserves_tool_result_preview` | `Edit` 结果分离模型可见的 concise text 与 UI-only `display_preview`；SDK replay/headless IPC/TUI 保留 `tool_use_result`，prompt/transcript 复用 ratatui FileEdit updated renderer 显示结构化 diff |
| AgentTool tool boundary filtering | `engine/agent/mod.rs`, `ipc/agent_settings.rs`, `ipc/builtin_agents.rs` | `cargo test -p claude-code-rs child_tool_boundary_tests`; `cargo test -p claude-code-rs agent` | 子 agent 创建前按内置/用户/项目 agent 定义解析 `tools` / `disallowedTools`，支持 `Bash(...)` 规格的基础工具名解析，按工具名去重，并让 Explore/Plan/code-reviewer 等只读内置 agent 不再继承全量工具 |
| AgentTool team context inheritance | `crates/cc-engine/src/types/config.rs`, `engine/lifecycle/mod.rs`, `engine/agent/mod.rs`, `engine/agent/fork.rs` | `cargo test -p claude-code-rs test_query_engine_inherits_agent_team_context`; `cargo test -p claude-code-rs agent`; `cargo test -p cc-engine` | 父会话 `team_context` 通过 `AgentContext` 注入 child engine 的 AppState，避免子 agent 中的 `SendMessage` 因默认 AppState 丢失当前团队 |
| AgentTool multi-agent spawn entry | `engine/agent/mod.rs`, `engine/agent/tool_impl.rs`, `tools/team_spawn.rs` | `cargo test -p claude-code-rs teammate_spawn_request`; `cargo test -p claude-code-rs multi_agent_spawn_fields`; `cargo test -p claude-code-rs agent_teammate_result_annotation`; `cargo test -p claude-code-rs input_json_schema_exposes_plan_mode`; `cargo test -p claude-code-rs plan_mode_flag_only_accepts_plan_mode` | `Agent` schema 支持上游 `name` / `team_name` / `mode`；`name` 触发 in-process `TeamSpawn` teammate 路径，输出 `teammate_spawned` / `teammate_id` / `team_name`，并把 `mode: "plan"` 传递为 teammate plan-mode requirement |
| WebFetch redirect policy | `tools/web_fetch.rs` | `cargo test -p claude-code-rs web_fetch`; `cargo build -p claude-code-rs --release` | WebFetch 关闭 reqwest 自动重定向，手动限制最多 10 跳；只自动跟随同 scheme/port 且 host 仅 `www.` 变体的 redirect，跨 host/scheme/port/credential redirect 返回 `redirect_detected` 诊断和目标 URL |
| WebFetch Content-Type dispatch | `tools/web_fetch.rs` | `cargo test -p claude-code-rs web_fetch`; `cargo build -p claude-code-rs --release` | 按 MIME 分发 HTML/JSON/text/binary：HTML 提取文本，JSON pretty-print，文本型 application MIME 直出，PDF/Office/image/audio/video 等二进制响应返回 `binary: true` 诊断，不把 raw bytes 放进模型上下文 |
| WebFetch environment proxy support | `tools/web_fetch.rs` | `cargo test -p claude-code-rs web_fetch`; `cargo build -p claude-code-rs --release` | WebFetch 显式解析 `HTTPS_PROXY` / `HTTP_PROXY` / `ALL_PROXY` 及小写变体，并支持 `NO_PROXY` exact、domain suffix、host:port 与 wildcard 绕过，避免代理行为依赖 reqwest 默认配置 |
| WebFetch cookie/credential boundary | `tools/web_fetch.rs` | `cargo test -p claude-code-rs web_fetch`; `cargo build -p claude-code-rs --release` | 对齐上游未支持 cookie/credential URL 的安全边界：URL 规范化和输入校验拒绝 embedded username/password，Rust client 不启用 cookie store，避免静默携带认证语义 |
| TaskTools TaskOutput block/timeout retrieval | `tools/tasks.rs` | `cargo test -p claude-code-rs task_output`; `cargo test -p claude-code-rs tasks` | `TaskOutput` schema 支持上游 `block` / `timeout`，阻塞读取会等待 task 离开 pending/running 状态或返回 `timeout`，非阻塞运行中返回 `not_ready`；结果包含 `retrieval_status` + nested `task`，并保留旧 flat output 字段 |
| TaskTools task type taxonomy | `tools/tasks.rs` | `cargo test -p claude-code-rs task_kind`; `cargo test -p claude-code-rs task_create_schema`; `cargo test -p claude-code-rs tasks` | `TaskCreate.kind` schema 使用上游 task type names（`local_bash` / `remote_agent` / `in_process_teammate` 等），并把历史 `local_shell` / `workflow` / `monitor` / `team` alias canonicalize 到持久化记录 |
| TaskTools remote supervisor metadata | `tools/tasks.rs`, `commands/tasks_cmd.rs`, `engine/agent/supervisor.rs` | `cargo test -p claude-code-rs remote_task`; `cargo test -p claude-code-rs supervisor_metadata`; `cargo test -p claude-code-rs tasks` | `TaskStore` v4 持久化上游 remote supervisor 恢复字段：`tool_use_id`、`remote_task_type`、`remote_session_id`、`remote_task_metadata`、`poll_started_at`；`TaskCreate`/`TaskGet`/`TaskList`/`TaskOutput`/`/tasks show` 均暴露这些元数据 |
| TaskTools remote restart recovery marker | `tools/tasks.rs` | `cargo test -p claude-code-rs remote_task`; `cargo test -p claude-code-rs task_output_treats_recoverable`; `cargo test -p claude-code-rs tasks`; `cargo build -p claude-code-rs --release` | 重启加载时将携带 remote identity 的未完成任务恢复为 `recoverable`，保留 `previous_status` / `recovered_at` 与 remote metadata；`TaskOutput` 将 `recoverable` 视为仍在等待，避免把未 reconnect 的 remote session 误报为成功 |
| TaskTools remote restore poll timer reset | `tools/tasks.rs` | `cargo test -p claude-code-rs remote_task`; `cargo test -p claude-code-rs tasks`; `cargo build -p claude-code-rs --release` | 对齐上游 resume 行为，remote task 恢复为 `recoverable` 时重置 `poll_started_at` 为恢复时间，避免 remote review 使用离线前的 poll timer 立即触发 30 分钟超时；终态 remote task 保持原始 poll timestamp |
| TaskTools remote review timeout guard | `tools/tasks.rs` | `cargo test -p claude-code-rs remote_review_timeout`; `cargo test -p claude-code-rs remote_task`; `cargo build -p claude-code-rs --release` | `TaskStore` 读取/list 时刷新 remote review 状态；`ultrareview` 或 metadata `isRemoteReview=true` 的 active/recoverable 任务若超过 `poll_started_at + 30min`，会持久化为 `failed` 并写入 timeout retained output |
| PlanMode persistence and task linking | `plan_workflow.rs`, `tools/plan_mode.rs`, `tools/tasks.rs`, `crates/cc-types/src/plan_workflow.rs` | `cargo test -p claude-code-rs implementation_task_link_requires_approved_plan`; `cargo test -p claude-code-rs tasks`; `cargo build -p claude-code-rs --release` | Plan workflow 以 `.cc-rust/current-plan-workflow.json` 持久化 draft / approval / implementation trace；IPC / daemon 保守 classifier 可进入 Plan mode；`TaskCreate` 会把 approved/implementing plan link 到新任务并推进到 `implementing` |
| PlanMode team approval mailbox flow | `tools/send_message.rs`, `teams/in_process.rs`, `teams/runner.rs`, `ui/components/command_surface/adapters/tasks.rs` | `cargo test -p claude-code-rs plan_approval`; `cargo test -p claude-code-rs send_message`; `cargo test -p claude-code-rs in_process -- --test-threads=1`; `cargo build -p claude-code-rs --release` | `plan_approval_request` 会把 teammate 标记为等待审批；`plan_approval_response` 会清除 pending、应用返回的 `permissionMode`，并把批准/驳回反馈排入 teammate 下一轮；任务 surface 暴露等待审批与 permission mode |
| PlanMode dedicated plan-file writes | `tools/execution/security.rs`, `tools/execution/pipeline.rs`, `tools/execution/tests.rs` | `cargo test -p claude-code-rs plan_file`; `cargo test -p claude-code-rs execution` | Plan mode 保持普通非只读工具拒绝，但允许 `Write` / `Edit` / `FileWrite` / `FileEdit` 仅写当前解析出的 plan 文件；pipeline 对该专用写入跳过普通 Plan-mode prompt，支持非交互计划增量维护 |
| FileWriteTool | `tools/fs/file_write.rs`, `tools/fs/safe_write.rs` | 既有 safe_write / file_write 测试 | 已覆盖临时文件 + rename、恢复备份、大小限制、权限保持、二进制拒绝 |
| SkillTool | `tools/skill.rs`, `crates/cc-skills/src/*` | 既有 cc-skills / SkillTool 测试 | 已覆盖依赖解析、版本管理、frontmatter 诊断、hot reload、fork/inline 上下文 |
| LSP | `tools/lsp.rs`, `lsp_service/*` | 既有 LSP service / tool 测试 | 已覆盖 `didChange`、被动 `publishDiagnostics`、completion 与 diagnostics snapshot |
| Background Agent lifecycle | `engine/agent/supervisor.rs`, `tools/tasks.rs` | 既有 supervisor / task store 测试 | background worktree、权限回调传递、取消与 shutdown cleanup 已收口 |

---

## Phase 0: 类型基础

完整定义了所有核心类型，与 TS 功能对等。

| # | 模块 | 文件 | 行数 | 说明 |
|---|------|------|------|------|
| P0.1 | Message 枚举 | `types/message.rs` | 283 | ContentBlock, Usage, 7 种消息类型, QueryYield |
| P0.2 | Tool trait | `types/tool.rs` | 204 | Tool trait 18 个方法, ToolUseContext, FileStateCache |
| P0.3 | 循环状态 | `types/state.rs` | 114 | QueryLoopState, AutoCompactTracking, BudgetTracker |
| P0.4 | 查询配置 | `types/config.rs` | 121 | QueryParams, QueryEngineConfig, ThinkingConfig |
| P0.5 | 应用状态 | `types/app_state.rs` | 57 | AppState, SettingsJson |
| P0.6 | 状态转换 | `types/transitions.rs` | 47 | Terminal (10 种), Continue (7 种) |

**小计: 6 个文件, 826 行**

---

## Phase 1: 状态机骨架

核心查询循环与引擎生命周期，Rust 实现比 TS 更详细。

| # | 模块 | 文件 | 行数 | 测试 | 说明 |
|---|------|------|------|------|------|
| P1.1 | query loop | `query/loop_impl.rs` | 1,024 | 4 | 完整 8 步循环 + 恢复路径 |
| P1.2 | 依赖注入 | `query/deps.rs` | 146 | 0 | QueryDeps trait (可 mock) |
| P1.3 | token 预算 | `query/token_budget.rs` | 64 | 0 | checkTokenBudget + diminishing returns |
| P1.4 | stop hooks | `query/stop_hooks.rs` | 163 | 4 | has_tool_use, extract_tool_uses |
| P1.5 | QueryEngine | `engine/lifecycle.rs` | 1,393 | 8 | Phase A-E 完整分发 + QueryEngineDeps |
| P1.6 | SDK 类型 | `engine/sdk_types.rs` | 142 | 0 | SdkMessage 7 种变体 |
| P1.7 | 输入处理 | `engine/input_processing.rs` | 155 | 5 | 斜杠命令解析 + UserMessage 构建 |
| P1.8 | 系统提示 | `engine/system_prompt.rs` | 640 | 7 | 7 静态段落 + 动态段落 + 缓存边界 |
| P1.9 | 结果判定 | `engine/result.rs` | 233 | 6 | isResultSuccessful + extractTextResult |
| P1.10 | 提示段落 | `engine/prompt_sections.rs` | 210 | — | 段落组装 + 工具 prompt |
| P1.11 | CLI 入口 | `main.rs` | ~400 | 0 | clap CLI + 快速路径 + TUI 集成 + print mode |
| P1.12 | 关闭清理 | `shutdown.rs` | 129 | 0 | SIGINT handler + abort + transcript flush |

**小计: 12 个文件, ~4,700 行, 34 测试**

---

## Phase 2: 工具基础设施

工具注册、执行管线和 Hook 系统完整实现。

| # | 模块 | 文件 | 行数 | 测试 | 说明 |
|---|------|------|------|------|------|
| P2.1 | 工具注册 | `tools/registry.rs` | 115 | 4 | get_all_tools + find_tool_by_name, 18 工具 |
| P2.2 | 并发编排 | `tools/orchestration.rs` | 535 | — | partitionToolCalls + 并行/串行批次 |
| P2.3 | 执行管线 | `tools/execution.rs` | 608 | — | run_tool_use() 8 步管线 |
| P2.4 | Hook 系统 | `tools/hooks.rs` | 855 | — | 完整子进程执行 + JSON 解析 + matcher |

**小计: 4 个文件, 2,113 行**

---

## Phase 3: 权限与配置

权限决策与设置加载核心逻辑完整。

| # | 模块 | 文件 | 行数 | 测试 | 说明 |
|---|------|------|------|------|------|
| P3.1 | 规则引擎 | `permissions/rules.rs` | 274 | 7 | deny→allow→ask 优先级 + glob 匹配 |
| P3.2 | 决策状态机 | `permissions/decision.rs` | 458 | 7 | 模式匹配 + denial tracker |
| P3.3 | 危险检测 | `permissions/dangerous.rs` | 217 | 11 | 16 种危险模式正则 |
| P3.4 | 设置加载 | `config/settings.rs` | 294 | 3 | 3 层合并 (global → project → env) |
| P3.5 | CLAUDE.md | `config/claude_md.rs` | 129 | 0 | 文件发现 + 上下文注入 |

**小计: 5 个文件, 1,372 行, 28 测试**

---

## Phase 4: 上下文管理 (压缩管线)

完整实现 snip → micro → autocompact 管线，与 TS 功能对等。

| # | 模块 | 文件 | 行数 | 测试 | 说明 |
|---|------|------|------|------|------|
| P4.1 | 消息工具 | `compact/messages.rs` | 306 | 4 | normalizeForAPI + 交替模式 |
| P4.2 | 微压缩 | `compact/microcompact.rs` | 261 | 2 | 阈值裁剪 + 最近 N 结果保护 |
| P4.3 | 历史裁剪 | `compact/snip.rs` | 217 | 2 | turn 识别 + 边界消息 |
| P4.4 | 结果预算 | `compact/tool_result_budget.rs` | 224 | 2 | 磁盘持久化 + async I/O + 预览生成 |
| P4.5 | 管线编排 | `compact/pipeline.rs` | 300 | 3 | snip → micro → autocompact 编排 |
| P4.6 | 压缩决策 | `compact/auto_compact.rs` | 52 | 3 | 80% 阈值判定 |
| P4.7 | 全量压缩 | `compact/compaction.rs` | 426 | 8 | 决策 + 跟踪 + prompt + boundary |
| P4.8 | token 估算 | `utils/tokens.rs` | 170 | 5 | 4 chars/token 启发式 |
| P4.9 | 文件缓存 | `utils/file_state_cache.rs` | 193 | 0 | LRU 缓存 + hash/timestamp |

**小计: 9 个文件, 2,149 行, 29 测试**

---

## Phase 6: 会话持久化

| # | 模块 | 文件 | 行数 | 说明 |
|---|------|------|------|------|
| P6.1 | 会话存储 | `session/storage.rs` | 328 | JSON 持久化 + NDJSON 序列化 |
| P6.2 | 对话记录 | `session/transcript.rs` | 187 | NDJSON append + sync |
| P6.3 | 会话恢复 | `session/resume.rs` | 53 | cwd 匹配 + 消息加载 |

**小计: 3 个文件, 568 行**

---

## Phase 8: 高级本地工具

PlanMode、Worktree、Skill 完整实现真实逻辑。

| # | 模块 | 文件 | 行数 | 测试 | 说明 |
|---|------|------|------|------|------|
| P8.1 | PlanMode | `tools/plan_mode.rs` | 432 | 8 | 真实状态转换: save/restore pre_plan_mode, 验证 agent 上下文 |
| P8.2 | Worktree | `tools/worktree.rs` | 724 | 8 | 真实 git worktree: 创建/清理, 变更检测, 失败关闭安全 |
| P8.3 | Skill | `tools/skill.rs` | 454 | 7 | 技能查找/扩展/注入: 注册表查询, 参数替换, new_messages |

**小计: 3 个文件, 1,610 行, 23 测试**

---

## Phase 9: API 客户端 (活跃提供商)

| # | 模块 | 文件 | 行数 | 说明 |
|---|------|------|------|------|
| P9.1 | 客户端 | `api/client.rs` | 871 | SSE 字节流解析 + 重试 |
| P9.2 | 流解析 | `api/streaming.rs` | 112 | SSE 解析 + StreamAccumulator |
| P9.3 | 重试 | `api/retry.rs` | 99 | 错误分类 + 指数退避 |
| P9.4 | Anthropic 提供商 | `api/providers.rs` | 311 | AnthropicProvider 完整实现 |

**小计: 4 个文件, 1,393 行**

> 注: Bedrock/Vertex 仅保留函数签名 (`unimplemented!`)，见简化文档。

---

## Phase 11: MCP 协议

| # | 模块 | 文件 | 行数 | 说明 |
|---|------|------|------|------|
| P11.1 | 客户端 | `mcp/client.rs` | 1,008 | stdio 传输 + JSON-RPC 2.0 + McpManager |
| P11.2 | 发现 | `mcp/discovery.rs` | 47 | JSON 配置加载 |
| P11.3 | 工具 | `mcp/tools.rs` | 296 | McpToolWrapper 委托真实 McpClient |

**小计: 3 个文件, 1,351 行**

---

## Phase 12: 网络工具 (接近完整)

| # | 模块 | 文件 | Rust 行数 | TS 行数 | 缩减 | 说明 |
|---|------|------|-----------|---------|------|------|
| P12.1 | WebFetch | `tools/web_fetch.rs` | 553 | 1,131 | 51% | HTML→text, 缓存, URL 验证, 截断 |
| P12.2 | WebSearch | `tools/web_search.rs` | 529 | 569 | 7% | Brave Search API, 域名过滤 |
| P12.3 | LSP 工具 | `tools/lsp.rs` | 877 | 2,005 | 56% | 9 种操作, 回退文本分析 |
| P12.4 | NotebookEdit | `tools/notebook_edit.rs` | 530 | 587 | 10% | replace/insert/delete 完整实现 |
| P12.5 | 技能系统 | `skills/` (3 文件) | 989 | — | — | 加载/注册/5个内置技能 |
| P12.6 | 插件系统 | `plugins/` (3 文件) | 931 | — | — | manifest/loader/注册 |

**小计: 8 个文件, 4,409 行**

---

## Phase 13: 终端 UI (完整 TUI)

基于 ratatui + crossterm 的全屏终端界面，已与 QueryEngine 完整集成。

| # | 模块 | 文件 | 行数 | 测试 | 说明 |
|---|------|------|------|------|------|
| P13.1 | TUI 集成 | `ui/tui.rs` | ~300 | 0 | 异步事件循环, tokio::spawn 引擎查询, mpsc 通道通信, 终端状态守卫 |
| P13.2 | App 主框架 | `ui/app.rs` | ~470 | 0 | 消息历史, 输入状态, 权限对话框, 滚动, 输入历史, 状态栏, 费用跟踪 |
| P13.3 | 消息渲染 | `ui/messages.rs` | 404 | 0 | User/Assistant/System/Progress/Attachment 5 种消息渲染 |
| P13.4 | Markdown 渲染 | `ui/markdown.rs` | 259 | 0 | pulldown-cmark: 标题, 粗体, 斜体, 代码块, 列表, 链接, 引用, 分隔线 |
| P13.5 | 输入框 | `ui/prompt_input.rs` | 250 | 0 | 光标, 水平滚动, UTF-8 安全, Ctrl 快捷键 |
| P13.6 | 权限对话框 | `ui/permissions.rs` | 244 | 0 | 居中覆盖层, Allow/Deny/AlwaysAllow, 快捷键 Y/N/A |
| P13.7 | 主题系统 | `ui/theme.rs` | 116 | 0 | 23 种预定义 RGB 样式 |
| P13.8 | 旋转动画 | `ui/spinner.rs` | 95 | 0 | 10 帧 Braille 点阵动画 |
| P13.9 | Diff 渲染 | `ui/diff.rs` | 96 | 0 | 增/删/上下文着色, similar crate |

**小计: 9 个文件, ~2,234 行**

---

## Phase 14A: 本地补充模块

| # | 模块 | 文件 | 行数 | 说明 |
|---|------|------|------|------|
| P14A.1 | 常量定义 | `config/constants.rs` | 454 | 模型 ID, API 版本, token 限制 |
| P14A.2 | utils/bash | `utils/bash.rs` | 704 | 命令解析, shell 转义, heredoc 检测 |
| P14A.3 | utils/git | `utils/git.rs` | 687 | git2: status/diff/log/branch/shallow |
| P14A.4 | utils/shell | `utils/shell.rs` | 323 | shell 检测, 环境初始化, Git Bash |
| P14A.5 | utils/messages | `utils/messages.rs` | 497 | 消息格式化, 截断, 摘要, 统计 |
| P14A.6 | keybindings | `ui/keybindings.rs` | 425 | 快捷键注册, 上下文解析, 自定义绑定 |
| P14A.7 | vim 模式 | `ui/vim.rs` | 847 | Normal/Insert/Visual, hjkl, dd/yy/p/w/b |
| P14A.8 | 迁移系统 | `session/migrations.rs` | 300 | 版本检测, v1→v2→v3 迁移链 |
| P14A.9 | 任务子系统 | `tools/tasks.rs` | 648 | TaskStore + 6 工具 (Create/Get/Update/List/Stop/Output) |
| P14A.10 | 内存系统 | `session/memdir.rs` | 385 | CRUD + 搜索 + 上下文注入 |

**小计: 10 个文件, 5,270 行**

---

## Phase 14B: 命令系统 (第一、二批)

**第一批 — 高频核心命令**

| # | 命令 | 文件 | 行数 | 说明 |
|---|------|------|------|------|
| P14B.1 | /exit | `commands/exit.rs` | 63 | 退出 REPL |
| P14B.2 | /version | `commands/version.rs` | 48 | 版本号 |
| P14B.3 | /model | `commands/model.rs` | 132 | 切换模型 |
| P14B.4 | /cost | `commands/cost.rs` | 195 | token 用量 |
| P14B.5 | /session | `commands/session.rs` | 151 | 会话列表/切换 |
| P14B.6 | /resume | `commands/resume.rs` | 146 | 恢复会话 |
| P14B.7 | /files | `commands/files.rs` | 175 | 引用文件列表 |
| P14B.8 | /context | `commands/context.rs` | 198 | 上下文管理 |
| P14B.9 | /permissions | `commands/permissions_cmd.rs` | 300 | 权限查看/修改 |
| P14B.10 | /hooks | `commands/hooks_cmd.rs` | 184 | hook 管理 |

**第二批 — 中频功能命令**

| # | 命令 | 文件 | 行数 | 说明 |
|---|------|------|------|------|
| P14B.11 | /commit | `commands/commit.rs` | 95 | git commit + 模型辅助 |
| P14B.12 | /review | `commands/review.rs` | 81 | 代码审查 |
| P14B.13 | /branch | `commands/branch.rs` | 97 | 分支管理 |
| P14B.14 | /export | `commands/export.rs` | 154 | JSON/Markdown 导出 |
| P14B.15 | /rename | `commands/rename.rs` | 41 | 重命名会话 |
| P14B.16 | /stats | `commands/stats.rs` | 85 | 消息/token 统计 |
| P14B.17 | /effort | `commands/effort.rs` | 51 | 思考力度设置 |
| P14B.18 | /fast | `commands/fast.rs` | 256 | 快速模式 + 状态 |
| P14B.19 | /memory | `commands/memory.rs` | 85 | CLAUDE.md 管理 |
| P14B.20 | /plan | `commands/plan.rs` | 88 | 计划模式切换 |
| — | 注册表 | `commands/mod.rs` | 379 | 27 命令注册 + 别名 + 参数 |

**小计: 21 个文件, 2,905 行**

---

## 完整实现统计

| 类别 | 文件数 | 行数 | 测试数 |
|------|--------|------|--------|
| 类型基础 (Phase 0) | 6 | 826 | — |
| 状态机骨架 (Phase 1) | 12 | 4,772 | 34 |
| 工具基础设施 (Phase 2 基础) | 4 | 2,113 | 4 |
| 权限与配置 (Phase 3) | 5 | 1,372 | 28 |
| 上下文管理 (Phase 4) | 9 | 2,149 | 29 |
| 会话持久化 (Phase 6) | 3 | 568 | — |
| 高级工具 (Phase 8) | 3 | 1,610 | 23 |
| API 客户端 (Phase 9) | 4 | 1,393 | — |
| MCP 协议 (Phase 11) | 3 | 1,351 | — |
| 网络工具 + 技能/插件 (Phase 12) | 8 | 4,409 | — |
| 本地补充 (Phase 14A) | 10 | 5,270 | — |
| 命令系统 (Phase 14B) | 21 | 2,905 | — |
| **合计** | **88** | **~28,738** | **118+** |

---

## 2026-04-27 Full-Build Updates

| Module | Files | Notes |
| --- | --- | --- |
| TaskTools persistence (MVP-003) | `crates/claude-code-rs/src/tools/tasks.rs`, `crates/claude-code-rs/src/commands/tasks_cmd.rs`, `crates/claude-code-rs/src/engine/agent/tool_impl.rs` | Versioned task records under the cc-rust data root, bounded output sidecars, restart interruption recovery, dependency/cancellation/recovery metadata, `/tasks` detail/cancel/delete, and background local-agent runtime cancellation tokens. |

## 2026-04-29 Full-Build Updates

| Module | Files | Notes |
| --- | --- | --- |
| Skill packages (MVP-010) | `crates/cc-skills/src/{lib.rs,loader.rs,bundled.rs}`, `crates/claude-code-rs/src/commands/skills_cmd.rs`, `crates/claude-code-rs/src/commands/reload_plugins_cmd.rs`, `crates/claude-code-rs/src/ipc/subsystem_handlers.rs`, `crates/claude-code-rs/src/main.rs`, `crates/claude-code-rs/src/mcp/tools.rs` | Versioned skill package metadata, frontmatter/layout diagnostics, dependency graph resolution, registry revisions, explicit reload invalidation, plugin skill reload integration, bundled skill versions, and MCP `skill://` resource ingestion. Remote skill marketplace/state loading and continuous file watching are intentional crops. |

## 2026-05-04 Full-Build Updates

| Module | Files | Notes |
| --- | --- | --- |
| Ratatui P0 UI parity milestone | `crates/claude-code-rs/src/ui/rendering/progress_bar.rs`, `crates/claude-code-rs/src/ui/messages/user_bash_output_message.rs`, `crates/claude-code-rs/src/ui/diff/structured_diff.rs`, `crates/claude-code-rs/src/ui/components/search_box.rs`, `crates/claude-code-rs/src/ui/components/history_search_dialog.rs`, `crates/claude-code-rs/src/ui/rendering/tool_activity.rs`, `docs/RATATUI_UI_PARITY.md` | Completed the P0 ratatui foundations for shell output formatting, structured diff hunks, shared search/fuzzy filtering, Ctrl+R in-session history search, shared progress bars, and richer tool activity rendering. Residuals are explicit: latest shell-output auto-expand needs runtime/event wiring and Ctrl+R remains in-session until a persistent timestamped history reader exists. |
| Ratatui P1 UI parity surfaces | `crates/claude-code-rs/src/ui/components/command_surface/surfaces/config.rs`, `crates/claude-code-rs/src/ui/components/command_surface/surfaces/tasks.rs`, `crates/claude-code-rs/src/ui/components/status_widget.rs`, `crates/claude-code-rs/src/ui/mcp/*`, `crates/claude-code-rs/src/commands/mcp_cmd.rs`, `crates/claude-code-rs/src/ui/diff/file_edit_diff.rs`, `crates/claude-code-rs/src/ui/messages/file_edit_tool_updated_message.rs` | Completed model/theme/effort pickers, runtime task panels, statusline runtime context and optional indicators, MCP approval/copy/multiselect/card surfaces with project-scoped approve/reject persistence, shared file-edit diff/update render surfaces, and live transcript dispatch for FileEdit structured previews. Remaining items are backend-gated: live IDE/PR data and Claude Desktop MCP discovery. |

## 2026-05-07 Documentation Archive Updates

| Module | Files | Notes |
| --- | --- | --- |
| Extensibility active runtime closure | `docs/archive/extensibility-phase1-mcp-lifecycle-2026-05-06.md` through `docs/archive/extensibility-phase6-integration-closure-2026-05-06.md`, `docs/archive/completed-gap-closures-2026-05-07.md` | Archived Phase 1-6 completion notes from active status docs. Current active runtime covers MCP lifecycle, remote HTTPS SSE, OAuth, Streamable HTTP, custom-agent safety, and runtime tool-registry refresh. |
| Runtime storage unification docs | `docs/archive/superpowers/plans/2026-04-18-phase1-runtime-storage-unification.md`, `docs/archive/superpowers/specs/2026-04-18-phase1-runtime-storage-unification-design.md`, `docs/STORAGE.md` | Moved the completed storage plan/spec out of active `superpowers/`; active path reference remains `docs/STORAGE.md`. |
| Known issue cleanup | `docs/KNOWN_ISSUES.md`, `docs/archive/resolved-known-issues-2026-05-07.md`, `docs/archive/issues/` | Consolidated open issues and review findings into `KNOWN_ISSUES.md`; moved resolved UI/frontend issues and raw historical review reports to archive. |
