# Remote Control Gateway OMX/Subagent 执行计划

日期：2026-05-08

源计划：`development/archive/plan/remote-control-gateway-execution-plan-2026-05-08.md`

参考脚本：

- `scripts/codex-task-sequence.md`
- `docs/scripts/README.md`
- `scripts/codex-task-sequence.ps1`

配套执行文件：

- `scripts/achieve/run-remote-control-gateway-omx-sessions.ps1`
- `docs/scripts/achieve/remote-control-gateway-omx-tasks-2026-05-08.txt`

## 1. 目标

用 OMX + Codex fresh sessions + 按需 subagent 的方式实现 remote-control gateway。执行过程固定使用：

```text
model: gpt-5.5
reasoning_effort: medium
sandbox: danger-full-access
```

“减少思考过程”不通过降低模型或跳过验证实现，而通过以下手段实现：

- 每个 session 只执行一个明确任务。
- 每个任务声明 owned files、forbidden files、验证命令和提交要求。
- 输出只写行动、证据、变更、风险，不写长篇推理。
- 中途 review gate 单独成 session。
- 高风险阶段一次只跑一个 session；低风险阶段可按需批量跑 2-3 个。

## 2. 已执行的规划输入

- 已用 `gpt-5.5` / `medium` default subagent 做脚本规范分析。
- 已用 `gpt-5.5` / `medium` default subagent 做 remote-control 分片风险分析。
- 已运行 `omx --version`，本机 OMX 版本为 `oh-my-codex v0.15.3`。
- 已尝试 `omx explore`，Windows 下当前 harness 不可用，OMX 提示原因是 POSIX shell wrapper 依赖；本计划要求后续 read-only shell 查询优先用 `omx sparkshell`，失败时退回 PowerShell + `rg`，不要把它当作仓库问题。

## 3. 执行契约

### 3.1 模型和输出

所有执行脚本必须传：

```powershell
-Model "gpt-5.5"
-ReasoningEffort "medium"
```

每个 Codex session 的 prompt 必须包含：

```text
固定模型 gpt-5.5，reasoning medium。减少显式思考过程；只报告行动、证据、变更、验证和风险。
```

### 3.2 OMX / subagent 使用规则

- `omx --version` 是脚本 preflight。
- 简单只读查询优先 `omx sparkshell <command> <args...>`；如果 sidecar 回退到原生命令，继续执行并记录。
- `omx explore` 在当前 Windows 环境不作为硬依赖。
- 每个 implementation session 可以按需启动最多 2 个 default subagent，且必须显式要求 `gpt-5.5` / `medium`。
- subagent 只做独立、只读、可验证分析，或在明确 disjoint write scope 时做小范围实现。
- 不允许多个 subagent 写同一文件族。
- review session 优先用 subagent 做只读复核，不直接修改；若发现阻塞问题，再由当前 session 做最小修正并提交。

### 3.3 Git 提交规则

- 每个会修改文件的 session 必须在验证通过后提交一次。
- read-only session 不提交。
- review session 若只读无改动，不提交；若修复 review-blocking issue，单独提交。
- 每次提交只 stage 当前 session owned files，不能 stage 用户已有的无关 dirty worktree。
- 提交消息遵守 AGENTS.md Lore Commit Protocol，至少包含：
  - intent line
  - Constraint
  - Confidence
  - Scope-risk
  - Tested
  - Not-tested

### 3.4 单文件代码量和重构检测

硬门槛：

- `crates/claude-code-rs/src/daemon/routes.rs` 不继续承载 remote-control 主逻辑；remote route 超过约 150 行必须拆到 `daemon/gateway_routes.rs` 或 `crates/gateway/src/api.rs`。
- 单个 handler 超过 100 行必须拆 validation / auth / response mapping。
- `remote_cmd.rs` 超过 450 行必须拆 `remote_cmd/output.rs`、`remote_cmd/client.rs` 或等价子模块。
- `crates/gateway/src/runner.rs` 只做协调；如果同时包含 auth/store/policy/delivery/event mapping，必须拆。
- `command_surface/mod.rs` 只做 enum/title/render/handle_key/constructor 接线；具体 UI rows 放 `adapters/remote.rs`，交互放 `surfaces/remote.rs`。
- `status_widget.rs` 不允许加入阻塞网络 IO。

每个 code session 结束前运行一次轻量检测：

