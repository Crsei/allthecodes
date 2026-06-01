# cc-rust 对标 `claude-code-bun` 差异、Web UI 评估与落地规划

> 更新日期: 2026-04-18
> 范围: `F:\AIclassmanager\cc\claude-code-bun` 对比 `F:\AIclassmanager\cc\rust`
> 目标: 把 “Rust 还没实现的部分、哪些能在当前架构内稳妥实现、Web 端 UI 进行度、以及 REPL/项目结构差异” 收敛到一份文档里。

## 1. 结论摘要

- `cc-rust` 已经具备继续追赶的核心基础: QueryEngine、tools、slash commands、MCP client、headless IPC、OpenTUI 前端、Web UI 前端都已落地。
- 当前最大差距不在模型调用或基础工具，而在三类上游集成能力:
  - 浏览器集成链路: `run in chrome` / `--chrome` / native host / 扩展探测 / browser MCP。
  - 单一 REPL 入口: `claude-code-bun` 以 `main.tsx -> replLauncher.tsx -> screens/REPL.tsx` 为中心，`cc-rust` 目前分散为 `src/ui/`、`ui/`、`web-ui/` 三套前端表面。
  - 前端状态一致性: Web UI 只覆盖了部分 SDK 事件与交互路径，仍偏“最小可用调试面板”。
- 对 `rust-lite` 来说，最稳妥的路线不是直接复刻 `claude-code-bun` 的超大 REPL，而是先把当前已有三层架构打通:
  - 先补齐 Web UI 当前缺口。
  - 再统一 `headless`/OpenTUI/Web 的事件与状态模型。
  - 最后再做 Chrome/browser 集成基础设施。

## 2. 当前对标快照

### 2.1 `claude-code-bun` 已有而 `cc-rust` 明显缺失的部分

| 领域 | `claude-code-bun` 现状 | `cc-rust` 现状 | 判断 |
|------|------------------------|----------------|------|
| Chrome 集成 | `src/main.tsx` 支持 `--chrome` / `--no-chrome`；`src/utils/claudeInChrome/setup.ts` 负责扩展探测、native host、动态 MCP 配置、system prompt 注入；`src/commands/chrome/chrome.tsx` 提供交互命令 | 无 `--chrome`、无浏览器扩展探测、无 native host、无 browser MCP server、无 `/chrome` | 明确 gap |
| 交互式 REPL 主入口 | `main.tsx -> replLauncher.tsx -> screens/REPL.tsx`，REPL 承担消息、权限、MCP、remote、bridge、fullscreen、commands 等集中控制 | Rust 侧拆为 `src/ui/`、`ui/`、`web-ui/` 三套界面 | 架构差异，非单点缺功能 |
| 浏览器端能力挂接 | Chrome 相关工具以 MCP 形式注入，REPL 可见、命令可控、prompt 可增强 | `cc-rust` 有 `src/mcp/` 客户端能力，但无 browser MCP 的内建接入层 | 可扩展但未实现 |
| REPL 生态能力 | `screens/REPL.tsx` 吸收了 MCP、bridge、assistant、remote session、权限桥、消息重放等复杂交互 | Rust 的 TUI/OpenTUI/Web UI 各自只承载其中一部分 | 需要先统一前端战略 |

### 2.2 `cc-rust` 已有、可以作为后续落地基础的部分

| 基础设施 | 证据 | 说明 |
|----------|------|------|
| Web UI 服务端 | `src/main.rs` 的 `--web` / `--web-port` / `--no-open`，`src/web/mod.rs`，`src/web/handlers.rs` | 已有 Axum API + SSE + 静态资源嵌入 |
| Web UI 前端 | `web-ui/src/*` | 已有聊天页、侧栏、调试面板、流式内容块渲染 |
| Headless IPC 前端 | `ui/src/main.tsx`，`src/ipc/*` | 已有 OpenTUI 前端与 JSONL 协议 |
| 内置 TUI | `src/ui/tui.rs`，`src/ui/app.rs` | 仍可用，但已不再是唯一前端形态 |
| MCP 客户端 | `src/mcp/{client,discovery,manager,tools}.rs` | 能接外部 MCP server，是后续 browser 工具接入的基础 |
| Computer Use | `src/computer_use/*` + `--computer-use` | 已有本地桌面控制能力，但不是 Chrome/浏览器扩展集成 |
| Slash commands | `src/commands/*` + `web-ui/src/lib/api.ts` 的 `/api/command` | Web UI 已能调用服务端斜杠命令 |

