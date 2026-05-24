# Workspace 拆环执行计划

> 日期：2026-05-14
> 范围：继续把 `crates/claude-code-rs/src/**` 迁移到 workspace crates 时，先切断会形成 Cargo 包级循环的运行时依赖。

## 目标依赖方向

- `cc-types`、`cc-ipc-protocol` 只承载 DTO、trait、JSONL/IPC contract。
- `cc-engine` 可以依赖 DTO/protocol crate，但不能依赖 `cc-ipc` 运行时状态。
- `cc-ipc`、root binary、TUI/headless glue 可以依赖 `cc-engine`，并通过 adapter 注入运行时能力。
- `cc-tools` 保持工具规格和稳定 metadata；需要 engine、teams、daemon、plugins 的执行逻辑放到 engine 或 root glue。
- `claude-code-rs` 最终只保留 CLI、startup、runtime adapter wiring 和 binary entry。

## 当前已落地

### Cut 1：切断 `cc-engine -> cc-ipc`

状态：已完成第一刀。

变更：

- `cc-engine` 不再依赖 `cc-ipc` crate。
- Agent tree 的 register/update/snapshot/active-count 通过 `cc_engine::agent_runtime::AgentTreeRuntime` 抽象。
- `cc-engine` 默认使用 crate 内 in-memory agent tree，保证非 IPC 场景仍可工作。
- root binary 在 `ipc::runtime_adapters::ensure_installed()` 中安装 agent tree runtime adapter；当前实现由 engine runtime 持有 agent tree，IPC 只通过 host trait 查询快照。
- `main.rs` 在创建 `QueryEngine` 前安装 runtime adapters，避免 agent 先启动再接 adapter。
- `cc-engine` 只显式依赖 `cc-ipc-protocol` 的 agent/subsystem DTO。

验证：

- `cargo check -p cc-engine`
- `cargo check -p claude-code-rs`
- `cargo test -p cc-engine agent_runtime`
- `cargo build --workspace --release`

`claude-code-rs` build script 仍会提示本机没有 `npm`，因此跳过 web-ui build；这是当前机器的已知环境 warning。

## 下一批拆环

### Cut 2：收紧 `cc-engine -> cc-ipc-client`

当前 `cc-engine::ipc_compat` 为 `QueryEngine` 实现 `cc_ipc_client` 的 callback/query traits。短期可工作，但长期更干净的方向是：

- 把 callback host trait 下沉到 `cc-types::callbacks`，或让 `cc-ipc-client` 使用闭包 setter。
- query-turn host trait 如果继续需要 `SdkMessage`，优先放在 protocol/client 边界，不让 engine 直接依赖 IPC client helper。

完成标准：

- `cc-engine/Cargo.toml` 不再含 `cc-ipc-client`。
- IPC callback/query helper 仍能在 root glue 中安装到 `QueryEngine`。

### Cut 3：收敛 `engine <-> tools`

目标：

- `cc-engine` 保留 agent、query loop、tool execution runtime adapter。
- `cc-tools` 不主动查询 engine、teams、plugins、daemon 运行时。
- Agent/team/task 相关工具通过 `AgentRuntimeAdapters` 或独立 adapter trait 获取 runtime 能力。

优先处理：

- `cc-engine::agent_runtime::AgentToolRegistry` 接 root tool registry。
- `TeamSpawnTool`、task store、hook runner 已经有 adapter 雏形，继续去掉 root-only direct path。

### Cut 4：处理 `browser <-> mcp`

目标方向二选一：

- 保留 `cc-browser -> cc-mcp -> cc-types`；`mcp/tools.rs` 中依赖 browser 检测/渲染的部分迁到 `cc-browser`。
- 或把共享配置/分类 DTO 下沉到 `cc-types::mcp`，两边只依赖 DTO。

完成标准：

- `cc-mcp` 不再依赖 browser runtime 逻辑。
- browser MCP tool rendering 和 detection 保持在 `cc-browser`。

### Cut 5：整理 `commands/ui/ipc` 边界

目标：

- `cc-commands` 只暴露 command metadata、parse、handler contract 和业务输出 DTO。
- UI/TUI 只通过 dispatcher/command result DTO 与 commands 通信。
- status-line payload、subsystem snapshots、plan workflow event 继续下沉到 `cc-types` 或 `cc-ipc-protocol`。

## 执行规则

- 每一刀先切一个 Cargo 边，不混入行为重写。
- 新下沉的类型必须是 DTO/trait，不携带 root runtime。
- 每次至少跑对应 crate 的 `cargo check`；触碰 root glue 时再跑 `cargo check -p claude-code-rs`。
- 如果出现 warning，优先修掉真实 unused import/dead code；确属机器环境的 build-script warning 单独记录。

---

## 完成状态（2026-05-21）

**判定：⚠️ 部分完成**

已完成：
- **Cut 1**：切断 `cc-engine -> cc-ipc` 已完成。`cc-engine` 不再依赖 `cc-ipc` crate，agent tree 通过 `AgentTreeRuntime` 抽象，root 在 `main.rs` 中安装 runtime adapter。验证命令通过。

未完成：
- **Cut 2**：收紧 `cc-engine -> cc-ipc-client` 尚未开始。
- **Cut 3**：收敛 `engine <-> tools` 尚未开始。
- **Cut 4**：处理 `browser <-> mcp` 尚未开始。
- **Cut 5**：整理 `commands/ui/ipc` 边界尚未开始。
