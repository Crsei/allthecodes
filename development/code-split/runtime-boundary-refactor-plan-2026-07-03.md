# 组合根与运行时边界收敛实施记录

> **状态校准（2026-07-20）：** 本计划已在 2026-07-03 至 2026-07-04 执行。下文 checkbox 保留为当时的实施规格，不代表当前任务仍是 `not started`；当前残留项统一回到
> [`codebase-optimization-plan-2026-07-03.md`](codebase-optimization-plan-2026-07-03.md) 跟踪。

**Goal:** 将当前架构评审中指出的组合根、运行时服务、工具策略、QueryEngine 状态和配置映射边界收窄，避免 full build 阶段重新形成隐性大耦合。

**Architecture:** 先加护栏和文档同步，再按边界分阶段迁移。`full_init.rs` 逐步降为启动编排入口，运行时服务通过显式对象传入，工具策略由 metadata/capability 驱动，状态和配置先领域化再考虑锁粒度或 crate 边界。所有阶段保持现有行为兼容，不以 Lite 缩减为理由删除上游行为。

**Tech Stack:** Rust workspace, Tokio, async-trait, serde/serde_json, clap, GitHub Actions, cargo fmt/clippy/test/build.

## Global Constraints

- 当前阶段是 Full Build。新代码必须按上游完整版行为对齐，不再按 Lite 缩减。
- 持久化路径必须使用 allthecodes 隔离路径：`~/.allthecodes/`、`.allthecodes/settings.json`、`.allthecodes/skills/`、Keychain service `"allthecodes"`。
- 不新增 process-wide `LazyLock<RwLock<_>>`、`OnceLock<RwLock<_>>` 或 callback 注册表作为新架构入口；兼容层可以短期保留，但新路径必须显式注入。
- 不新增 UI 层权限、shell 风险、tool JSON 解析逻辑；UI 只能消费后端 typed payload 或 view model。
- 不把 `allthecodes-query` 重新引入为并行实现。query loop 边界需要先通过文档和 ADR 决定，避免再次出现双实现。
- 本地验证使用仓库指定 Rust 环境：

```bash
export CARGO_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo
export RUSTUP_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/rustup
export CARGO_TARGET_DIR=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.tmp/allthecodes-target
export PATH="$CARGO_HOME/bin:$PATH"
```

## 实施状态与残留边界

九个任务均能在当前 Git 历史和源码结构中找到落地证据：

| 任务 | 落地证据 | 当前边界 |
|---|---|---|
| RB-001 | `a268787a` | 质量门与 query boundary 文档已落地 |
| RB-002 | `b78639f8`, `dfa63a74` | startup composition 已拆分；后续新增模式仍需防止组合根回涨 |
| RB-003 | `7bc67cf1`, `ed0ab9ce` | `RuntimeServices` 已显式注入，包括 ACP engine 路径 |
| RB-004 | `e61322b9`, `679d2a8b` | tool metadata/policy 已落地；typed permission 与 shell 单一判定仍由 CS-003/006 跟踪 |
| RB-005 | `0dd53b12` | `EngineSharedState` 已领域化；锁粒度与高扇出状态仍是残留治理 |
| RB-006 | `95eddec6`, `9b278004` | `ToolExecutionPipeline` 已落地；主函数和 pipeline 体量仍由 CS-001 跟踪 |
| RB-007 | `21ea2b0d`, `4f22e341`, `cf49004f` | `QueryTurnState`、`SubmitTransaction` 和 typed events 已落地；巨型主函数仍由 CS-004/005 跟踪 |
| RB-008 | `aa3b7d92` | runtime settings 已领域化 |
| RB-009 | `d40be644` | query loop ADR 已记录，且继续禁止平行 `allthecodes-query` 实现 |

因此，本文件的用途是解释原始迁移设计和提交边界。新工作不得重新执行整套任务，也不得因下文保留的未勾选 checkbox 把已存在的结构判定为未实现。

---

## 评审结论整理

当前项目方向是正确的：它已经不是把功能堆进单一二进制的结构，而是有明确 workspace、engine、tools、permissions、config、session、IPC、Web、Daemon、TUI 分层。真正需要治理的是几个正在变大的边界：

