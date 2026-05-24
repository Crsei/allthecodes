# cc-rust 最终发布计划

> 更新日期: 2026-05-21
> 范围: `F:\AIclassmanager\cc\rust`
> 阶段: 全量构建 / Full Build 发布收口

本文是最终发布前的总控计划。它不替代现有状态文档，而是把仍未实现、实现不完美、需要裁剪决策和需要验证的事项按发布顺序收束起来。

现有事实入口仍是：

- [WORK_STATUS.md](WORK_STATUS.md): 当前完成度基线。
- [IMPLEMENTATION_GAPS.md](IMPLEMENTATION_GAPS.md): 未完备项、runtime caveats、intentional crop。
- [KNOWN_ISSUES.md](KNOWN_ISSUES.md): 当前开放问题和代码审查发现。
- [TECH_DEBT.md](TECH_DEBT.md): 代码层技术债。
- [RATATUI_UI_PARITY.md](RATATUI_UI_PARITY.md): Rust TUI 对标矩阵。

## 0. 最近完成的发布收口

以下条目不再按“待实现主线”处理，但仍需要在最终发布前完成 closeout 证据归档：

| 范围 | 已完成状态 | 发布前剩余动作 |
| --- | --- | --- |
| Crate 重构 / owner migration | `claude-code-rs/src/engine/**` 与 `claude-code-rs/src/ipc/**` 已删除；engine/agent 实现由 `cc-engine` 拥有，IPC JSONL runtime、agent settings 与共享 protocol/handler facade 由 `cc-ipc` / `cc-ipc-client` / `cc-ipc-protocol` 拥有；root binary 仅保留 startup、Rust TUI 和 runtime adapter glue。2026-05-14 默认 workspace gates 已 green。 | 关闭 thin-binary source guards：剩余 cross-crate UI path shims、root-style imports、allow attributes、Codex compatibility path hits 需要清零或登记为 intentional residual；完成后把 crate migration 计划迁入 archive，并补 [archive/COMPLETED_FULL.md](archive/COMPLETED_FULL.md)。 |
| Ratatui UI 美化 / P0-P1 parity | OMX closeout 已确认共享 UI primitives、settings/safety surfaces、message/composer surfaces、agent/team/task/search/integration surfaces 和对应 snapshot 更新已落地；P0/P1 基础面不再作为发布阻塞主线。 | 保留 runtime residual 跟踪：最新 shell output 自动展开、跨会话 history 质量、真实 Browser MCP/IDE/PR 数据路径，以及最终 UI snapshot / e2e release gate。 |
| Rust TUI cfg-test production wiring | 2026-05-21 Phase 1-16 已完成：`CommandSurface`、`CommandSurfaceDialog`、`Dialog`/`Tabs` helpers、agent create/edit wizard、MCP detail/tool panes、permissions、tasks/team、snapshot/runtime hooks 都已进入生产构建；验证通过 `cargo test -p claude-code-rs ui::`、`cargo build --workspace --release`、`git diff --check`；实现提交 `9ac3ae5`。 | 不再把这些 surface 作为“test-only 未接线”发布缺口；剩余只跟踪真实 backend 数据路径、live Browser/remote 证据和最终 release gate。 |
| Auto mode enable policy | `SAFETY-001` 已修复：`permissions.enableAutoMode=false` 现在由统一的 permission transition helper 强制执行，启动配置、Web settings、`/permissions`、`/config` 与子上下文不能绕过进入 Auto mode。 | 发布证据保留新增回归测试：`cc-permissions auto_mode`、`cc-commands auto_respects_disabled_policy`、`cc-startup build_tool_permission_context_blocks_startup_auto_when_disabled`、`cc-web set_permission_mode_auto_respects_disabled_policy`、`claude-code-rs --test e2e_permissions`；[KNOWN_ISSUES.md](KNOWN_ISSUES.md) 中保持 `SAFETY-001` Fixed。 |
| Safety closeout | `SAFETY-002` 到 `SAFETY-005` 已修复：Plan `allowedPrompts` 在 Auto mode 恢复后立即剥离危险 transient allow；Plan approval UI 展示具体去重规则；sandbox `allowedCommands` 对 sandbox availability 和 compound shell argv fail-closed；classifier redaction 覆盖 JSON secret 字段。 | 发布证据保留本轮验证：`cargo test -p cc-tools plan_mode -- --nocapture`、`cargo test -p cc-sandbox allowed_command -- --nocapture`、`cargo test -p cc-engine sandbox_allowed_command -- --nocapture`、`cargo test -p cc-safety redaction -- --nocapture`、`cargo fmt --check`；[KNOWN_ISSUES.md](KNOWN_ISSUES.md) 中保持 `SAFETY-002` 到 `SAFETY-005` Fixed。 |

