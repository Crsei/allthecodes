# allthecodes-engine 优化计划

日期：2026-05-29

范围：`crates/allthecodes-engine/**` 及其直接迁移边界，包括仍残留的
`crates/allthecodes-query/**`、engine 对外 API、hook runtime、路径隔离和
构建门禁。

## 背景

`crates/allthecodes-engine` 当前已经不是早期 scaffold。它实际拥有
`QueryEngine` lifecycle、query loop、agent、exec tools、system prompt、
status line、hooks、session memory、Langfuse bridge、MCP tool adapter 等核心
运行时模块。

但仓库里仍有迁移尾巴：

- `crates/allthecodes-query` 仍存在，与 engine 内 `query/` 发生漂移。
- engine 的 Cargo/lib 注释仍描述 Phase 6 incremental extraction。
- engine 自身 `cargo check --all-features --tests` 仍有 warning。
- hooks 子系统仍有结构性 stub。
- 路径隔离仍残留 `.cc-rust` / `cc_rust_*` / `CC_RUST_*` 兼容痕迹。

本计划目标是先把 engine 收束成单一可信运行时 owner，再补齐 Full Build
行为缺口。

## 当前基线

采样命令：

```bash
export CARGO_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo
export RUSTUP_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/rustup
export PATH="$CARGO_HOME/bin:$PATH"

cargo check -p allthecodes-engine --all-features --tests
cargo check -p allthecodes-query
cargo check -p allthecodes-query --tests
```

采样结果：

- `cargo check -p allthecodes-engine --all-features --tests`：通过，但有 6 个
  warning。
- `cargo check -p allthecodes-query`：通过。
- `cargo check -p allthecodes-query --tests`：失败，`Usage` 初始化缺
  `reasoning_output_tokens` 字段。
- `crates/allthecodes-engine/src` 约 29k 行，最大热点集中在：
  `query/loop_helpers.rs`、`query/loop_impl.rs`、`agent/supervisor.rs`、
  `lifecycle/deps/execute.rs`、`agent/tool_impl.rs`、`tools/exec/bash.rs`。

## 目标

完成后应满足：

- `allthecodes-engine` 是 query/lifecycle/agent 运行时的唯一权威实现。
- 不再维护漂移的 `allthecodes-query` 双实现。
- engine crate 独立 check/test 无 Rust warning。
- hook runtime 中的 stub 有明确实现、降级行为或 intentional 记录。
- 所有持久化路径遵守 `~/.allthecodes/` 和 `.allthecodes/` 隔离规则。
- Cargo/lib/module 注释与 Full Build 当前状态一致。

## 非目标

- 不在本计划中一次性重写 `QueryEngine` 或 query loop 架构。
- 不把 root crate、TUI、daemon、web 的所有 residual 一次性迁完。
- 不启用全 workspace `clippy -D warnings`；这应在 warning 归零后单独计划。
- 不删除必须保留的旧环境变量兼容，除非有迁移说明和测试覆盖。

## Phase 0：迁移卫生与 warning 清零

目的：先让 engine crate 自身成为稳定基线。

工作项：

- 清理 `allthecodes-engine` 当前 6 个 warning。
- 将 engine 内部仍使用的 `allthecodes_engine::...` 自引用改为 `crate::...`。
- 更新 `crates/allthecodes-engine/Cargo.toml` 的 Phase 6 scaffold 注释。
- 更新 `crates/allthecodes-engine/src/lib.rs` 中的 `cc-engine` / Phase 6
  说明。
- 检查 `tools/exec/mod.rs` 中指向不存在文档的
  `src/tools/ARCHITECTURE.md` 注释，改为真实文档路径或删除。

退出条件：

```bash
cargo check -p allthecodes-engine --all-features --tests
```

输出中没有 Rust source warning。

## Phase 1：收束 allthecodes-query 双实现

目的：消除 query loop 双份源码和测试漂移。

当前事实：