## 3. 分层判断: 哪些部分可以确保 Rust 实现

这里按三层判断:

- A 类: 当前 `cc-rust` 架构内可直接推进，落地风险可控。
- B 类: Rust 可以做，但需要新增子系统或扩出 `rust-lite` 的边界。
- C 类: 当前不建议作为 `rust-lite` 近期目标。

### 3.1 A 类: 当前架构内可确保实现

| 项目 | 当前依据 | 为什么可确保实现 | 建议优先级 |
|------|----------|------------------|------------|
| `--web` 真正自动打开浏览器 | `src/dashboard.rs` 已有跨平台 `open_browser()`；`src/web/mod.rs` 目前只打印日志 | 这是纯本地启动体验问题，已有现成 helper 可复用 | P0 |
| Web UI 补齐更多 SDK 事件 | `web-ui/src/lib/api.ts` 已处理 `system_init` / `assistant` / `stream_event` / `result` / `tool_use_summary` | 只需继续把 `user_replay`、`compact_boundary`、权限事件、更多 tool 结果类型纳入 store 和组件 | P0 |
| Web UI 会话/状态一致性 | `src/web/handlers.rs` 已暴露 `/api/state`、`/api/settings`、`/api/command`；前端已有 Zustand store | 服务端接口与前端 store 都在，主要是补状态映射与 UX | P0 |
| Web UI 命令/权限交互增强 | 当前已有 `Sidebar`、`CommandPalette`、`PermissionPanel`，但没有运行时权限弹层 | 权限请求本质上是事件流和弹窗问题，不需要重写引擎 | P1 |
| 长对话渲染优化 | 当前前端无 transcript 虚拟列表，`npm run build` 产物约 `369 kB` 主 JS | 这是纯前端工程问题，可独立优化 | P1 |
| 三套前端共用事件标准化 | `SdkMessage` 已存在；Web 与 headless 都建立在消息流之上 | 可以抽一个共享“前端视图模型”层，减少 Web/OpenTUI/TUI 重复适配 | P1 |
| 外部 browser MCP 的接入与展示 | Rust 已有 `src/mcp/` 客户端体系 | 即便没有内建 Chrome 扩展，也能先支持接入外部 browser MCP server | P1 |

### 3.2 B 类: Rust 可以实现，但需要新增子系统

| 项目 | 现状 | 为什么不是 A 类 | 建议 |
|------|------|------------------|------|
| 完整 `run in chrome` / `--chrome` | 当前完全缺失 | 不是简单开浏览器，而是一整套 CLI flag、扩展探测、native host、browser MCP、prompt、命令面板联动 | 先拆成里程碑，不要一次性复刻 |
| `claude-code-bun` 风格的一体化 REPL | Rust 目前有 `src/ui/`、`ui/`、`web-ui/` 三套前端 | 这涉及产品方向决定，而不只是补代码 | 先选主前端，再决定是否统一 |
| Browser-native MCP server 内建 | 目前只有 MCP client，没有内建 browser MCP server | 需要新的 server/runtime/扩展通信边界 | 若做，单独建 `browser/` 子系统 |
| REPL 级 bridge / remote / assistant 生态 | `claude-code-bun` 的 REPL 承担很多远程与桥接流 | 这些能力超出 `rust-lite` 当前主线 | 除非产品目标改变，否则延后 |

### 3.3 C 类: 当前不建议纳入 `rust-lite` 近期目标

