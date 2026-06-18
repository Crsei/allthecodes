# Workspace 依赖目标

> 范围：crate 迁移完成后，workspace 内各 crate 的理想依赖形态。本文重点细化
> browser / MCP / tool / engine 的依赖边界，同时给出其他 crate 的目标依赖方向。
>
> 本文是目标状态参考文档。如果当前代码与本文不一致，除非其他仍有效的文档明确标记为
> intentional，否则应将该不一致视为剩余迁移工作。

## Codex 依赖表带来的经验

OpenAI Codex Rust workspace 的依赖表位于
`/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/codex/codex-rs/Cargo.toml`。
它不应被逐项复制到本项目；更有价值的是它体现出的工程边界：

- workspace 根统一治理 crate 版本、edition、license、profile、lint 和 patch。
- 外部依赖只在 workspace 根统一版本，features 尽量由具体 crate 按需声明。
- 可复用能力被拆成小 crate，例如 exec policy、shell command、sandboxing、
  file-search、skills、state、thread-store、protocol、utils 子 crate。
- 核心 runtime crate 依赖 contract / adapter，而不是互相读取内部状态。
- TUI 依赖能力型 helper crate，例如 file-search、fuzzy-match、terminal-detection、
  approval-presets、sandbox-summary，而不是直接依赖 core runtime implementation。
- 安全、许可、重复依赖和已知例外通过 `cargo-deny` / `cargo-shear` 类配置持续治理。

本项目的目标不是变成 Codex 的 crate 命名镜像，而是把这些经验映射到
`cc-*` 边界：先稳定 contract，再拆 runtime owner，最后收紧依赖和 lint。

## 目标

浏览器自动化应按所有权拆分，而不是按历史模块路径拆分：

- `cc-browser` 拥有浏览器专属行为。
- `cc-mcp` 拥有 MCP 协议、发现、transport、配置和 MCP 工具集成。
- `cc-tools` 拥有稳定工具契约和可复用工具元数据。
- `cc-engine` 在构建 prompt、权限和执行行为时消费 browser/MCP/tool 契约。
- `claude-code-rs` root 只负责启动数据和 adapter wiring。

最终形态必须避免可复用 crate 反向依赖 root-private 或 engine-private runtime 类型。

## 理想分层

下面表示依赖方向：图中靠下的 crate 可以依赖靠上的 crate，但靠上的 crate
不能依赖靠下的 crate。

```text
contracts:
  cc-types / cc-ipc-protocol
    -> leaf/core domains:
       cc-utils / cc-config / cc-models / cc-auth / cc-observability /
       cc-keybindings / cc-bootstrap
    -> persisted/provider domains:
       cc-api / cc-session / cc-compact / cc-permissions / cc-sandbox /
       cc-skills
    -> runtime domains:
       cc-tools / cc-mcp / cc-browser / cc-computer-use / cc-lsp-service /
       cc-services / cc-tasks / cc-teams / cc-plugins / cc-daemon /
       gateway / cc-engine / cc-query / cc-commands / cc-ipc /
       cc-ipc-client
    -> UI and binary glue:
       cc-ui / claude-code-rs
```

这个顺序并不要求每个 crate 都依赖所有更上层的 crate。它表示共享契约向下流向
runtime consumer，而 runtime state 不能向上流入共享 crate。

## 全局依赖规则

- `cc-types` 和 `cc-ipc-protocol` 只拥有 contract / DTO / wire shape，不拥有
  runtime。
- 领域 crate 可以依赖 contract 和更底层的 core domain，但不能依赖 UI、root
  binary、daemon process state 或 engine-private 类型。
- Runtime crate 之间需要共享信息时，应通过 DTO、trait 或 adapter contract，而不是
  横向读取对方内部状态。
- `cc-ui` 消费 UI-facing DTO 和 adapter，不拥有 engine、daemon、MCP、browser 的
  runtime implementation。
- `claude-code-rs` root 是 composition boundary：可以依赖 library crate 来组装进程，
  但任何 library crate 都不能依赖 root。
- 不为迁移方便添加只保留旧路径的 wrapper、re-export 或 compatibility layer。
- workspace 根应统一 `version`、`edition`、`license`、profile 和 lint 策略；成员
  crate 应逐步迁移到 `version.workspace = true`、`edition.workspace = true`。
