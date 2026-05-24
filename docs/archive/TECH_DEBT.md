# cc-rust 技术债务归档

> 更新日期: 2026-05-08
>
> 本文件保存从 [../TECH_DEBT.md](../TECH_DEBT.md) 移出的历史技术债：已实现、已勘误，或已被当前代码结构超越的条目。仍活跃的债务只保留在主文档中。

---

## 2026-05-07 归档批次

### 结构拆分已完成

| 原条目 | 归档状态 | 当前证据 |
| --- | --- | --- |
| `query/loop_impl.rs` 1105 行巨型异步生成器 | 已实现 | 已拆为 `query/loop_impl.rs`、`query/loop_helpers.rs`、`query/loop_tests.rs`，并新增 `query/turn_context.rs` 承载 turn context。 |
| `engine/lifecycle.rs` 1703 行上帝文件 | 已实现 | 已拆为 `engine/lifecycle/{mod,submit_message,deps,helpers,types,tests}.rs`。 |
| `mcp/client.rs` 1008 行混合多职责 | 已实现 / 已过期 | MCP runtime 现位于 `mcp/{mod,runtime,tools}.rs`，历史 `client.rs` 大文件已不再是当前结构。 |
| QueryEngine 的 10 个独立 `Arc<Mutex/RwLock>` 字段 | 已实现 | `engine/lifecycle/mod.rs` 已有 `QueryEngineState`，QueryEngine 通过单个 `Arc<RwLock<QueryEngineState>>` 管理可变 session state。 |

### API 抽象与重试重复已收敛

| 原条目 | 归档状态 | 当前证据 |
| --- | --- | --- |
| API 重试逻辑代码重复约 90 行 | 已实现 / 已过期 | `api/retry.rs` 已有 `RetryConfig`、`ApiErrorCategory`、`categorize_api_error()` 和 `categorize_stream_start_error()`。剩余风险改为“字符串错误分类仍脆弱”，留在主文档。 |
| API provider 抽象不足 | 部分已实现 | `api/stream_provider.rs` 已提供 `StreamProvider` trait，Anthropic/OpenAI-compatible/Google streaming dispatch 走 trait。provider 内部转换仍分散，因此主文档保留新的、更窄条目。 |
| 模型别名硬编码重复 | 部分已实现 | 命令侧别名已集中到 `model_registry.rs`。展示名和 cutoff 仍在 `cc-config::constants`，因此主文档保留“模型元数据未完全统一”。 |

### 安全性与 unwrap 清理批次已完成

原始审计把命令处理器和 API 客户端中的 `panic!()` 归类为生产代码问题；后续核对发现这些 panic 主要是 `#[cfg(test)]` 测试断言，不再作为生产 CRITICAL 债务保留。真正修复的是多个生产路径裸 `.unwrap()` / 运行时正则编译问题。

| 修复 | 影响文件 | 变更 |
| --- | --- | --- |
| 运行时正则编译 | `crates/cc-utils/src/bash.rs` | 多个 heredoc / redirect / multiline regex 已改为 `static LazyLock<Regex>`。 |
| `engine/lifecycle.rs` 裸 `.unwrap()` | `engine/lifecycle.rs` 历史结构 | 已随 lifecycle 模块拆分和 expect/message 化清理。 |
| `utils/cwd.rs` 裸 `.lock().unwrap()` | `utils/cwd.rs` 历史结构 | 替换为带上下文的 expect 或迁移到当前 crate 结构。 |
| `utils/abort.rs` 裸 `.lock().unwrap()` | `utils/abort.rs` 历史结构 | 替换为带上下文的 expect 或迁移到当前 crate 结构。 |
| `tools/tasks.rs` 裸 `.lock().unwrap()` | `tools/tasks.rs` 历史结构 | 替换为带上下文的 expect / 当前 task store 结构。 |
| `query/token_budget.rs` unsafe unwrap 模式 | `query/token_budget.rs` | `budget.unwrap()` 已改为安全解构。 |
| `services/prompt_suggestion.rs` NaN 风险 | `services/prompt_suggestion.rs` | `partial_cmp().unwrap()` 已改为 fallback 排序。 |
| `session/audit_export.rs` `last().unwrap()` | `session/audit_export.rs` | 已改为 optional fallback。 |
| `tools/config_tool.rs` `.as_object_mut().unwrap()` | `tools/config_tool.rs` | 已改为带前置保证说明的 expect。 |
| `tools/worktree.rs` `session.unwrap()` | `tools/worktree.rs` | 已改为带前置保证说明的 expect。 |

### 可维护性批次已完成