```powershell
Get-ChildItem crates\gateway\src,crates\claude-code-rs\src\daemon,crates\claude-code-rs\src\commands,crates\claude-code-rs\src\ui\components -Recurse -Include *.rs |
  ForEach-Object { [pscustomobject]@{ File = $_.FullName; Lines = (Get-Content $_.FullName).Count } } |
  Where-Object { $_.Lines -gt 450 } |
  Sort-Object Lines -Descending
```

若触发阈值，当前 session 必须在 final note 说明是否已拆分；如果未拆分，必须留下 blocker。

### 3.5 错误诊断原则

减少冗余安全设计，不堆多套互相重叠的 auth/error wrapper。统一使用清晰诊断：

```text
code: stable machine-readable code
message: 人能看懂的错误
action: 下一步排查或修复建议
context: redacted provider/run/source/path 摘要
```

必须明显诊断的错误：

- daemon stopped / stale / missing `FEATURE_KAIROS`
- missing / invalid daemon control token
- non-loopback bind without remote token
- bad remote token / bad origin / CORS blocked
- payload too large: `413 payload_too_large`
- busy reject: `409 busy`
- queue full: `429 queue_full`
- unsupported steer: `501 unsupported`
- stale approval / stale ask-user: `409 stale_response`
- run not found / corrupt event log / replay unavailable
- duplicate idempotency returns existing run
- webhook disabled because secret missing
- bad HMAC / missing signature
- event ignored by filter
- adapter blocked: Telegram token missing/invalid, Lark credential missing, target not allowlisted
- delivery unsupported / callback blocked / callback SSRF rejected / retry exhausted

## 4. 按需任务数量策略

执行脚本支持：

- `-StartAt <n>`：从指定 session id 开始。
- `-EndAt <n>`：到指定 session id 结束。
- `-OnlySession <id-or-name>`：只跑一个 session。
- `-MaxSessions <n>`：本次最多跑几个 session。
- `-DryRun`：只打印将要执行的 session，不调用 Codex。

建议：

- Phase 0 / docs / final verification：一次可跑 2-3 个 session。
- Phase 1 / Phase 2：一次最多 2 个 session。
- Phase 3 / Phase 5 / Phase 8：一次只跑 1 个 session，然后 review。
- review gate：单独跑，不与实现 session 混跑。

## 5. Session 切分

### Session 00 - Baseline And Partition

类型：只读

目标：确认当前 dirty worktree、源计划、OMX 可用性和 remote-control 代码基线。

Owned files：无

要求：

- 读取源计划和本计划。
- 用 `git status --short` 列出 dirty files。
- 用 `rg` / `omx sparkshell` 定位 `/remote`、daemon route、protocol、command surface、status widget。
- 输出建议本次最多运行几个后续 session。

验证：

```powershell
omx --version
git status --short
rg -n "remote|gateway|CommandSurface|status_widget" crates docs
```

提交：无。

### Session 01 - Phase 0 Boundary Docs

类型：文档

目标：冻结 `gateway` / daemon / ipc / `/remote` / Telegram/Lark adapter 边界。

Owned files：

- `development/reference/remote-control-current-state.md`
- `development/archive/IMPLEMENTATION_GAPS.md`
- `development/archive/plan/remote-control-gateway-execution-plan-2026-05-08.md`

要求：

- 不写代码。
- 明确 `/api/*` 不是公网 remote-control API。
- 明确 Telegram/Lark 第一版只做连接、健康检查、状态诊断、测试发送。

验证：

```powershell
rg -n "remote-control|Telegram|Lark|ipc|/remote|gateway" development/reference development/archive/IMPLEMENTATION_GAPS.md development/archive/plan
git diff --check
```

提交：提交文档边界更新。

### Session 02 - Phase 1A Gateway Crate Foundation

类型：实现

目标：新增 `crates/gateway` crate、路径函数、`RemoteSource`、session key 和 redaction 基础。

Owned files：

- `crates/gateway/Cargo.toml`
- `crates/gateway/src/lib.rs`
- `crates/gateway/src/source.rs`
- `crates/gateway/src/session_key.rs`
- `crates/gateway/src/config.rs`
- `crates/cc-config/src/paths.rs`

Forbidden files：

- `crates/claude-code-rs/src/daemon/routes.rs`
- `crates/claude-code-rs/src/ui/**`
- `crates/claude-code-rs/src/ipc/**`

验证：

