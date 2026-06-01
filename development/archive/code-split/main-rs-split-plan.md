# main.rs 拆分计划

> 原文件: `crates/allthecodes/src/main.rs`
> 原始行数: ~1534
> 目标: 拆分为 5 个子模块 + main.rs 瘦身，每个模块 ≤ 300 行

---

## 1. 当前结构分析

### 1.1 文件内容分布

| 区间 | 行号 | 内容 | 行数 |
|------|------|------|------|
| 模块声明 | L1–L16 | `mod` 声明 + 文件头注释 | 16 |
| 导入语句 | L18–L47/L279–L287 | `use` 语句 (分两段合并) | 40 |
| 函数组 A | L49–L180 | 模型解析相关函数 (7 个) | 132 |
| 函数组 B | L182–L215 | Skill 日志报告函数 | 34 |
| Trait impls | L217–L278 | 4 个 trait impl + 2 个结构体 | 62 |
| 函数组 C | L289–L359 | Daemon 适配器函数 (5 个) | 71 |
| 入口函数 | L365–L521 | `fn main()` | 157 |
| 核心初始化 | L527–L1492 | `async fn run_full_init()` | 966 |
| 函数组 D | L1494–L1534 | Skill 命令注册/持久化 (2 个) | 41 |

### 1.2 当前模块树

```
crates/allthecodes/src/
├── main.rs                        (1534 行)  ← 目标文件
├── cli.rs                         (154 行)    Cli 结构体定义
├── classifier_model.rs            (99 行)
├── command_runtime_bridge.rs      (754 行)
├── dashboard.rs                   (365 行)
├── plan_workflow.rs               (87 行)
├── shutdown.rs                    (183 行)
├── app_runtime_adapters/
│   ├── mod.rs
│   ├── callbacks.rs
│   ├── ingress.rs
│   ├── query_runner.rs
│   └── sdk_mapper.rs
├── app_subsystem_handlers/
│   ├── mod.rs (re-exports)
│   ├── ide.rs / lsp.rs / mcp.rs / mcp_config.rs
│   ├── plugin.rs / skill.rs / snapshot.rs
├── ui/                            (数十个子模块)
├── tests/                         (测试相关)
└── ...其他
```

### 1.3 `run_full_init` 内部阶段分布

```
Phase B.1:   设置加载(cwd→settings→permission_mode)      L528–L599
Phase B.3:   插件/工具/技能初始化                        L601–L756
Phase B.3a-i: LSP 声明集成                              L610–L638
Phase B.3a-ii: 插件命令注册                             L642–L668
Phase B.3a-iii: 遥测初始化 (#[cfg(feature=telemetry)])  L672–L755
Phase B.3c:  技能初始化                                 L759–L783
Phase B.3d:  MCP 服务器发现与连接                       L787–L919
Phase B.3e:  Computer Use 工具                           L921–L929
Phase B.4:   AppState 构建 + 模型解析                   L933–L1096
Phase B.5:   仅初始化快速路径                            L1099–L1102
Phase B.6:   会话恢复                                    L1104–L1162
Phase B.7-B.8: QueryEngine 创建 + 配置                  L1164–L1247
Phase B.8a-b: SessionStart hook + Audit sink            L1254–L1307
Phase B.8.1:  ProcessState 初始化                       L1309–L1319
Phase B.9:   非交互输出模式 (json/print)                L1321–L1343
Phase B.10:  Web UI 模式                                L1345–L1359
Phase B.11-B.12: Daemon/TUI/Headless 模式               L1361–L1491
```

---

## 2. 拆分方案

### 2.1 拆分后模块树

```
crates/allthecodes/src/
├── main.rs                        (~200 行)   ← 仅保留 main() + 模块声明
├── cli.rs                         (154 行)    ← 不变
├── startup_model.rs               (~130 行)   ← 新增：模型解析函数组
├── startup_traits.rs              (~80 行)    ← 新增：trait impl + root 结构体
├── startup_skills.rs              (~90 行)    ← 新增：skill 相关函数
├── startup_daemon_adapters.rs     (~70 行)    ← 新增：daemon 适配器函数
├── full_init.rs                   (~970 行)   ← 新增：run_full_init + 子阶段函数
│   (内部包含 7 个私有阶段函数)
├── classifier_model.rs            (99 行)     ← 不变
├── command_runtime_bridge.rs      (754 行)    ← 不变
├── plan_workflow.rs               (87 行)     ← 不变
├── dashboard.rs                   (365 行)    ← 不变
├── shutdown.rs                    (183 行)    ← 不变
├── app_runtime_adapters/                      ← 不变
├── app_subsystem_handlers/                    ← 不变
├── ui/                                        ← 不变
└── tests/                                     ← 不变
```

