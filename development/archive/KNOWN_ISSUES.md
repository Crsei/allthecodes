# cc-rust 当前问题汇总

> 更新日期: 2026-07-19

本文是当前开放问题、代码审查发现和文档状态问题的唯一活跃入口。已修复、已失效或只具历史价值的问题已迁移到：

- [archive/resolved-known-issues-2026-05-07.md](resolved-known-issues-2026-05-07.md)
- [archive/issues/](issues)

## 1. 构建与实现阻塞

| ID | 严重度 | 状态 | 范围 | 摘要 | 详情 |
| --- | --- | --- | --- | --- | --- |

## 2. 安全与权限

| ID | 严重度 | 状态 | 范围 | 摘要 | 详情 |
| --- | --- | --- | --- | --- | --- |
| SAFETY-001 | 高 | Fixed | Auto mode | `permissions.enableAutoMode=false` 现在由统一的 permission transition helper 强制执行，启动配置、Web、`/permissions`、`/config` 与子上下文不能绕过进入 Auto。 | [2026-05-07 review](issues/2026-05-07-code-review-findings.md) §四 |
| SAFETY-002 | 高 | Fixed | Plan `allowedPrompts` | Auto -> Plan -> ExitPlanMode 追加的 `allowedPrompts` 规则会在恢复 Auto mode 后立即复用危险 allow 规则剥离逻辑，宽泛/解释器/package runner Bash 规则进入 Auto mode stripped side buffer。 | 同上 |
| SAFETY-003 | 高 | Fixed | Sandbox `allowedCommands` | `allowedCommands` 仅在 workspace sandbox 且 OS-level sandbox 可用时预批准；匹配改为 argv 结构化检查，链式/管道命令中未显式允许的子命令不会搭车放行。 | 同上 |
| SAFETY-004 | 高 | Fixed | classifier redaction | classifier redaction 覆盖 JSON/object-like secret 字段，包括 `password`、`apiKey`、`api_key`、`token`、`accessToken`、`refreshToken`、`secret` 等。 | 同上 |
| SAFETY-005 | 中 | Fixed | Plan approval UI | ExitPlanMode 审批提示现在列出将写入的去重后 transient allowed prompt rules，而不是只显示数量。 | 同上 |
| SAFETY-006 | 中 | Fixed | Auto classifier Thinking stage | ApiClient-backed classifier 的 Thinking stage 不能用 `budget_tokens=2048` 搭配 `max_tokens=512`，否则 Anthropic thinking 请求会被 provider 拒绝；现已把 Thinking 请求上限提高到大于 thinking budget。 | 2026-05-18 permission classifier review follow-up |
| SAFETY-007 | 低 | Fixed | Auto classifier policy context | Runtime 接线最初传入空 `AutoModeSettings`，导致 `permissions.autoMode` 中的 environment/allow/softDeny 说明不会进入 classifier prompt；现已把合并后的配置传入 classifier request。 | 2026-05-18 permission classifier review follow-up |

## 3. 模型与 provider 文档/兼容性