- `crates/allthecodes/src/full_init.rs` 仍是过宽组合根。
- 全局 registry/callback 让单 session CLI 容易跑通，但对 daemon、多 workspace、多 engine 和并行测试不友好。
- `Tool` trait 同时承担 spec、schema、permission、execution、展示、MCP server、结果大小、路径提取、中断和 auto classifier 输入。
- `QueryEngineState` 阶段性集中锁是合理的，但类型边界正在变宽。
- `submit_message`、query loop 和 tool execution 是高风险主路径，需要 pipeline 化和测试护栏。
- `SettingsJson`、`AppState`、`ToolAppState` 手动映射过多，新增字段容易漂移。
- README、crate 注释和实际 query loop 位置需要同步。
- 源码注释乱码和 CI 质量门禁需要前移到 PR/push。

已有优势需要保留：

- 入口和生命周期已经用 Phase/Step 注释表达流程。
- `QueryDeps` 是有效的解耦点，应继续细分为能力接口。
- 权限系统已独立成状态机，Auto mode fallback 和 denial tracking 方向正确。
- `ToolProvider` 已经意识到 dependency cycle，后续应从全局 provider 迁向显式 runtime services。

---

## 目标文件结构

本计划建议的新增或重点修改路径：

- Modify: `crates/allthecodes/src/main.rs`
- Modify: `crates/allthecodes/src/full_init.rs`
- Create: `crates/allthecodes/src/startup/mod.rs`
- Create: `crates/allthecodes/src/startup/startup_context.rs`
- Create: `crates/allthecodes/src/startup/runtime_composition.rs`
- Create: `crates/allthecodes/src/startup/settings_runtime.rs`
- Create: `crates/allthecodes/src/startup/plugin_runtime.rs`
- Create: `crates/allthecodes/src/startup/tool_catalog.rs`
- Create: `crates/allthecodes/src/startup/mcp_runtime.rs`
- Create: `crates/allthecodes/src/startup/model_runtime.rs`
- Create: `crates/allthecodes/src/startup/app_state_factory.rs`
- Create: `crates/allthecodes/src/startup/engine_factory.rs`
- Create: `crates/allthecodes/src/startup/mode_router.rs`
- Modify: `crates/allthecodes-engine/src/lifecycle/mod.rs`
- Create: `crates/allthecodes-engine/src/lifecycle/state.rs`
- Create: `crates/allthecodes-engine/src/runtime_services.rs`
- Modify: `crates/allthecodes-engine/src/lifecycle/deps/mod.rs`
- Modify: `crates/allthecodes-engine/src/lifecycle/deps/execute.rs`
- Create: `crates/allthecodes-engine/src/lifecycle/deps/tool_pipeline.rs`
- Modify: `crates/allthecodes-engine/src/query/loop_impl.rs`
- Create: `crates/allthecodes-engine/src/query/turn_state.rs`
- Create: `crates/allthecodes-engine/src/query/recovery.rs`
- Modify: `crates/allthecodes-engine/src/lifecycle/submit_message/mod.rs`
- Create: `crates/allthecodes-engine/src/lifecycle/submit_message/transaction.rs`
- Modify: `crates/allthecodes-tools/src/tool.rs`
- Modify: `crates/allthecodes-tools/src/registry.rs`
- Create: `crates/allthecodes-tools/src/metadata.rs`
- Modify: `crates/allthecodes-config/src/runtime_settings.rs`
- Modify: `crates/allthecodes-engine/src/types/app_state.rs`
- Modify: `.github/workflows/ci.yml`
- Modify: `README.md`
- Modify: `crates/allthecodes/Cargo.toml`

---

## Task 1: 质量护栏和文档同步

**Files:**

- Modify: `.github/workflows/ci.yml`
- Modify: `README.md`
- Modify: `crates/allthecodes/Cargo.toml`
- Modify: `crates/allthecodes-engine/src/query/loop_impl.rs`
- Modify: `development/code-split/codebase-optimization-plan-2026-07-03.md`

**Interfaces:**

- Consumes: existing CI workflow named `ci`
- Produces: PR/push quality gate covering fmt, clippy, workspace tests, tools contract/full feature tests, and a documented query-loop boundary

- [ ] **Step 1: Inspect current CI and query-loop docs**

Run:

```bash
sed -n '1,220p' .github/workflows/ci.yml
rg -n "allthecodes-query|query loop|QueryDeps|full_init|mojibake|閻|闁" README.md crates/allthecodes/Cargo.toml crates/allthecodes-engine/src/query/loop_impl.rs
```

Expected: CI currently exists but is narrower than the requested PR gate; `crates/allthecodes/Cargo.toml` contains mojibake dependency comments.

- [ ] **Step 2: Fix mojibake comments without touching dependency semantics**

Replace corrupted comments in `crates/allthecodes/Cargo.toml` with plain English dependency groups:

