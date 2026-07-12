# allthecodes 未完备项与全量构建 TODO

> 更新日期: 2026-07-13 | 当前阶段: 全量构建 / Full Build

本文只登记仍未补齐、仍需重评或明确 intentional crop 的内容。已确认实现或已关闭的历史记录已迁移到：

- [archive/COMPLETED_FULL.md](COMPLETED_FULL.md)
- [archive/COMPLETED_SIMPLIFIED.md](COMPLETED_SIMPLIFIED.md)
- [archive/completed-gap-closures-2026-05-07.md](completed-gap-closures-2026-05-07.md)

开放问题与代码审查发现统一看 [KNOWN_ISSUES.md](KNOWN_ISSUES.md)。当前完成度基线看 [WORK_STATUS.md](../../docs/WORK_STATUS.md)。最终发布阶段、门禁和预期效果看 [FINAL_RELEASE_PLAN.md](FINAL_RELEASE_PLAN.md)。

2026-07-04 关闭项：KAIROS resident assistant / daemon parity 主线已完成。system prompt resident sections、assistant/bridge/proactive/scheduler worker ownership、automation state DTO、notification/channel ingress、dream memory distillation、scheduler supervision、HTTP/SSE history replay、daemon CLI submit/sleep/stop E2E 和 graceful shutdown request 监听已落地。默认验证不需要真实 provider；live provider smoke 因需要凭据/网络保留为按需检查。

2026-05-21 关闭项：Rust TUI cfg-test production wiring plan Phase 1-16 已完成，`CommandSurface`、`CommandSurfaceDialog`、agent create/edit、MCP detail/tool panes、permissions、tasks/team、dialog/tabs helpers 已进入生产构建；验证通过 `cargo test -p allthecodes ui::`、`cargo build --workspace --release`、`git diff --check`，实现提交 `9ac3ae5`。该计划不再作为活跃缺口，只保留真实 backend/e2e 证据和 runtime residual。

## 1. 当前仍未完成或仅部分完成