### 2.2 新文件详细规格

---

#### 文件 1: `src/startup_model.rs` (~130 行)

**职责**: 命令行模型别名解析、可用模型校验、thinking/effort 提取。

**函数列表** (全部 `pub(crate)`):

| 函数签名 | 来源行 | 说明 |
|----------|--------|------|
| `resolve_startup_model(requested, provider_default, hardcoded_default, available, settings)` | L49–L89 | 主模型解析：CLI > provider > hardcoded > fallback |
| `resolve_startup_model_list_entry(entry, settings)` | L91–L103 | 解析 availableModels 单条 |
| `resolve_model_alias_for_effective_settings(name, settings)` | L105–L134 | SOTA/MOTA/FOTA 别名 + 常规别名解析 |
| `check_startup_available(model, available, settings)` | L164–L180 | 检查模型是否在可用列表 |
| `settings_thinking_enabled(settings)` | L136–L152 | 提取 thinking 启用状态 |
| `output_config_effort(output_config)` | L154–L158 | 从 output_config 提取 effort |
| `settings_effort_value(settings)` | L160–L162 | 综合 effort 值解析 |

**依赖**:
- `allthecodes_commands::model::{is_removed_legacy_model_alias, resolve_model_alias}`
- `allthecodes_models::replacement_for_removed_legacy_alias`
- `settings::EffectiveSettings`
- `serde_json::Value`
- `tracing::{warn, debug}`

**被依赖**: `main.rs` (当前) → 拆后改为 `full_init.rs` 调用

---

#### 文件 2: `src/startup_traits.rs` (~80 行)

**职责**: 为 `Cli` 实现抽取出的 trait (`StartupCli`, `DumpSystemPromptCli`)；定义 `RootDashboardEmitter` 和 `RootAgentToolRegistry` 结构体及其 trait 实现。

**项列表**:

| 项 | 来源行 | 可见性 | 说明 |
|----|--------|--------|------|
| `impl StartupCli for Cli` | L217–L229 | `impl` | 3 个方法: `cwd()`, `chrome()`, `no_chrome()` |
| `impl DumpSystemPromptCli for Cli` | L231–L243 | `impl` | 3 个方法: `model()`, `system_prompt()`, `append_system_prompt()` |
| `struct RootDashboardEmitter` | L245 | `pub(crate)` | 空结构体 |
| `impl DashboardEmitter for RootDashboardEmitter` | L247–L270 | `impl` | 代理到 `crate::dashboard::emit_subagent_event` |
| `struct RootAgentToolRegistry` | L272 | `pub(crate)` | 空结构体 |
| `impl AgentToolRegistry for RootAgentToolRegistry` | L274–L278 | `impl` | 代理到 `registry::get_all_tools()` |

**依赖**:
- `crate::cli::Cli`
- `crate::dashboard`
- `startup::runtime_config::StartupCli`
- `startup::fast_paths::DumpSystemPromptCli`
- `startup::tool_registry as registry`
- `allthecodes_engine::agent_runtime::{DashboardEmitter, AgentToolRegistry}`
- `allthecodes_engine::types::tool::Tool`
- `std::sync::Arc`

**被依赖**:
- `main.rs` — 原有内联创建 `RootDashboardEmitter` / `RootAgentToolRegistry`
- `full_init.rs` — 需要访问这两个结构体类型

---

#### 文件 3: `src/startup_skills.rs` (~90 行)

**职责**: Skill 加载报告记录、插件 Skill 发现、用户可调用 Skill 命令注册、Skill 使用数据持久化。

**函数列表** (全部 `pub(crate)`):