| ID | 严重度 | 状态 | 范围 | 摘要 | 详情 |
| --- | --- | --- | --- | --- | --- |
| PROVIDER-001 | 高 | Fixed | Rust TUI / provider profile | Rust TUI 缺少删除和管理 provider profile 的入口，Web `/api/profiles` 更新还会把完整 profile 重建为空配置；运行时只允许 Anthropic/OpenAI/Codex。 | 新增 `/providers` 配置与 preset 分区、创建/完整替换/激活/删除向导和非交互命令；TUI、provider API 与 profile API 统一使用无损原子存储。17 个静态 provider、Bedrock、Vertex 可从 active profile 构造运行时，Foundry 与未知/custom/ACP 保持可管理但拒绝激活；所有读取与快照隐藏秘密。 |
| PROVIDER-002 | 高 | Reopened / In Progress | OpenAI-compatible SSE / CLI tool sessions | 原始 60/120 秒长流、EOF 伪完成与 stream reconnect 问题已修复；Session `d4fe751e-becd-482a-8855-367219b20609` 在默认配置下约 218 秒的 `gpt-5.6-sol` 调用成功。后续真实 CLI 验收又证明：`5a1a8694-a5d5-4723-bf6e-255364f291c4` 停在 tool result 后的刷新/收尾窗口；`155e5992-0936-474c-bc4b-a93f39597383` resume 将 orphan tool call 发给 provider，HTTP 400 又被误判为 stdout 成功/exit 0；`ce228c8e-0f76-41ca-a9ee-985c082d4b33` 同 submit 的 Read 状态未被后续 Edit 识别并反复失败。 | 按唯一权威计划 `development/bugs/2026-07-19-codex-oauth-responses-stream-timeout-retry-fix-plan.md` 补齐非阻塞工具刷新、rollout-first tool result 耐久化、resume 协议修复、session-owned file cache、tool-error loop guard 与统一非交互失败退出。原始长流证据保留在 `development/worktree-workflow-artifacts/2026-07-20-codex-stream-recovery-cli-contract.html`；本轮完成后证据写入 `development/worktree-workflow-artifacts/2026-07-20-codex-cli-session-tool-state-hardening.html`。 |

当前开放 provider 项为 `PROVIDER-002`。其他已关闭记录见
[archive/resolved-model-context-2026-05-07.md](resolved-model-context-2026-05-07.md)。

## 4. Context / compact

| ID | 严重度 | 状态 | 范围 | 摘要 | 详情 |
| --- | --- | --- | --- | --- | --- |

当前无开放项。已关闭记录见 [archive/resolved-model-context-2026-05-07.md](resolved-model-context-2026-05-07.md)。

## 5. UI / runtime residuals