## 1. 发布定义

最终发布不是“没有任何未来增强”，而是达到以下状态：

1. 支持面内的功能与上游 TypeScript / Bun 版本行为对齐，或者已经在 [IMPLEMENTATION_GAPS.md](IMPLEMENTATION_GAPS.md) 登记为 intentional crop。
2. 高危开放问题关闭，不再存在会导致静默降级、权限绕过、上下文错误估算、模型路由错误或持久化损坏的已知缺陷。
3. 发布门禁命令在干净工作区通过：`cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`、`cargo build --release`。
4. provider、daemon、TUI、headless IPC、MCP、session/export、permissions 等关键路径都有可重复的目标测试或 e2e 证据。
5. 文档不再用历史 `rust-lite` 口径描述当前边界；所有“暂不做”都有复审条件。

## 2. 发布阶段

| 阶段 | 目标 | 退出条件 |
| --- | --- | --- |
| R0: 范围冻结 | 固定最终发布支持面，逐项重评历史 Deferred。 | 所有未跟随上游的能力都进入 intentional crop，或进入后续阶段任务。 |
| R1: 阻塞缺陷清零 | 先修构建、测试、权限、安全、上下文和模型路由阻塞。 | [KNOWN_ISSUES.md](KNOWN_ISSUES.md) 中高危项关闭或降级并有证据。 |
| R2: 核心能力补齐 | 收束 API、PlanMode、TaskTools、Team Memory、WebFetch、Daemon、Session Export、Computer Use。 | 每条核心链路有目标测试，真实 provider/daemon/browser 路径有 e2e 或明确裁剪。 |
| R3: 产品体验补齐 | 收束 Ratatui UI、Browser MCP、插件/配置/设置面和命令面；Web UI 不纳入本轮 P2。 | 用户可见主路径无“占位可用但实际不可用”的状态。 |
| R4: 可观测与运维 | 补 traceable logging、audit/session export、daemon soak、发布包和路径隔离复核。 | 发布后问题能定位、导出、复现和回滚。 |
| R5: 发布候选 | 完整门禁、安装/升级/回滚验证、文档冻结。 | 生成 release candidate，发布说明列出已支持能力、裁剪项、已知残余风险。 |

## 3. 发布门禁

### G0 范围与裁剪门禁

- [IMPLEMENTATION_GAPS.md](IMPLEMENTATION_GAPS.md) 中的历史 Deferred 不允许继续停留在“以后再说”状态。
- 每个 deferred surface 必须进入三类之一：本次实现、下个版本延期且有复审条件、intentional crop。
- 已确认裁剪项必须包含理由、决策者、日期和复审触发条件。

### G1 构建与测试门禁

