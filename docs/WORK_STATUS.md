# allthecodes 工作状态总览

> 更新日期: 2026-07-13 | 分支历史名: `rust-lite` | 当前阶段: 全量构建 / Full Build

本文件只保留当前阶段仍需要判断和执行的状态。已经确认实现、已关闭或只具历史价值的阶段记录统一看：

- [development/archive/COMPLETED_FULL.md](../development/archive/COMPLETED_FULL.md)
- [development/archive/completed-gap-closures-2026-05-07.md](../development/archive/completed-gap-closures-2026-05-07.md)
- [development/archive/issues/](../development/archive/issues/)

缩减实现、未完备项和 intentional crop 统一看 [IMPLEMENTATION_GAPS.md](../development/archive/IMPLEMENTATION_GAPS.md)。开放问题与代码审查发现统一看 [KNOWN_ISSUES.md](../development/archive/KNOWN_ISSUES.md)。最终发布顺序、发布门禁和预期效果看 [FINAL_RELEASE_PLAN.md](../development/archive/FINAL_RELEASE_PLAN.md)。

## 当前结论

allthecodes 已不再按历史 "Lite" 边界维护。触及上游能力时，默认按 `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-bun/` 的完整行为对齐；确需保留裁剪时，必须写入 [IMPLEMENTATION_GAPS.md](../development/archive/IMPLEMENTATION_GAPS.md) 的 "Intentional 裁剪"。

### Agentic workflow injection defense（2026-07-13）

`development/ai_check/2026-07-11-agentic-workflow-injection-defense-plan.zh.md` 的 task4–6 已实现第一版闭环：

- setup-chain / supply-chain 扫描器只做有界静态读取，覆盖 DNS TXT 解码、curl/wget 管道执行、postinstall 下载、Docker 未知二进制下载、凭据路径、动态下载、未知域名、Action 浮动引用和宽泛 workflow 权限。
- scanner Deny 在 canonical tool security gate 中 fail-closed；scanner Ask 与 tainted FileWrite/Shell/Download/PackageInstall 等请求进入一次性、精确到工具名与 sanitized input 的用户审批，`Bypass`/`Auto` 不会绕过 Deny。
- `SecurityDecisionRecord`、`EventKind::SecurityDecision`、IPC permission metadata 和 TUI permission rendering 只传播规则 ID、source category 和 digest，不保存远程正文、secret 或原始命令。
- deterministic fixtures 已覆盖恶意 setup、postinstall、Docker 下载、floating/pinned Action、普通 cargo build/test、只读 Grep 与精确审批输入变化。

本轮 focused tests 与 `cargo test -p allthecodes --test agentic_workflow_injection_e2e -- --nocapture` 已通过；`cargo check --workspace` 也通过。workspace 级 clippy/release gate 当前被共享工作区中 `allthecodes-mcp/src/runtime.rs` 测试引用缺失的 `take_installed_manager` 阻塞；定向 engine clippy 已通过。

### Runtime verification evidence / Session Report（2026-07-13）

`development/runtime/2026-07-11-runtime-verification-evidence-plan.md` 的 Task 1–6 已完成第一版实现闭环：

- record/replay 现在保存版本化的 verification、artifact 和 `SessionReportGenerated` 事实；工具结果只在已知成功 build/test/lint/security 命令且 exit code 为 0 时计为正向证据，并保存命令 digest 而不是原文。
- `targeted_tests`、`build_and_test`、`release_gate` 通过正常工具/权限路径执行 bounded verify-continue，最多三轮；缺少证据最终为 `Incomplete`，不会伪造通过。
- transaction flush 后生成原子写入、redacted、带 canonical record head digest 的 `SessionReportV1`，并通过 Web `GET /api/sessions/{id}/report`、IPC/TUI summary 和 metadata-only telemetry 暴露。
- 已验证：session report 2 项、engine verification 7 项、verification e2e 5 项、Web not-generated handler 1 项、telemetry feature test 1 项；`cargo check -p allthecodes`、`cargo fmt --all --check` 和定向 engine clippy 均通过。

这不是完整 traceable logging 体系的关闭声明。统一的 durable runtime audit event log、全链路 `submit_id/turn_id/request_id/event_id` 传播、完整 daemon/IPC/permission/stream/progress 事件覆盖，以及让 audit export 以该事实源为主仍是残余工作；详见 [traceable-logging-plan.md](../development/archive/plan/traceable-logging-plan.md)。workspace clippy 仍受共享工作区缺失的 `allthecodes-mcp::take_installed_manager` 测试辅助函数阻塞。