| 项目 | 原因 |
|------|------|
| 完整复刻 `screens/REPL.tsx` 的全部模式与旁路能力 | 会把 Rust 项目重新拖回一个超大前端单体，和当前分层方向冲突 |
| `claude-code-bun` 式 assistant install wizard / remote attach / bridge 体系 | 这不是 lite 版本的最短路径，会显著放大维护面 |
| 企业级 MCP 政策、远程受管设置、复杂 remote session 生命周期 | 与当前 `cc-rust` 的核心目标不匹配 |

## 4. `run in chrome` 专项判断

### 4.1 `claude-code-bun` 的真实组成

`run in chrome` 在 `claude-code-bun` 里不是一个小功能，而是以下链路的组合:

1. CLI 层:
   - `src/main.tsx` 提供 `--chrome` / `--no-chrome`。
2. 配置与启用逻辑:
   - `src/utils/claudeInChrome/setup.ts` 处理默认启用、自动启用、环境变量、订阅约束。
3. 浏览器扩展探测:
   - 扫描 Chromium 浏览器数据目录，判断扩展是否安装。
4. Native host 安装:
   - 创建 wrapper script。
   - 安装 native host manifest。
   - Windows 写注册表。
5. MCP 接入:
   - 为 Chrome 暴露 `mcp__claude-in-chrome__*` 工具。
   - 把 browser 工具注入 REPL 可用工具池。
6. Prompt 与命令层:
   - 注入 Chrome system prompt。
   - 通过 `/chrome` 命令展示安装、重连、权限管理入口。

### 4.2 `cc-rust` 当前缺什么

| 缺口 | 当前现状 |
|------|----------|
| CLI flag | `src/main.rs` 没有 `--chrome` / `--no-chrome` |
| 浏览器扩展探测 | 无 |
| native host 安装 | 无 |
| browser MCP server | 无 |
| browser prompt 注入 | 无 |
| `/chrome` 命令 | 无 |
| 浏览器权限/连接状态 UI | 无 |

### 4.3 但 Rust 不是完全没基础

- `src/mcp/*` 已能消费外部 MCP server。
- `src/computer_use/*` 已有本地桌面控制能力，可作为浏览器自动化之外的补充。
- `src/dashboard.rs` 已有跨平台 `open_browser()` helper，可先把“打开浏览器”与“控制浏览器”这两个概念分开。

### 4.4 建议的 Rust 落地顺序

1. 先修正 `--web` 的真实行为:
   - `--web` 启动后可选自动打开默认浏览器。
   - 当前 `--no-open` 只是“不打印提示”，并没有实际 open 行为。
2. 再支持“外部 browser MCP server 接入”:
   - 让用户能通过现有 MCP 配置接入浏览器工具。
   - Web/OpenTUI 能显示 browser tool call。
3. 再决定是否做“内建 Chrome 集成”:
   - 如果继续对标 `claude-code-bun`，单独新增 `src/browser/` 或 `src/chrome/` 子系统。
4. 最后才考虑 `--chrome` 命令级体验:
   - CLI flag。
   - `/chrome` 命令。
   - 扩展状态面板。
   - native host 安装与重连。

结论:

- “自动打开 Web UI 页面” 是 A 类，马上能做。
- “完整 run in chrome” 是 B 类，能做，但必须拆成子项目。

## 5. Web 端 UI 当前进行度

### 5.1 已完成部分

以 2026-04-18 实际代码与构建结果为准，Web UI 已不是占位页:

- 前端工程可正常构建:
  - `web-ui` 执行 `npm run build` 成功。
- 已有聊天主流程:
  - `ChatPanel` + `InputBar` + `MessageList`。
- 已接入 SSE 流式响应:
  - `web-ui/src/lib/api.ts` 解析 `stream_event`，支持 text/thinking/tool_use 增量。
- 已支持 content block 渲染:
  - `AssistantMessage.tsx` 能显示 text、tool_use、tool_result、thinking、redacted_thinking。
- 已支持服务端设置和命令:
  - `/api/settings` 与 `/api/command` 已接通。
- 已有调试视图:
  - `DebugPanel` 包含 Events / Messages / Timeline 三个 tab。