```toml
# Async runtime
# Serialization
# Error handling
# Time
# Tracing
# Filesystem and paths
# Shell parsing
# Terminal UI
# Text rendering
# Networking
# Authentication and environment
# Synchronization
# Workspace crates
# AST parsing
# Platform sandbox helpers
```

Expected: only comments change in `Cargo.toml`; dependency names, versions, features, and ordering stay unchanged.

- [ ] **Step 3: Synchronize query-loop boundary wording**

Update README and code-split docs to state the current boundary explicitly:

```text
The canonical query loop currently lives in `crates/allthecodes-engine/src/query/`.
Do not recreate `allthecodes-query` as a parallel implementation. If the query loop is
extracted again, it must be a single canonical crate depending on typed abstractions,
with no duplicate engine-internal implementation.
```

Expected: docs no longer imply that a separate `crates/allthecodes-query` crate is currently active.

- [ ] **Step 4: Expand CI before major refactors**

Update `.github/workflows/ci.yml` so PR/push runs:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p allthecodes-tools --no-default-features --features contract
cargo test -p allthecodes-tools --features full
```

Expected: release workflow remains release-only; normal CI catches formatting, warnings, and tests earlier.

- [ ] **Step 5: Verify and commit**

Run:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Expected: all commands pass or failures are recorded as explicit pre-existing blockers in the task result.

Commit:

```bash
git add -A -- .github/workflows/ci.yml README.md crates/allthecodes/Cargo.toml crates/allthecodes-engine/src/query/loop_impl.rs development/code-split/codebase-optimization-plan-2026-07-03.md
git commit -m "chore: add code split quality gates"
```

---

## Task 2: 拆分 `full_init.rs` 组合根

**Files:**

- Modify: `crates/allthecodes/src/main.rs`
- Modify: `crates/allthecodes/src/full_init.rs`
- Create: `crates/allthecodes/src/startup/mod.rs`
- Create: `crates/allthecodes/src/startup/startup_context.rs`
- Create: `crates/allthecodes/src/startup/runtime_composition.rs`
- Create: `crates/allthecodes/src/startup/settings_runtime.rs`
- Create: `crates/allthecodes/src/startup/plugin_runtime.rs`
- Create: `crates/allthecodes/src/startup/tool_catalog.rs`
- Create: `crates/allthecodes/src/startup/mcp_runtime.rs`
- Create: `crates/allthecodes/src/startup/model_runtime.rs`
- Create: `crates/allthecodes/src/startup/app_state_factory.rs`
- Create: `crates/allthecodes/src/startup/engine_factory.rs`
- Create: `crates/allthecodes/src/startup/mode_router.rs`

**Interfaces:**

- Consumes: `Cli`, existing `run_full_init(cli).await`
- Produces: `StartupContext`, `RuntimeComposition`, `ModeRouter`

- [ ] **Step 1: Add module skeletons with no behavior movement**

Add `mod startup;` in `main.rs` only if the name conflict with `use allthecodes_startup as startup;` is first resolved by renaming the external import:

```rust
use allthecodes_startup as startup_crate;
```

Then create root module exports:

```rust
pub(crate) mod app_state_factory;
pub(crate) mod engine_factory;
pub(crate) mod mcp_runtime;
pub(crate) mod mode_router;
pub(crate) mod model_runtime;
pub(crate) mod plugin_runtime;
pub(crate) mod runtime_composition;
pub(crate) mod settings_runtime;
pub(crate) mod startup_context;
pub(crate) mod tool_catalog;
```

Expected: compilation still succeeds before moving behavior.

- [ ] **Step 2: Extract `StartupContext`**

Move cwd, CLI-derived flags, initial prompt, permission-mode override, Chrome enablement, and init-only decisions into:

```rust
pub(crate) struct StartupContext {
    pub cli: Cli,
    pub cwd: std::path::PathBuf,
    pub initial_prompt: Option<String>,
    pub mode: StartupMode,
    pub chrome_enabled: bool,
    pub computer_use_enabled: bool,
}
```

Expected: `run_full_init` starts with `let startup = StartupContext::from_cli(cli).await?;`.

- [ ] **Step 3: Extract subsystem builders one at a time**

Move behavior from `full_init.rs` into these builders in order:

```rust
SettingsRuntimeBuilder::build(&startup).await?;
PluginRuntimeBuilder::build(&startup, &settings).await?;
ToolCatalogBuilder::build(&startup, &settings, &plugins).await?;
McpRuntimeBuilder::build(&startup, &settings, &tool_catalog).await?;
ModelRuntimeBuilder::build(&startup, &settings).await?;
AppStateFactory::build(&startup, &settings, &tool_catalog, &mcp).await?;
EngineFactory::build(&startup, &runtime).await?;
```

Expected: each extraction compiles independently; no behavior changes are mixed into extraction commits.

- [ ] **Step 4: Replace tail dispatch with `ModeRouter`**

Move server/headless/ACP/TUI routing into:

```rust
pub(crate) struct ModeRouter {
    runtime: RuntimeComposition,
}

