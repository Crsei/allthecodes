# KAIROS Rust TUI Control And Display Plan

> 状态：已实施（2026-07-15；provider-free CLI lifecycle E2E 已通过，PTY 视觉 E2E 待单独补充）
> 前置：[KAIROS Runtime Control Plan](2026-07-15-kairos-runtime-control-plan.md)
> API 计划可并行到 protocol DTO 完成，但 TUI 本地控制不得依赖 HTTP daemon 已经运行。

## 目标

在 Rust TUI 中新增 `/kairos` command surface，使用户可以直接启用并启动 KAIROS、查看 readiness/worker/automation/bridge 状态、切换 child gates、重启和停止。TUI 调用本地 typed controller adapter，不启动 shell 子命令，也不要求用户退出后设置环境变量。

## UX 状态模型

新增 UI-only snapshot，不复用 `engine::AppState.kairos_active`：

```rust
pub struct KairosUiStatus {
    pub lifecycle: KairosLifecycleState,
    pub enabled: bool,
    pub restart_required: bool,
    pub worker_summary: String,
    pub last_error: Option<String>,
}
```

`AppState.kairos_active` 继续表示“当前 QueryEngine 本身运行在 KAIROS assistant mode”。普通 TUI 控制后台 daemon 时保持 false。footer 和 surface 使用 `KairosUiStatus` 表示外部 daemon 状态，避免把本地 engine 误当 daemon worker。

## Surface 设计

`/kairos` 无参数打开 `KairosSurface`，包含四页：

### Overview

- lifecycle：stopped/starting/ready/restarting/stopping/stale/failed
- desired、effective、running profile 摘要
- supervisor PID、health URL、readiness
- restart-required 和最后 transition/error
- 主动作按状态变化：
  - disabled + stopped：`Enable & Start`
  - enabled + stopped：`Start`
  - starting/restarting：`Refresh`，禁用重复操作
  - ready + clean：`Stop`，并提供 `r Restart`
  - ready + drift：`Restart to apply`
  - stale/failed：`Repair & Start`，先经 controller stale cleanup

### Features

逐项显示 `desired / effective / running / source`：

- KAIROS
- Brief
- Channels
- Push notifications
- GitHub webhooks
- Proactive

Enter 只写配置；`a` 执行 reconcile/restart。父 gate 关闭时 child row 显示 blocked reason，不允许展示为有效开启。

### Workers

- supervisor、assistant、proactive、scheduler、bridge-sync 的 kind/PID/status/restart count/last update。
- automation state：standby/running/sleeping/needs-input/context-blocked、next tick、sleep reason。
- workers 不存在时区分 disabled、not scheduled、crashed 和 unknown。

### Bridge

- 列出 workspace、assistant session、remote key、lease owner/expiry、last ack/run。
- 支持显式 resume/attach、new、release；active foreign lease 必须显示 conflict，不自动 takeover。
- `Enable & Start` 不自动 attach bridge session。

## Task T1：`/kairos` command 与 surface 注册

### 文件

- Create: `crates/allthecodes/src/ui/command_surface/surfaces/kairos.rs`
- Create: `crates/allthecodes/src/ui/command_surface/adapters/kairos.rs`
- Modify: `crates/allthecodes/src/ui/command_surface/surfaces/mod.rs`
- Modify: `crates/allthecodes/src/ui/command_surface/adapters/mod.rs`
- Modify: `crates/allthecodes/src/ui/command_surface/mod.rs`
- Modify: `crates/allthecodes/src/ui/command_surface/tests.rs`
- Modify: slash command files from Runtime Task R4

### 实施

- 增加 `CommandSurface::Kairos(KairosSurface)` 和 title/render/key dispatch。
- `/kairos` 空参数打开 surface；有参数执行 slash command。
- surface 只根据已有 `KairosUiSnapshot` 渲染，不在 `render()` 或 key handler 内做文件/网络/process I/O。
- action 通过 `CommandSurfaceOutcome::Submit` 生成稳定 `/kairos ...` 命令；若 typed action outcome 能显著减少字符串解析，可新增专用 outcome，但最终仍必须进入同一 command runtime。
- `Enable & Start` 对应一个原子 `/kairos enable --scope local --start` action。
- start 不要求确认；stop、disable+stop 和 release foreign-sensitive state 使用现有确认模式或明确 `--confirm` 二次动作。

### 测试

- `/kairos` alias/surface registry 完整。
- 每个 lifecycle 的 primary action 正确。
- feature dependency/source/restart-required 渲染。
- workers empty/crashed/ready 和 bridge conflict 渲染。
- 键盘：Left/Right/Tab、Up/Down、Enter、r、a、Esc。
- render path 不执行 I/O。

## Task T2：后台 snapshot poller 与 subsystem events

### 文件

- Create: `crates/allthecodes/src/ui/tui/kairos.rs`
- Modify: `crates/allthecodes/src/ui/tui.rs`
- Modify: `crates/allthecodes/src/ui/tui/subsystem_events.rs`
- Modify: `crates/allthecodes/src/ui/app/domain.rs`
- Modify: `crates/allthecodes/src/ui/app/render.rs`
- Modify: `crates/allthecodes/src/ui/app.rs`
- Test: `crates/allthecodes/src/ui/tui/tests.rs`
- Test: `crates/allthecodes/src/ui/app/tests.rs`

### 实施