```powershell
cargo test -p cc-config gateway --lib
cargo test -p gateway source session_key config
cargo check -p gateway --message-format short
```

提交：提交 gateway foundation。

### Session 03 - Phase 1B Run Store And Policy

类型：实现

目标：实现 run schema、events、store、idempotency、busy policy skeleton。

Owned files：

- `crates/gateway/src/run.rs`
- `crates/gateway/src/events.rs`
- `crates/gateway/src/store.rs`
- `crates/gateway/src/policy.rs`
- `crates/gateway/tests/store.rs`

要求：

- store 写 `~/.cc-rust/gateway/**`。
- `events.ndjson` append failure 必须返回明显错误。
- 不引入 daemon/UI 依赖。

验证：

```powershell
cargo test -p gateway run events store policy
cargo check -p gateway --message-format short
```

提交：提交 run/store/policy。

### Review A - Schema Store Review

类型：review gate

目标：只读 review Session 02-03。

检查：

- 状态机是否足够表达 queued/running/waiting/completed/failed/cancelled/recoverable。
- redaction 是否覆盖 token/secret/authorization。
- idempotency 是否不会重复危险 command。
- 路径是否只落在 `~/.cc-rust`。
- 单文件大小是否触发拆分。

验证：

```powershell
cargo test -p gateway
git diff --stat HEAD~2..HEAD
```

提交：只读无提交；如修复 blocker，单独提交。

### Session 04 - Phase 2A Adapter Registry And Telegram

类型：实现

目标：实现 adapter trait、registry、Telegram connect/health/test-message skeleton。

Owned files：

- `crates/gateway/src/adapters/mod.rs`
- `crates/gateway/src/adapters/telegram.rs`
- `crates/gateway/src/config.rs`
- `crates/gateway/tests/adapters.rs`

要求：

- 不做 inbound conversation。
- 不触发模型。
- token 永不进入日志、status、错误原文。

验证：

```powershell
cargo test -p gateway adapters telegram
cargo check -p gateway --message-format short
```

提交：提交 adapter registry / Telegram。

### Session 05 - Phase 2B Lark And Adapter Status API Model

类型：实现

目标：实现 Lark outbound/webhook/app credentials 连接诊断和 provider-neutral adapter status。

Owned files：

- `crates/gateway/src/adapters/lark.rs`
- `crates/gateway/src/adapters/mod.rs`
- `crates/gateway/src/config.rs`
- `crates/gateway/tests/adapters.rs`

要求：

- 缺凭据返回 `blocked` reason。
- outbound-only 必须标为 `connected_outbound_only`，不能宣称 inbound ready。

验证：

```powershell
cargo test -p gateway adapters lark
cargo check -p gateway --message-format short
```

提交：提交 Lark adapter。

### Session 06 - Phase 3A Runner And Daemon Protocol Bridge

类型：实现

目标：新增 gateway runner 协调层和 daemon bridge 类型，不迁移全部 worker ownership。

Owned files：

- `crates/gateway/src/runner.rs`
- `crates/gateway/src/lib.rs`
- `crates/claude-code-rs/src/daemon/gateway_bridge.rs`
- `crates/claude-code-rs/src/daemon/mod.rs`
- `crates/claude-code-rs/src/daemon/protocol.rs`

要求：

- `gateway` 不依赖 `claude-code-rs`。
- daemon protocol payload 扩展保持旧 command 兼容。
- 错误必须可诊断，不用 `anyhow!("failed")` 这类泛化错误作为对外输出。

验证：

```powershell
cargo test -p gateway runner
cargo test -p claude-code-rs daemon::protocol
cargo check -p claude-code-rs --message-format short
```

提交：提交 runner bridge skeleton。

### Session 07 - Phase 3B Worker Ownership Migration

类型：实现，高风险

目标：解决 `/api/submit` 直接执行与 worker command deferred 的生命周期漂移。

Owned files：

- `crates/claude-code-rs/src/daemon/routes.rs`
- `crates/claude-code-rs/src/daemon/supervisor.rs`
- `crates/claude-code-rs/src/daemon/protocol.rs`
- `crates/claude-code-rs/src/daemon/gateway_bridge.rs`
- 直接相关 daemon tests

要求：

- 不把 remote-control 主逻辑塞进 `routes.rs`。
- 保持旧 `/api/*` 行为兼容，除非源计划明确要求迁移并有测试。
- 如果迁移无法一次完成，必须留下明确 blocker，不伪装为可恢复。