当前已确认完成并归档的主线包括：

- API 基线：Anthropic、OpenAI compatible、Google Gemini、Azure、Bedrock、Vertex 均有运行时支持；真实 provider/e2e 覆盖仍是后续质量门。
- Anthropic-compatible coding API：direct API key、direct bearer、custom base URL bearer、Bedrock/Vertex model mapping、SOTA/MOTA/FOTA defaults、prompt-cache TTL/global gates 已有 mock smoke matrix；2026-05-21 真实 smoke 已通过 direct bearer、compatible custom base URL bearer、prompt-cache default 与 TTL/global gates，direct API key、Bedrock、Vertex 因缺少对应凭据跳过。
- 认证：API key、系统 Keychain、OAuth PKCE、token 持久化与刷新已落地。
- 工具基线：Bash、PowerShell、Read、Write、Edit、Grep、Glob、Agent、Skill、LSP、Tasks、Web、Brief、Sleep 等主路径已落地。
- Agent Teams：in-process backend、`/team`、`TeamSpawn`、`SendMessage`、Team Dashboard 已收口；tmux/iTerm2 pane backend 是 intentional crop。
- Extensibility：hooks、skills、custom-agent active runtime safety、MCP stdio/local SSE/remote SSE/Streamable HTTP/OAuth/reconnect/tool refresh 已按当前标准面闭环。
- MCP scope isolation：`mcpBindings` 已支持 `global/project/session/thread` 四级 binding；旧 `mcpServers` 继续生成兼容隐式 binding；engine、agent、skill fork、CLI/IPC/Web/TUI 展示均按 binding context 过滤 MCP tools/resources/calls。TUI 当前可编辑 global/project/session binding，thread binding 由 CLI/IPC/Web 编辑。
- ACP v2 adapter：`--acp` JSON-RPC stdio server 已完成 review-fix baseline。已验证 `initialize`、auth、session new/load/resume/list/close/delete/prompt/cancel/set_config_option、permission request bridge、prompt/update mapping、stdout purity smoke；`session.prompt.image/audio/embeddedContext` 与 `session.mcp.*` 仍为 intentional unadvertised scope。
- KAIROS resident assistant / daemon parity：system prompt resident-assistant sections、assistant/bridge/proactive/scheduler worker ownership、automation state DTO、push notification/channel ingress、dream memory distillation、scheduler supervision、HTTP/SSE history replay、daemon CLI submit/sleep/stop E2E、bridge session workspace reuse/lease/resume 已落地；live provider smoke 仍按需运行。
- FEATURE_PROACTIVE parity: standalone feature gate, `/proactive`, local TUI tick driver, daemon proactive worker, Sleep early-wake, automation status, and focused no-provider test coverage are implemented. Live provider soak remains an operator-run validation because it requires credentials.
- Ratatui UI：P0/P1 基础面已完成；运行时 residual 见 [KNOWN_ISSUES.md](../development/archive/KNOWN_ISSUES.md)，未跟踪 parity 缺口见 [ratatui-ui-parity-untracked-gap-plan-2026-05-08.md](../development/archive/plan/ratatui-ui-parity-untracked-gap-plan-2026-05-08.md)。
- Runtime storage：`ALLTHECODES_HOME` / `~/.allthecodes/` 和项目级 `.allthecodes/` 路径隔离已落地，旧计划归档。
- Crate migration：root binary 已删除旧 `src/engine/**` 与 `src/ipc/**`；engine/agent 实现由 `allthecodes-engine` 拥有，IPC JSONL runtime、agent settings 与共享 protocol/handler facade 由 `allthecodes-ipc` / `allthecodes-ipc-client` / `allthecodes-ipc-protocol` 拥有，root 仅保留 startup、UI 与 runtime adapter glue。
- Codebase optimization Phase 4：Rust TUI 状态解耦已落地；`App` facade 下沉到 domain stores、overlay dispatcher、message view-model 与 runtime view state，最终验证见 [codebase-optimization-plan-2026-07-03.md](../development/code-split/codebase-optimization-plan-2026-07-03.md)。
- Hermes-like runtime 基础层：以 `QueryEngine` 为唯一 agent loop，外围补齐 `session_search` warm memory、`/session search`、Web search API、模型可见 `SessionSearch`、审批式 memory/skill proposal、background review proposal queue、scheduled task registry/daemon dispatch，以及 `DelegateTask` 可追踪 delegation envelope；实施记录见 [hermes-runtime.md](../development/archive/implemented/hermes-runtime.md)。