| 范围 | 当前状态 | 说明 |
| --- | --- | --- |
| API providers | 基线完成，真实凭据证据部分收口 | Bedrock 原生 AWS EventStream、Vertex direct service-account JWT exchange、provider capability DTO、Azure/OpenAI/Foundry 命名诊断、Foundry fail early、Anthropic-compatible bearer/custom base URL、SOTA/MOTA/FOTA 默认值和 provider smoke matrix 已接入；2026-05-21 真实 smoke 已通过 direct bearer、compatible custom base URL bearer、prompt-cache default 与 TTL/global gates，direct API key、Bedrock、Vertex 因缺少对应凭据跳过。 |
| Team Memory 客户端同步 | 代码已接通，待验证/文档收口 | `ui/team-memory-server/sync.ts` / `watcher.ts` 与 Rust daemon spawn 参数已接通；仍需同步、断线恢复、冲突处理 e2e。 |
| TaskTools remote/multi-type runtime | 本地与 UI production 完成，remote runtime parity 未完 | 持久化、依赖字段、输出保留、`TaskOutput` 阻塞/超时、task taxonomy、remote metadata、recoverable marker、restore timer reset、remote review timeout guard、local-agent 取消已完成；`/tasks` production surface 已接入 BackendMessage、task list/detail、shell/remote/agent/team/MCP/dream/workflow detail renderers、stop/delete/refresh actions。仍需 remote/multi-type poller/reconnect runtime parity。 |
| Hermes runtime execution bridge | 基础层完成，child/runtime execution parity 未完 | `session_search` warm memory、`/session search`、Web/API search、模型可见 `SessionSearch`、审批式 memory/skill proposals、background review proposals、scheduled task registry/daemon dispatch 和 `DelegateTask` delegation envelope 已落地。剩余缺口：scheduled task 的 `cwd` 目前只进入 source/event/system prompt metadata，尚未作为 `QueryEngine` 执行 cwd override；`DelegateTask` 会创建 child session、task 记录、worktree metadata 和 searchable bootstrap transcript，但尚未直接启动 caller-provided child `QueryEngine` runtime。 |
| PlanMode auto-mode parity | 基础完成，classifier parity 未完 | 保守 classifier、计划持久化、approval lifecycle、实现任务关联、团队审批 mailbox、plan file 写入白名单已落地；仍需 full auto-mode LLM classifier parity 和 `allowedPrompts` 语义分类收口。 |
| WebFetch browser-grade 能力 | HTTP-only release scope | redirect budget / cross-host diagnostic、Content-Type 分发、环境代理/`NO_PROXY`、Cookie/credential 边界已完成；browser-grade JS rendering 已登记为 §6 intentional crop，本次发布不承诺。 |
| Daemon live-provider smoke | 默认 E2E 完成，真实模型 smoke 按需 | `crates/allthecodes/tests/e2e_cli.rs` 已覆盖 stopped/start/status/submit/sleep/stop、worker IDs、automation state、history DTO 和 graceful shutdown；`development/reference/kairos-live-smoke.sh` 仍需真实 provider 凭据、provider 网络和空闲端口，因此不是默认门禁。 |
| Session export | API snapshot/schema v2 已接入，context collapse residual | 导出 schema v2 已包含 raw transcript、api view summary、`ApiRequestSnapshot`、custom title 和图片块可读占位；仍缺 context collapse 原生事件、mode/tag 来源和完整 api-view 投影。 |
| Crate migration / thin binary | Engine + IPC owner migration 已落地，thin-binary guard 未关闭 | 旧 root `src/engine/**` 与 `src/ipc/**` 已删除；`allthecodes-engine` 拥有 engine/agent，`allthecodes-ipc`/`allthecodes-ipc-client`/`allthecodes-ipc-protocol` 拥有 IPC runtime、client helper 与 wire DTO；IPC envelope 已有 version/min-compat 策略。仍需收束剩余 root-style imports、allow-attribute hits、Codex compatibility path hits，并跑完整 workspace/release gates。 |
| Remote-control gateway control plane | 基础完成，release evidence 未全收口 | `crates/gateway` 已拥有 `/remote-control/v1/**`、RemoteSource/session/run、auth、adapter registry、durable events、delivery 和 recovery；现有 daemon `/api/*` 仍不是公网 remote-control API。剩余工作是 release gate 验证、public exposure policy 和 admin UX。 |
| Telegram/Lark gateway adapter connectivity | outbound 完成，inbound 裁剪 | 第一版支持连接、健康检查、provider-neutral 状态诊断和 allowlisted test-message 发送；不做 inbound conversation、完整远程会话控制或绕过 gateway runner 触发模型。 |
| Local `/remote` and TUI remote surface | 基础完成，验证残留 | `/remote` slash command、`RemoteSurface`、remote status indicator 已存在并读取 local gateway status/adapters/runs/events。剩余工作是完整 release gate 验证和真实运维证据。 |
| Remote/teleport command surfaces | teleport deferred | `/channels` is intentionally limited to gateway-backed outbound adapter status for now. Inbound channel sessions and any `/teleport` command remain deferred until a product/runtime contract exists; do not present placeholders as real remote-control capability. |
| Computer Use parity | 核心安全/执行路径已补，Windows deep integration 待验证 | `mcp__computer-use__*` 工具名、Executor 串行化、真实 lock、截图安全临时文件、Linux Wayland capability 误报已修。剩余缺口：Win32 COM/UI Automation/virtual cursor/window border/input indicator 仍主要是模块级能力，尚未完整接入工具/executor 可达路径；Wayland `ydotool` 输入 backend 未实现；真实 Windows/macOS/Linux 桌面 smoke 证据待补。 |
| Hook runtime parity | command/http 基线已接，prompt/agent/file watcher 仍缺 | `PreToolUse updated_input` 重校验、critical 非零退出 fail-closed、HTTP hook + SSRF guard、hook events/async registry/session hooks 基线已接。剩余缺口：prompt/agent hook 仍未接真实 LLM/runtime 执行，当前显式报错；FileChanged watcher、hooksConfigSnapshot/变更检测、skill improvement LLM 写回和 malformed hook config 结构化诊断仍待补。 |
| Agentic workflow injection defense | task4–6 第一版已落地，workspace release gate 待完成 | setup-chain / supply-chain 静态扫描、scanner Deny/Ask 接入 canonical tool gate、精确一次性审批、redacted security record/audit/IPC/UI metadata 和 deterministic attack fixtures 已实现。扫描器是有界 lexical analysis，不执行仓库代码；复杂 YAML/脚本语义仍可能产生保守 Ask。focused/allthecodes E2E 与 workspace check 已通过；workspace clippy/release gate 当前受共享工作区 `allthecodes-mcp/src/runtime.rs` 缺失 `take_installed_manager` 影响。 |
| Runtime verification evidence / Session Report | 第一版闭环已落地，完整 traceable logging 仍未完成 | canonical verification/artifact/report records、成功工具证据分类、最多三轮 verify-continue、redacted/tamper-linked `SessionReportV1`、Web/IPC/TUI/telemetry surfaces 和 deterministic e2e 已实现。仍缺统一 durable runtime audit event log、完整 submit/turn/request/event correlation、daemon/IPC/permission/stream/progress 覆盖，以及 audit export 以该事实源为主。workspace clippy 仍受共享工作区缺失的 `allthecodes-mcp::take_installed_manager` 测试辅助函数阻塞。 |
| Phase 5 Vault encrypted storage parity | transitional credentials store | `VaultHttpFetch` 已补 `vault_auth_key` schema、key@host permission prompt、HTTPS/private-network/redirect/timeout/body-cap/secret-scrub 边界；但凭据读取仍来自 `{data_root}/credentials.json` 的 `vault` 对象和 deprecated `credential_ref` alias，不宣称 encrypted vault parity。后续需实现/接入受支持 encrypted vault storage 后再关闭。 |