impl ModeRouter {
    pub(crate) async fn run(self) -> anyhow::Result<std::process::ExitCode>;
}
```

Expected: `run_full_init` becomes high-level orchestration:

```rust
pub(crate) async fn run_full_init(cli: Cli) -> anyhow::Result<ExitCode> {
    let startup = StartupContext::from_cli(cli).await?;
    let runtime = RuntimeComposition::build(startup).await?;
    ModeRouter::new(runtime).run().await
}
```

- [ ] **Step 5: Verify and commit**

Run:

```bash
cargo fmt --all --check
cargo test -p allthecodes startup
cargo check --workspace
```

Expected: `full_init.rs` retains only orchestration and legacy helpers not yet migrated; moved builders have focused unit tests.

Commit:

```bash
git add -A -- crates/allthecodes/src/main.rs crates/allthecodes/src/full_init.rs crates/allthecodes/src/startup
git commit -m "refactor: split startup composition root"
```

---

## Task 3: 引入显式 `RuntimeServices`

**Files:**

- Modify: `crates/allthecodes/src/main.rs`
- Modify: `crates/allthecodes/src/startup/runtime_composition.rs`
- Modify: `crates/allthecodes/src/startup/engine_factory.rs`
- Create: `crates/allthecodes-engine/src/runtime_services.rs`
- Modify: `crates/allthecodes-engine/src/lib.rs`
- Modify: `crates/allthecodes-engine/src/lifecycle/mod.rs`
- Modify: `crates/allthecodes-tools/src/registry.rs`
- Modify: `crates/allthecodes-permissions/src/decision.rs`

**Interfaces:**

- Consumes: existing global tool registry providers and permission message callbacks
- Produces: explicit services object passed into `QueryEngineConfig` or engine construction

- [ ] **Step 1: Define explicit services**

Create:

```rust
pub struct RuntimeServices {
    pub tool_registry: std::sync::Arc<dyn ToolRegistryService>,
    pub permission_message_resolver: std::sync::Arc<dyn PermissionMessageResolver>,
    pub hook_runner: std::sync::Arc<dyn HookRunnerService>,
    pub command_dispatcher: std::sync::Arc<dyn CommandDispatcherService>,
    pub model_client_factory: std::sync::Arc<dyn ModelClientFactoryService>,
}
```

Expected: traits live in `allthecodes-engine` only when they are engine-facing abstractions; concrete adapters stay in root/startup crates.

- [ ] **Step 2: Add compatibility adapter for existing globals**

Implement:

```rust
impl RuntimeServices {
    pub fn from_process_defaults() -> Self;
}
```

Expected: current CLI behavior continues while new tests can construct services without mutating process globals.

- [ ] **Step 3: Thread services through engine construction**

Update engine construction so `QueryEngine` receives services from `EngineFactory`, not from implicit global lookups.

Expected: root startup still installs old providers for compatibility, but the canonical engine path reads from `RuntimeServices`.

- [ ] **Step 4: Add isolation tests**

Create tests proving two engine instances can use different tool registries and permission resolvers in one process.

Run:

```bash
cargo test -p allthecodes-engine runtime_services
```

Expected: tests do not need global provider resets.

- [ ] **Step 5: Verify and commit**

Run:

```bash
cargo fmt --all --check
cargo test -p allthecodes-engine runtime_services
cargo check --workspace
```

Commit:

```bash
git add -A -- crates/allthecodes/src/main.rs crates/allthecodes/src/startup crates/allthecodes-engine/src crates/allthecodes-tools/src/registry.rs crates/allthecodes-permissions/src/decision.rs
git commit -m "refactor: inject runtime services explicitly"
```

---

## Task 4: 给工具系统增加 declarative metadata

**Files:**

- Create: `crates/allthecodes-tools/src/metadata.rs`
- Modify: `crates/allthecodes-tools/src/lib.rs`
- Modify: `crates/allthecodes-tools/src/tool.rs`
- Modify: `crates/allthecodes-tools/src/registry.rs`
- Modify: `crates/allthecodes-engine/src/lifecycle/deps/mod.rs`
- Modify: `crates/allthecodes-permissions/src/dangerous/auto_mode.rs`

**Interfaces:**

- Consumes: existing `Tool` trait and `ToolPolicy`
- Produces: `ToolMetadata`, `ToolCapabilities`, `ToolRisk`, `ToolConcurrency`, `ToolVisibility`

- [ ] **Step 1: Add metadata types**

Create:

```rust
pub struct ToolMetadata {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub capabilities: ToolCapabilities,
    pub risk: ToolRisk,
    pub concurrency: ToolConcurrency,
    pub visibility: ToolVisibility,
}