| 函数签名 | 来源行 | 说明 |
|----------|--------|------|
| `log_skill_report(scope, report)` | L182–L215 | 记录 SkillLoadReport 的诊断 |
| `discover_plugin_skills_for_root()` | L302–L332 | 加载插件贡献的 Skill 定义 |
| `register_user_invocable_skill_commands()` | L1494–L1523 | 注册/更新用户可调用的 Skill 为动态命令 |
| `persist_skill_usage()` | L1525–L1534 | 持久化 Skill 使用计数 |

**依赖**:
- `allthecodes_skills::{SkillDefinition, SkillLoadReport, SkillLoadOptions, ...}`
- `allthecodes_skills::loader::load_skill_from_file_path`
- `allthecodes_skills::{SkillSource, SkillDiagnosticSeverity}`
- `allthecodes_commands::dynamic_registry::{CommandSource, DynamicCommandEntry, ExecutionStrategy}`
- `allthecodes_plugins::{discover_plugin_skill_definitions, ...}`
- `allthecodes_config::paths`
- `tracing::{info, warn, debug}`

**被依赖**: `full_init.rs` 中的 B.3c 阶段

---

#### 文件 4: `src/startup_daemon_adapters.rs` (~70 行)

**职责**: Daemon 运行时适配器安装和相关的工厂函数。

**函数列表** (全部 `pub(crate)`):

| 函数签名 | 来源行 | 说明 |
|----------|--------|------|
| `install_daemon_runtime_adapters()` | L289–L300 | 设置 `DaemonRuntimeAdapters` |
| `daemon_command_dispatcher()` | L334–L336 | 创建 `DefaultCommandDispatcher` |
| `daemon_command_executor()` | L338–L340 | 创建 `EngineCommandExecutor` |
| `daemon_route_github_pr_activity(payload, event, delivery_id)` | L342–L359 | GitHub PR 活动路由 |

**依赖**:
- `allthecodes_daemon::runtime::{DaemonRuntimeAdapters, set_runtime_adapters, GithubPrActivityRouteOutcome}`
- `allthecodes_commands::{DefaultCommandDispatcher, EngineCommandExecutor, get_all_commands}`
- `allthecodes_teams::pr_activity::{parse_github_pr_activity, route_github_pr_activity}`
- `allthecodes_plugins::init_plugins`
- `registry` (= `startup::tool_registry`)
- `std::sync::Arc`
- `serde_json::Value`

**被依赖**: `main.rs` (L471) 中 `main()` 调用 `install_daemon_runtime_adapters()`

---

#### 文件 5: `src/full_init.rs` (~970 行)

**职责**: Phase B 完整初始化流程 — 承载 `run_full_init` 函数以及拆出的子阶段私有函数。

**公共项**:
- `pub(crate) async fn run_full_init(cli: Cli) -> anyhow::Result<ExitCode>` — 保持签名不变

**推荐的私有阶段函数** (在 `full_init.rs` 内，不暴露到模块外):

| 阶段函数 | 对应原阶段 | 预计行数 | 说明 |
|----------|-----------|---------|------|
| `init_working_directory(cli, cwd)` | B.1 前半 (L530–L540) | ~15 | --cwd 切换工作目录 |
| `handle_first_run()` | B.1 后半 (L543–L557) | ~20 | 首次运行初始化 |
| `load_and_merge_settings(cwd, cli)` | B.1–B.2 (L560–L599) | ~50 | 加载设置、env、权限模式 |
| `init_plugins_lsp_telemetry(cwd)` | B.3a (L601–L755) | ~160 | 插件初始化、LSP 声明、遥测 |
| `init_skills_and_chrome(cwd, tools)` | B.3c (L759–L797) | ~50 | 技能 + Chrome 子系统 |
| `init_mcp_servers(cwd, chrome_wanted, tools)` | B.3d (L799–L919) | ~130 | MCP 发现、连接、浏览器检测 |
| `resolve_detected_client(backend)` | B.4 前半 (L935–L963) | ~35 | API 客户端创建 |
| `resolve_model_with_settings(cli, merged_config, detected_client, backend)` | B.4 后半 (L966–L1002) | ~45 | 模型确定（含 fallback） |
| `handle_session_resume(cli, cwd)` | B.6 (L1104–L1162) | ~65 | 会话恢复逻辑 |
| `build_engine_config(cwd, cli, tools, ...)` | B.7 (L1170–L1193) | ~25 | QueryEngineConfig 构建 |
| `configure_engine(engine, ...)` | B.8–B.8.1 (L1195–L1319) | ~130 | 引擎配置 + hook + audit + process_state |
| `run_output_mode(engine, cli)` | B.9 (L1321–L1343) | ~25 | JSON/Print 输出模式 |
| `run_daemon_mode(engine, cli, cwd)` | B.11–B.12 (L1369–L1447) | ~85 | Daemon 模式 |
| `run_tui_mode(engine, cli, model)` | B.12 后半 (L1449–L1491) | ~45 | TUI/Headless 模式 |