发布前必须通过：

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release
```

专项门禁至少覆盖：

```bash
cargo test -p claude-code-rs --test e2e_terminal
cargo test -p claude-code-rs --test e2e_browser_mcp
cargo test -p claude-code-rs daemon::routes
cargo test -p claude-code-rs daemon::sse
cargo test -p claude-code-rs commands::daemon_cmd
cargo test -p claude-code-rs tools::hooks
cargo test -p claude-code-rs engine::lifecycle
cargo test -p claude-code-rs query::loop_helpers
```

真实 provider e2e 需要凭据时，发布记录要写明运行环境、跳过原因或替代证据。

### G2 安全与权限门禁

- Auto mode、Plan mode、sandbox、allowed command/prompt、classifier redaction 不允许存在已知绕过。
- 所有“配置存在但损坏”的路径必须返回可见诊断，不能静默当作缺省配置。
- Critical hook、权限 hook、安全 hook 的失败策略必须符合 fail-closed / warn-only 分级。

### G3 状态与持久化门禁

- `~/.cc-rust/` 路径隔离必须覆盖 credentials、settings、skills、daemon、session、plugins 和 cache。
- session 保存/恢复、task store、team mailbox、daemon command/event、plugin metadata 不允许因解析失败静默覆盖。
- Session export / audit export 能导出可追踪、可验证的记录；不完整字段必须标注清楚。

### G4 UI 与运行时门禁

- Ratatui 主交互路径、headless IPC、MCP 面板、权限对话框、配置面板、`/agents` 创建/编辑、`/permissions`、`/tasks`、`/team` 必须能完成真实工作流。
- UI snapshot、message suite、关键 e2e 覆盖 shell output、file edit、permission、task、MCP、history/search、resize。
- Browser MCP 与 Computer Use 至少有一条真实或 fake-server e2e，证明图片/tool result 不丢失。

### G5 文档与发布门禁

- `docs/README.md`、[WORK_STATUS.md](WORK_STATUS.md)、[IMPLEMENTATION_GAPS.md](IMPLEMENTATION_GAPS.md)、[KNOWN_ISSUES.md](KNOWN_ISSUES.md) 与发布说明一致。
- 旧 `Lite` wording 只允许出现在历史说明、分支名说明或 archive。
- 发布说明必须列出：支持矩阵、intentional crop、迁移说明、验证命令、已知残余风险。

## 4. 未实现与不完美项总表

### P0: 发布阻塞

| 范围 | 当前状态 | 发布口径 | 证据入口 |
| --- | --- | --- | --- |
| e2e terminal | `phase6.rs` 已纳入 `crates/claude-code-rs/tests/e2e_terminal/` 的 tracked 文件集。 | e2e terminal 测试可在干净 checkout 运行，不依赖本地遗留文件。 | [archive/resolved-known-issues-2026-05-07.md](archive/resolved-known-issues-2026-05-07.md) `TEST-001` |
| Context compact | auto compact 使用本地压缩后的 request estimate；exact count preflight 使用最终请求边界。 | 模型请求大小估算可信，compact 不会错误跳过或误判。 | [archive/resolved-model-context-2026-05-07.md](archive/resolved-model-context-2026-05-07.md) `CONTEXT-001` / `CONTEXT-002` |
| Model/provider mapping | Bedrock model mapping、removed legacy alias rejection 与文档口径已收敛。 | provider 模型路由准确；文档、配置校验、运行时错误口径一致。 | [archive/resolved-model-context-2026-05-07.md](archive/resolved-model-context-2026-05-07.md) `MODEL-001` / `MODEL-002` / `DOC-001` |
| Critical post hooks | critical post/failure hook 错误已返回 visible failed `ToolExecResult`，optional hook 保持 warn-only。 | critical hook 失败影响当前 step/tool，optional hook 才 warn-only。 | [archive/TECH_DEBT.md](archive/TECH_DEBT.md) `Critical post-tool and post-failure hook propagation` |

### P1: 核心能力必须补齐或裁剪

| 范围 | 当前状态 | 预期发布效果 | 证据入口 |
| --- | --- | --- | --- |
| API providers | Bedrock/Vertex/Azure 基线存在；provider capability DTO、Azure OpenAI alias、Azure/Foundry unsupported diagnostics 与 Foundry fail-early 已接入。真实 provider/e2e 和更多 mock fixtures 仍需补齐。 | 每个声明支持的 provider 有配置文档、能力矩阵、真实或受控 e2e；不支持项 fail early。 | [IMPLEMENTATION_GAPS.md](IMPLEMENTATION_GAPS.md), [mvp-optimization-plans/MVP-001-api-providers-plan.md](mvp-optimization-plans/MVP-001-api-providers-plan.md); 验证: `cargo test -p cc-api provider_validation`, `cargo test -p cc-api test_from_env_result_errors_for_explicit_foundry`, `cargo test -p cc-commands login` |
| Team Memory | Rust daemon spawn 参数接通，但同步、断线恢复、冲突处理 e2e 未闭环。 | 多客户端同步可恢复、冲突可诊断，Team Memory 旧 plan/spec 可归档。 | [IMPLEMENTATION_GAPS.md](IMPLEMENTATION_GAPS.md) |
| TaskTools remote/multi-type | 本地 task 基础完成，remote/multi-type poller/reconnect parity 未完。 | 远程/多类型后台任务可恢复、可轮询、可取消、可在 `/tasks` 统一查看。 | [IMPLEMENTATION_GAPS.md](IMPLEMENTATION_GAPS.md) |
| PlanMode auto-mode | 保守 classifier 与持久化已落地，full LLM classifier parity 和 `allowedPrompts` 分类未完。 | Plan 创建、恢复、审批、退出、规则写入都有统一策略和 e2e。 | [IMPLEMENTATION_GAPS.md](IMPLEMENTATION_GAPS.md), [KNOWN_ISSUES.md](KNOWN_ISSUES.md) |
| WebFetch | HTTP redirect/MIME/proxy/credential boundary 完成；JS rendering 已按 HTTP-only release scope 写入 intentional crop。 | 本次发布只承诺 HTTP fetch；browser-grade JS rendering 不作为 WebFetch 发布能力。 | [IMPLEMENTATION_GAPS.md](IMPLEMENTATION_GAPS.md) §6, [archive/COMPLETED_FULL.md](archive/COMPLETED_FULL.md) |
| Daemon ownership | `/api/submit` 与 `/api/abort` 已只投递 `cc-daemon` command，assistant worker 执行 submit/abort 并写 event log；permission response 已 durable ack。resize/history 仍是 residual。 | submit/abort/permission/resize/history 由 worker/scheduler 模型拥有，HTTP route 不再承担兼容执行路径。 | [reference/DAEMON_OPERATIONS.md](reference/DAEMON_OPERATIONS.md), [plan/daemon-usability-plan.md](plan/daemon-usability-plan.md) |
| Session export | Schema v2 已加入 raw transcript、api view summary、`ApiRequestSnapshot`、custom title 与图片块可读占位；context collapse、mode/tag 来源和完整 apiView 投影仍是 residual。 | 导出包可审计、可回放、能说明模型实际看到的上下文和裁剪历史。 | [plan/session-export-implementation-guide.md](plan/session-export-implementation-guide.md); 验证: `cargo test -p cc-session request_snapshot`, `cargo test -p cc-session session_export`, `cargo test -p cc-commands session_export`, `cargo test -p cc-engine lifecycle` |
| Computer Use | 外置 MCP 和图片链路多数完成；session export 现在保留图片块 metadata 占位并避免 base64 丢失/膨胀。fake-server 与 session save/restore 回归仍需补齐。 | screenshot 图片块在工具结果、下一轮模型请求、session 保存/恢复、导出中不丢失。 | [plan/computer-use-implementation-checklist.md](plan/computer-use-implementation-checklist.md); 验证: `cargo test -p cc-session session_export` |
| Browser MCP | `--chrome-native-host`、Chrome MCP bridge 和 Browser MCP 配置 smoke/e2e 已存在；真实第三方 Browser MCP server 仍需手动 release evidence。 | fake bridge/native-host 路径可重复验证；缺少真实第三方 server 证据时不声称 live server 已验证。 | [KNOWN_ISSUES.md](KNOWN_ISSUES.md) `UI-004` |
| Crate / IPC ownership closeout | Engine + IPC owner migration 已落地，root `engine/**` / `ipc/**` source 已删除；IPC envelope version/min-compat 策略和 roundtrip tests 已补。thin-binary guard 仍有 path shims、root-style imports、allow attributes 与 Codex compatibility path hits。 | 关闭 source guard，固定 IPC 协议版本策略；crate migration 从活跃计划迁入 archive/completed-full。 | [IMPLEMENTATION_GAPS.md](IMPLEMENTATION_GAPS.md), [plan/crate-migration-phase-plan-2026-05-14.md](plan/crate-migration-phase-plan-2026-05-14.md), [reference/CRATE_MIGRATION_PHASE0_OWNER_GUARD_MATRIX.md](reference/CRATE_MIGRATION_PHASE0_OWNER_GUARD_MATRIX.md); 验证: `cargo test -p cc-ipc-protocol envelope` |
| Plugin diagnostics | `/reload_plugins` 已通过 `cc-plugins::ReloadReport` / command runtime 输出 global metadata/cache/manifest diagnostics；后续只保留测试隔离 residual。 | plugin metadata/cache/manifest 错误对用户可见，不再表现为“没有插件”。 | [TECH_DEBT.md](TECH_DEBT.md) Remaining P1 follow-ups |
| Auth compatibility | `resolve_auth()` / `resolve_codex_auth_token()` 已委托 `try_*` diagnostic API 并在兼容 wrapper 中记录 warning；坏凭据/keychain/token 读取失败不再被 diagnostic API 表现为 unauthenticated。 | 外部调用迁移到 diagnostic API；兼容 wrapper 不吞掉关键凭据错误。 | [TECH_DEBT.md](TECH_DEBT.md) Remaining P1 follow-ups |

### P2: 产品体验与支持面

| 范围 | 当前状态 | 预期发布效果 | 证据入口 |
| --- | --- | --- | --- |
| Ratatui UI polish baseline | P0/P1 基础与美化已完成：共享 primitives、settings/safety、message/composer、agent/team/task/search/integration surfaces 以及 snapshot 更新已收口；cfg-test production wiring 已把 `/agents` create/edit、`/mcp` detail/tools、`/permissions`、`/tasks`、`/team`、dialog/tabs helpers 接入生产构建。 | 发布前保持 snapshot gate；新增 UI 只能作为真实 surface 接入，不再引入空占位。 | [RATATUI_UI_PARITY.md](RATATUI_UI_PARITY.md) `Ratatui UI parity OMX closeout`, [archive/ratatui-ui-parity-omx-execution-report-2026-05-08.md](archive/ratatui-ui-parity-omx-execution-report-2026-05-08.md), [plan/cfg-test-production-wiring-plan-2026-05-21.md](plan/cfg-test-production-wiring-plan-2026-05-21.md); 验证: `cargo test -p claude-code-rs ui::`, `cargo build --workspace --release` |
| Ratatui shell output residual | 最新 Bash/PowerShell tool result 已由 runtime context 自动展开；历史 shell 长输出默认折叠，选中后可展开/折叠查看 detail。 | 长输出默认策略符合上游体验，用户能快速展开、折叠、查看细节。 | [RATATUI_UI_PARITY.md](RATATUI_UI_PARITY.md), [KNOWN_ISSUES.md](KNOWN_ISSUES.md) `UI-002` |
| Ratatui history residual | Ctrl+R 按当前 workspace 读取跨会话持久 prompt history，条目带 session/title/cwd 来源与时间；空态明确。 | 跨会话 prompt history 结果稳定带来源和时间；缺少后端数据时给出清晰空状态。 | [KNOWN_ISSUES.md](KNOWN_ISSUES.md) `UI-003` |
| Ratatui backend-gated surfaces | LSP/IDE/Chrome/channel/Claude Desktop MCP 等 UI surface 已有状态入口或 snapshot 覆盖；Browser/Chrome fake e2e 存在，真实第三方 Browser MCP 证据仍待 release 手动补充。 | live backend 不可用时显示诊断；可用时有真实 e2e 或 fake-server 证据。 | [RATATUI_UI_PARITY.md](RATATUI_UI_PARITY.md), [KNOWN_ISSUES.md](KNOWN_ISSUES.md) `UI-004` |
| Remote channels | `/channels` 明确只展示 gateway-backed outbound adapter status/control；Telegram/Lark adapter 支持 HTTP connect/test-message。Inbound conversation、remote triggers 写入 roadmap/crop。 | Telegram/Lark 等远程入口可连接、测试、显示状态；不把 inbound channel sessions 当成本轮能力。 | [reference/REMOTE_CONTROL_GATEWAY.md](reference/REMOTE_CONTROL_GATEWAY.md) |
| Voice | `/voice`、`voiceEnabled`、keybinding 和 language normalization 保留为兼容面；audio/STT 后端是明确 unsupported-build crop。 | UI/CLI 明确显示不可用原因，不声称真实录音或转写支持。 | [claude-code-configuration/voice-dictation.md](claude-code-configuration/voice-dictation.md) |
| Browser native host | `--chrome-native-host`、Chrome MCP bridge 和 smoke tests 已存在；真实 Chrome extension/第三方 server 验证作为 release evidence。 | fake bridge/native-host 端到端证据可重复；真实 server 缺失时不声称已验证。 | `crates/claude-code-rs/tests/e2e_chrome_native_host.rs`, `crates/claude-code-rs/tests/e2e_chrome_mcp_bridge.rs` |
| LSP transport | `workspace/configuration` request 返回 per-item `null` 数组；notification 明确诊断为无 id、不可响应。 | LSP server 请求处理有响应策略，不再只记录未实现诊断。 | `crates/cc-lsp-service/src/client.rs` |
| Branch command | `/branch` fork 成功后通过 `SwitchSession` 切换 engine/TUI/headless/daemon/web active session pointer。 | 新分支会话切换后，后续消息进入正确 session。 | `crates/cc-commands/src/branch.rs` |
| Terminal setup | `/terminal-setup` 只描述当前 runtime 真实支持的 sync update、mouse capture、scroll speed；Shift+Enter/editor 等仍标为 manual setup。 | 支持面内的 terminal setup 明确当前 runtime 行为；不可自动修改用户 terminal 配置的项不再暗示自动配置。 | `crates/cc-commands/src/terminal_setup.rs` |
| Documentation debt | 活跃顶层 docs 改为 Full Build 当前事实；历史 Lite 口径仅保留在历史说明或 archive。 | 顶层 docs 只反映 Full Build 当前事实；历史内容迁入 archive。 | [KNOWN_ISSUES.md](KNOWN_ISSUES.md) `DOC-003`, [TECH_DEBT.md](TECH_DEBT.md) |

### P3: 发布后增强或已裁剪

| 范围 | 决策 | 发布处理 |
| --- | --- | --- |
| Agent Teams tmux/iTerm2 pane backend | 已登记 intentional crop。 | 发布说明列入裁剪项；复审触发条件沿用 [IMPLEMENTATION_GAPS.md](IMPLEMENTATION_GAPS.md)。 |
| Windows OS-level sandbox primitive | 已登记 intentional crop。 | 发布说明列入裁剪项；`/sandbox require` 保持 fail-closed 和 unavailable 诊断。 |
| LogoV2/装饰动画/纯 React 抽象 | Ratatui 不需要 1:1 复制。 | 保持为不适用或后续美化，不阻塞发布。 |
| Remote/service/analytics/MDM/internal Ant-only | 历史 deferred，需要 R0 重评。 | 不允许继续模糊 deferred；本次不做就写 crop 或下版本计划。 |

## 5. 推荐执行顺序

1. 重验 P0 闭环：`TEST-001`、CONTEXT、MODEL、critical hook 保持已关闭状态，活跃问题入口不再列为发布阻塞。
2. 收 P1 核心链路：API provider e2e、PlanMode、TaskTools、Team Memory、Daemon ownership、Session Export、Computer Use、Browser MCP。
3. 收 P2 用户体验：Ratatui runtime residuals、remote channel 决策、voice/browser/LSP/branch/terminal setup；cfg-test production wiring 已完成，不再作为待执行主线。
4. 做全仓文档收口：迁移完成历史到 archive，清理 Lite/mojibake，补最终发布说明草稿。
5. 跑 release candidate 门禁：完整 cargo gate、真实 provider smoke、daemon soak、TUI snapshot、headless/Web/Brower MCP e2e。

## 6. 模块验收效果

| 模块 | 发布时应达到的效果 |
| --- | --- |
| CLI / QueryEngine | 主循环可恢复、可中断、可压缩、可执行工具；错误不会被误报为成功或缺省状态。 |
| Tools / permissions | 每个工具调用都有权限、hook、执行、结果、审计链路；高风险策略 fail closed。 |
| API providers | 支持矩阵可信，模型别名和 provider ID 一致，真实错误有 actionable diagnostic。 |
| Session / compact / export | 用户能导出原始 transcript、API view、compact/microcompact 记录和 request snapshot。 |
| Daemon | 后台 supervisor/worker/scheduler 有清晰 ownership；HTTP/SSE 只是控制面，不隐藏执行失败。 |
| MCP / Browser / Computer Use | 外部 server 可发现、可授权、可调用；图片和结构化结果能进入模型上下文与保存层。 |
| Ratatui / headless | 主路径可完成真实工作，缺失功能不会以空 UI 或占位结果伪装成可用。 |
| Docs / release notes | 用户知道能用什么、不能用什么、为什么不能用、如何验证和回滚。 |

## 7. 发布说明草稿要求

最终 release note 至少包含：

- 支持平台和已验证环境。
- provider 支持矩阵和需要的环境变量/凭据。
- 命令、工具、MCP、TUI、daemon 支持矩阵。
- intentional crop 列表和复审触发条件。
- 从历史 `rust-lite` 配置迁移到 Full Build 的注意事项。
- 发布门禁命令及通过摘要。
- 已知残余风险，只允许列中低风险或明确被产品接受的风险。

## 8. 文档维护规则

1. 本文只保留发布总控视图，不展开长实现设计。
2. 具体实现计划继续放在 `docs/plan/`；完成后迁移到 `docs/archive/`。
3. 新增开放问题先写 [KNOWN_ISSUES.md](KNOWN_ISSUES.md)，再在本文按发布优先级引用。
4. 新增功能缺口先写 [IMPLEMENTATION_GAPS.md](IMPLEMENTATION_GAPS.md)，除非它只是纯代码质量问题。
5. 完成一个条目时，同步更新本文、源状态文档和 archive 记录，避免活跃文档保留已完成历史。

## 9. Remote-Control Gateway Release Gate (2026-05-08)

Remote-control gateway can be considered release-gate complete only when the following evidence is attached to the release record:

| Gate | Required evidence |
| --- | --- |
| API contract | `docs/reference/REMOTE_CONTROL_GATEWAY.md` lists every `/remote-control/v1/**` endpoint, request shape, response shape, auth requirement, and stable error code. |
| Local UX | `/remote status`, `/remote adapters`, `/remote runs`, `/remote show`, `/remote events`, `/remote stop`, `/remote doctor`, the command palette metadata, `RemoteSurface`, and status-widget remote indicator are documented. |
| Security | Bad daemon token, bad remote token, bad origin, payload too large, HMAC failure, duplicate idempotency, busy, queue full, and stale response cases have stable diagnostics and tests. |
| Recovery | Daemon startup recovery, queued run visibility, recoverable active runs, pending approval/user retention, and stale session-lock recovery are documented and tested. |
| Adapter scope | Telegram and Lark are documented as connect/health/status/test-message only; full inbound conversational control is explicitly follow-up work. |
| Non-goals | Public hosted gateway, multi-tenant SaaS, full WebSocket parity, remote desktop control, and real mid-turn `steer` are not release claims. |

Remote-control-specific verification commands:

```powershell
cargo test -p gateway
cargo test -p claude-code-rs remote_cmd ui::components::command_palette ui::components::command_surface ui::components::status_widget
cargo test -p claude-code-rs daemon::protocol daemon::routes daemon::sse daemon::supervisor
cargo check -p claude-code-rs --message-format short
```

The Session 16 docs gate verified documentation consistency only. It did not rerun the full code gate; the implementation report records that as a remaining release risk until Session 17/final verification is run.