## 活跃待办

| 范围 | 当前状态 | 下一步 |
| --- | --- | --- |
| API providers | 基线完成，真实凭据质量门部分收束 | provider validation DTO、Azure/Foundry 命名诊断、Anthropic-compatible coding 契约和 mock smoke matrix 已补；2026-05-21 `scripts/provider_smoke_matrix.py real` 已通过可用凭据覆盖的 bearer/custom-base/prompt-cache 场景，下一步补 direct API key、Bedrock、Vertex/Azure 等缺凭据真实证据。 |
| Team Memory 客户端同步 | 代码路径已接通，验证与文档收口未完 | 补同步、断线恢复、冲突处理 e2e；通过后归档旧 Team Memory plan/spec。 |
| TaskTools | 多数基础已完成，remote/multi-type poller parity 仍开放 | 对齐远程/多类型后台任务 poller/reconnect runtime。 |
| Hermes runtime follow-ups | 基础层完成，执行桥接仍有残留 | `session_search`、审批队列、learnable skills、background review、scheduled task registry 和 `DelegateTask` envelope 已落地；scheduled task 的 `cwd` 目前作为 source/system prompt metadata 保留，尚未覆盖 `QueryEngine` 实际执行 cwd；`DelegateTask` 已创建 child session/task/worktree metadata 并可由父 session 工具追踪，但尚未直接启动 caller-provided child `QueryEngine`。 |
| PlanMode | 保守 classifier、持久化、审批和 plan file 白名单已完成 | 补 full auto-mode LLM classifier parity，并覆盖 plan 创建/恢复/审批/e2e。 |
| WebFetch | HTTP-only release scope | redirect/MIME/proxy/credential 边界已完成；browser-grade JS rendering 已写入 [IMPLEMENTATION_GAPS.md](../development/archive/IMPLEMENTATION_GAPS.md) §6 intentional crop。 |
| Daemon / KAIROS | resident assistant parity 主线已落地；bridge session 可按 workspace 默认复用并持久化 assistant session id | 默认验证覆盖 CLI stopped/start/status/submit/sleep/stop、worker IDs、automation state、history DTO、graceful shutdown、bridge session storage/selector/worker/API/slash command。可选 live smoke 需要真实 provider 凭据和网络；GrowthBook 公网行为、Telegram/Lark 入站会话仍按 intentional/deferred scope 处理。 |
| Session export | schema v2 与 API request snapshots 已接入，projection residual 开放 | 补 context collapse 原生事件、mode/tag 来源和完整 apiView 投影。 |
| Runtime verification evidence / Session Report | 第一版闭环已落地，完整 traceable logging 仍开放 | canonical verification/artifact/report records、三轮 bounded verify-continue、redacted/tamper-linked `SessionReportV1`、Web/IPC/TUI/telemetry surfaces 和 deterministic e2e 已通过；下一步补统一 durable audit event source 与全链路 correlation/event coverage。 |
| Crate migration | Engine + IPC owner migration landed; verification in progress | IPC envelope version/min-compat 已补；下一步收束剩余 root-style imports、allow attributes、Codex compatibility path hits，并补齐 thin-binary closeout 文档。 |
| ACP live-provider smoke | Binary/stdout smoke target landed; deterministic ACP runtime tests pass; live real-model smoke is on-demand | `acp_stdio_real_model_prompt_smoke` is ignored by default because it requires configured credentials, provider access, and network. On 2026-07-03, explicit local runs against current `backend=codex` timed out after 300s after `available_commands_update` + `state_update: running`, with no model content or idle. |
| UI/runtime issues | P0/P1 基础完成；Phase 4 TUI state decoupling 已落地；仍有 residuals 和未跟踪 parity 缺口 | 运行时 residual 见 [KNOWN_ISSUES.md](../development/archive/KNOWN_ISSUES.md)；`⚠️ 部分` / `❌ 缺失` 的未跟踪功能按 [ratatui-ui-parity-untracked-gap-plan-2026-05-08.md](../development/archive/plan/ratatui-ui-parity-untracked-gap-plan-2026-05-08.md) 分阶段处理。 |
| 文档状态一致性 | 本轮已收敛顶层入口 | 后续每完成一个模块，都同步迁移完成记录到 archive，避免活跃 TODO 文档堆积完成历史。 |

## 活跃文档入口