pub struct ToolCapabilities {
    pub read_files: bool,
    pub write_files: bool,
    pub run_processes: bool,
    pub spawn_agents: bool,
    pub use_network: bool,
    pub control_browser: bool,
    pub control_desktop: bool,
}
```

Expected: default metadata preserves current behavior for every tool before policies consume it.

- [ ] **Step 2: Add `metadata()` to `Tool` without splitting execution yet**

Add a default method:

```rust
fn metadata(&self) -> ToolMetadata {
    ToolMetadata::from_tool_name(self.name())
}
```

Expected: this is a compatibility step; existing tools compile without immediate edits.

- [ ] **Step 3: Convert hard-coded allowlists to metadata checks**

Replace string allowlist checks in:

- `crates/allthecodes-tools/src/registry.rs`
- `crates/allthecodes-engine/src/lifecycle/deps/mod.rs`
- `crates/allthecodes-permissions/src/dangerous/auto_mode.rs`

with capability/risk predicates:

```rust
metadata.capabilities.spawn_agents
metadata.risk <= ToolRisk::Low
metadata.visibility.allow_non_interactive
```

Expected: `"Bash"`, `"Agent"`, `"Read"` string comparisons remain only in compatibility tests and registry seed definitions.

- [ ] **Step 4: Add policy tests**

Run:

```bash
cargo test -p allthecodes-tools tool_metadata
cargo test -p allthecodes-engine auto_mode_allowlist
cargo test -p allthecodes-permissions auto_mode
```

Expected: new tests prove policy decisions come from metadata and retain old visible behavior.

- [ ] **Step 5: Verify and commit**

Run:

```bash
cargo fmt --all --check
cargo test -p allthecodes-tools
cargo test -p allthecodes-engine lifecycle::deps
cargo check --workspace
```

Commit:

```bash
git add -A -- crates/allthecodes-tools/src crates/allthecodes-engine/src/lifecycle/deps crates/allthecodes-permissions/src/dangerous/auto_mode.rs
git commit -m "refactor: add tool metadata policies"
```

---

## Task 5: 领域化 `QueryEngineState`

**Files:**

- Modify: `crates/allthecodes-engine/src/lifecycle/mod.rs`
- Create: `crates/allthecodes-engine/src/lifecycle/state.rs`
- Modify: `crates/allthecodes-engine/src/lifecycle/submit_message/mod.rs`
- Modify: `crates/allthecodes-engine/src/query/loop_impl.rs`
- Modify: `crates/allthecodes-engine/src/query/deps.rs`

**Interfaces:**

- Consumes: existing `Arc<RwLock<QueryEngineState>>`
- Produces: one outer lock holding typed sub-states

- [ ] **Step 1: Introduce sub-state structs without changing lock strategy**

Create:

```rust
pub(crate) struct TranscriptState { /* messages, usage, recorder */ }
pub(crate) struct PermissionState { /* denials, callbacks, auto trackers */ }
pub(crate) struct ToolRuntimeState { /* tools, file cache, discovered skills */ }
pub(crate) struct SessionRuntimeState { /* sleep, background agent, progress */ }
pub(crate) struct EngineSharedState {
    pub transcript: TranscriptState,
    pub permissions: PermissionState,
    pub tools: ToolRuntimeState,
    pub runtime: SessionRuntimeState,
    pub app_state: AppState,
}
```

Expected: one `RwLock<EngineSharedState>` remains; this task is about domain boundaries, not performance tuning.

- [ ] **Step 2: Move field access behind methods**

Add narrow methods for common mutations:

```rust
impl EngineSharedState {
    pub(crate) fn append_message(&mut self, message: Message);
    pub(crate) fn update_usage(&mut self, usage: &Usage, cost_usd: f64);
    pub(crate) fn record_permission_denial(&mut self, denial: PermissionDenial);
    pub(crate) fn set_tools(&mut self, tools: Tools);
}
```

Expected: callers stop reaching into unrelated state fields.

- [ ] **Step 3: Update submit and query callers**

Update `submit_message`, `QueryDeps`, and query loop code to use the sub-state API.

Expected: borrow scopes shrink; no new locks are introduced.

- [ ] **Step 4: Add state invariants tests**

Run:

```bash
cargo test -p allthecodes-engine engine_shared_state
```

Expected: tests cover message append, usage update, permission denial tracking, and tool runtime updates.

- [ ] **Step 5: Verify and commit**

Run:

```bash
cargo fmt --all --check
cargo test -p allthecodes-engine lifecycle
cargo check --workspace
```

Commit:

```bash
git add -A -- crates/allthecodes-engine/src/lifecycle crates/allthecodes-engine/src/query
git commit -m "refactor: domainize engine shared state"
```

---

## Task 6: Pipeline 化 tool execution

**Files:**

- Modify: `crates/allthecodes-engine/src/lifecycle/deps/execute.rs`
- Modify: `crates/allthecodes-engine/src/lifecycle/deps/permission.rs`
- Create: `crates/allthecodes-engine/src/lifecycle/deps/tool_pipeline.rs`
- Modify: `crates/allthecodes-engine/src/lifecycle/deps/mod.rs`
- Modify: `crates/allthecodes-engine/src/tool_runtime/execution/security.rs`

**Interfaces:**

- Consumes: existing `execute_tool_impl` canonical boundary
- Produces: `ToolExecutionPipeline`, `ToolExecutionPlan`, structured stage results

- [ ] **Step 1: Write baseline tests before moving logic**

Create tests covering current behavior:

```rust
#[test]
fn tool_execution_preserves_pre_hook_modified_input() { /* current behavior */ }