| 修复 | 原始状态 | 归档说明 |
| --- | --- | --- |
| `engine/lifecycle.rs` 模块化 | 原 1703 行单文件 | 已拆到 `engine/lifecycle/` 目录模块。后续只追踪具体剩余长流程，不再追踪旧单文件问题。 |
| `query/loop_impl.rs` 拆分 | 原 1105 行单文件 | 已拆分核心循环、helpers、tests 和 turn context。后续只追踪 inner stream / result helper 等剩余点。 |
| `mcp/client.rs` 拆分 | 原 1008 行单文件 | 当前 MCP runtime 已不是旧 client 大文件结构。 |

### 架构改善批次已部分完成

| 任务 | 状态 | 说明 |
| --- | --- | --- |
| 合并 QueryEngine 多个 Arc 字段 | 已实现 | 已合并为 `QueryEngineState`。 |
| 提取重试逻辑到统一策略 | 已实现 / 名称不同 | 当前实现名为 `RetryConfig` + `ApiErrorCategory`，不是原建议的 `RetryPolicy`。 |
| 重构 API 提供商为 trait 抽象 | 已实现主干 | 当前实现名为 `StreamProvider`；provider 内部转换仍是活跃债务。 |

---

## 2026-04-08 原始审计快照

原始报告标题为 “cc-rust 技术债务审计报告”，审计日期 2026-04-08，分支历史名 `rust-lite`。该报告中的严重程度统计已经不再代表当前状态，仅保留作历史背景：

| 等级 | 原总问题数 | 原已修复 | 原剩余 | 备注 |
| --- | --- | --- | --- | --- |
| CRITICAL | 4 | 2 | 2 | “生产代码 panic” 后续已勘误为测试断言，不再按 CRITICAL 保留。 |
| HIGH | 5 | 1 | 4 | `mcp/client.rs`、QueryEngine 多锁、重试重复、provider trait 主干已被当前结构覆盖。 |
| MEDIUM | 11 | 1 | 10 | 运行时正则已修；allow/dead_code、test helper、输入解析、协议版本等仍在主文档。 |

---

## 归档维护规则

1. 新完成的技术债从 `docs/TECH_DEBT.md` 移入本文件时，需要附上完成证据。
2. 如果条目只是误报或被后续架构替代，标记为“已勘误”或“已过期”。
3. 本文件不承载新的 TODO；发现仍未完成的内容应移回主文档。
## P1 fail-fast phases 2-6 completed

These entries were moved out of the active P1 fail-fast debt after the recorded task runs under `target/codex-runs/p1-defensive-after-phase1/*.last-message.txt` reported passing targeted tests and `cargo check -p claude-code-rs --message-format short`.

