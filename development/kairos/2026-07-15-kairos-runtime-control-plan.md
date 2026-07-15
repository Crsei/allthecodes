# KAIROS Runtime Control Implementation Plan

> 状态：已实施（2026-07-15）
> 前置：无
> 后续：[KAIROS API Control Plan](2026-07-15-kairos-api-control-plan.md)、[KAIROS TUI Control Plan](2026-07-15-kairos-tui-control-plan.md)

## 目标

建立一个可被 CLI、斜杠命令、HTTP handler 和 Rust TUI 共同调用的 KAIROS runtime control layer。它负责配置解析、持久化、feature 依赖、daemon lifecycle、readiness 和状态快照；调用方不再直接读写环境变量或复制 `start_daemon()` 逻辑。

## 核心决策

1. Domain DTO 放入 `allthecodes-types`，避免 `commands -> daemon` 的依赖环。
2. typed settings 是 desired state；环境变量继续作为兼容 override，并在 snapshot 中显示来源。
3. running state 记录 daemon 启动时的 effective profile；desired 与 running 不同即 `restart_required=true`。
4. controller API 不打印 stdout、不返回 `ExitCode`；CLI 仅做 parse/format adapter。
5. start/stop/restart 都必须经过现有 operation lock、stale cleanup 和 readiness。
6. 普通 TUI 的本地 engine 与 daemon assistant worker是两个明确 owner；controller 只管理独立 daemon。

## 共享类型

新增 `crates/allthecodes-types/src/kairos.rs`：

```rust
pub struct KairosFeatureProfile {
    pub enabled: bool,
    pub brief: bool,
    pub channels: bool,
    pub push_notifications: bool,
    pub github_webhooks: bool,
    pub proactive: bool,
}

pub enum KairosConfigScope { User, Project, Local }
pub enum KairosValueSource { Default, Managed, User, Project, Local, Environment, Cli }

pub enum KairosLifecycleState {
    Stopped,
    Starting,
    Ready,
    Stopping,
    Restarting,
    Stale,
    Failed,
}

pub struct KairosRuntimeSnapshot {
    pub desired: KairosFeatureProfile,
    pub effective: KairosFeatureProfile,
    pub running: Option<KairosFeatureProfile>,
    pub sources: KairosProfileSources,
    pub lifecycle: KairosLifecycleState,
    pub restart_required: bool,
    pub supervisor: Option<KairosSupervisorSnapshot>,
    pub workers: Vec<KairosWorkerSnapshot>,
    pub automation: Option<KairosAutomationSnapshot>,
    pub last_transition: Option<KairosLifecycleTransition>,
}

pub enum KairosControlAction { Start, Stop, Restart, Reconcile }
pub struct KairosControlRequest { /* cwd, port, profile/scope, timeout */ }
pub struct KairosControlResult { /* action, changed, snapshot */ }
```

要求：

- 所有外部枚举使用 `snake_case` serde 名称。
- profile 字段与 `FeatureFlags` 的 KAIROS child gates 一一对应。
- dependency normalization 必须是纯函数并返回 diagnostics；`enabled=false` 时 Brief/channels/push/webhooks 不能成为 effective true。
- `FEATURE_KAIROS=1` 隐含 proactive 的现有语义必须保留。
- snapshot 不包含 control token、provider key、完整命令行或其他 secret。

## Task R1：Typed profile 与 settings 持久化

### 文件

- Create: `crates/allthecodes-types/src/kairos.rs`
- Modify: `crates/allthecodes-types/src/lib.rs`
- Modify: `crates/allthecodes-config/src/settings/types.rs`
- Modify: `crates/allthecodes-config/src/settings/raw.rs`
- Modify: `crates/allthecodes-config/src/settings/effective.rs`
- Modify: `crates/allthecodes-config/src/settings/schema.rs`
- Modify: `crates/allthecodes-config/src/settings/write.rs`
- Modify: `crates/allthecodes-config/src/features.rs`
- Test: `crates/allthecodes-config/src/settings/tests.rs`
- Test: `crates/allthecodes-config/src/features.rs`

### 实施