**注意**: 这些阶段函数创建为私有(非 `pub`)函数，仅在 `full_init.rs` 内部被 `run_full_init` 调用。它们不是独立的模块间接口，因此不需要文档化签名到外部。上述列表仅为 `run_full_init` 重构的指导，不影响外部调用者。

**依赖** (按功能分组):

```rust
// 标准库
use std::process::ExitCode;
use std::sync::Arc;
use std::path::Path;
use std::collections::{HashSet, HashMap};

// CLI
use clap::Parser;
use crate::cli::Cli;

// 启动支持 (allthecodes_startup)
use allthecodes_startup as startup;
use startup::runtime_config::{
    build_tool_permission_context, chrome_cli_override, resolve_cwd, resolve_permission_mode, StartupCli,
};
use startup::tool_registry as registry;
use startup::modes;        // run_json_mode, run_print_mode

// 外部 crate
use allthecodes_web as web;
use allthecodes_config::settings;
use allthecodes_engine::lifecycle::QueryEngine;
use allthecodes_engine::types::app_state::{AppState, SettingsJson};
use allthecodes_engine::types::config::QueryEngineConfig;
use allthecodes_engine::mcp_tool_adapter::mcp_tools_to_tools;
use allthecodes_mcp::discovery::discover_mcp_servers;
use allthecodes_mcp::manager::McpManager;
use allthecodes_browser::session::{ChromeSession, ChromeEnablement};
use allthecodes_browser::detection;
use allthecodes_api::api::client::ApiClient;
use allthecodes_engine::codex_exec;
use allthecodes_session::resume;
use allthecodes_teams::reconnection;
use allthecodes_types::message::Message;
use allthecodes_observability::{AuditConfig, AuditContext, AuditSink, ...};
use allthecodes_tools::hooks::ShellHookRunner;
use allthecodes_tools::runtime::tool_search;
use allthecodes_types::hooks;
use allthecodes_bootstrap;
use allthecodes_utils::git;
use allthecodes_plugins;
use allthecodes_lsp_service;
use allthecodes_services::langfuse;
use allthecodes_services::telemetry;        // cfg(feature = "telemetry")
use allthecodes_engine::telemetry_bridge;   // cfg(feature = "telemetry")
use allthecodes_engine::agent_runtime;
use allthecodes_engine::agent::fork;
use allthecodes_commands;
use allthecodes_commands::dynamic_registry;
use allthecodes_daemon;
use allthecodes_ipc::headless;
use allthecodes_safety::classifier;

// error/logging
use anyhow::Context;
use serde_json::Value;
use tracing::{debug, error, info, warn};

// 统一窗口
use crate::ui::tui;
use crate::dashboard;
use crate::app_runtime_adapters;
use crate::classifier_model;
use crate::startup_model;          // 新增 — 模型解析
use crate::startup_skills;         // 新增 — skill 函数
use crate::startup_daemon_adapters; // 新增 — daemon 适配器
```

---

### 2.3 瘦身后的 `main.rs` (~200 行)

**保留内容**:
- `mod` 声明 (约 12 行，新增 4 个模块声明)
- `use` 语句 (约 20 行)
- `fn main() -> ExitCode` (全量保留，~157 行)
- `install_daemon_runtime_adapters()` 调用保留在 main() 中