验证：

```powershell
cargo test -p claude-code-rs daemon::protocol daemon::routes daemon::supervisor
cargo check -p claude-code-rs --message-format short
```

提交：提交 daemon execution ownership 变更。

### Review B - Execution Ownership Review

类型：强制 review gate

目标：验证 Phase 3 是否真实解决 run lifecycle 所有权。

检查：

- `POST /runs` 到 command/event/run store 的链路是否唯一。
- supervisor route 是否仍直接执行模型并造成双写。
- abort / permission / ask-user 是否 run-scoped。
- `routes.rs` 是否超过阈值并需要拆分。

提交：只读无提交；修复 blocker 单独提交。

### Session 08 - Phase 4A HTTP API Capabilities And Runs

类型：实现

目标：实现 gateway HTTP router 的 capabilities、create run、get run。

Owned files：

- `crates/gateway/src/api.rs`
- `crates/gateway/src/auth.rs`
- `crates/claude-code-rs/src/daemon/server.rs`
- `crates/claude-code-rs/src/daemon/gateway_routes.rs` 或等价薄接线
- `crates/gateway/tests/api.rs`

要求：

- route handler 只做解析/认证/调用 runner/响应映射。
- `/api/*` 不受影响。

验证：

```powershell
cargo test -p gateway api auth
cargo test -p claude-code-rs daemon::routes
cargo check -p claude-code-rs --message-format short
```

提交：提交 capabilities/runs API。

### Session 09 - Phase 4B Events Stop Approval AskUser Adapters API

类型：实现

目标：实现 run events replay、stop、approval、ask-user 和 adapters endpoint。

Owned files：

- `crates/gateway/src/api.rs`
- `crates/gateway/src/events.rs`
- `crates/gateway/src/runner.rs`
- `crates/gateway/src/adapters/mod.rs`
- `crates/gateway/tests/api.rs`

验证：

```powershell
cargo test -p gateway api events runner adapters
cargo test -p claude-code-rs daemon::sse
cargo check -p claude-code-rs --message-format short
```

提交：提交 remaining gateway API。

### Review C - API Contract Review

类型：review gate

目标：检查 external API、稳定错误码、旧 API 兼容和 event replay。

提交：只读无提交；修复 blocker 单独提交。

### Session 10 - Phase 4.5A Local Gateway Client And `/remote`

类型：实现

目标：新增 local gateway client 和 `/remote` slash command。

Owned files：

- `crates/claude-code-rs/src/daemon/gateway_client.rs`
- `crates/claude-code-rs/src/commands/remote_cmd.rs`
- `crates/claude-code-rs/src/commands/mod.rs`
- 直接相关 command tests

要求：

- `/remote` mutating 操作走 gateway API/local client。
- 不直接写 daemon command 文件。
- 输出必须 redacted。
- `remote_cmd.rs` 超过 450 行必须拆。

验证：

```powershell
cargo test -p claude-code-rs remote_cmd commands::tests::test_all_commands_registered
cargo check -p claude-code-rs --message-format short
```

提交：提交 `/remote` command。

### Session 11 - Phase 4.5B TUI Remote Surface

类型：实现

目标：新增 TUI `RemoteSurface`、command palette metadata、status widget indicator。

Owned files：

- `crates/claude-code-rs/src/ui/components/command_palette/metadata.rs`
- `crates/claude-code-rs/src/ui/components/command_surface/mod.rs`
- `crates/claude-code-rs/src/ui/components/command_surface/adapters/mod.rs`
- `crates/claude-code-rs/src/ui/components/command_surface/adapters/remote.rs`
- `crates/claude-code-rs/src/ui/components/command_surface/surfaces/mod.rs`
- `crates/claude-code-rs/src/ui/components/command_surface/surfaces/remote.rs`
- `crates/claude-code-rs/src/ui/components/status_widget.rs`
- `crates/claude-code-rs/src/ui/app/status.rs`
- related snapshots/tests

要求：

- render path 不做阻塞 IO。
- `RemoteSurface` 空参数打开；`/remote status` 继续走 text command。

验证：

```powershell
cargo test -p claude-code-rs ui::components::command_palette
cargo test -p claude-code-rs ui::components::command_surface
cargo test -p claude-code-rs ui::components::status_widget
cargo test -p claude-code-rs ui::app::tests
cargo check -p claude-code-rs --message-format short
```