- 在 settings 中增加 `kairos` typed object，不用 `extra` 或自由 JSON 保存核心字段。
- raw 字段使用 `Option<bool>`，让 source-aware merge 能区分“未设置”和显式 false；effective profile 完整 materialize。
- 增加 read-modify-write helper，只修改 `kairos` subtree，保留未知 keys、providers、MCP 和用户现有设置。
- TUI 默认 scope 为 `Local`；CLI/API 可显式选择 `User`、`Project`、`Local`。
- `FeatureFlags` 增加从 effective profile + env override 解析的入口。不要通过修改当前进程环境变量来模拟配置。
- 保留 `FeatureFlags::from_env()` 和 runtime override，供兼容测试、CI 与 `/experimental` 使用。
- 返回每个 profile 字段的 source；如环境变量覆盖持久化值，snapshot 必须显示 `environment`。

### 测试

- user/project/local 的覆盖顺序和 source map。
- 显式 false 能覆盖低层 true。
- 写入 KAIROS profile 不删除 settings 中未知 key。
- parent/child gate normalization 和 diagnostics。
- env override、KAIROS-implies-proactive、runtime override 的兼容行为。
- 路径只使用 `.allthecodes` / `ALLTHECODES_HOME`。

## Task R2：抽取 typed daemon controller

### 文件

- Create: `crates/allthecodes-daemon/src/process_state/controller.rs`
- Modify: `crates/allthecodes-daemon/src/process_state/mod.rs`
- Modify: `crates/allthecodes-daemon/src/process_state/management.rs`
- Modify: `crates/allthecodes-daemon/src/readiness.rs`
- Modify: `crates/allthecodes-daemon/src/process_state/types.rs`
- Test: `crates/allthecodes-daemon/src/process_state/tests.rs`
- Test: `crates/allthecodes/tests/e2e_cli.rs`

### 接口

```rust
pub trait KairosController: Send + Sync {
    fn snapshot(&self, request: KairosSnapshotRequest) -> Result<KairosRuntimeSnapshot>;
    fn configure(&self, request: KairosConfigureRequest) -> Result<KairosRuntimeSnapshot>;
    fn control(&self, request: KairosControlRequest) -> Result<KairosControlResult>;
}
```

允许最终使用具体 struct 而不是 trait，但 commands/TUI tests 必须能注入 fake adapter。

### 实施

- 将 `start_daemon`、`stop_daemon`、`restart_daemon` 的核心逻辑移入 controller；management module 仅保留 CLI parse、文本输出和 `ExitCode` 映射。
- controller 内部复用现有 operation lock，不允许并发 start/restart 互相穿透。
- start 顺序固定为：resolve profile -> validate enabled -> stale cleanup -> spawn detached child -> write starting transition -> readiness -> read process state -> ready/failed transition。
- child process 显式接收 resolved KAIROS profile；不得依赖父进程刚刚 `set_var` 的全局副作用。
- stop 必须先写 graceful shutdown request，等待现有 grace period，再按当前平台规则兜底终止。
- restart 必须是一个 operation-lock 临界区，不能在 stop/start 之间释放锁。
- start 对已 ready 且 profile 相同返回 idempotent success；profile 不同时返回 conflict/restart-required，除非 action 是 restart/reconcile。
- 端口占用、binary path、spawn、readiness timeout、stale cleanup failure 都映射为稳定 error kind。

### 测试

- stopped -> starting -> ready。
- ready + same profile -> unchanged success。
- ready + changed profile -> restart required。
- stale PID cleanup 后可启动。
- 并发操作只允许一个 owner。
- readiness timeout 产生 failed transition，不能返回 ready。
- stop/restart 的 PID 和 worker state 发生预期变化。
- 测试使用临时 `ALLTHECODES_HOME`、随机端口和无真实 provider 的启动配置。

## Task R3：Running profile 与 lifecycle state

### 文件

- Modify: `crates/allthecodes-daemon/src/process_state/types.rs`
- Modify: `crates/allthecodes-daemon/src/process_state/storage.rs`
- Modify: `crates/allthecodes-daemon/src/process_state/paths.rs`
- Modify: `crates/allthecodes-daemon/src/supervisor.rs`
- Modify: `crates/allthecodes-daemon/src/runtime.rs`
- Test: corresponding process-state and supervisor tests