## 2. 活跃方案文档

以下文档仍是活跃入口，不应归档为“已完成”：

- [computer-use-implementation-checklist.md](computer-use-implementation-checklist.md): Computer Use 落地清单，仍是待实施能力。
- [session-export-implementation-guide.md](session-export-implementation-guide.md): Rust 侧仍缺完整导出基础设施。
- [traceable-logging-plan.md](traceable-logging-plan.md): 可追溯日志体系仍是 Draft；runtime verification/session report 已完成第一版证据闭环，但不等于整套 durable audit 体系完成。
- [daemon-usability-plan.md](daemon-usability-plan.md): daemon 可用化主干已分阶段落地，但仍有 worker/route ownership 余量。
- [plan/crate-migration-phase-plan-2026-05-14.md](plan/crate-migration-phase-plan-2026-05-14.md): Phase 0-12 implementation slices 已落地且 workspace build/test gates green；thin-binary closeout 仍未关闭，当前 blockers 见 [reference/CRATE_MIGRATION_PHASE0_OWNER_GUARD_MATRIX.md](../reference/CRATE_MIGRATION_PHASE0_OWNER_GUARD_MATRIX.md) 的 Phase 12 verification snapshot。
- [reference/remote-control-current-state.md](../reference/remote-control-current-state.md): remote-control gateway / daemon / ipc / `/remote` / Telegram/Lark adapter 边界已冻结，后续实现需保持该职责划分。
- [plan/cfg-test-production-wiring-plan-2026-05-21.md](plan/cfg-test-production-wiring-plan-2026-05-21.md): Phase 1-16 已完成；保留为实现记录，后续可迁移到 archive，不再作为活跃 TODO。
- [superpowers/plans/2026-04-11-team-memory-sync.md](superpowers/plans/2026-04-11-team-memory-sync.md): Team Memory 客户端同步仍需 e2e 与文档收口。
- [superpowers/specs/2026-04-11-team-memory-sync-design.md](superpowers/specs/2026-04-11-team-memory-sync-design.md): Team Memory 验证清单仍有效。
- [superpowers/plans/2026-04-12-tools-commands-test-coverage.md](superpowers/plans/2026-04-12-tools-commands-test-coverage.md): 测试覆盖补齐计划仍有效。
- [superpowers/plans/2026-04-09-pty-commands-and-multi-turn.md](superpowers/plans/2026-04-09-pty-commands-and-multi-turn.md): PTY 交互测试扩展计划仍有效。
- [superpowers/specs/2026-04-20-workspace-split-design.md](superpowers/specs/2026-04-20-workspace-split-design.md): workspace split 后续 phase 仍开放。

## 3. 已知设计限制与 runtime caveats