- TUI 启动后立即请求一次 local controller snapshot，之后低频轮询（建议 1-2 秒）；process/file inspection 使用 `spawn_blocking`。
- 增加 `SubsystemEvent::KairosSnapshotUpdated` 和 `KairosSnapshotFailed`；event handler 更新 App，不直接渲染通知风暴。
- lifecycle operation 进行中可临时提高 polling，ready/failed/stopped 后恢复低频。
- App 保存完整 latest snapshot 与精简 footer status；打开 surface 时从 App 注入 snapshot。
- 为 command surface constructor 增加显式 UI context，避免把 daemon 状态塞入 engine AppState。
- TUI 退出不自动停止 daemon；daemon 是用户显式开启的后台服务。
- 连续相同错误只更新状态，不每 1 秒写一条 transcript notice。

### 测试

- snapshot event 更新 footer 和 surface source。
- poll failure 显示 unknown/error，但不把上次 ready 假装成当前 ready。
- operation transition 后 poll interval 正确恢复。
- TUI 退出不发送 stop。
- fake controller 保证测试无 daemon、网络和 provider 依赖。

## Task T3：Footer、notice 与错误反馈

### Footer 文案

| 状态 | 示例 |
| --- | --- |
| disabled/stopped | 不显示，或在用户打开过 surface 后显示 `KAIROS off` |
| starting | `KAIROS starting` |
| ready | `KAIROS ready · 4 workers` |
| ready + drift | `KAIROS restart required` |
| sleeping | `KAIROS sleeping · 12m` |
| needs input | `KAIROS needs input` |
| failed/stale | `KAIROS error` / `KAIROS stale` |

### 实施

- KAIROS footer 与现有 proactive footer 合并去重：后台 KAIROS 已包含 proactive 时，不显示两个相互矛盾的状态。
- operation success 只产生一条可操作 notice，包含 ready URL/PID 或 stopped 结果。
- error notice 显示稳定 error code、简短原因和下一动作（retry/restart/repair/config），不展示 backtrace 或 secret。
- readiness 未完成前不能显示 ready；收到 start accepted 只显示 starting。
- restart-required 必须能从 footer 直接按快捷键进入 surface，而不是只埋在 `/kairos status` 文本里。

### 测试

- footer 宽度不足时按项目现有截断规则渲染。
- proactive standalone、KAIROS+proactive、sleeping、needs-input 组合不冲突。
- duplicate snapshot 不重复写 notice。

## Task T4：直接启动 E2E

### 文件

- Modify: `crates/allthecodes/tests/pty_tui_e2e.rs` 或当前 Rust TUI PTY test target
- Modify: `crates/allthecodes/tests/e2e_cli.rs`（仅共享 fixture 时）
- Add/update snapshots under the test target's existing snapshot location

### 场景

1. 使用临时 `ALLTHECODES_HOME`、临时 workspace、随机端口启动普通 Rust TUI。
2. 输入 `/kairos`，断言 Overview 为 disabled/stopped。
3. Enter 执行 `Enable & Start`。
4. 断言先出现 starting，最终出现 ready，不接受直接跳到假 ready。
5. 打开 Workers，断言 supervisor/assistant 和 profile 允许的 workers 可见。
6. 切换 Brief 或 Channels，断言 source 为 local、restart-required 出现。
7. 执行 restart，等待新 PID/operation 完成，restart-required 清除。
8. stop，断言 stopped；settings profile 仍 enabled。
9. 再次 start，证明持久化生效且无需环境变量。
10. disable+stop，断言 profile disabled 且 daemon stopped。

provider 使用不可达本地 fake endpoint；测试只验证进程 readiness 和控制面，不发模型请求。若 PTY harness 在目标平台不可用，使用现有 headless TUI/event harness 做等价验证并明确记录 skipped reason。

## Task T5：文档与 command help

### 文件

- Modify: `development/reference/DAEMON_OPERATIONS.md`
- Modify: `development/hided_features/current-hidden-features.md`
- Modify: `docs/WORK_STATUS.md`
- Modify: slash command palette/help snapshots

记录：

- `/kairos` 与 `/daemon` 的职责区别。
- `Enable & Start` 默认 local scope。
- 普通 TUI 与 daemon assistant 的 owner 边界。
- TUI 退出不停止 daemon。
- feature 修改何时需要 restart。
- bridge attach 为显式动作。

## TUI 完成门禁

```bash
cargo fmt --all -- --check
cargo test -p allthecodes-commands kairos
cargo test -p allthecodes command_surface::tests --lib
cargo test -p allthecodes tui::tests --lib
cargo test -p allthecodes app::tests --lib
cargo test -p allthecodes --test pty_tui_e2e kairos -- --nocapture
cargo check -p allthecodes
```

最终再运行：

```bash
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace --release
```

如 full workspace test 未取得 exit 0，尤其停在 `pty_tui_e2e`，不得把计划标记完成；应记录具体未完成 gate 和活跃进程/日志证据。

## TUI 完成标准

- 用户从普通 Rust TUI 内无需 shell 命令即可配置并启动 KAIROS。
- UI 状态来自 typed runtime snapshot，不来自 `FEATURE_KAIROS` 猜测或 stdout 文本解析。
- readiness、worker、automation、restart-required、error 和 bridge lease 都有明确展示。
- 同一动作在 TUI、slash command、CLI 和 API 中使用同一 controller 语义。
- 普通 TUI 本地 engine 不被静默转换为第二个 daemon assistant owner。