- workspace 根只统一外部依赖版本。除非确实是全局固定能力，依赖 features 应留在
  使用该能力的 crate 本地声明，避免 `tokio`、`reqwest`、`keyring`、`image`、
  `syntect` 等依赖被无关 crate 间接放大。
- 新增依赖前先判断它属于 contract、runtime owner、UI helper 还是 binary glue。
  不允许为了省事把能力直接加到 `claude-code-rs` root 后再由其他 crate 反向消费。
- 对 full build 有帮助的外部依赖可以参考 Codex，但必须按本项目边界落位；例如
  `shlex`/`starlark` 属于 exec policy，`nucleo` 属于 file-search / UI completion，
  `rmcp`/`schemars` 属于 MCP 协议层，`arboard` 属于 TUI 平台能力。

## Workspace 治理目标

Codex 的 workspace 根同时承担依赖版本、lint、profile 和 patch 管理。本项目应按
风险分阶段吸收：

- `workspace.package`：统一 `version`、`edition`、`license`。当前 full build
  阶段可先保留 Rust 2021，待 warning / clippy 收敛后评估迁到 Rust 2024。
- `workspace.lints`：先启用低风险 clippy 规则，例如 `redundant_clone`、
  `manual_ok_or`、`manual_map`、`needless_collect`、`uninlined_format_args`。
  `unwrap_used`、`expect_used` 应在错误恢复路径补齐后再启用。
- `profile`：保留快速 dev build，同时补齐 release 体积目标，例如 `strip`、
  `lto`、`codegen-units` 的明确策略。
- dependency audit：增加 `deny.toml` 或等价配置，记录许可证、RustSec 例外、
  重复版本和故意保留的风险依赖。例外必须写明依赖路径和移除条件。
- unused dependency audit：引入 cargo-shear 类检查时，应只忽略平台条件或 build
  script 导致的误报，不把普通未使用依赖加入长期 ignore。

## 依赖 Feature 目标

Codex 的经验表明，大型 workspace 不能依赖“根部开满 feature”的做法。本项目的目标：

- `tokio`：workspace 根只统一版本；各 crate 自行声明 `macros`、`process`、
  `signal`、`net`、`fs`、`time`、`rt-multi-thread` 等实际需要的 feature。
- `reqwest`：API / MCP / daemon 按需启用 `json`、`stream`、`cookies`、TLS backend。
  不应让纯 DTO、config、permissions crate 间接带上 HTTP client 能力。
- `keyring`：默认关闭不必要 feature，平台能力留在 `cc-auth` 或更窄的 keyring owner。
- `image` / `syntect` / `tree-sitter`：只允许 UI、browser rendering、computer-use 或
  具体 analyzer crate 启用；root feature 可以做最终二进制开关，但 library crate
  不应无条件依赖。
- `git2` / git 能力：优先放在 `cc-session`、未来 `cc-git-utils` 或明确的 repo helper
  crate；不要让 config、types、UI 为了小功能依赖 libgit2。
- `regex`：热路径和简单匹配优先评估 `regex-lite`；完整正则能力只给确实需要的 crate。

## Browser / MCP 规则

- `cc-browser` 不能依赖 `claude-code-rs`。
- `cc-browser` 不能依赖 `cc-engine`。
- `cc-browser` 不能依赖 `cc-mcp` runtime internals。
- `cc-mcp` 不能依赖 `cc-browser`。
- `cc-engine` 可以作为 runtime consumer 依赖 `cc-browser`、`cc-mcp` 和
  `cc-tools`。
- `claude-code-rs` root 可以依赖所有 library crate 来组装二进制启动流程，但任何
  library crate 都不能依赖 root。

这样可以让浏览器分类/渲染独立于 MCP transport，也让 MCP discovery 独立于
browser runtime 行为。

## 其他 Crate 的理想依赖

本节描述目标依赖方向。这里的“可依赖”不是强制依赖清单，而是允许的上游边界；
实际 `Cargo.toml` 应只声明当前实现确实需要的依赖。