| 范围 | 状态 | 说明 |
| --- | --- | --- |
| UI resize 回流 | 部分收口 | Rust TUI 已有 width-aware virtual scroll 回归；TS/OpenTUI fullscreen/maximize 白行问题仍在 [KNOWN_ISSUES.md](KNOWN_ISSUES.md) 跟踪。 |
| Rust TUI shell output | 已接 runtime context | renderer 支持 expanded/collapsed/detail view；最新 Bash/PowerShell tool result 自动展开，历史长输出默认折叠，选中后可展开 detail。 |
| Rust TUI Ctrl+R history | 已接 workspace 持久历史 | Ctrl+R 按当前 workspace 读取跨会话 prompt history，条目带 session/title/cwd 来源和时间；无数据时显示明确空态。 |
| Rust TUI command surfaces | 已接 production runtime | cfg-test gated surface 已关闭：`/agents`、`/config`、`/diff`、`/hooks`、`/login`、`/mcp`、`/memory`、`/permissions`、`/remote`、`/sandbox`、`/skills`、`/tasks`、`/team` 等无参数入口在生产构建打开对应 `CommandSurface`；剩余风险是 live backend 数据和 release e2e 证据，不是空占位。 |
| Browser MCP real-server path | fake/e2e 已有，真实 server 手动证据待补 | `e2e_browser_mcp`、`e2e_chrome_native_host`、`e2e_chrome_mcp_bridge` 覆盖 fake/unreachable/native-host 路径；真实第三方 Browser MCP server 截图/console/network 仍是 release 手动证据，不作为默认已验证声明。 |
| Anthropic-compatible provider smoke | Mock matrix complete, real smoke partially verified | `scripts/provider_smoke_matrix.py mock` 覆盖 direct API key、direct bearer、compatible bearer + custom base URL、Bedrock/Vertex model mapping、prompt-cache strip/enabled knobs；2026-05-21 `real` 模式通过 direct bearer、compatible custom base URL bearer、prompt-cache default 与 TTL/global gates，并因缺少 `ANTHROPIC_API_KEY`、AWS、Vertex 凭据跳过对应真实 provider。脚本会 redact secrets/auth headers。 |

## 4. 历史 Deferred 重评队列

历史 `rust-lite` deferred 不再自动等于“不实现”。触及时按以下类别处理：

- 远程控制与多端集成：`/remote-control`、`/desktop`、`/mobile`、`bridge/`、`remote/`。
  - 当前 remote-control 计划已明确拆分为 gateway 控制面、daemon 执行宿主、ipc 本地 headless bridge、`/remote`/TUI 本地操作面，以及 Telegram/Lark adapter 连通性；不得把现有 `/api/*` 直接当作公网 remote-control API。
- 服务端/传输扩展：`server/`、SSE/WebSocket/Worker transport、MCP server mode。
- 远程/运营能力：`Monitor`、`PushNotification`、`SubscribePR`、`Workflow`、遥测与 MDM 同步。
- Ant-only 命令与内部工具：`/agents-platform`、`/ant-trace`、`CtxInspect`、`OverflowTest`、`Tungsten` 等。

重评规则：

1. 默认按上游完整行为补齐。
2. 若决定延期，必须有明确理由与复审条件。
3. 若决定不跟随上游，必须登记到 §6 "Intentional 裁剪"。
4. 不允许仅以 "lite 版本不做" 作为理由。

## 5. 代码层技术债入口

`TECH_DEBT.md` 中仍然活跃的债务来源包括：

- `#![allow(unused)]` 清理未完成。
- `test_ctx()` 等测试样板未完全收束到共享 helper。
- 模型别名映射未完全合并到单一查找表。
- 工具输入解析风格不统一。
- IPC subsystem 仍需更多 per-subsystem serialization contract tests；envelope version/min-compat 策略已接入 `allthecodes-ipc-protocol`。

## 6. Intentional 裁剪

本节登记全量构建阶段确认不跟随上游的裁剪项。格式：

`- <模块/功能>: <裁剪理由> | <决策者> | <日期> | <复审触发条件>`