| ID | 严重度 | 状态 | 范围 | 摘要 | 详情 |
| --- | --- | --- | --- | --- | --- |
| UI-001 | 中 | Open | TS/OpenTUI resize | Rust TUI resize 已收口；TS/OpenTUI 在 Windows maximize/fullscreen 后仍可能留下白色横行或未完整 repaint。 | 历史 #1/#17 |
| UI-002 | 中 | Fixed | Rust TUI shell output | 最新 Bash/PowerShell tool result 现在由 runtime context 自动展开；历史长输出默认折叠，选中后可展开/折叠查看 detail。 | 历史 #21 |
| UI-003 | 中 | Fixed | Rust TUI Ctrl+R | Ctrl+R 现在按当前 workspace 读取跨会话持久 prompt history，条目带 session/title/cwd 来源和时间；无后端数据时显示明确空态。 | 历史 #22 |
| UI-004 | 中 | Evidence pending | Browser MCP | Browser MCP / Chrome native host / Chrome MCP bridge 已有 fake bridge/native-host 端到端证据；真实第三方 Browser MCP server 与 Chrome extension 仍是 release 手动证据，缺失时不声称 live server 已验证。 | 历史 Browser MCP |
| UI-006 | 中 | Open | Rust TUI theme parity | 组件层主题接线已补齐，但 `/config theme` 允许的 `solarized`、`monokai`、`nord` 等主题名尚未映射到新的 design theme palette。 | 当前未知主题会按 fallback 主题渲染；完成组件 parity 前需要补齐所有配置主题名到 `ThemeProvider`/design palette 的映射与快照覆盖。 |
| UI-007 | 低 | Review | Rust TUI dialog overlays | `Dialog::handle_key()`、`ExitGuard`、直接 dialog 路径和 Ctrl+C/Ctrl+D exit guard 已进入生产构建；overlay stack 仍主要保存 overlay metadata。 | 当前真实弹窗路径已有 Esc/取消/确认和 exit guard；只有需要多 overlay z-index/集中 dispatch 时，才继续补 overlay-stack runtime contract。 |
| UI-008 | 低 | Review | Rust TUI tabs parity | `Tabs` production API、header/navigation helpers、content height/header focus 读路径和 snapshots 已接入；content-pane callback parity 仍是组件增强项。 | 已不属于 cfg-test 未接线问题；后续如调用方需要受控 pane callbacks，再按具体 surface 补测试。 |
| UI-009 | 中 | Fixed | Rust TUI cfg-test production wiring | Phase 1-16 已完成：`CommandSurface`、agent create/edit、MCP detail/tools、permissions、tasks/team、dialog/tabs helpers 和 runtime snapshots 已进入生产构建。 | 验证: `cargo test -p claude-code-rs ui::`、`cargo build --workspace --release`、`git diff --check`；实现提交 `9ac3ae5`。 |
| UI-010 | 中 | Fixed | Rust TUI prompt/resume/permissions/scroll | 用户反馈输入框未继承用户消息背景、运行中无法继续输入、`/resume` 缺少面板、permissions 弹窗窄终端下 `Always exact` 不完整、session 不默认显示底部且鼠标无法滚动。 | 已让 prompt 输入行整行使用用户消息背景；运行中输入保持可编辑，`Tab` 才显式排队并在当前 turn 结束后发送；`/resume` 空参数打开 session 面板；permission dialog 加宽并支持按钮换行；TUI 启动同步已恢复历史并默认定位底部；mouse capture 默认关闭以保留终端原生拖选，设置 `ALLTHECODES_ENABLE_MOUSE_CAPTURE=1` 后支持 TUI 滚轮和滚动条，`ALLTHECODES_DISABLE_MOUSE=1` 可强制关闭捕获。 |
| UI-011 | 中 | Fixed | Rust TUI mouse focus + subagent task alias | 用户反馈鼠标滚轮应滚动聊天记录，输入框历史只应通过键盘上/下键切换；聊天框中要求调用 subagent 时模型可能发出上游 `Task` 工具名并显示调用失败。 | App 对滚轮事件统一滚动 session/transcript；prompt 历史只由键盘上/下键触发。Agent runtime 同时注册 `Agent` 和上游兼容 `Task` 工具名，`Task` 复用同一 subagent 实现。 |
| UI-012 | 中 | Fixed | Rust TUI welcome logo | 0.1.13 logo 将九宫格与 `ALLTHECODES` tracker 分开，3×3 整格密度不足以清晰表达全部字母；后续运行态发现各逻辑列之间仍有固定空格，整体宽度过大。 | 已按 [integrated glyph plan](../tui/2026-07-16-allthecodes-tui-logo-integrated-glyph-plan.md) 改为九宫格内 6×6 子像素/half-block 多边形和五帧单色几何渐变，并按 [grid spacing plan](../tui/2026-07-16-tui-grid-spacing-tightening-plan.md) 删除列间固定空格、将外框和布局宽度收紧为 8 列；颜色阶段等待单色运行态确认。 |
| UI-013 | 中 | Fixed | Rust TUI command panels | 会话区已有文本时打开斜杠命令列表或命令 surface，鼠标滚轮仍会滚动底层会话，文本会移动到面板下方并从未覆盖的单元格透出。 | 命令面板打开时滚轮不再修改会话 `scroll_offset`；command palette 与 prompt-adjacent command surface 绘制前都会清空占用矩形。回归测试覆盖两类面板的输入隔离与缓冲区遮挡。 |
| UI-014 | 中 | Fixed | Rust TUI prompt/status/startup/layout | 大粘贴、有效 slash command、启动期用户可见 WARN、重复 model、请求级 context 状态和短会话 prompt 锚点曾出现用户可见不一致。 | [2026-07-18 TUI fix plan](../tui/2026-07-18-tui-paste-command-warning-status-bottom-layout-fix-plan.md) 与 follow-up `c29a329f` 已落地结构化 paste range、cwd/registry/TTL command metadata cache、覆盖 API key/云凭据/JWT/通用 secret assignment 的 startup diagnostic 脱敏、最新单次 Assistant request usage、优先 effective `context_window` 的 capacity resolver、model/agent/context 单一 status owner 和 bottom-anchored layout。新增真实 PTY 2/2 通过，覆盖 512 字符 paste、model fallback warning、单一 model 与 100x24 的 prompt/context/footer 坐标；fmt、相关 unit、clippy、非 PTY workspace lib 与 release build 通过。完整 PTY 10 线程结果为 221 passed、36 ignored、2 个独立既有等待窗口失败，未标为全绿。 |
| UI-015 | 中 | Fixed | Rust TUI task mutations | 用户反馈消息流仍显示 `TaskCreate` / `TaskUpdate`，而任务状态应默认通过 spinner 上方的 `✔` / `◻` TaskList 呈现。 | 普通与 verbose 消息流均隐藏 task mutation tool use 及配对 result；收到 TaskCreate/TaskUpdate 时自动打开并在 result 后刷新 session/team-scoped TaskList，非任务工具不自动打开，Ctrl+T 手动切换保持不变。 |