提交：提交 TUI remote surface。

### Review D - Local UX And Redaction Review

类型：review gate

目标：检查 `/remote` 和 TUI 是否泄密、是否绕过 gateway、是否阻塞 UI。

提交：只读无提交；修复 blocker 单独提交。

### Session 12 - Phase 5 Security Hardening

类型：实现，高风险

目标：实现 auth/origin/payload/rate-limit/HMAC/idempotency/redaction release gate。

Owned files：

- `crates/gateway/src/auth.rs`
- `crates/gateway/src/webhook.rs`
- `crates/gateway/src/policy.rs`
- `crates/gateway/src/api.rs`
- `crates/gateway/src/store.rs`
- related tests

要求：

- 不堆重复安全层；统一 `GatewayDiagnostic` / `GatewayError` 输出。
- 非 loopback 无 remote token 必须 fail closed。
- redaction 测试覆盖 `/remote`、TUI、logs、run meta。

验证：

```powershell
cargo test -p gateway auth webhook policy api store
cargo test -p claude-code-rs remote_cmd ui::components::command_surface
git diff --check
```

提交：提交 security hardening。

### Review E - Security Gate

类型：强制 review gate

目标：没有本 gate 通过，不允许标记 MVP 可用。

检查：

- bad token / bad origin / payload too large / HMAC / duplicate idempotency / queue full 错误码稳定。
- 日志、run meta、TUI、`/remote` 不泄漏 token。
- 非 loopback 缺 token 启动失败。

提交：只读无提交；修复 blocker 单独提交。

### Session 13 - Phase 6 Declarative Webhooks

类型：实现

目标：声明式 webhook route、HMAC、event filter、prompt rendering、兼容现有 webhook paths。

Owned files：

- `crates/gateway/src/webhook.rs`
- `crates/claude-code-rs/src/daemon/webhook.rs`
- `crates/claude-code-rs/src/daemon/routes.rs`
- related webhook tests

验证：

```powershell
cargo test -p gateway webhook
cargo test -p claude-code-rs daemon::webhook
cargo test -p claude-code-rs tools::pr_activity
cargo check -p claude-code-rs --message-format short
```

提交：提交 declarative webhooks。

### Session 14 - Phase 7 Delivery Router

类型：实现

目标：实现 delivery target、callback/local/channel delivery 和失败事件。

Owned files：

- `crates/gateway/src/delivery.rs`
- `crates/gateway/src/runner.rs`
- `crates/gateway/src/events.rs`
- `crates/gateway/tests/delivery.rs`

验证：

```powershell
cargo test -p gateway delivery runner events
cargo test -p claude-code-rs daemon::channels
cargo check -p gateway --message-format short
```

提交：提交 delivery router。

### Session 15 - Phase 8 Recovery Queue E2E

类型：实现，高风险

目标：恢复、队列、stale lock、daemon restart 和 e2e smoke。

Owned files：

- `crates/gateway/src/store.rs`
- `crates/gateway/src/runner.rs`
- `crates/gateway/src/policy.rs`
- `crates/claude-code-rs/tests/e2e_gateway.rs`
- directly related daemon process/supervisor tests

验证：

```powershell
cargo test -p gateway store runner policy
cargo test -p claude-code-rs daemon::process_state daemon::supervisor
cargo test -p claude-code-rs --test e2e_gateway
cargo check -p claude-code-rs --message-format short
```

提交：提交 recovery/e2e。

### Review F - Recovery Gate

类型：review gate

目标：验证 daemon restart、queued run、pending approval、stale lock、duplicate idempotency。

提交：只读无提交；修复 blocker 单独提交。

### Session 16 - Phase 9 Docs Release Gate

类型：文档

目标：保存实现效果、缺陷和后续跟踪方案。

Owned files：

- `development/reference/REMOTE_CONTROL_GATEWAY.md`
- `development/archive/CLI_REFERENCE.md`
- `development/archive/FINAL_RELEASE_PLAN.md`
- `development/archive/KNOWN_ISSUES.md`
- `development/archive/plan/remote-control-gateway-implementation-report-2026-05-08.md`

要求：

- 报告必须包含：实现效果、已知缺陷、验证命令、未测项、后续跟踪计划、对应 commit 列表。
- 未完成项不能写成已完成。

验证：

```powershell
rg -n "remote-control|/remote|RemoteSurface|Telegram|Lark|known issue|follow-up" docs
git diff --check
```