**移除内容**:
- `resolve_startup_model` 等 7 个函数 → `startup_model.rs`
- `log_skill_report` → `startup_skills.rs`
- `impl StartupCli for Cli` 等 4 个 trait impl → `startup_traits.rs`
- `RootDashboardEmitter` / `RootAgentToolRegistry` → `startup_traits.rs`
- `install_daemon_runtime_adapters` 等 5 个函数 → `startup_daemon_adapters.rs`
- `discover_plugin_skills_for_root` → `startup_skills.rs`
- `register_user_invocable_skill_commands` → `startup_skills.rs`
- `persist_skill_usage` → `startup_skills.rs`
- `async fn run_full_init` → `full_init.rs`

**最终 `main.rs` 内容概览**:
```rust
// Module declarations
mod app_runtime_adapters;
mod app_subsystem_handlers;
mod classifier_model;
mod cli;
mod command_runtime_bridge;
mod ui;
mod plan_workflow;
mod shutdown;
mod dashboard;

// NEW modules
mod startup_model;
mod startup_traits;          // 注意: pub(crate) 项给 full_init.rs 使用
mod startup_skills;
mod startup_daemon_adapters;
mod full_init;

// use statements
use std::process::ExitCode;
use clap::Parser;
use tracing::{error, info};
use allthecodes_startup as startup;
use crate::cli::Cli;
use startup::runtime_config::{resolve_cwd, StartupCli};
use crate::startup_daemon_adapters::install_daemon_runtime_adapters;

fn main() -> ExitCode {
    startup::load_env_files();
    allthecodes_tools::registry::install_tool_registry_providers(
        registry::root_tool_registry_providers(),
    );
    startup::engine_runtime::install(
        Arc::new(crate::startup_traits::RootDashboardEmitter),
        Arc::new(crate::startup_traits::RootAgentToolRegistry),
    );
    // ... Computer Use / Browser 权限回调注册 ...
    let cli = Cli::parse();
    // ... fast paths ...
    rt.block_on(async {
        match crate::full_init::run_full_init(cli).await { ... }
    });
}
```

---

## 3. 依赖关系分析

### 3.1 模块依赖图

```
main.rs
  ├── startup_daemon_adapters.rs  (仅 main() 调用)
  ├── startup_traits.rs           (main() 使用 RootDashboardEmitter/AgentToolRegistry)
  ├── startup_model.rs            (full_init.rs 调用)
  ├── startup_skills.rs           (full_init.rs 调用)
  ├── full_init.rs                (main() 调用 run_full_init)
  │     ├── startup_model.rs
  │     ├── startup_skills.rs
  │     ├── command_runtime_bridge.rs
  │     ├── dashboard.rs
  │     ├── classifier_model.rs
  │     ├── plan_workflow.rs
  │     ├── shutdown.rs
  │     ├── app_runtime_adapters/
  │     ├── ui/
  │     └── (所有外部 allthecodes-* crate)
  ├── cli.rs                      (startup_traits.rs 使用 Cli)
  ├── dashboard.rs
  ├── command_runtime_bridge.rs
  ├── classifier_model.rs
  ├── plan_workflow.rs
  ├── shutdown.rs
  ├── app_runtime_adapters/
  ├── app_subsystem_handlers/
  └── ui/
```

**关键依赖流**: 所有新模块都是叶子节点 — 它们被 main.rs 或 full_init.rs 导入，但不会反过来导入 main.rs。不存在循环依赖。

**`startup_traits.rs` 对 `crate::cli::Cli` 的依赖**: `Cli` 定义在 `cli.rs`，`startup_traits.rs` 通过 `use crate::cli::Cli` 引用。这是标准的下级模块引用上级模块的模式，Rust 模块系统原生支持，无编译问题。

### 3.2 可见性要求

| 项 | 当前可见性 | 拆分后可见性 |
|---|-----------|-------------|
| `resolve_startup_model` | 私有 | `pub(crate)` — 被 `full_init.rs` 调用 |
| 其他模型函数 | 私有 | `pub(crate)` — 被 `full_init.rs` 调用 |
| `RootDashboardEmitter` | 私有 | `pub(crate)` — 被 `main.rs` 创建 |
| `RootAgentToolRegistry` | 私有 | `pub(crate)` — 被 `main.rs` 创建 |
| `impl StartupCli for Cli` | 私有 | `pub(crate)` — 被 `startup::runtime_config` 通过 trait 访问 |
| `impl DumpSystemPromptCli for Cli` | 私有 | `pub(crate)` |
| `install_daemon_runtime_adapters` | 私有 | `pub(crate)` — 被 `main.rs` 调用 |
| 其他 daemon 函数 | 私有 | `pub(crate)` |
| `log_skill_report` | 私有 | `pub(crate)` — 被 `full_init.rs` 调用 |
| `discover_plugin_skills_for_root` | 私有 | `pub(crate)` |
| `register_user_invocable_skill_commands` | 私有 | `pub(crate)` |
| `persist_skill_usage` | 私有 | `pub(crate)` |
| `run_full_init` | 私有 | `pub(crate)` — 被 `main.rs` 调用 |