判断:

- 它已经达到“可用的浏览器聊天前端”。
- 但距离 `claude-code-bun` 的完整 REPL 前端还差一层“运行时控制台”。

### 5.2 目前仍明显缺失的部分

| 方向 | 当前问题 |
|------|----------|
| 事件覆盖不完整 | `api.ts` 没有真正消费 `user_replay`、`compact_boundary` 等事件 |
| 权限交互缺失 | 没有像终端 REPL 那样的运行时权限请求弹层 |
| 会话与恢复能力弱 | 当前主要围绕单会话聊天，不含 session list / resume / transcript management |
| 前端信息架构偏调试态 | 侧栏更像设置抽屉，缺少真正的 workspace/context/session 结构 |
| 长对话性能未优化 | 目前没有 transcript virtualization，长会话会越来越重 |
| 移动端与窄屏策略简单 | DebugPanel 在小屏直接隐藏，Sidebar 也是桌面思路 |
| 视觉层仍偏默认模板 | 配色和布局功能够用，但没有形成产品层级和状态层级 |

### 5.3 优化建议

#### P0: 先补功能完整性

- 补全所有关键 SDK 事件的前端消费:
  - 至少补 `user_replay`、`compact_boundary`、更完整的 tool_result 与错误态。
- 增加运行时权限请求 UI:
  - Web 端不应只有静态 `PermissionPanel`，还需要“当前工具请求批准”的模态层。
- 修正 clear / rewind / command 后的状态一致性:
  - 当前 `/api/command` 的 `clear` 只清理前端消息，不代表引擎上下文一定同步清空。

#### P1: 再补产品层体验

- 新增 Session 维度:
  - 当前会话信息、历史会话、resume 入口、当前 workspace/cwd 显示。
- 新增工具维度:
  - Tool stream 过滤器、Tool timeline、失败工具聚合。
- 新增连接维度:
  - 断线重连、流中 abort、重新附着当前会话。

#### P1: 做性能和结构优化

- Transcript 虚拟列表。
- DebugPanel 与 Markdown 渲染按需加载。
- 只在需要时保留 raw event 日志，避免长会话内存持续增长。

#### P2: 做视觉重构

- 不要继续停留在“典型管理后台/默认 Tailwind 面板”。
- 建议把信息架构改为三层:
  - 顶部: workspace + session + model 状态。
  - 中间: transcript / tools / debug 可切换主视区。
  - 侧边: settings / permissions / command palette。
- 明确 streaming、thinking、tool、system message 的层级颜色与边框语言。

## 6. REPL 与项目结构对比

### 6.1 关键区别

| 维度 | `claude-code-bun` | `cc-rust` |
|------|-------------------|-----------|
| 启动中心 | `src/main.tsx` | `src/main.rs` |
| REPL 挂载 | `replLauncher.tsx -> screens/REPL.tsx` | `src/ui/tui.rs` 或 `ui/src/main.tsx` 或 `--web -> web-ui` |
| REPL 主体 | 一个超大的 `screens/REPL.tsx` | 多前端表面分散实现 |
| 前端模式 | 以本地终端 REPL 为中心，其他模式向它汇聚 | TUI / headless+OpenTUI / Web 三套并存 |
| MCP 集成位置 | 深度嵌入 `main.tsx` 与 REPL 生命周期 | Rust 侧已有 MCP client，但前端对 MCP 呈现分散 |
| 浏览器集成 | Chrome 作为独立系统接入 REPL | 尚未进入 Rust 主流程 |

### 6.2 `claude-code-bun` 的 REPL 结构

最短调用链:

```text
src/main.tsx
  -> src/replLauncher.tsx
  -> src/components/App.tsx
  -> src/screens/REPL.tsx
```

它的优点:

- 单入口，很多交互逻辑集中。
- MCP、commands、permissions、fullscreen、remote 等都能在同一运行时状态下组合。

它的缺点:

- `REPL.tsx` 非常重。
- 大量功能耦合在一个巨型前端文件与其 hooks 生态上。

