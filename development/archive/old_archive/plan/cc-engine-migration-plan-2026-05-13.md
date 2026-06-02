# cc-engine 迁移执行计划

> 日期：2026-05-13
> 范围：将 `crates/claude-code-rs/src/engine/**` 的主要模块迁移到 `crates/cc-engine/**`，并把 `claude-code-rs` 收敛为入口层和兼容适配层。

## 目标

把旧 `engine` 目录从 root crate 中拆出来，形成可独立演进的 `cc-engine` 核心包。

迁移完成后：

- `cc-engine` 拥有 `QueryEngine` 生命周期实现。
- `cc-engine` 拥有 `result`、`input_processing`、`codex_exec`、`system_prompt`、`prompt_sections`、`output_style`、`effort`、`agent` 相关实现。
- SDK 输出结构最终落到共享类型 crate：`cc-types::sdk`。
- `claude-code-rs` 只保留入口、UI、IPC、daemon、tools、commands 等壳层逻辑。
- root crate 不再通过 `#[path = "..."]` 继续承载真实 `engine` 实现。
- 所有旧导入路径先通过 shim 过渡，最后再统一删除。

## 当前状态

> 更新：2026-05-21 复核当前代码后，本节反映实际仓库状态。上方分阶段计划保留为历史执行计划。

### 已落到 `cc-engine` 的内容

- `types/{app_state, config, tool}`：已进入 `cc-engine::types`
- `status_line`：已进入 `cc-engine::status_line`
- `agent_runtime`：已进入 `cc-engine::agent_runtime`
- `lifecycle`：已在 `cc-engine/src/lifecycle/**`，并通过 `cc_engine::lifecycle` 公开
- `query`：已进入 `cc-engine::query`
- `tool_runtime` 与 `tools/exec`：已进入 `cc-engine`
- `agent/{mod, dispatch, fork, supervisor, tool_impl, worktree, builtin_agents}`：已进入 `cc-engine::agent`
- `codex_exec.rs`、`effort.rs`、`input_processing.rs`、`output_style.rs`、`prompt_sections.rs`、`result.rs`、`system_prompt.rs`：已进入 `cc-engine`
- `hooks`、`services::{tool_use_summary, session_memory, langfuse}`、`mcp_tool_adapter`、`skill_tool`、`worktree_hooks`：已进入 `cc-engine`
- SDK 输出结构：已由共享 crate `cc-types::sdk` 承载

### root crate 当前职责

- `crates/claude-code-rs/src/engine/` 已删除，不再承载 engine 主实现。
- `crates/claude-code-rs/src/types/mod.rs` 已删除，不再承载 `cc_engine::types` shim。
- root crate 仍保留入口层、Rust TUI、startup 调度、命令运行时 bridge、IPC/headless runtime adapter、daemon/web 启动 glue、plan workflow glue 等壳层/适配逻辑。

### 剩余兼容 shim / adapter

- 已不存在 `crates/claude-code-rs/src/engine/lifecycle/mod.rs` 的 `#[path]` shim。
- 已不存在 `crates/claude-code-rs/src/types/mod.rs` 的 `cc_engine::types` shim。
- `crates/claude-code-rs/src/ui/mod.rs` 仍保留 `pub use cc_engine::status_line;`，用于兼容 `crate::ui::status_line::*` 调用点。
- `crates/claude-code-rs/src/ui/status/status_line_resolver.rs` 仍是 root UI 侧的状态栏字段预解析适配层，不是 engine 主实现。

## 迁移原则

1. 先搬纯叶子，再搬边界层，最后搬高耦合运行时。
2. 每一阶段都保持 `claude-code-rs` 可编译。
3. `cc-engine` 不能反向依赖 `claude-code-rs` 私有模块。
4. 只在公共 API 和测试稳定后再删除 shim。
5. 迁移顺序按依赖图走，不按文件大小走。

## 模块分层