| Crate | 理想上游依赖 | 不应依赖 |
| --- | --- | --- |
| `cc-types` | 外部基础库，例如 `serde`、`chrono`、`uuid` | 任意 `cc-*` runtime crate、root、UI、filesystem/runtime owner |
| `cc-ipc-protocol` | `cc-types`、serde 类 wire 依赖 | `cc-ipc`、`cc-ipc-client`、engine、UI、root |
| `cc-utils` | 外部纯 helper 依赖，必要时依赖 `cc-types` | 配置持久化、API、engine、UI、daemon、root |
| `cc-config` | `cc-types`、`cc-utils` | engine、tools runtime、UI、daemon、root |
| `cc-models` | `cc-types`、`cc-config` 中稳定配置 DTO | API transport、engine、UI、root |
| `cc-auth` | `cc-types`、`cc-config`、`cc-utils` | `cc-api` provider client、engine、UI、root |
| `cc-api` | `cc-types`、`cc-models`、`cc-auth`、`cc-config`、`cc-observability` | engine lifecycle、query loop、UI、daemon、root |
| `cc-observability` | `cc-types` 和 tracing/metrics 外部库 | engine、UI、daemon、root binary state |
| `cc-keybindings` | `cc-types`、`cc-config` | `cc-ui` terminal state、commands runtime、root |
| `cc-bootstrap` | `cc-config`、`cc-auth`、`cc-observability` 的稳定 contract | engine/query/tool runtime、UI、daemon、root-private startup modules |
| `cc-session` | `cc-types`、`cc-config`、`cc-utils` | engine runtime、UI、daemon、root |
| `cc-compact` | `cc-types`、`cc-models`、必要的 provider/adapter contract | query loop、UI、root-private transcript state |
| `cc-permissions` | `cc-types`、`cc-config`、`cc-sandbox` 的 contract | UI prompt implementation、engine-private tool registry、root |
| `cc-sandbox` | `cc-types`、`cc-config`、platform 外部库 | engine、tools runtime、UI、root |
| `cc-skills` | `cc-types`、`cc-config`、`cc-utils` | engine lifecycle、MCP runtime manager、UI、root |
| `cc-tools` | `cc-types`、`cc-permissions` contract、必要的 core domain DTO | engine lifecycle、UI、daemon、teams/plugins runtime internals、root |
| `cc-mcp` | `cc-types`、`cc-config`、`cc-tools` contract、auth/config DTO | `cc-browser`、engine lifecycle、UI、root |
| `cc-browser` | `cc-types`、`cc-config`、必要的 tool identity DTO | `cc-mcp` runtime、`cc-engine`、UI、root |
| `cc-computer-use` | `cc-types`、`cc-config`、`cc-tools` contract、platform 外部库 | engine-private registry、UI、browser/MCP internals、root |
| `cc-lsp-service` | `cc-types`、`cc-config`、`cc-tools` command/tool adapter contract | UI renderer、engine lifecycle、root |
| `cc-services` | `cc-types`、`cc-config`、`cc-session`、明确的 service adapter contract | UI、root、engine-private globals |
| `cc-tasks` | `cc-types`、`cc-session` 或 persistence contract | UI、daemon worker internals、engine-private state、root |
| `cc-teams` | `cc-types`、`cc-tasks`、`cc-session`/memory contract、`cc-tools` contract | UI、daemon process internals、root |
| `cc-plugins` | `cc-types`、`cc-config`、`cc-mcp` contract、`cc-tools` metadata contract | engine lifecycle、UI、root |
| `cc-daemon` | `cc-types`、`cc-config`、`cc-session`、`cc-tasks`、`cc-teams`/plugin adapter contract | `cc-ui`、root-private startup modules、engine internals |
| `gateway` | `cc-daemon` public control-plane contract、`cc-types`、HTTP 外部库 | UI、root-private state、engine internals |
| `cc-engine` | `cc-types`、`cc-models`、`cc-api`、`cc-session`、`cc-compact`、`cc-permissions`、`cc-tools`、`cc-mcp`、`cc-browser` 等显式 contract | `cc-ui`、`cc-ipc-client`、root、daemon process internals |
| `cc-query` | `cc-types`、`cc-engine` public adapter、stream/cancellation DTO | UI、root、tool implementation internals |
| `cc-commands` | `cc-types`、`cc-config`、`cc-session`、`cc-tools`/domain command contract | UI renderer、root-private state、engine internals，除非通过 adapter 注入 |
| `cc-ipc` | `cc-ipc-protocol`、`cc-ipc-client` contract、明确的 runtime adapters | UI terminal internals、root-private modules、未通过 adapter 暴露的 engine internals |
| `cc-ipc-client` | `cc-ipc-protocol`、`cc-types`、client transport dependencies | `cc-ipc` server runtime、engine、UI、root |
| `cc-ui` | `cc-types`、`cc-ipc-protocol`、`cc-commands` DTO、`cc-keybindings`、UI-facing adapters | root process lifecycle、engine/daemon/MCP/browser implementation internals |
| `claude-code-rs` | 所有需要组装的 workspace library crate | 不作为任何 library crate 的依赖目标 |