#[test]
fn tool_execution_denied_permission_does_not_call_tool() { /* current behavior */ }

#[test]
fn tool_execution_records_audit_after_success() { /* current behavior */ }
```

Run:

```bash
cargo test -p allthecodes-engine tool_execution
```

Expected: tests pass before refactor.

- [ ] **Step 2: Add pipeline stages**

Create stage methods:

```rust
validate_input()
sanitize_input()
security_validate()
run_pre_hooks()
resolve_permission()
maybe_prompt_user()
call_tool()
run_post_hooks()
record_and_audit()
```

Expected: each stage returns a typed result enum rather than ad hoc early-return data.

- [ ] **Step 3: Convert `execute_tool_impl` to orchestration**

Keep `execute_tool_impl` as the canonical entry point but reduce it to constructing context, invoking the pipeline, and returning the final tool result.

Expected: permission, hook, audit, security, and result handling each have their own test target.

- [ ] **Step 4: Verify failure paths**

Run:

```bash
cargo test -p allthecodes-engine tool_execution_pipeline
cargo test -p allthecodes-engine lifecycle::deps
```

Expected: tests cover validate failure, pre-hook deny, permission deny, interactive timeout, tool error, post-hook error, and audit emission.

- [ ] **Step 5: Verify and commit**

Run:

```bash
cargo fmt --all --check
cargo test -p allthecodes-engine tool_execution
cargo check --workspace
```

Commit:

```bash
git add -A -- crates/allthecodes-engine/src/lifecycle/deps crates/allthecodes-engine/src/tool_runtime/execution/security.rs
git commit -m "refactor: pipeline tool execution"
```

---

## Task 7: Pipeline 化 query/submit 生命周期

**Files:**

- Modify: `crates/allthecodes-engine/src/query/loop_impl.rs`
- Modify: `crates/allthecodes-engine/src/query/loop_helpers.rs`
- Create: `crates/allthecodes-engine/src/query/turn_state.rs`
- Create: `crates/allthecodes-engine/src/query/recovery.rs`
- Modify: `crates/allthecodes-engine/src/query/mod.rs`
- Modify: `crates/allthecodes-engine/src/lifecycle/submit_message/mod.rs`
- Create: `crates/allthecodes-engine/src/lifecycle/submit_message/transaction.rs`
- Modify: `crates/allthecodes-engine/src/lifecycle/submit_message/stream_handler.rs`

**Interfaces:**

- Consumes: `QueryDeps`, existing query loop stream behavior, existing submit side effects
- Produces: `QueryTurnState`, recovery modules, `SubmitTransaction`

- [ ] **Step 1: Add turn state tests**

Create state transition tests:

```rust
#[test]
fn query_turn_transitions_from_preparing_to_finished() { /* state path */ }

#[test]
fn query_turn_rejects_streaming_after_tool_execution() { /* invalid edge */ }