| 层级 | 模块 | 依赖特征 | 建议顺序 |
| --- | --- | --- | --- |
| L0 | `codex_exec.rs`、`effort.rs` | 基本是纯函数和常量 | 最先 |
| L1 | `result.rs`、`input_processing.rs`、SDK 输出类型 | 依赖 `cc-types` 和生命周期类型；SDK 输出类型最终落到 `cc-types::sdk` | 第二波 |
| L2 | `prompt_sections.rs`、`output_style.rs`、`system_prompt.rs` | 依赖配置、路径、git、prompt 组装 | 第三波 |
| L3 | `agent/*` | 依赖 `QueryEngine`、权限、worktree、tasks、mcp、hooks | 最后 |
| L4 | `lifecycle` 公共化收口 | 只是把已迁入的源码真正暴露到 `cc-engine` | 和第二波并行收口 |

## 分阶段计划

### Phase 0：冻结基线

目标：记录迁移前行为，避免把既有缺陷误判成迁移回归。

工作项：

- 记录旧 `engine` 文件清单和当前 `cc-engine` 目录结构
- 记录 `QueryEngine`、`Agent`、`system prompt`、`output style`、`effort` 的现有测试覆盖
- 确认 `cc-engine` 当前的公开面只包含 `types`、`status_line`、`agent_runtime`

完成标准：

- 有可回溯的迁移前基线
- 没有行为改动

### Phase 1：先拆纯叶子

目标：把最容易独立的逻辑先挪进 `cc-engine`，缩小 root crate 的核心负担。

优先迁移：

- `codex_exec.rs`
- `effort.rs`
- `result.rs`

说明：

- `codex_exec.rs` 和 `effort.rs` 基本不依赖运行时状态，可以先落地
- `result.rs` 与消息收尾逻辑强相关，适合和生命周期边界一起收口

完成标准：

- `cc-engine` 内可直接引用这些模块
- root crate 不再把这些模块当作核心实现源

### Phase 2：迁移输入与 SDK 边界

目标：把查询入口和外部可见输出结构从 root crate 拆到 `cc-engine`。

优先迁移：

- `input_processing.rs`
- SDK 输出类型（执行结果：落到 `cc-types::sdk`）

说明：

- `input_processing.rs` 负责 slash command 识别和 user message 构造，依赖面相对稳定
- SDK 输出类型是 `QueryEngine` 对外输出的结构层；实际收口时放入共享 crate `cc-types::sdk`，供 `cc-engine`、TUI、IPC adapter 等共同消费

需要同步处理的调用点：

- `claude-code-rs` 中所有旧 `crate::engine::sdk_types::*` 的引用，切到 `cc_types::sdk::*`
- `claude-code-rs` 中所有旧 `crate::engine::input_processing::*` 的引用，切到 `cc_engine::input_processing::*`

完成标准：

- SDK 输出类型不再由 root crate 主持
- 输入处理逻辑不再依赖 root crate 的模块布局

### Phase 3：迁移 prompt 组装链

目标：把系统提示词生成链从 root crate 中拆出。

优先迁移：

- `prompt_sections.rs`
- `output_style.rs`
- `system_prompt.rs`

依赖处理：

- `output_style.rs` 目前只需要一个稳定的路径/数据根访问点，应该先抽成轻量 adapter，再迁入 `cc-engine`
- `system_prompt.rs` 依赖 `claude_md`、git 状态、工作目录和工具上下文，需要把 root-only helper 变成显式输入参数或轻量 trait
- `prompt_sections.rs` 可以作为纯缓存和拼装层最先进入

完成标准：

- `cc-engine` 可以独立构建系统提示词
- root crate 只提供少量环境适配，不再承载 prompt 组装主体

### Phase 4：迁移 `agent` 子树

目标：把子代理、工作树、后台 supervisor、工具适配整体迁入 `cc-engine`。

优先迁移：

- `agent/mod.rs`
- `agent/dispatch.rs`
- `agent/fork.rs`
- `agent/supervisor.rs`
- `agent/tool_impl.rs`
- `agent/worktree.rs`

依赖处理：

- `QueryEngine` 需要先稳定为 `cc-engine` 的正式 public API
- 权限相关接口要以 `cc-types` 的类型为主，避免反向依赖 root crate
- worktree、task、mcp、hooks、bash 校验等 root 依赖要么抽成 adapter，要么改成 workspace crate 调用

完成标准：

- Agent tool 的所有执行路径都能在 `cc-engine` 内闭环
- root crate 不再直接持有 agent 的真实实现

### Phase 5：收口 `lifecycle`