提交：提交 docs/release gate。

### Session 17 - Final Verification

类型：验证

目标：最终收敛，不主动改代码；若失败，输出最小可执行修复建议。

验证：

```powershell
git diff --check
cargo test -p gateway
cargo test -p claude-code-rs remote_cmd ui::components::command_palette ui::components::command_surface ui::components::status_widget
cargo test -p claude-code-rs daemon::protocol daemon::routes daemon::sse daemon::supervisor
cargo check -p claude-code-rs --message-format short
```

提交：无；若修复验证 blocker，单独提交。

## 6. 推荐执行命令

只看将运行什么：

```powershell
.\scripts\achieve\run-remote-control-gateway-omx-sessions.ps1 -DryRun
```

一次只跑一个高风险 session：

```powershell
.\scripts\achieve\run-remote-control-gateway-omx-sessions.ps1 -OnlySession 7
```

从头开始，但每次最多跑 2 个 session：

```powershell
.\scripts\achieve\run-remote-control-gateway-omx-sessions.ps1 -StartAt 0 -MaxSessions 2
```

继续到下一个 review gate：

```powershell
.\scripts\achieve\run-remote-control-gateway-omx-sessions.ps1 -StartAt 8 -EndAt 11
.\scripts\achieve\run-remote-control-gateway-omx-sessions.ps1 -OnlySession "Review D"
```

底层调用仍走：

```powershell
.\scripts\codex-task-sequence.ps1 `
  -Tasks @("<single session prompt>") `
  -WorkDir . `
  -OutputDir .\target\codex-runs\remote-control-gateway\<session> `
  -Sandbox danger-full-access `
  -Model "gpt-5.5" `
  -ReasoningEffort "medium"
```

## 7. 完成定义

本执行计划完成后必须满足：

- 每个修改 session 都有对应 commit。
- 每个 review gate 有 last-message 记录，阻塞项已修或明确保留。
- `crates/gateway` 不反向依赖 `claude-code-rs`。
- `/remote` 和 TUI 不绕过 gateway API/local client。
- 单文件大小阈值无未解释超限。
- 对外错误都有 stable code/message/action。
- 最终报告保存在 `development/archive/plan/remote-control-gateway-implementation-report-2026-05-08.md`。
- `development/archive/KNOWN_ISSUES.md` 记录仍未完成或有用户可见影响的缺陷。
- 最终验证命令、未测项和后续跟踪方案写入报告。

---

## 完成状态（2026-05-21）

**判定：⚠️ 部分完成（Session 00-16 已完成，Session 17 最终验证未执行）**

已完成（Session 00-16，对应 20 个 commits）：
- **Session 00**（Baseline）：已完成
- **Session 01**（Phase 0 Boundary Docs）：已完成
- **Session 02**（Phase 1A Gateway Crate Foundation）：已完成
- **Session 03**（Phase 1B Run Store And Policy）：已完成
- **Review A**（Schema Store Review）：已完成
- **Session 04**（Phase 2A Adapter Registry + Telegram）：已完成
- **Session 05**（Phase 2B Lark + Adapter Status）：已完成
- **Session 06**（Phase 3A Runner + Daemon Bridge）：已完成
- **Session 07**（Phase 3B Worker Ownership）：已完成
- **Review B**（Execution Ownership Review）：已完成
- **Session 08**（Phase 4A HTTP API + Runs）：已完成
- **Session 09**（Phase 4B Events/Stop/Approval/AskUser）：已完成
- **Review C**（API Contract Review）：已完成
- **Session 10**（Phase 4.5A Local Gateway Client + `/remote`）：已完成
- **Session 11**（Phase 4.5B TUI Remote Surface）：已完成
- **Review D**（Local UX And Redaction Review）：已完成
- **Session 12**（Phase 5 Security Hardening）：已完成
- **Review E**（Security Gate）：已完成
- **Session 13**（Phase 6 Declarative Webhooks）：已完成
- **Session 14**（Phase 7 Delivery Router）：已完成
- **Session 15**（Phase 8 Recovery Queue E2E）：已完成
- **Review F**（Recovery Gate）：已完成
- **Session 16**（Phase 9 Docs Release Gate）：已完成

未完成：
- **Session 17**（Final Verification）：未执行 — 依赖最终的 `cargo test -p gateway`、`cargo test -p claude-code-rs` 全套验证命令