#[test]
fn query_turn_records_abort_during_tool_execution() { /* abort edge */ }
```

Run:

```bash
cargo test -p allthecodes-engine query_turn_state
```

Expected: state machine behavior is locked before it is wired into the main loop.

- [ ] **Step 2: Extract recovery strategies**

Move prompt-too-long, max-output-token, fallback-model retry, and stream-timeout behavior into `query/recovery.rs`.

Expected: `loop_impl.rs` calls recovery functions and no longer contains each full recovery branch inline.

- [ ] **Step 3: Add `SubmitTransaction`**

Create a transaction object for submit side effects:

```rust
pub(crate) struct SubmitTransaction {
    pub appended_messages: Vec<Message>,
    pub usage_delta: Option<Usage>,
    pub emitted_events: Vec<SdkMessage>,
    pub persistence: PersistencePlan,
}
```

Expected: `submit_message_with_overrides` appends, persists, updates usage/cost, emits events, and flushes stream through one object.

- [ ] **Step 4: Wire query events into submit transaction**

Let query produce typed turn events; let submit consume them and commit side effects.

Expected: query loop no longer directly owns unrelated submit persistence details.

- [ ] **Step 5: Verify and commit**

Run:

```bash
cargo fmt --all --check
cargo test -p allthecodes-engine query_turn_state
cargo test -p allthecodes-engine submit_transaction
cargo test -p allthecodes-engine lifecycle
cargo check --workspace
```

Commit:

```bash
git add -A -- crates/allthecodes-engine/src/query crates/allthecodes-engine/src/lifecycle/submit_message
git commit -m "refactor: split query submit pipeline"
```

---

## Task 8: 领域化配置映射

**Files:**

- Modify: `crates/allthecodes-config/src/runtime_settings.rs`
- Modify: `crates/allthecodes-config/src/settings/types.rs`
- Modify: `crates/allthecodes-config/src/settings/effective.rs`
- Modify: `crates/allthecodes-config/src/settings/raw.rs`
- Modify: `crates/allthecodes-config/src/settings/tests.rs`
- Modify: `crates/allthecodes-engine/src/types/app_state.rs`
- Modify: `crates/allthecodes-tools/src/tool.rs`
- Modify: `crates/allthecodes/src/startup/app_state_factory.rs`

**Interfaces:**

- Consumes: `RawSettings`, `EffectiveSettings`, `SettingsJson`, `AppState`, `ToolAppState`
- Produces: domain settings structs and roundtrip/projection tests

- [ ] **Step 1: Introduce domain settings structs**

Group settings by domain:

```rust
pub struct RuntimeSettings {
    pub core: CoreSettings,
    pub model: ModelSettings,
    pub permissions: PermissionSettings,
    pub sandbox: SandboxSettings,
    pub ui: UiSettings,
    pub memory: MemorySettings,
    pub network: NetworkSettings,
    pub speech: SpeechSettings,
    pub integrations: IntegrationSettings,
    pub sources: SourceMap,
}
```

Expected: JSON compatibility remains stable through serde flattening or explicit conversion.

- [ ] **Step 2: Replace manual AppState mapping with conversion methods**

Add conversions:

```rust
impl TryFrom<&EffectiveSettings> for AppState { /* preserve current fields */ }
impl From<&AppState> for ToolAppState { /* preserve current fields */ }
impl AppState {
    pub fn apply_tool_app_state(&mut self, state: ToolAppState) { /* existing behavior */ }
}
```

Expected: `full_init` and `app_state_factory` stop copying settings field by field.

- [ ] **Step 3: Add projection drift tests**

Add tests that assert critical fields survive:

```rust
#[test]
fn effective_settings_project_to_app_and_tool_state() { /* model, backend, permission mode, cwd, env */ }