- `crates/allthecodes-query/src/{loop_impl,loop_helpers,deps,stop_hooks}`
  与 `crates/allthecodes-engine/src/query/**` 已经不同。
- `allthecodes-query --tests` 已经无法编译。
- 当前主要实际消费点是 `allthecodes-safety/src/classifier.rs` 引用
  `allthecodes_query::deps::{ModelCallParams, QueryDeps}`。

工作项：

- 将 `allthecodes-safety` 切到 `allthecodes_engine::query::deps`。
- 移除 `allthecodes-safety/Cargo.toml` 中的 `allthecodes-query` 依赖。
- 检查 root `crates/allthecodes/Cargo.toml` 是否仍需要
  `allthecodes-query`；无使用则删除依赖。
- 选择最终策略：
  - 首选：删除 `crates/allthecodes-query` crate 和 workspace dependency。
  - 备选：改成薄 re-export crate，仅转发
    `allthecodes_engine::query::*`，且不保留独立实现和旧测试。
- 对比旧 `allthecodes-query` 测试，迁回仍有价值但 engine 缺失的测试场景。

退出条件：

```bash
rg "allthecodes_query|allthecodes-query" crates Cargo.toml
cargo check --workspace --tests
```

不再存在双实现依赖；workspace tests check 不因旧 query crate 失败。

## Phase 2：补齐 hook runtime stub

目的：Full Build 阶段不能继续把结构性 stub 当成完成实现。

优先级：

1. `hooks/prompt_hook.rs`
2. `hooks/agent_hook.rs`
3. `hooks/file_watcher.rs`
4. `hooks/api_query_helper.rs`
5. `hooks/skill_improvement.rs`

工作项：

- 对照 TypeScript 上游实现确认输入、超时、取消、错误恢复和阻断语义。
- prompt hook 接入非流式模型调用、JSON schema 响应解析和 blocking result。
- agent hook 接入子 agent/query runtime，补 structured output 解析。
- file watcher 使用 `notify` 或已有 watcher 抽象，支持静态和动态 watch path。
- api query helper 接入 engine/API 边界，替换空 closure。
- skill improvement 在启用开关下接入 post-sampling hook，并实现安全的读写流程。

退出条件：

- 每个 stub 文件要么有完整实现和测试，要么在
  `development/archive/IMPLEMENTATION_GAPS.md` 标为 intentional，并说明保留原因。
- 不允许默认静默 success 的 hook stub 留在生产路径。

验证：

```bash
cargo test -p allthecodes-engine hooks
cargo check -p allthecodes-engine --all-features --tests
```

## Phase 3：路径隔离与旧命名收敛

目的：保证 allthecodes 与原版 Codex / 历史 cc-rust 路径不会混用。

当前需要处理的 engine 内 residual：

- `output_style.rs` 中 `.cc-rust/output-styles` fallback。
- `tools/exec/repl.rs` 中 `cc_rust_repl_*` 临时文件前缀。
- 多个 `CC_RUST_*` 环境变量 fallback。
- 测试中 `cc_rust_test_*` 临时路径命名。

工作项：

- 将新写入/新创建路径统一为 `.allthecodes` / `allthecodes_*`。
- 对只读 legacy fallback 做明确策略：
  - 如果要保留，文档标为 legacy read-only compatibility。
  - 如果删除，补迁移说明和测试。
- 将环境变量读取集中到 helper，避免每个模块各自处理 legacy fallback。
- 更新相关测试断言和 docs。

退出条件：

```bash
rg "\\.cc-rust|cc-rust|cc_rust|CC_RUST|~/.Codex|\\.Codex|~/.codex|Codex" \
  crates/allthecodes-engine/src -g '*.rs'
```

结果只剩明确 intentional compatibility，且有注释或文档说明。

## Phase 4：engine public API 分层

目的：降低 UI、daemon、commands、startup 对 engine 内部结构的直接耦合。

工作项：