## 6. 文档状态问题

| ID | 严重度 | 状态 | 范围 | 摘要 | 详情 |
| --- | --- | --- | --- | --- | --- |
| DOC-002 | 中 | Open | Extensibility implementation map | Phase 5/6 closure 与旧“部分实现”状态冲突；Phase 5 实施记录、future fields、WebSocket/out-of-scope 口径需收口。 | [2026-05-07 review](issues/2026-05-07-code-review-findings.md) §五 |
| DOC-003 | 中 | Review | stale Lite wording | 顶层 release/current-state/CLI docs 已改为 Full Build 与当前 crate 路径口径；command reference、TUI command UI reference、final release plan、implementation gaps 已同步 2026-05-21 production wiring 状态。plan/archive/mvp 文档中的历史 Lite 文字只按历史上下文保留，后续成为活跃 release reference 时继续清理。 | 本轮文档清理发现 |

## 7. ACP adapter residuals (2026-07-03)

| ID | Severity | Status | Scope | Summary | Detail |
| --- | --- | --- | --- | --- | --- |
| ACP-001 | Medium | Open | Binary real-model smoke | The deterministic ACP stdio smoke passes, but the on-demand real-model binary smoke did not complete in this environment. | `acp_stdio_real_model_prompt_smoke` uses the normal allthecodes config/auth/model path and is ignored by default because it requires credentials, provider access, and network. Explicit local runs against current `backend=codex` timed out after 300s after `available_commands_update` and `state_update: running`, with no model content or final idle. |
| ACP-002 | Medium | Intentional | ACP multimodal prompt and MCP | ACP image/audio/embedded-context prompt blocks and per-session MCP are intentionally unadvertised for this branch. | `session.prompt.image`, `session.prompt.audio`, `session.prompt.embeddedContext`, and `session.mcp.*` remain separate feature work. Current behavior is explicit rejection with `InvalidParams` or omitted capabilities, not partial support. |

## 8. Hermes Runtime residuals (2026-07-04)

| ID | Severity | Status | Scope | Summary | Detail |
| --- | --- | --- | --- | --- | --- |
| HERMES-001 | Medium | Open | Scheduled tasks | Scheduled task `cwd` is preserved as registry/source/system-prompt metadata, but does not yet override the `QueryEngine` execution cwd. | `dispatch_due_agent_tasks` routes through the existing engine with `QuerySource::ScheduledTask`. The engine submit API currently has no per-submit cwd override, so tool execution still uses the daemon engine cwd until that API is extended. |
| HERMES-002 | Medium | Open | `DelegateTask` runtime | `DelegateTask` creates a child session, task record, worktree metadata, and searchable bootstrap transcript, but does not yet directly start a child `QueryEngine` with the returned `child_session_id`. | The current tool is a durable delegation envelope that parents can track with task tools and resume via the child session id. Full runtime parity requires a child-agent execution bridge that accepts caller-provided lineage/session ids. |
| HERMES-003 | Low | Open | Web task list scope | Web `/api/tasks` now lists scheduled tasks and the default task store; session-scoped task stores are still only visible through session tools unless a task-list/session scope is added to the API. | `TaskListTool` stores tasks under the current session-derived task list. The Web endpoint has no `task_list_id` or session parameter yet, so it cannot enumerate every session-local `DelegateTask` record without a protocol extension. |