### 6.3 `cc-rust` 的 REPL/前端结构

当前更像“三层表面 + 一个核心引擎”:

```text
src/main.rs
  -> src/ui/*                 内置 ratatui TUI
  -> src/ipc/* + ui/src/*     headless + OpenTUI
  -> src/web/* + web-ui/src/* 浏览器 Web UI
```

它的优点:

- 引擎和前端边界更清楚。
- Web 与终端前端都能围绕同一个消息流演进。
- 后续更适合做“多前端共享核心”。

它的缺点:

- 消息适配、状态模型、组件能力重复。
- 同一个产品能力要在三套前端里各写一遍。
- 当前没有一个像 `screens/REPL.tsx` 那样的“唯一产品主界面”。

### 6.4 一个容易混淆但必须澄清的点

`src/tools/repl.rs` 的 `ReplTool` 只是“执行代码片段的工具”，不是 `claude-code-bun` 意义上的交互式 REPL UI。

所以后续谈 “对标 REPL” 时，应该对标的是:

- `claude-code-bun/src/screens/REPL.tsx`
- 而不是 Rust 的 `src/tools/repl.rs`

## 7. 推荐的目标结构

不建议把 Rust 直接重写成 Bun 的单文件 REPL，而建议收敛成如下结构:

```text
src/
  engine/          核心会话与消息流
  query/           回合执行
  tools/           工具
  commands/        斜杠命令
  mcp/             MCP client
  ipc/             headless 协议
  web/             Web API / SSE / static serving
  frontend/
    presenter/     共享 ViewModel / event normalization
    terminal/      OpenTUI 主前端
    web/           Web UI 主前端
  browser/         可选: 后续 Chrome/browser 集成
```

具体建议:

- `src/ui/` 的 ratatui TUI 进入 legacy/fallback 状态，不再继续作为主投入方向。
- `ui/` 与 `web-ui/` 共享一套事件归一化层，而不是各自重复解释 `SdkMessage`。
- 如果未来要做 `run in chrome`，新能力应进入独立 `browser/` 子系统，不要散落进 `web/` 或 `mcp/`。

## 8. 分阶段路线图

### Phase 1: 先把当前 Web 能力补齐

- 让 `--web` 真正支持自动打开浏览器。
- 补全 Web UI 对 SDK 事件、权限请求、会话状态的覆盖。
- 补 transcript 性能与移动端布局。

交付结果:

- Web UI 从“可用 demo”提升到“稳定前端”。

### Phase 2: 统一前端视图模型

- 抽取 shared event/view-model 层。
- 让 `ui/` 与 `web-ui/` 共享消息、工具、权限、usage 的解释逻辑。
- 冻结 `src/ui/` 为 fallback，不再继续堆功能。

交付结果:

- 新功能不再需要在三处重复接线。

### Phase 3: 先做 browser MCP 接入，而不是直接做 `--chrome`

- 允许通过现有 MCP 配置接入外部 browser MCP server。
- 在 Web/OpenTUI 中正确展示 browser tool。
- 增加 browser tool 专用的 tool rendering 和权限提示。

交付结果:

- 用户已经能“在 Rust 里使用浏览器工具”，即便还没有完整 Chrome 扩展链路。

### Phase 4: 再评估是否做内建 `run in chrome`

- CLI flag。
- 扩展探测。
- native host 安装。
- `/chrome` 命令。
- browser MCP server 内建。

交付结果:

- 达到 `claude-code-bun` 的 Chrome 集成方向。

## 9. 最终建议

近期不要把目标写成“完整复刻 `claude-code-bun` REPL”。

更合理的目标是:

1. 先把 `cc-rust` 现有 Web/OpenTUI 前端变成真正可长期维护的主界面。
2. 用共享事件模型收敛三套前端的重复逻辑。
3. 以 MCP 接入为中间层，先获得浏览器工具能力。
4. 最后才决定要不要把 `run in chrome` 做成内建的第一方能力。

这条路线更符合 `rust-lite` 的现实边界，也更容易持续交付。