- 明确 public API 层级：
  - `lifecycle`：`QueryEngine` 和 submit/abort/steer/session 控制。
  - `query`：query loop 和 `QueryDeps` contract。
  - `agent_runtime`：agent 状态、adapter、snapshot。
  - `types`：`AppState`、`QueryEngineConfig`、tool type re-export。
  - `status_line`：payload 和 runner。
- 给 `QueryEngineConfig` 增加 builder 或 `Default` + helper constructor，减少新增
  字段时全仓构造点爆炸。
- 将过大的 `AppState` 读写拆出更小的 snapshot/adapter，优先服务 command/TUI
  状态同步。
- 减少 engine 对 sibling crates 的非必要 re-export。

退出条件：

- 新功能不再要求外部 crate 直接读写 `QueryEngineState` 内部字段。
- `QueryEngineConfig` 新增字段时，大多数测试和外部构造点无需机械补字段。
- public module 文档能说明每层职责。

## Phase 5：复杂热点拆分

目的：在不大改行为的前提下，降低最大文件的维护成本。

优先拆分：

- `query/loop_impl.rs`：拆出 streaming attempt、terminal check、tool batch
  continuation。
- `query/loop_helpers.rs`：拆出 recovery、tool batching、message construction。
- `agent/supervisor.rs`：拆出 registry/state、worktree setup、task lifecycle。
- `lifecycle/deps/execute.rs`：拆出 permission/audit/hook/tool-call result 保存。
- `tools/exec/bash.rs`：拆出 validation、permission/preflight、process streaming。

拆分原则：

- 每次只拆一个职责，不做行为重写。
- 先移动函数和测试，再做小范围命名清理。
- 拆分后保留原有测试覆盖，并补一个跨模块行为测试防回归。

退出条件：

- 单文件复杂度下降，且没有新增 public API 泄漏。
- 关键行为测试仍通过。

## Phase 6：验证与文档收口

目的：把优化结果变成可持续门禁。

必跑验证：

```bash
cargo check -p allthecodes-engine --all-features --tests
cargo test -p allthecodes-engine --all-features
cargo check --workspace --tests
cargo build --workspace --release
```

文档更新：

- `docs/WORK_STATUS.md`：更新 crate migration / engine 状态。
- `development/archive/IMPLEMENTATION_GAPS.md`：移除已补齐 stub，或标 intentional。
- 如删除 `allthecodes-query`：补一条迁移完成记录到 archive。
- 如保留 legacy env/path fallback：补兼容策略说明。

退出条件：

- Full Build 状态文档不再把 engine 描述为 Phase 6 scaffold。
- 构建门禁输出没有新增 Rust warning。
- 删除/保留的 residual 都有明确记录。

## 风险与缓解

| 风险 | 影响 | 缓解 |
| --- | --- | --- |
| 删除 `allthecodes-query` 影响 safety classifier | auto-mode classifier 编译失败 | 先切 classifier 到 engine deps，再删 crate |
| hook stub 实现引入阻断行为变化 | 用户请求被错误阻断或静默放行 | 每类 hook 加 success/blocking/error/timeout 测试 |
| 路径兼容清理破坏旧用户配置 | custom output style 找不到 | legacy fallback 先只读保留，并文档化 |
| 大文件拆分引入行为漂移 | query/agent 主流程回归 | 每次只做机械拆分，先跑 engine tests |
| workspace release 构建受环境 warning 干扰 | 误判失败 | 区分 Rust source warning 和已知 npm 环境提示 |

## 建议执行顺序

1. Phase 0：engine warning 和文档注释清理。
2. Phase 1：删除或薄化 `allthecodes-query`。
3. Phase 3：路径隔离 residual 清理。
4. Phase 2：hook stub parity。
5. Phase 4/5：API 分层和复杂热点拆分。
6. Phase 6：全门禁和文档收口。

前两步优先级最高，因为它们能先建立单一可信基线，避免后续功能补齐时继续在旧
query crate 和 engine query 之间双维护。