### 3.3 `#[cfg]` 条件编译

`#[cfg(feature = "telemetry")]` 块 (L672–L755) 完全位于 `run_full_init` 内部。拆分到 `full_init.rs` 后，条件编译块保持不变，不影响编译。

---

## 4. 迁移步骤

步骤顺序设计为每步完成后 `cargo check` 和 `cargo test` 通过。

### 步骤 0: 前置准备

```bash
mkdir -p crates/allthecodes/src
# 确认当前 main.rs 完整可编译
cargo check -p allthecodes
cargo test -p allthecodes
```

### 步骤 1: 创建 `startup_model.rs`

1. 复制 `main.rs` 的 L49–L180 到 `crates/allthecodes/src/startup_model.rs`
2. 所有函数改为 `pub(crate)` 可见性
3. 添加独立 `use` 语句：
   ```rust
   use allthecodes_commands::model::{is_removed_legacy_model_alias, resolve_model_alias};
   use allthecodes_models::replacement_for_removed_legacy_alias;
   use allthecodes_config::settings;
   use serde_json::Value;
   use tracing::{warn, debug};
   ```
4. 在 `main.rs` 中添加 `mod startup_model;`，保留 `use crate::startup_model::*;`
5. 运行 `cargo check` 确认编译通过，删除 `main.rs` 中被移出的 L49–L180
6. **风险检查**: 确认 `full_init.rs`（步骤 5 创建）中对模型函数的调用路径已更新

### 步骤 2: 创建 `startup_traits.rs`

1. 复制 `main.rs` 的 L217–L278 到 `crates/allthecodes/src/startup_traits.rs`
2. 添加独立 `use` 语句：
   ```rust
   use std::sync::Arc;
   use crate::cli::Cli;
   use crate::dashboard;
   use allthecodes_startup as startup;
   use startup::runtime_config::StartupCli;
   use startup::fast_paths::DumpSystemPromptCli;
   use startup::tool_registry as registry;
   use allthecodes_engine::agent_runtime::{DashboardEmitter, AgentToolRegistry};
   use allthecodes_engine::types::tool::Tool;
   use serde_json::Value;
   ```
3. 在 `main.rs` 中添加 `mod startup_traits;`
4. 删除 `main.rs` 中的 L217–L278
5. `impl StartupCli for Cli` 的 trait 实现移动到 `startup_traits.rs` 后，需要在 `main.rs` 中确认 `startup::runtime_config::StartupCli` 的 trait 可用性。由于 trait 和 impl 同时移入 `startup_traits.rs`，而 `Cli` 来自 `crate::cli::Cli`，外部调用方（如 `startup::runtime_config`）通过 `use crate::startup_traits::StartupCli as _` 看不见 impl。需要：
   - 在 `startup_traits.rs` 中使用 `impl super::cli::Cli` 并确保 trait 在作用域内
   - 或在 `main.rs` 的 `mod startup_traits;` 之后添加 `use startup_traits::*;` 确保 impl 可见
6. **风险检查**: `impl StartupCli for Cli` 的 trait bound 必须在 `main()` 函数（及 future `full_init.rs`）中可见。若 `main()` 中调用 `startup::runtime_config::resolve_cwd(&cli)` 需要 `StartupCli` trait，必须在调用点使用 `use crate::startup_traits::StartupCli as _;` 或通过 `use startup_traits::*;` 导入。

### 步骤 3: 创建 `startup_skills.rs`

1. 复制以下函数到 `crates/allthecodes/src/startup_skills.rs`：
   - `log_skill_report()` (L182–L215)
   - `discover_plugin_skills_for_root()` (L302–L332)
   - `register_user_invocable_skill_commands()` (L1494–L1523)
   - `persist_skill_usage()` (L1525–L1534)