## 8. Effort / Reasoning Provider Semantics (2026-07-18)

Plan: [`development/tui/2026-07-18-model-effort-provider-semantics-fix-plan.md`](../tui/2026-07-18-model-effort-provider-semantics-fix-plan.md)

| ID | Severity | Status | Scope | Summary | Detail |
| --- | --- | --- | --- | --- | --- |
| EFFORT-001 | High | Fixed | Codex `/effort` token copy | Codex effort picker & `/effort` no-arg display showed misleading `4096/10240/24576/32768 tokens` text, implying a server-side budget. Codex wire is `reasoning.effort`; the upstream server controls the actual budget. | Token text now gated to the Anthropic fixed-budget transport only and labeled `local request budget`. Codex picker & `/effort` display show level + provenance only. |
| EFFORT-002 | High | Fixed | Restart display drift | TUI status/picker had three duplicate `effort_value → output_config → effort_level` fallback chains; profile=low + `output_config=high` produced inconsistent TUI vs wire state after restart. | Centralized in `allthecodes_engine::effort::resolve_effort_for_state` with a single priority contract (per-turn override → profile baseline → bundled default → auto). TUI surfaces and request builder share this resolver. |
| EFFORT-003 | High | Fixed | Per-turn override silently dropped for Codex | `resolve_model_reasoning_effort` preferred the profile baseline (`model_reasoning_effort`) over `effort_value`, so `SubmitMessageOverrides.effort` (which only writes `effort_value`) was down-ranked for Codex and the override could be silently dropped on the wire. | Resolver order changed: turn override → session → profile baseline → bundled default. Per-turn override now reaches the Codex `reasoning.effort` wire field for that turn, and restores to baseline on the next turn. |
| EFFORT-004 | Medium | Fixed | `/effort auto` persisting concrete default | `/effort auto` previously wrote a concrete default value into settings, silently locking the user to that level after restart. | `/effort auto` now removes the explicit override from the profile; resolver falls back to the bundled `default_reasoning_level` per model at request time; settings holds no persistent default for `auto`. |
| EFFORT-005 | Medium | Fixed | Codex profiling cross-pollution | `/effort <level>` on the Codex path wrote `authProfiles.<active>.modelReasoningEffort` AND root `output_config.effort`; the latter is the Anthropic-only wire field. | Codex `/effort` path no longer calls `set_output_config_effort` / `set_raw_output_config_effort`; persistence is provider-specific via `persist_user_profile_reasoning_effort`. |
| EFFORT-006 | Low | Fixed | Bundled Codex catalog origin opaque | The static `codex_capability_entries()` snapshot had no version/source/maintenance label; runtime callers (`/login`, `/model`, `/fast`) copied it into user settings with no way to distinguish bundled origin from a user edit later. | Added `BUNDLED_CODEX_CATALOG_VERSION` / `BUNDLED_CODEX_CATALOG_UPDATED` / `BUNDLED_CODEX_CATALOG_SOURCE` constants and accessors; picker surfaces provenance as `Bundled levels` / `Configured levels` / `Supported levels`. |

## 9. 更新规则

1. 新增开放问题写入本文，不再新增 `docs/issues*.md` 或 `docs/issues/` 下的活跃问题文档。
2. 只读审查原文、长日志和历史复盘放入 [archive/issues/](issues)。
3. 修复完成后，把问题从本文移到 [archive/resolved-known-issues-2026-05-07.md](resolved-known-issues-2026-05-07.md) 或后续同类 resolved archive。
4. 文档中若只有“已实现/已修复”历史，不应留在活跃入口；迁入 `development/archive/`。

## 10. Remote-Control Gateway Residuals (2026-05-08)