## 候选拆分 Crate

Codex 把若干易膨胀能力拆成独立 crate。本项目补 full build 时，可以按以下顺序新增
或调整 crate。是否新增取决于实际代码体量；如果现有 crate 内模块足够小，也可以先
作为内部模块落地，但依赖方向应提前按下表约束。

| 候选 crate | Codex 对应经验 | 目标职责 | 理想上游依赖 | 不应依赖 |
| --- | --- | --- | --- | --- |
| `cc-execpolicy` | `codex-execpolicy` | prefix-based command policy、审批规则、可解释决策 | `cc-types`、`cc-config`、`cc-utils`、`shlex`、可选 `starlark` | engine、UI、root、具体 tool runtime |
| `cc-shell-command` | `codex-shell-command` / `codex-shell-escalation` | shell 解析、展示、风险摘要、Unix escalation adapter | `cc-types`、`cc-config`、`cc-execpolicy` contract、platform 外部库 | UI renderer、engine lifecycle、root |
| `cc-file-search` | `codex-file-search` | ignore-aware 文件枚举、fuzzy match、TUI completion 数据源 | `cc-types`、`cc-config`、`ignore`、`nucleo` 或等价 matcher | engine、MCP、browser、root |
| `cc-git-utils` | `codex-git-utils` | repo detection、branch/status helper、路径归一化 | `cc-types`、`cc-utils`、轻量 git 外部库或 shell adapter | UI、engine-private state、root |
| `cc-state` | `codex-state` | turn/session runtime state、已提示依赖、临时环境注入 | `cc-types`、`cc-config`、`cc-session` contract | UI、root、MCP/browser implementation internals |
| `cc-core-skills` | `codex-core-skills` | 内置 skill 打包、skill metadata、dependency contract | `cc-types`、`cc-config`、`cc-utils`、`include_dir` | engine lifecycle、MCP runtime manager、UI、root |
| `cc-terminal-detection` | `codex-terminal-detection` | 终端能力、颜色、clipboard/platform probing | `cc-types`、platform 外部库 | engine、MCP、browser、root |
| `cc-sandbox-summary` | `codex-utils-sandbox-summary` | sandbox/approval human-readable summary | `cc-types`、`cc-sandbox` contract、`cc-permissions` contract | UI state、engine lifecycle、root |

这些候选拆分不应制造新的横向 runtime 依赖。拆分的判据是“能力是否有独立 owner 和
稳定 contract”，不是“Codex 有同名 crate”。

## Runtime 边界细则

- `cc-engine` 可以编排模型、工具、权限、browser/MCP prompt 行为，但对 UI、IPC
  client、daemon 和 root 的交互必须通过 adapter contract。
- `cc-query` 负责 turn/stream/cancellation 边界，不应成为工具实现或 UI 状态 owner。
- `cc-tools` 负责 tool trait、schema、metadata 和 registry policy；具体执行如果需要
  teams、plugins、daemon 或 UI，应通过 adapter 或留在对应 owner。
- `cc-daemon` 和 `gateway` 可以组成后台控制面，但不应回读 root startup state 或 UI
  terminal state。
- `cc-ui` 应是可复用 Rust TUI 层；terminal raw-mode、signal handling、进程启动顺序
  留在 `claude-code-rs` root glue。

## Crate 职责

### `cc-browser`