2. 添加独立 `use` 语句
3. 在 `main.rs` 中添加 `mod startup_skills;`
4. 删除原位置代码

### 步骤 4: 创建 `startup_daemon_adapters.rs`

1. 复制 L289–L359 到 `crates/allthecodes/src/startup_daemon_adapters.rs`
2. 所有函数标记 `pub(crate)`
3. 添加独立 `use` 语句
4. 在 `main.rs` 中添加 `mod startup_daemon_adapters;`
5. 删除原位置代码
6. 将 `main()` 中的 `install_daemon_runtime_adapters()` 调用改为 `crate::startup_daemon_adapters::install_daemon_runtime_adapters()`

### 步骤 5: 创建 `full_init.rs`

1. 复制 `run_full_init` 函数 (L527–L1492) 到 `crates/allthecodes/src/full_init.rs`
2. 添加完整的 `use` 语句块（参见 2.2 文件 5 依赖部分）
3. 替换内联调用：
   - `startup_model::resolve_startup_model(...)`
   - `startup_model::resolve_model_alias_for_effective_settings(...)`
   - `startup_model::settings_thinking_enabled(...)`
   - `startup_model::settings_effort_value(...)`
   - `startup_model::check_startup_available(...)`
   - `startup_skills::log_skill_report(...)`
   - `startup_skills::discover_plugin_skills_for_root()`
   - `startup_skills::register_user_invocable_skill_commands()`
   - `startup_daemon_adapters::daemon_command_dispatcher()`
   - `startup_daemon_adapters::daemon_command_executor()`
4. 将函数标记为 `pub(crate)`
5. 在 `main.rs` 中添加 `mod full_init;`
6. 删除 `main.rs` 中的 L527–L1492
7. 将 `main.rs` 中的 `run_full_init(cli).await` 改为 `crate::full_init::run_full_init(cli).await`

### 步骤 6: 提取 `run_full_init` 子阶段函数 (可选优化)

此步骤为推荐但不强制。创建 `full_init.rs` 内部的私有辅助函数：

1. 提取 `init_working_directory` (B.1 前半)
2. 提取 `handle_first_run` (B.1 后半)
3. 提取 `load_and_merge_settings` (B.1–B.2)
4. 提取 `init_plugins_lsp_telemetry` (B.3a)
5. 提取 `init_skills_and_chrome` (B.3c)
6. 提取 `init_mcp_servers` (B.3d)
7. 提取 `resolve_detected_client` 和 `resolve_model_with_settings`
8. 提取 `handle_session_resume`
9. 提取 `build_engine_config` 和 `configure_engine`
10. 提取 `run_output_mode` / `run_daemon_mode` / `run_tui_mode`

每个阶段函数提取后，`run_full_init` 降为按阶段调用子函数的 ~150 行骨架。

### 步骤 7: 清理 `main.rs` 冗余

1. 删除不再需要的 `use` 语句（已移到各子模块的导入）
2. 确认 `main.rs` 的 `mod` 声明和 `main()` 正确引用新模块
3. 格式化和 lint：

```bash
cargo fmt -p allthecodes
cargo clippy -p allthecodes
cargo check -p allthecodes
cargo test -p allthecodes
```

---

## 5. 风险与注意事项

### 5.1 Trait impl 可见性风险 (高)

**风险**: `impl StartupCli for Cli` 和 `impl DumpSystemPromptCli for Cli` 被移到 `startup_traits.rs` 后，这些 trait 的 impl 在 `main.rs` / `full_init.rs` 中不再自动可见。如果 `startup::runtime_config::resolve_cwd()` 要求 `StartupCli` trait bound，而调用处没有导入该 trait，将导致编译错误。

**缓解方案**: 
- 在 `full_init.rs` 顶部添加 `use crate::startup_traits::StartupCli as _;` (使用 `as _` 导入 trait 但不引入命名空间)
- 或在 `startup_traits.rs` 中 `pub use startup::runtime_config::StartupCli;` 导出 trait 本身，然后在 `full_init.rs` 中使用 `use crate::startup_traits::StartupCli as _;`

### 5.2 `use allthecodes_startup as startup` 与 `mod startup_model` 命名冲突