| ID | Severity | Status | Scope | Summary | Detail |
| --- | --- | --- | --- | --- | --- |
| REMOTE-001 | Medium | Open | Gateway busy policy | Mid-turn `steer` is intentionally unsupported. | `GatewayPolicy::supports_steer` defaults to false and `BusyPolicy::Steer` returns `501 unsupported`. Capabilities must not imply live steering until `QueryEngine` has explicit mid-turn injection semantics. |
| REMOTE-002 | Medium | Open | Telegram/Lark adapters | Telegram and Lark are outbound-control adapters only. | The first gateway release supports adapter configuration status, HTTP connect/health checks, and allowlisted test messages. Full inbound conversational remote control remains follow-up work. |
| REMOTE-003 | Medium | Open | Webhook configuration | Declarative webhook routes are code-backed but not yet backed by a full admin CRUD surface. | Built-in route ids resolve secrets from environment variables and use generic defaults for unknown route ids. A durable route-management UX/API is still needed before broad operator use. |
| REMOTE-004 | Medium | Open | Release verification | Session 16 performed docs-gate verification, not full remote-control code verification. | The next release step must run the Session 17 command set before claiming the gateway implementation is fully green. |
| REMOTE-005 | Low | Open | Public exposure | Public hosted gateway and multi-tenant SaaS are non-goals for this release. | Non-loopback use requires explicit remote-token policy and origin controls; production hosting design remains out of scope. |

## 11. Ratatui UI parity OMX verification residuals (2026-05-10)

| ID | Severity | Status | Scope | Summary | Detail |
| --- | --- | --- | --- | --- | --- |
| UI-005 | Low | Open | line endings | `git diff --check` passes but reports CRLF-to-LF normalization warnings for several touched files. | The warnings are not whitespace errors, but commit packaging should expect Git normalization on touched Rust/docs files. |

## 12. Agent Teams / Swarm residuals (2026-05-18)

| ID | Severity | Status | Scope | Summary | Detail |
| --- | --- | --- | --- | --- | --- |
| TEAMS-001 | Medium | Open | teammate session resume | TeamContext resume by session id is wired for team leads, but teammate self-session resume still depends on persisting `TeamMember.session_id`. | `restore_team_context_for_session()` matches `TeamFile.lead_session_id` and member `session_id`. Current in-process spawn records teammate members with `session_id: None`, so a teammate's own saved session cannot be restored by session id until runner/spawn records the child session id back into the team file. |

## 13. PTY E2E 测试发现的问题 (2026-05-23)

| ID | 严重度 | 状态 | 范围 | 摘要 | 详情 |
| --- | --- | --- | --- | --- | --- |
| PTY-001 | 低 | Fixed | PTY harness `status_bar()` | `status_bar()` 不再只读取最后一行；当 vt100 当前 screen buffer 最后一行为空时，会自底向上查找状态栏候选行，并从累积纯文本回退提取最近一次状态栏片段。 | 修复文件：`crates/claude-code-rs/tests/pty_tui_e2e/harness.rs`。验证：`cargo test -p claude-code-rs --test pty_tui_e2e status -- --nocapture`，状态栏相关离线用例通过。 |
| PTY-002 | 中 | Fixed | PTY model_flow 测试 | `ask_model_identity`/完整 model flow 的模型身份询问现在会检测首轮 `Conversation interrupted` 或 `Error:`，并对真实后端 transient interruption 自动重试一次。 | 修复文件：`crates/claude-code-rs/tests/pty_tui_e2e/model_flow.rs`。在线用例仍保留 `#[ignore]`，需要真实 API key/network；修复目标是让已观察到的首轮 transient interruption 不再直接导致测试失败。 |

## 14. 测试隔离 / 长跑门禁 (2026-07-18)

> 来源：复盘 Codex session `019f668f-207c-7853-aa03-e9f0755bcd9e`（2026-07-15→16）。该 session 串行暴露了 4 类根因，故障本身已在历史代码 commit 修复；本节作为「已修复 + 防回归」记录留存，并对应 [`CLAUDE.md`](../../CLAUDE.md) / [`AGENTS.md`](../../AGENTS.md) 的「测试分层验证 SOP」5 条。