| Phase | Archived status | Changed files | Verification |
| --- | --- | --- | --- |
| Phase 2 - Hook criticality and fail-closed policy | Completed. Existing hooks remain optional by default, critical pre-tool hooks fail the current tool step, critical post/failure/stop hooks surface errors, and hook IO failures are traced. | `crates/cc-types/src/hooks.rs`; `crates/claude-code-rs/src/tools/hooks/mod.rs`; `crates/claude-code-rs/src/tools/hooks/pre_tool.rs`; `crates/claude-code-rs/src/tools/hooks/post_tool.rs`; `crates/claude-code-rs/src/tools/hooks/execution.rs`; `crates/claude-code-rs/src/engine/lifecycle/deps.rs`; `crates/claude-code-rs/src/query/loop_tests.rs` | `cargo test -p claude-code-rs tools::hooks -- --nocapture`; `cargo test -p claude-code-rs engine::lifecycle -- --nocapture`; `cargo check -p claude-code-rs --message-format short` |
| Phase 3 - Single canonical tool execution path | Completed. The legacy duplicate orchestration module was deleted and production execution now uses the canonical `QueryDeps::execute_tool` path. | `crates/claude-code-rs/src/tools/mod.rs`; deleted `crates/claude-code-rs/src/tools/orchestration.rs`; `crates/claude-code-rs/src/tools/hooks/mod.rs`; `crates/claude-code-rs/src/tools/hooks/post_tool.rs` | `cargo test -p claude-code-rs tools::execution -- --nocapture`; `cargo test -p claude-code-rs query::loop_helpers -- --nocapture`; `cargo test -p claude-code-rs tools::hooks -- --nocapture`; `cargo check -p claude-code-rs --message-format short` |
| Phase 4 - Existing config/data must not masquerade as missing | Completed. Existing invalid MCP/plugin/config/env/permission/auth state now produces diagnostics or operation errors instead of empty/default/no-auth behavior where the phase touched call sites. | `crates/cc-mcp/src/discovery.rs`; `crates/cc-auth/src/lib.rs`; `crates/cc-auth/src/codex_cli.rs`; `crates/cc-types/src/permissions.rs`; `crates/claude-code-rs/src/main.rs`; `crates/claude-code-rs/src/engine/system_prompt.rs`; `crates/claude-code-rs/src/ipc/subsystem_handlers.rs`; `crates/claude-code-rs/src/plugins/loader.rs`; `crates/claude-code-rs/src/plugins/mod.rs`; `crates/claude-code-rs/src/plugins/refresh.rs`; `crates/claude-code-rs/src/commands/memory.rs`; `crates/claude-code-rs/src/commands/model_add.rs`; `crates/claude-code-rs/src/commands/doctor.rs`; `crates/claude-code-rs/src/commands/login.rs`; `crates/claude-code-rs/src/commands/logout.rs`; `crates/claude-code-rs/src/commands/voice_cmd.rs`; `crates/claude-code-rs/src/startup/mod.rs`; `crates/claude-code-rs/src/startup/runtime_config.rs`; `crates/claude-code-rs/src/api/client/mod.rs` | `cargo test -p cc-mcp discovery -- --nocapture`; `cargo test -p claude-code-rs plugins -- --nocapture`; `cargo test -p claude-code-rs commands::memory -- --nocapture`; `cargo test -p claude-code-rs commands::model_add -- --nocapture`; `cargo test -p claude-code-rs startup -- --nocapture`; `cargo test -p cc-types permissions -- --nocapture`; `cargo test -p cc-auth --lib`; `cargo check -p claude-code-rs --message-format short` |
| Phase 5 - Protocol and runtime result cardinality | Completed. Spawned tool panics now return a failed result for the original tool use; required streaming fields error instead of defaulting; output style fallback emits diagnostics. | `crates/claude-code-rs/src/query/loop_helpers.rs`; `crates/claude-code-rs/src/api/streaming.rs`; `crates/claude-code-rs/src/engine/output_style.rs`; `crates/claude-code-rs/src/engine/system_prompt.rs`; `crates/claude-code-rs/src/commands/config_cmd.rs` | `cargo test -p claude-code-rs query::loop_helpers -- --nocapture`; `cargo test -p claude-code-rs api::streaming -- --nocapture`; `cargo test -p claude-code-rs engine::output_style -- --nocapture`; `cargo check -p claude-code-rs --message-format short` |
| Phase 6 - Team coordination and worktree isolation | Completed. Coordination-affecting mailbox errors now become visible task failures, shutdown auto-approval requires explicit opt-in, and background worktree fallback fails visibly unless explicitly allowed. | `crates/claude-code-rs/src/teams/runner.rs`; `crates/claude-code-rs/src/engine/agent/supervisor.rs` | `cargo test -p claude-code-rs teams::runner -- --nocapture`; `cargo test -p claude-code-rs engine::agent -- --nocapture`; `cargo check -p claude-code-rs --message-format short`; touched-file `git diff --check` |

Remaining risks recorded at the time of the 2026-05-07 archival were startup fast-path MCP fallback, tracing-only hook IO diagnostics, synthetic plugin metadata diagnostics, legacy auth compatibility wrappers, missing broader team E2E coverage, and broad lint allows. The 2026-05-08 closure section below records which of those follow-ups were subsequently resolved and which remain active in `docs/TECH_DEBT.md`.

## 2026-05-08 P1 follow-up closure sessions

These follow-ups were moved out of active P1 fail-fast debt after the session-specific last-message files reported passing targeted verification and Review Session B did not block closure for the row. The plugin diagnostics follow-up was partially closed in this batch; the remaining `/reload_plugins` user-facing visibility issue was closed later in the residual closure section below.