**风险**: 当前 `use allthecodes_startup as startup` 将外部 crate 别名为 `startup`。创建 `mod startup_model` 不会冲突，但需要注意不能创建名为 `mod startup` 的模块。

**缓解方案**: 本计划创建 `startup_model`、`startup_traits` 等，均以 `startup_` 为前缀而不是命名为 `startup`，无冲突。

### 5.3 `RootDashboardEmitter` 和 `RootAgentToolRegistry` 的创建

**风险**: 这两个结构体当前在 `main()` 中直接构造 (`Arc::new(RootDashboardEmitter)`)。移到 `startup_traits.rs` 后需要导入路径变更。

**缓解方案**: 在 `main.rs` 中使用 `crate::startup_traits::RootDashboardEmitter` 全限定路径，或通过 `use startup_traits::*;` 导入。

### 5.4 `main()` 中的 `#[cfg(feature = "telemetry")]` 引用

**风险**: L672–L755 的 telemetry 代码块在 `main()` 的调用位置 (`rt.block_on`) 之外。拆分到 `full_init.rs` 后，feature gate 随函数移动，编译隔离性不变。

**缓解方案**: 无。`cfg` 属性直接包裹在 `full_init.rs` 的函数体内，不会导致编译条件丢失。

### 5.5 全局副作用初始化顺序

**风险**: `main()` 在调用 `run_full_init` 前执行了：
1. `startup::load_env_files()`
2. `allthecodes_tools::registry::install_tool_registry_providers()`
3. `startup::engine_runtime::install()`
4. Computer Use / Browser 权限回调注册

这些必须在 `run_full_init` 之前执行。拆分后仍然保留在 `main()` 中，不影响。

**缓解方案**: 无变动，顺序保持不变。

### 5.6 `crate::ui::tui` 交叉依赖

**风险**: `run_full_init` 中创建 `crate::ui::tui::run_tui(engine, initial_prompt, &model, shutdown_token)`。`full_init.rs` 需要在模块级导入 `crate::ui::tui`。

**缓解方案**: 在 `full_init.rs` 中添加 `use crate::ui::tui;`。

### 5.7 Git blame 追踪

- `main.rs` 内容大幅减少，git blame 对保留行仍准确
- 5 个新文件的首次 commit 显示为拆分操作
- 推荐使用 `git mv` 方式处理如果后续还需拆分（当前 plan 不需要）

### 5.8 测试兼容性

**风险**: 当前 `main.rs` 有少量内联测试（dashboard.rs 有测试，但 main.rs 本身无 `#[cfg(test)] mod tests`）。拆分后 `full_init.rs` 可能需要添加测试覆盖新提取的阶段函数。

**缓解方案**: 初始拆分保持 `run_full_init` 签名不变，测试通过外部集成/CI 覆盖。后续可单独为子阶段函数编写单元测试。

---

## 6. 统计汇总

| 拆分前后 | 文件 | 行数 | 变化 |
|---------|------|------|------|
| 拆分前 | `main.rs` | ~1534 | — |
| 拆分后 | `main.rs` | ~200 | -1334 行 |
| 拆分后 | `startup_model.rs` | ~130 | 新增 |
| 拆分后 | `startup_traits.rs` | ~80 | 新增 |
| 拆分后 | `startup_skills.rs` | ~90 | 新增 |
| 拆分后 | `startup_daemon_adapters.rs` | ~70 | 新增 |
| 拆分后 | `full_init.rs` | ~970 | 新增 (含子阶段提取后可降至 ~200) |
| **拆分后合计** | **6 文件** | **~1540** | **总行数基本不变** |

| 指标 | 值 |
|------|----|
| 新文件数 | 5 |
| 保留/不变文件数 | 8 (cli, classifier_model, command_runtime_bridge, plan_workflow, dashboard, shutdown + app_runtime_adapters, app_subsystem_handlers) |
| 预计编译时间变化 | 无显著变化 (同一 crate 内) |
| 迁移步骤数 | 7 步 |
| 主要风险点 | 2 (trait impl 可见性, 导入路径变更) |

---

*计划版本: 1.0*
*生成日期: 2026-05-30*
*对应文件: crates/allthecodes/src/main.rs*