### 实施

- 在 supervisor state 中记录 schema-versioned `running_kairos_profile`、启动时间和配置 digest；旧 state 缺字段时可兼容读取。
- lifecycle transition 持久化到 daemon 数据根，例如 `kairos-lifecycle.json`；原子写入并限制为当前最后状态，不建立无限日志。
- transition 至少包含 action、from/to、timestamp、operation id、可公开 error code/message。
- controller 支持可选的 in-process lifecycle sink；Web host、daemon route 和 TUI 可将 transition 接入各自事件通路。持久化 snapshot 始终是跨进程事实来源，不能依赖某个 sink 存活。
- daemon child 完成 startup/readiness 后主动写入并发布 `ready` transition；由外部 TUI/CLI 启动时也不能缺失最终状态。
- snapshot 聚合 desired/effective/running、supervisor、workers 和 automation state。
- `restart_required` 只比较需要进程重建的字段；纯 runtime/durable state（例如当前 `/proactive` pause）不误报配置漂移。
- stopped daemon 仍可返回完整 desired/effective/source snapshot。

### 测试

- 旧 supervisor JSON 向后兼容。
- profile digest 稳定且与字段顺序无关。
- desired/running drift 正确设置 restart-required。
- stale/failed/stopped 不被折叠为同一状态。
- error snapshot 可序列化且已脱敏。

## Task R4：CLI 与 slash-command runtime adapter

### 文件

- Modify: `crates/allthecodes-daemon/src/process_state/management.rs`
- Modify: `crates/allthecodes-commands/src/daemon_cmd.rs`
- Create: `crates/allthecodes-commands/src/kairos_cmd.rs`
- Modify: `crates/allthecodes-commands/src/lib.rs`
- Modify: `crates/allthecodes/src/command_runtime_bridge.rs`
- Test: command unit tests and e2e CLI tests

### 命令契约

```text
/kairos                       status or open TUI surface
/kairos status
/kairos enable [--scope local|project|user] [--start]
/kairos disable [--scope ...] [--stop]
/kairos start
/kairos stop
/kairos restart
/kairos feature <brief|channels|push-notifications|github-webhooks|proactive> <on|off>
/kairos bridge sessions
/kairos bridge resume <session-id>
```

### 实施

- `KairosCommandRuntime` 接收 typed snapshot/configure/control/bridge callbacks；不让 commands crate 依赖 daemon crate。
- lifecycle callback 使用 async adapter，或在 root adapter 内通过 `spawn_blocking` 调用同步 controller；等待 readiness 时不得阻塞 Rust TUI event loop。
- `/daemon start` 和 `/daemon restart` 改为调用同一 controller，保留原命令名兼容。
- `/kairos enable --start` 使用单个 reconcile operation，避免配置已写但启动未执行的无状态反馈；失败结果明确显示配置是否已保存。
- 文本输出同时显示 lifecycle、desired/effective/running、source、restart-required、workers 和最后错误。
- `/experimental` 文案补充“仅当前进程，不启动 KAIROS daemon”，但行为不变。

### 测试

- fake runtime 覆盖每个命令和错误映射。
- `/daemon start|restart` 不再返回 shell hint。
- child gate 名称和 dependency error 稳定。
- bridge command 继续复用现有 lease API。

## Runtime 完成门禁

```bash
cargo fmt --all -- --check
cargo test -p allthecodes-types kairos
cargo test -p allthecodes-config kairos
cargo test -p allthecodes-commands kairos
cargo test -p allthecodes-commands daemon
cargo test -p allthecodes-daemon process_state
cargo test -p allthecodes-daemon supervisor
cargo test -p allthecodes --test e2e_cli daemon
cargo check -p allthecodes-daemon
cargo check -p allthecodes
```

完成标准：不用 HTTP 和 TUI，仅通过 typed controller、CLI 和 `/kairos` command 已能在临时 home 下完成配置、start/readiness/status/restart/stop 全闭环。