| Follow-up | Archived status | Changed files | Verification and review evidence | Remaining risk |
| --- | --- | --- | --- | --- |
| MCP startup diagnostics | Completed. Legacy `discover_mcp_servers()` now preserves invalid per-server diagnostics, and startup fast-path MCP discovery no longer hides invalid existing config behind `unwrap_or_default()` while preserving missing-file-empty behavior. | `crates/cc-mcp/src/discovery.rs`; `crates/claude-code-rs/src/startup/fast_paths.rs` | `target/codex-runs/session-01-mcp-startup-diagnostics/task-01.last-message.txt`: `cargo test -p cc-mcp discovery -- --nocapture`; `cargo test -p claude-code-rs startup::fast_paths -- --nocapture`; `cargo check -p claude-code-rs --message-format short`. Review B reported no MCP blocker. | No known remaining risk in the Session 1 scope. |
| Auth command diagnostics | Completed for `/logout`, `/voice status`, and `/voice diagnose`; present-but-invalid credentials now surface diagnostics instead of looking like no auth. | `crates/claude-code-rs/src/commands/logout.rs`; `crates/claude-code-rs/src/commands/voice_cmd.rs` | `target/codex-runs/session-02-auth-command-surfaces/task-01.last-message.txt`: `cargo test -p cc-auth --lib`; `cargo test -p claude-code-rs commands::logout -- --nocapture`; `cargo test -p claude-code-rs commands::voice_cmd -- --nocapture`; `cargo check -p claude-code-rs --message-format short`; touched-file `git diff --check`. Review B reported no auth command blocker. | Legacy `cc-auth` compatibility wrappers still collapse diagnostics for external callers and remain active debt. |
| Hook IO public diagnostics | Completed for the hook execution layer; recoverable stdin/stdout/stderr/wait IO failures are now attached to public hook result context, and timeout kill failures are included in returned error text. | `crates/claude-code-rs/src/tools/hooks/execution.rs` | `target/codex-runs/session-03-hook-io-diagnostics/task-01.last-message.txt`: `cargo test -p claude-code-rs tools::hooks -- --nocapture`; `cargo check -p claude-code-rs --message-format short`. Review B reported no hook IO blocker. | Tests cover diagnostic propagation through parser/result data, not every real OS pipe failure mode. The separate critical post/failure hook propagation residual was closed later in the residual closure section below. |
| Plugin diagnostics schema | Partially completed. Synthetic global metadata error plugin entries were removed and a dedicated plugin diagnostics surface was added; malformed installed manifests remain visible as plugin `Error` status and diagnostics. The residual `/reload_plugins` output gap was closed later in the residual closure section below. | `crates/claude-code-rs/src/plugins/loader.rs`; `crates/claude-code-rs/src/plugins/mod.rs`; `crates/claude-code-rs/src/plugins/refresh.rs` | `target/codex-runs/session-04-plugin-diagnostics-schema/task-01.last-message.txt`: `cargo test -p claude-code-rs plugins -- --nocapture`; `cargo check -p claude-code-rs --message-format short`; touched-file `git diff --check`. Review A and Review B found a remaining medium issue that was assigned to the later residual plan. | Superseded by the residual `/reload_plugins` global diagnostics closure below. |
| Team E2E smoke coverage | Completed for the requested bounded smoke addition. Worktree setup failure now has coverage for fallback disabled returning a visible error and fallback enabled returning normal cwd plus a visible startup warning. | `crates/claude-code-rs/src/engine/agent/supervisor.rs` | `target/codex-runs/session-05-team-e2e-coverage/task-01.last-message.txt`: `cargo test -p claude-code-rs teams::runner -- --nocapture`; `cargo test -p claude-code-rs engine::agent -- --nocapture`; `cargo test -p claude-code-rs worktree_setup_failure -- --nocapture`; `cargo check -p claude-code-rs --message-format short`; touched-file `git diff --check`. Review B reported no team blocker. | Coverage is still filtered/unit-smoke level, not a full multi-process team E2E run. |

## 2026-05-08 P1 Review B residual closure

These residuals were moved out of active P1 fail-fast debt after Sessions 1-2 reported passing targeted verification and Review Session A reported no blocking findings.

| Residual | Archived status | Changed files | Verification and review evidence | Remaining risk |
| --- | --- | --- | --- | --- |
| Critical post-tool and post-failure hook propagation | Completed. Critical `PostToolUse` and `PostToolUseFailure` hook errors now return visible failed `ToolExecResult` values from production lifecycle execution, while optional post/failure hook errors remain best-effort. | `crates/claude-code-rs/src/engine/lifecycle/deps.rs` | `target/codex-runs/p1-review-b-residual-session-01-hook-propagation/task-01.last-message.txt`: `cargo test -p claude-code-rs engine::lifecycle -- --nocapture`; `cargo test -p claude-code-rs tools::hooks -- --nocapture`; `cargo check -p claude-code-rs --message-format short`. `target/codex-runs/p1-review-b-residual-review-a/task-01.last-message.txt` reported no blocking findings. | Audit/langfuse completion is still emitted based on the underlying tool call before post-hook failure handling; the closure only covers user-visible `ToolExecResult` propagation. |
| `/reload_plugins` global diagnostics visibility | Completed. Global plugin metadata/cache diagnostics without `plugin_id` now have a `ReloadReport` surface and are rendered in `/reload_plugins` output while preserving per-plugin error output and clean success output. | `crates/claude-code-rs/src/plugins/refresh.rs`; `crates/claude-code-rs/src/commands/reload_plugins_cmd.rs` | `target/codex-runs/p1-review-b-residual-session-02-plugin-reload-diagnostics/task-01.last-message.txt`: `cargo test -p claude-code-rs commands::reload_plugins_cmd -- --nocapture`; `cargo test -p claude-code-rs plugins -- --nocapture`; `cargo check -p claude-code-rs --message-format short`; scoped `git diff --check`. `target/codex-runs/p1-review-b-residual-review-a/task-01.last-message.txt` reported no blocking findings. | Global registry/env isolation in plugin tests remains a watch item if plugin reload paths are touched again. |