- Agent Teams tmux/iTerm2 pane backend: allthecodes 当前主运行环境包含 Windows，外部 pane backend 会引入 tmux/iTerm2/窗口管理器耦合、跨平台清理语义和额外交互面；MVP-005 决定不实现外部 pane backend，而是把 in-process backend 做成唯一受支持路径并补齐生命周期控制。`PaneBackend` trait 保留为未来上游 parity 审查边界。 | Codex | 2026-04-28 | 用户明确需要可见终端 pane、上游 pane protocol 成为产品必需项，或 roadmap 切换到 Unix terminal-pane 优先发布。
- BashTool Windows Restricted Token / Job Object OS-level primitive: 上游 `@anthropic-ai/sandbox-runtime` 当前只对 macOS、Linux 与 WSL2 暴露 sandbox 支持，PowerShell permission UI 明确没有 sandbox toggle；allthecodes 不自研 Windows token/job sandbox，保留 Rust-level FS/network preflight、`/sandbox require` fail-closed 与 unavailable 诊断。 | Codex | 2026-05-05 | 上游发布 Windows sandbox-runtime backend、PowerShell sandbox toggle 成为产品必需项，或安全策略要求 Windows OS-level enforcement。
- WebFetch browser-grade JS rendering: 本次发布只承诺 HTTP fetch 能力，包括 redirect/MIME/proxy/`NO_PROXY`、credential URL 拒绝和无 cookie store 边界；不内置浏览器运行时，不执行页面 JavaScript，避免把 cookie/session/DOM 执行面引入普通 WebFetch。需要 JS 渲染的工作流应走外置 Browser MCP 或后续 native browser host 方案。 | Codex | 2026-05-17 | 外置 Browser MCP 升级为默认发布支持面、用户明确要求 JS-rendered page fetch，或安全模型允许受控浏览器 profile/session 隔离。
- Voice dictation audio/STT backend: allthecodes 当前只保留 `/voice`、`voiceEnabled`、keybinding 和 language normalization 兼容面；不声明真实 microphone capture、waveform UI、streaming transcription 或 Claude.ai voice STT。 | Codex | 2026-05-17 | 项目新增受支持 audio backend、STT client、local/SSH/WSL/auth 矩阵测试，并决定把 voice 纳入发布支持面。
- KAIROS Bridge/GrowthBook upstream surface: 本轮以 allthecodes local gateway/protocol 收口 bridge session、channel status、run timeline/output 和 daemon worker state；不直接复制上游公网 Bridge/GrowthBook 控制面，也不把 daemon `/api/*` 暴露为公网 remote-control API。 | Codex | 2026-07-04 | 产品定义公网 remote-control/experimentation contract、安全审计策略和 admin UX，并要求与上游 Bridge/GrowthBook 行为完全一致。
- Telegram/Lark inbound channel sessions and remote triggers: 本轮只承诺 gateway-backed outbound adapter status/control、connect health check 和 allowlisted test-message；不承诺入站 Telegram/Lark conversation、schedule remote triggers 或完整远程会话 parity。 | Codex | 2026-05-17 | 产品定义入站 channel session contract、gateway runner 注入语义和安全/审计策略，并补 e2e。
- Additional Hermes-style platform adapters (Slack/Discord/Matrix/etc.): Hermes runtime slice only adds shared source/session/task abstractions, scheduled tasks, and existing gateway-backed adapter status/control. New platform adapters are not implemented without a product/runtime contract for inbound identity, authorization, audit, delivery retry, and session ownership. | Codex | 2026-07-04 | 产品定义对应 platform adapter contract，并要求纳入 Hermes-like runtime release scope。

新增规则：

1. 任何进入本节的条目必须在 PR 中说明理由，并列出未来复审触发条件。
2. 每季度至少复审一次本节。
3. 过期未复审的条目回落到 TODO 队列。

## Ratatui UI parity follow-up after OMX closeout (2026-05-10)

The OMX parity pass narrowed several P1 UI gaps but did not eliminate all upstream parity work. Remaining tracked gaps:

- Remote/teleport: local `/remote`, `/channels`, Chrome, IDE, and LSP surfaces now expose real local status where available; inbound channel sessions and teleport remain deferred until product/runtime contracts exist.
- Command-surface production wiring: 2026-05-21 cfg-test wiring pass completed `/agents` create/edit, `/mcp` detail/tools, `/permissions`, `/tasks`, `/team`, dialog helpers, tabs helpers, command palette/runtime snapshots, and production no-arg routing. This is no longer a tracked gap.
- Persistent history: Ctrl+R uses workspace persistent history where backend data is available; cross-session history quality still depends on durable reader coverage and should remain under UI/runtime residual tracking.
- Full-suite verification: the 2026-05-14 crate-migration pass made the default workspace test gate green. Live PTY/API tests remain `#[ignore]` and must be run explicitly with real credentials/network when validating live model behavior.