拥有浏览器专属行为：

- Browser MCP 工具名分类。
- 基于纯 server/tool identity DTO 的 browser server detection。
- 浏览器权限分类。
- 浏览器工具结果渲染。
- 第一方 Chrome native host、bridge、session/state、setup、transport 和
  diagnostics。

`cc-browser` 应只消费稳定 DTO 或窄契约。它不应该需要 `Arc<dyn Tool>`、
`McpManager`、root 启动状态或 engine-private 工具 registry。

### `cc-mcp`

拥有 MCP 行为：

- MCP server config、discovery、auth、transport 和 runtime manager。
- MCP protocol DTO，以及 tool/resource definitions。
- 非浏览器专属的 MCP tool wrapper 行为。

`cc-mcp` 可以通过稳定 DTO 暴露 config/tool identity 数据。它不应该直接调用
browser detection 或 result rendering。

### `cc-tools`

拥有稳定工具契约：

- Tool trait 或等价的工具元数据契约。
- 非 engine crate 使用的 tool identity DTO。
- 非 engine-private 的 schema 和 registry policy helper。

长期目标是让 browser detection 消费这一层的 tool identity 数据，而不是依赖
`cc_engine::types::tool::Tool`。

### `cc-engine`

消费领域契约：

- 基于 browser 和 MCP snapshot 构建 system prompt section。
- 使用 `cc-browser` API 应用权限和渲染行为。
- 通过 `cc-tools` / MCP adapter 协调工具执行。

`cc-engine` 不应该成为可复用 browser detection 契约的 owner。

### `claude-code-rs`

只拥有二进制 wiring：

- CLI 解析和启动模式选择。
- Runtime adapter 安装。
- 将当前 root/bootstrap 数据转换为共享 DTO。
- 最终 process lifecycle glue。

迁移期间保留的任何 root-only adapter 都应是临时的，不应成为 browser/MCP 行为的
owner。

## Browser Detection 目标形态

Browser server detector 应该是纯函数，不应该接收 root-private runtime object。

推荐契约形态：

```rust
pub struct BrowserServerHint {
    pub name: String,
    pub browser_mcp: bool,
}

pub struct ToolIdentity {
    pub user_facing_name: String,
}

pub fn detect_browser_servers(
    servers: impl IntoIterator<Item = BrowserServerHint>,
    tools: impl IntoIterator<Item = ToolIdentity>,
) -> HashSet<String>;
```

具体命名可以不同，重要的是依赖规则：`cc-browser` 接收 plain data，而不是 engine
或 MCP runtime 类型。

启动时，root 或其他 composition layer 可以把当前 runtime object 适配为这些 DTO：

```text
cc_mcp::McpServerConfig -> BrowserServerHint
Arc<dyn Tool>           -> ToolIdentity
```

这个 adapter 属于 composition boundary。它不应迫使 `cc-browser` 依赖 `cc-mcp`
或 `cc-engine`。

## 当前 Root Browser 模块的期望终态

当前 root 模块 `crates/claude-code-rs/src/browser/` 应在剩余 live-tool detection
adapter 不再需要 root 或 engine-private 类型后消失。

可接受的终态是：

- 纯 detection primitives 位于 `cc-browser`。
- Browser registry install/snapshot helper 位于 `cc-browser`。
- Browser permission 和 result rendering 位于 `cc-browser`。
- 启动代码将 root/runtime 数据映射为共享 DTO，并调用 `cc-browser`。
- `crates/claude-code-rs/src/browser` 下不再保留可复用 browser 行为。

## 反模式

不要通过以下方式解决迁移压力：

- 为了访问 `Tool` 添加 `cc-browser -> cc-engine`。
- 为了访问 `McpServerConfig` 添加 `cc-browser -> cc-mcp`。
- 让 `cc-mcp -> cc-browser`，在 discovery 期间分类 browser-specific 工具。
- 添加只用于保留旧 `crate::browser::*` 路径的 compatibility crate 或 re-export
  layer。
- 添加隐藏 adapter 未安装问题的全局静默 fallback。

当两个 runtime crate 需要同一份信息时，应将共享信息移动到 DTO 或 contract crate，
而不是创建横向 runtime 依赖。