目标：把已经落到 `cc-engine/src/lifecycle/**` 的源码真正公开出来，取消 root shim。

工作项：

- 在 `cc-engine/src/lib.rs` 暴露 `pub mod lifecycle`
- 把 root crate 的 `engine/lifecycle` 改成短期兼容转发或直接删除
- 把所有 `crate::engine::lifecycle::*` 的消费点切到 `cc_engine::lifecycle::*`

完成标准：

- `lifecycle` 不再依赖 root crate 的 `#[path]` 载入
- `cc-engine` 成为 `QueryEngine` 的唯一源码归属

### Phase 6：删除旧 `engine` 主实现

目标：清掉 root crate 中的真实实现，只保留必要的兼容层或彻底移除。

工作项：

- 删除 `crates/claude-code-rs/src/engine/` 下的真实实现文件
- 仅保留必要的过渡性 re-export，直到所有调用点切换完成
- 更新文档和测试路径，移除旧模块名假设

完成标准：

- root crate 不再包含 `engine` 业务实现
- `cc-engine` 成为唯一权威实现

## 主要阻断点处理状态

历史阻断点当前已基本解除：

- `system_prompt.rs` 已迁入 `cc-engine`，依赖改为 workspace crate 和显式输入。
- `agent/*` 已迁入 `cc-engine::agent`，root crate 通过 runtime adapter 注入 dashboard、tool registry、builtin agent registry 等边界能力。
- SDK 输出结构已落到 `cc-types::sdk`，`result.rs` 已迁入 `cc-engine`。
- `output_style.rs` 与 `effort.rs` 已迁入 `cc-engine`，调用点已切到 `cc_engine::output_style` / `cc_engine::effort`。

剩余阻断点已不再是 engine 主实现迁移，而是 crate migration 的 thin-binary 收口：

- 清理 root crate 中剩余兼容 re-export，例如 `crate::ui::status_line`。
- 收敛 root-style imports、allow-attribute hits、Codex compatibility path hits，并登记确需保留的 intentional residual。
- 更新仍描述旧迁移阶段的注释和计划文档，例如 `crates/cc-engine/Cargo.toml` 中的 Phase 6 注释。

## 验收顺序

当前建议按下面的测试顺序验收：

1. `cargo test -p cc-engine`
2. `cargo test -p claude-code-rs ui::...`
3. `cargo build --workspace --release`

每一波迁移都要满足两个条件：

- 新 crate 侧测试通过
- root crate 侧仍可编译，且不再通过 `crate::engine::*` 消费旧实现

## 备注

- 这份计划和 `cc-daemon` 迁移计划是并列的，不要混成同一个阶段图
- `lifecycle` 已不再只是物理迁移，当前已由 `cc_engine::lifecycle` 公开。
- `output_style`、`effort`、`system_prompt` 已完成迁入；后续只需处理 thin-binary 收口和文档归档。

---

## 完成状态（2026-05-21 复核更新）

**判定：主体迁移已完成，剩余为 thin-binary / 兼容层收口**

已完成：

- `crates/claude-code-rs/src/engine/` 已删除。
- `crates/claude-code-rs/src/types/mod.rs` 已删除。
- `cc-engine` 已公开 `lifecycle`、`query`、`agent`、`agent_runtime`、`types`、`status_line`、`codex_exec`、`effort`、`input_processing`、`output_style`、`prompt_sections`、`result`、`system_prompt` 等模块。
- SDK 输出结构已由 `cc-types::sdk` 承载。
- root crate 调用点已切到 `cc_engine::...` 或 `cc_types::sdk`，不再通过 `crate::engine::*` 引用旧实现。
- `cargo test -p cc-engine` 于 2026-05-21 复核通过：415 tests passed，0 failed。

未完成 / 待收口：

- `crates/claude-code-rs/src/ui/mod.rs` 仍保留 `pub use cc_engine::status_line;` 兼容 re-export。
- root crate 仍保留 runtime adapters / command bridge / plan workflow / UI status resolver 等壳层 glue；这些不是旧 engine 主实现，但仍属于 thin-binary closeout 的审计范围。
- `crates/cc-engine/Cargo.toml` 等位置仍有过期的 Phase 6 注释，需要后续文档清理。
- 尚未在本次复核中重跑完整 `cargo build --workspace --release`。