#[test]
fn tool_app_state_roundtrip_preserves_mutable_fields() { /* file cache, settings, flags */ }
```

Run:

```bash
cargo test -p allthecodes-config settings_projection
cargo test -p allthecodes-engine app_state_projection
```

Expected: adding a new critical field requires updating a projection test.

- [ ] **Step 4: Verify and commit**

Run:

```bash
cargo fmt --all --check
cargo test -p allthecodes-config
cargo test -p allthecodes-engine app_state
cargo check --workspace
```

Commit:

```bash
git add -A -- crates/allthecodes-config/src crates/allthecodes-engine/src/types/app_state.rs crates/allthecodes-tools/src/tool.rs crates/allthecodes/src/startup/app_state_factory.rs
git commit -m "refactor: domainize runtime settings"
```

---

## Task 9: 明确 query loop 长期边界

**Files:**

- Modify: `README.md`
- Modify: `crates/allthecodes-engine/src/query/mod.rs`
- Modify: `crates/allthecodes-engine/Cargo.toml`
- Modify: `development/code-split/codebase-optimization-plan-2026-07-03.md`
- Create: `development/code-split/query-loop-boundary-decision-2026-07-03.md`

**Interfaces:**

- Consumes: current engine-internal query loop
- Produces: one recorded decision: keep query inside engine for now, or extract exactly once into canonical crate

- [ ] **Step 1: Record current state**

Document:

```text
`crates/allthecodes-engine/src/query/` is the only production query loop.
The previous independent `allthecodes-query` crate was removed because it duplicated behavior.
```

Expected: contributors can find the canonical path without inspecting workspace history.

- [ ] **Step 2: Define extraction criteria**

Record that query loop extraction is only allowed when:

- `QueryDeps` is split into stable capability traits.
- query loop no longer reaches engine-internal state directly.
- submit side effects are represented by typed events or `SubmitTransaction`.
- there is one production implementation after extraction.

Expected: future extraction cannot create a second implementation for tests or experiments.

- [ ] **Step 3: Verify and commit**

Run:

```bash
cargo fmt --all --check
cargo check --workspace
```

Commit:

```bash
git add -A -- README.md crates/allthecodes-engine/src/query/mod.rs crates/allthecodes-engine/Cargo.toml development/code-split/codebase-optimization-plan-2026-07-03.md development/code-split/query-loop-boundary-decision-2026-07-03.md
git commit -m "docs: record query loop boundary"
```

---

## Historical Execution Order

1. Task 1: quality gates and docs.
2. Task 2: split startup composition root.
3. Task 3: inject runtime services explicitly.
4. Task 4: add tool metadata policies.
5. Task 5: domainize engine shared state.
6. Task 6: pipeline tool execution.
7. Task 7: split query/submit pipeline.
8. Task 8: domainize runtime settings.
9. Task 9: record query loop boundary decision.

This order keeps behavior stable while reducing coupling. The first four tasks shrink the biggest architecture pressure points before touching the most fragile query/tool execution internals.

---

## Tracking Table

| ID | Area | Priority | Deliverable | Status | Evidence |
|---|---|---:|---|---|---|
| RB-001 | CI/docs | P0 | PR quality gate + query boundary docs + comment cleanup | done | `a268787a` |
| RB-002 | Startup | P0 | `run_full_init` becomes composition orchestration | done | `b78639f8`, `dfa63a74` |
| RB-003 | Runtime services | P0 | engine receives explicit services object | done | `7bc67cf1`, `ed0ab9ce` |
| RB-004 | Tools | P0 | metadata/capability policy replaces name strings | done / residual | `e61322b9`, `679d2a8b` |
| RB-005 | Engine state | P1 | `QueryEngineState` split into domain sub-states | done / residual | `0dd53b12` |
| RB-006 | Tool execution | P1 | canonical execution boundary becomes testable pipeline | done / residual | `95eddec6`, `9b278004` |
| RB-007 | Query/submit | P1 | query turn state + submit transaction | done / residual | `21ea2b0d`, `4f22e341`, `cf49004f` |
| RB-008 | Config | P1 | domain settings + projection drift tests | done | `aa3b7d92` |
| RB-009 | Query boundary | P2 | recorded long-term decision | done | `d40be644` |

---

## Definition of Done

- `cargo fmt --all --check` passes.
- `cargo clippy --workspace --all-targets -- -D warnings` passes, or any pre-existing blocker is recorded with exact file and diagnostic.
- `cargo test --workspace` passes, or any pre-existing blocker is recorded with exact file and diagnostic.
- `cargo build --workspace --release` passes before merge/push.
- No new global registry or process-wide callback path is introduced as the canonical path.
- No UI code re-parses tool JSON or shell commands for permission decisions.
- README, development docs, and crate comments agree on the active query loop boundary.
- Each completed task is committed separately with only explicitly related paths staged.

---

## Self-Review

- Spec coverage: the plan covers the requested conclusions, core layering, strengths to preserve, eight priority recommendations, and short/mid/long-term roadmap.
- Placeholder scan: no task relies on unspecified future detail; each task has concrete files, interfaces, commands, and exit conditions.
- Type consistency: `RuntimeServices`, `ToolMetadata`, `EngineSharedState`, `ToolExecutionPipeline`, `QueryTurnState`, and `SubmitTransaction` are introduced before later tasks consume them.