| ID | 严重度 | 状态 | 范围 | 摘要 | 详情 |
| --- | --- | --- | --- | --- | --- |
| TESTISO-001 | 高 | Fixed | 跨 worktree target 共享 | 并行 worktree 共用 `…/.tmp/allthecodes-target` 时，跨树 build/test 会链接到对方分支的陈旧 `allthecodes-types` 元数据，导致 phantom 编译错误（现象表现为「定义在某分支已存在但链接器找不到」）。 | 根因：`CARGO_TARGET_DIR` 全局固定值被多个 worktree 共享。规约：并行 worktree 各自 `CARGO_TARGET_DIR=…/.tmp/atc-<slug>`；CLAUDE.md / AGENTS.md §「测试分层验证 SOP」第 3 条。session 2026-07-16 复盘定位（聚焦跑包级缓存清理后协议/types 重新编译、陈旧产物消失为根因证据）。 |
| TESTISO-002 | 高 | Fixed | ACP 凭据测试未隔离 `CODEX_HOME` | 鉴权测试在非隔离环境下读取真机 `~/.codex` 登录态，把「格式无效但非空的 API key」误判为可用凭据，触发非确定性测试失败。 | 根因：测试夹具未显式重定向 `CODEX_HOME` 与凭据环境。规约：所有涉及认证/凭据的集成测试在夹具启动时显式隔离 `CODEX_HOME`、`ANTHROPIC_API_KEY` 与同类环境变量，并强制 `RUNTIME_ENVIRONMENT=test`；CLAUDE.md / AGENTS.md §「测试分层验证 SOP」第 4 条。session 同期 commit 落地 ACP 隔离修复。 |
| TESTISO-003 | 高 | Fixed | daemon Team Memory 请求被 `HTTP_PROXY` 接管 | Team Memory 向固定 loopback endpoint 的回环请求被系统 `HTTP_PROXY` 接管，引起错误超时分类，并把内部 secret 通过环境代理外泄。同时 daemon 新 submit 未清除上一次 abort 状态，导致 abort 后的 submit 继承取消态。 | 根因：内部 RPC 客户端未强制 `no_proxy()` + submit/abort 状态机缺互斥。规约：对所有 loopback / 内部 RPC 客户端强制 `no_proxy()`，secret-bearing 客户端禁止使用环境代理；submit 与 abort 必须互斥排序（旧 abort 先发生则新 submit 重置，新 abort 后发生则取消当前 submit）。代码修复 commit `71bb6768`。CLAUDE.md / AGENTS.md §「测试分层验证 SOP」第 4 条覆盖环境/代理隔离。 |
| TESTISO-004 | 高 | Fixed | IPC `Optional<security>` 反序列化用非 Optional 结构 | `security: None` 序列化为 JSON `null`，legacy adapter 按非 Optional 结构反序列化触发 `InvalidPayload` panic；测试任务 panic 后接收端仍持 sender clone，造成无限等待（不会被测试 harness 自然超时）。 | 根因：协议 DTO 对可空字段未声明 `Option<T>` + adapter 转换在 pending 注册之后执行。规约：协议 DTO 对所有可空字段声明 `Option<T>`，反序列化对 missing / null 双兼容，非法非 null 仍 fail-closed；adapter 转换提前到 pending 注册之前；交互测试对每条接收路径加 5 秒接收边界，未来同类回归快速失败而非无限挂起。代码修复 commit `46b22cad`。 |
| TESTISO-005 | 低 | Open | 单一巨型 PTY 套件绑架全仓验证 | 全仓 `cargo test --workspace` 把 `pty_tui_e2e`（251 项并发 PTY、约 32min/轮）与 daemon / IPC / types / protocol 等 crate 级测试捆在同一条命令里。任何 crate 级失败必须等到 PTY 跑完才暴露，单轮全仓验证返工成本极高（session 期末 5 轮约 160 分钟纯测试时间）。 | 主要整改是 SOP 层面，不需要改产品代码：CLAUDE.md / AGENTS.md §「测试分层验证 SOP」第 1、5 条强制「先分 crate 再单独 PTY」「一次任务 >2 轮全仓必须降级」。后续若 PTY 仍需进一步拆分独立 binary 由独立任务推进，本条作为「Open」防回归哨兵保留。 |