- [IMPLEMENTATION_GAPS.md](../development/archive/IMPLEMENTATION_GAPS.md): 未完备项、全量构建 TODO、intentional crop。
- [FINAL_RELEASE_PLAN.md](../development/archive/FINAL_RELEASE_PLAN.md): 最终发布顺序、门禁、未实现/不完美项和预期效果。
- [KNOWN_ISSUES.md](../development/archive/KNOWN_ISSUES.md): 当前开放问题、代码审查发现、文档状态问题。
- [COMMAND_REFERENCE.md](../development/archive/COMMAND_REFERENCE.md), [CLI_REFERENCE.md](../development/archive/CLI_REFERENCE.md), [USAGE_GUIDE.md](../development/archive/USAGE_GUIDE.md): 用户命令与使用说明。
- [DAEMON_OPERATIONS.md](../development/reference/DAEMON_OPERATIONS.md), [daemon-usability-plan.md](../development/archive/plan/daemon-usability-plan.md): daemon 当前操作面与后续计划。
- [RATATUI_UI_PARITY.md](../development/archive/RATATUI_UI_PARITY.md), [ratatui-ui-parity-untracked-gap-plan-2026-05-08.md](../development/archive/plan/ratatui-ui-parity-untracked-gap-plan-2026-05-08.md): Rust TUI 对标与后续 UI parity。
- [STORAGE.md](STORAGE.md): allthecodes 路径隔离与数据目录规则。
- [traceable-logging-plan.md](../development/archive/plan/traceable-logging-plan.md): 可追溯日志体系 draft；runtime verification/session report 已完成第一版证据闭环，但不等于整套 durable audit 体系完成。

## 历史 Deferred

历史 deferred 不再等于 "不做"。远程控制、多端集成、服务端扩展、遥测/MDM、Ant-only 命令和内部工具都需要在触及时重新评估：

- 要实现：补到对应 plan / implementation task。
- 要延期：保留在 [IMPLEMENTATION_GAPS.md](../development/archive/IMPLEMENTATION_GAPS.md) TODO 区。
- 要裁剪：写入 [IMPLEMENTATION_GAPS.md](../development/archive/IMPLEMENTATION_GAPS.md) "Intentional 裁剪"，说明理由、决策者、日期和复审触发条件。

## ACP v2 adapter status (2026-07-03)

Completed scope:

- JSON-RPC stdio runtime for `allthecodes --acp`, with stdout restricted to JSON-RPC frames and diagnostics on stderr.
- ACP v2 method baseline: `initialize`, `auth/login`, `auth/logout`, `session/new`, `session/load`, `session/resume`, `session/list`, `session/close`, `session/delete`, `session/prompt`, `session/cancel`, `session/set_config_option`, and `$/cancel_request`.
- Permission bridge from engine callbacks to ACP `session/request_permission` client requests, including allow/always-allow/deny/cancel/disconnect outcomes.
- Session replay/list/delete parity for persisted sessions, including workspace filtering, cursor/meta mapping, archive behavior, and active-session close before delete.
- Prompt update mapping for state, usage, text chunks, thinking, tool calls, progress, plan updates, retries, summaries, and tombstones.
- Config options for `model`, `mode`, and `thought_level`, with validation, update notifications, and per-turn submit overrides.

Unimplemented intentional scope:

- `session.prompt.image`
- `session.prompt.audio`
- `session.prompt.embeddedContext`
- `session.mcp.*`

Verification run in `.worktrees/acp-adapter`:

```bash
cargo test -p allthecodes-acp
cargo test -p allthecodes-acp --test prompt_updates prompt_acks_before_first_update
cargo test -p allthecodes-acp --test protocol_methods cancel_request_cancels_pending_prompt_before_ack
cargo test -p allthecodes-acp --test config_options
cargo test -p allthecodes-acp --test protocol_methods auth
cargo test -p allthecodes-acp --test protocol_methods capability
cargo test -p allthecodes-acp --test session_lifecycle delete
cargo test -p allthecodes-acp --test prompt_updates cancel_sends_idle_cancelled
cargo test -p allthecodes --test acp_stdio_smoke
cargo check -p allthecodes-acp
cargo check --workspace
cargo build --workspace --release
```

The on-demand ignored live-provider smoke was also exercised with the normal config/auth/model path and failed explicitly in this environment because the current Codex backend did not emit model content or final idle within 300 seconds.

## Ratatui UI parity OMX final verification (2026-05-10)

The ratatui UI parity OMX batch series has been closed out at documentation level. Available batch summaries/last messages through the final task were reviewed, intentional UI snapshot updates were accepted, and the feasible targeted UI verification set was run. The later crate-migration verification pass on 2026-05-14 made the default workspace test gate green.
