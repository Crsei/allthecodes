# allthecodes Web 端建设计划：Next.js + React + xterm.js

> 日期：2026-05-27
> 范围：`/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes-web/**`、`crates/allthecodes-web/**`、`crates/allthecodes-ipc*`、`crates/allthecodes/src/ui/**`、`crates/allthecodes-session/**`。
> 目标：为 allthecodes 建设一个 Web 端工作台，使用 Next.js + React + xterm.js，参考 hermes-web-ui 的视觉风格，同时支持普通网页对话系统和真实 TUI 展示。

## 背景

allthecodes 当前已经具备 Web 服务和运行时基础：

- `crates/allthecodes-web` 提供 Axum Web 服务入口，已有 `/api/chat`、`/api/state`、`/api/settings`、`/api/command`、`/api/sessions` 等路由。
- Next.js + React 前端项目在独立同级目录 `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes-web` 中实现。
- Rust Axum 服务、WebSocket、debug actions、PTY bridge 和静态嵌入逻辑仍在 allthecodes 仓库的 `crates/allthecodes-web/src` 中实现。
- `crates/allthecodes-web/src/static_files.rs` 当前预留的嵌入路径需要从历史的 `../../web-ui/dist` 调整为同级前端项目的构建产物路径，例如相对 crate manifest 的 `../../../allthecodes-web/dist`。
- `crates/allthecodes-ipc-protocol` 已定义 `FrontendMessage` / `BackendMessage`，可以支撑结构化前端事件流。
- `crates/allthecodes-ipc/src/headless.rs` 已有 JSONL headless runtime，可以作为 WebSocket IPC runtime 的协议基础。
- `crates/allthecodes/src/ui` 已有完整 Rust TUI，但它依赖终端能力，不应被硬改成 DOM UI。

本计划基于两个参考项目的调研结论：

- `claudecodeui`：React + Express，聊天 WebSocket 与 shell PTY WebSocket 分离；浏览器终端使用 `@xterm/xterm`，后端用 `node-pty`。allthecodes 不应引入 Express/node-pty，但应借鉴“结构化聊天流”和“真实终端流分离”的架构。
- `hermes-web-ui`：Vite + Vue + Pinia，不是 React/Next 项目；但其 Pure Ink 视觉语言、紧凑工作区布局、会话级实时状态管理、右侧 workspace drawer 和 xterm terminal panel 很适合作为 allthecodes Web UI 的设计参考。

## 实现归属

Web 前端项目归属于 `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes-web`，不是 `allthecodes/crates/allthecodes-web/web-ui`，也不是 `allthecodes/web-ui`。

归属规则：

- Next.js + React 前端源码：`/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes-web/**`。
- Next.js static export 产物：`/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes-web/dist/**`。
- Axum 路由、SSE、WebSocket、PTY bridge、debug actions：`/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes/crates/allthecodes-web/src/**`。
- 前端测试和测试工件约定：`/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes-web/test-results/**` 或 allthecodes workspace `target/web-ui-test-results/**`。
- root binary `crates/allthecodes` 只负责初始化 engine 并调用 `allthecodes_web::start_server`，不承载 Web 前端源码。

这样做的目的：

- 前端工程拥有独立 git 边界和 npm/Next.js 工具链。
- allthecodes Rust workspace 只承载后端运行时接入和静态资源服务。
- 避免在 allthecodes 仓库内新增容易与 npm package wrapper 混淆的 `web-ui/` 或 `crates/allthecodes-web/web-ui/`。
- 前端 build/test 在 `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes-web` 中执行；Rust route、WebSocket、debug action tests 在 allthecodes 仓库中执行。

## 核心目标

建设一个面向 coding agent 的本地 Web 工作台：

- 普通网页端对话系统：以 React 组件渲染结构化消息、tool use、tool result、permission、thinking、plan、usage 和 session history。
- TUI 展示：通过 xterm.js 连接后端 PTY WebSocket，真实运行 `allthecodes` TUI，保留 Ratatui/crossterm 行为；Web 端不得解释、重排、转换或改写 TUI 输出。
- 统一工作区：左侧 session/navigation，中间 chat，右侧 workspace drawer，drawer 内承载 Files、Terminal/TUI、Inspector 等面板。
- Agent 调试通道：构建过程必须保留可由开发者或 agent 使用的 diagnostics/debug surfaces，用于检查显示效果、原始事件流、对话记录、工具执行状态、权限请求、TUI 终端输出和测试截图。
- 与 Rust 后端保持单一事实来源：session、engine、工具权限、配置和运行时状态均由 Rust 管理；前端只持有 UI 状态和流式缓存。
- 不复制 AGPL 参考项目源码；只借鉴架构模式和 UX 规则。

## 非目标

本阶段不做以下事情：

- 不引入 Express、Node server、node-pty、Socket.IO 作为 allthecodes 的生产后端。
- 不把 xterm.js 当普通聊天消息渲染器。
- 不把 Rust TUI 逐组件翻译成 React DOM。
- 不让普通 Web chat 和 TUI tab 同时主动驱动同一个 session，除非已经实现 session ownership、锁定或只读观察策略。
- 不把 hermes-web-ui 的 Vue/Pinia/Naive UI 代码迁入 allthecodes。
- 不实现远程多用户协作、云部署鉴权、公开公网访问。
- 不在第一阶段实现插件 marketplace、动态图形化插件前端、复杂 IDE 文件编辑器。

## 总体架构

```text
Browser
  Next.js static-export React app
  source: /data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes-web
    - Chat workspace
    - Session sidebar
    - Message typed renderers
    - xterm.js TUI/terminal panel
    - Settings/commands surface
    - Debug drawer / event inspector
        |
        | HTTP REST
        | SSE stream
        | WebSocket IPC
        | WebSocket PTY
        v
Rust Axum server: allthecodes/crates/allthecodes-web/src
  - /api/state
  - /api/sessions
  - /api/chat              ordinary chat SSE
  - /api/abort
  - /api/settings
  - /api/command
  - /api/ipc/ws            structured FrontendMessage/BackendMessage, phase 2+
  - /api/tui/ws            xterm PTY bridge
  - /api/debug/*           dev-only diagnostics, phase-gated
        |
        v
QueryEngine / IPC / TUI / Session storage / diagnostics logs
```

两条实时通道必须分开：

- 普通网页对话：结构化事件。优先用现有 `/api/chat` SSE 输出 `SdkMessage`，后续可升级到 `/api/ipc/ws` 输出 `BackendMessage`。
- TUI 展示：字节流终端。`/api/tui/ws` 只负责 PTY 输入、输出、resize、exit，不参与普通 chat message state。

TUI 展示的不可变规则：

- xterm.js 只作为浏览器里的终端模拟器，显示 PTY 输出并发送键盘输入。
- `/api/tui/ws` 不解析 assistant/user/tool 消息，不把 ANSI 输出转换为 React message，也不根据 Web UI 状态重排 TUI 内容。
- TUI 中的对话过程、快捷键、权限提示、流式输出和布局仍由 Rust TUI 决定；Web 端只会因终端尺寸、字体、颜色、浏览器快捷键等终端环境因素产生可见差异。
- 如果同一个 session 已被 TUI tab 占用，普通 Chat tab 默认应进入只读状态或要求用户显式接管；反向亦然。

## 技术选型

### 前端

- Next.js App Router。
- React + TypeScript。
- xterm.js：`@xterm/xterm`、`@xterm/addon-fit`、`@xterm/addon-web-links`，可选 `@xterm/addon-webgl`。
- 状态管理：建议 Zustand 或 Jotai。避免 Redux boilerplate；避免把 server state 全塞入 localStorage。
- 数据请求：轻量 `fetch` wrapper + SSE reader；后续可引入 TanStack Query 管理 session/settings 的 cache。
- 组件模板：使用 `shadcn/ui` 作为基础组件模板，底层 Radix primitives + Tailwind/CSS variables，先保证开发速度、一致性和可访问性，后续在此基础上定制 Pure Ink 风格。
- 样式：以 shadcn/ui 的 theme variables 和 Tailwind tokens 为基础，补充 allthecodes 自己的语义 token。避免引入 MUI、Ant Design、Naive UI 等重型运行时 UI 框架。
- 图标：`lucide-react`。
- Markdown：`react-markdown` + `remark-gfm` + 代码块复制；Mermaid 和复杂图表后置。

### 后端

- 继续使用 Rust Axum。
- WebSocket 使用 Axum 内置 ws 支持。
- PTY 使用 Rust `portable-pty`，它已在 workspace dev/test dependency 中出现；如果生产 crate 需要，提升为相关 crate dependency。
- 静态资源继续由 `rust-embed` feature 嵌入 `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes-web/dist`。
- 普通 chat 继续调用 `QueryEngine::submit_message()`。
- 结构化 IPC 后续复用 `FrontendMessage` / `BackendMessage`，不要重新定义一套前后端协议。

## `allthecodes-web` 前端目录建议

```text
/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes-web/
  package.json
  next.config.mjs
  tsconfig.json
  app/
    layout.tsx
    page.tsx
    globals.css
  components.json
  src/
    components/
      ui/
        button.tsx
        input.tsx
        textarea.tsx
        dialog.tsx
        sheet.tsx
        tabs.tsx
        scroll-area.tsx
        dropdown-menu.tsx
        tooltip.tsx
        command.tsx
        badge.tsx
        separator.tsx
        resizable.tsx
      shell/
        AppShell.tsx
        Sidebar.tsx
        TopBar.tsx
        WorkspaceDrawer.tsx
      chat/
        ChatPanel.tsx
        MessageList.tsx
        MessageItem.tsx
        ChatInput.tsx
        MessageToolbar.tsx
        renderers/
          AssistantText.tsx
          UserMessage.tsx
          ToolUse.tsx
          ToolResult.tsx
          PermissionRequest.tsx
          ThinkingBlock.tsx
          PlanWorkflow.tsx
          UsageUpdate.tsx
          SystemInfo.tsx
      terminal/
        XtermPane.tsx
        useXterm.ts
        usePtySocket.ts
      sessions/
        SessionSidebar.tsx
        SessionList.tsx
        SessionSearch.tsx
      settings/
        SettingsPanel.tsx
        ModelSelector.tsx
        PermissionModeSelector.tsx
      command/
        CommandPalette.tsx
        SlashCommandMenu.tsx
      debug/
        DebugDrawer.tsx
        EventTimeline.tsx
        RawEventLog.tsx
        SessionTracePanel.tsx
        RendererStatePanel.tsx
        ScreenshotProbePanel.tsx
    lib/
      api.ts
      sse.ts
      websocket.ts
      types.ts
      message-normalizer.ts
      event-recorder.ts
      debug-client.ts
    store/
      chat-store.ts
      session-store.ts
      terminal-store.ts
      ui-store.ts
      diagnostics-store.ts
    styles/
      tokens.css
      theme.css
  dist/
  test-results/
```

allthecodes Rust 仓库中的后端接入目录建议：

```text
/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes/crates/allthecodes-web/
  Cargo.toml
  src/
    mod.rs
    handlers.rs
    state.rs
    static_files.rs
    ws/
      mod.rs
      ipc.rs
      tui.rs
    debug/
      mod.rs
      actions.rs
      diagnostics.rs
      event_log.rs
      replay.rs
    pty/
      mod.rs
      session.rs
```

## 视觉和交互原则

参考 hermes-web-ui，但按 allthecodes 的 coding agent 场景重写。

### 组件模板策略

Web UI 采用 `shadcn/ui` 作为基础组件模板，而不是从零手写所有控件。

使用原则：

- `src/components/ui/**` 存放 shadcn/ui 生成的基础组件，例如 Button、Input、Dialog、Sheet、Tabs、ScrollArea、DropdownMenu、Tooltip、Command、Badge、Separator、Resizable。
- `src/components/chat/**`、`shell/**`、`terminal/**`、`debug/**` 等目录只写业务组件，并组合 `components/ui` 中的基础控件。
- 初期尽量少改 shadcn 生成组件的结构，优先通过 CSS variables、Tailwind tokens、className 和 wrapper 组件定制。
- 当 Pure Ink 风格需要更强约束时，再对 `components/ui` 中的基础组件做有边界的本地修改。
- 不把 shadcn/ui 当不可改的第三方库；它是复制到项目中的组件模板，允许后期按 allthecodes 的产品需求演进。

优先引入的基础组件：

- `button`、`input`、`textarea`：composer、toolbar、settings 表单。
- `dialog`、`sheet`：permission、question、settings、mobile drawer。
- `tabs`、`scroll-area`、`resizable`：workspace drawer、Debug Drawer、Inspector。
- `dropdown-menu`、`tooltip`、`command`：command palette、message actions、model selector。
- `badge`、`separator`：tool status、usage、session metadata。

### 视觉语言

- 主体采用 Pure Ink 风格：黑、白、灰为主，细边框，轻背景层级，小字号，高信息密度。
- 状态色只用于必要场景：success、warning、error、info、permission risk。
- 默认圆角控制在 6px 到 10px；避免大面积圆润卡片和营销式视觉。
- 不使用装饰性渐变、orbs、bokeh、hero。
- 主题基于 shadcn/ui 的 CSS variables 表达，至少支持 light/dark；后续把默认 shadcn 主题收敛为 allthecodes Pure Ink token。
- 字体不随 viewport 线性缩放，保持工具界面的稳定密度。

### 应用布局

```text
┌──────────────────────────────────────────────────────────────┐
│ TopBar: workspace / model / session / status / command search │
├──────────────┬───────────────────────────────┬───────────────┤
│ Session nav  │ Chat conversation             │ Drawer        │
│              │                               │ Files         │
│ conversations│ messages                      │ TUI/xterm     │
│ agents       │ composer                      │ Inspector     │
│ settings     │                               │               │
└──────────────┴───────────────────────────────┴───────────────┘
```

- 桌面端：左侧导航固定，右侧 drawer 可开合。
- 移动端：左侧和右侧都变为 overlay drawer。
- 聊天主区域必须保持优先级最高；terminal 不抢占主内容，除非用户切到 TUI 专用视图。
- MessageList 只在用户接近底部时自动滚动，避免流式输出打断阅读。
- 运行中状态必须明显：Stop 按钮、streaming indicator、busy lock。

### 消息渲染

普通 Web chat 必须使用 typed renderer，不渲染原始终端文本。

消息类型至少包括：

- user message。
- assistant text。
- thinking / reasoning block，默认折叠或半折叠。
- tool use，显示工具名、参数摘要、展开详情。
- tool result，显示成功/失败、输出摘要、展开完整内容。
- permission request，提供 allow/deny/always allow/feedback。
- plan workflow，显示 pending/approved/rejected 状态。
- compact boundary。
- API retry / system info / error。
- usage update。
- session switch / command output。

## Agent 调试与可观测通道

Web 端构建过程必须为开发者和 agent 留出稳定的 debug 通道。目标不是把调试能力做成面向普通用户的复杂产品功能，而是在每个阶段都能回答以下问题：

- 当前屏幕真实渲染成什么样，是否有重叠、截断、空白、颜色不可读或响应式布局问题。
- 某条对话从提交到结束经历了哪些事件，事件顺序是否正确。
- SSE / WebSocket / PTY 原始数据是什么，前端 reducer 如何把它转成 UI state。
- 哪个 session、turn、message、tool、permission 或 TUI process 正在运行，是否卡住。
- 测试失败时能否保留截图、事件日志、session trace 和最小 replay 输入。

### Debug Drawer

前端应内置 dev-only `DebugDrawer`，默认只在 `NEXT_PUBLIC_ALLTHECODES_DEBUG=1` 或 Rust debug flag 开启时显示。

Debug Drawer 至少包含：

- Event Timeline：按时间显示 `session_id`、`turn_id`、`message_id`、event type、source、duration。
- Raw Event Log：展示原始 SSE event、WebSocket frame、PTY lifecycle event，可复制为 JSONL。
- Renderer State：展示 message reducer 输入、归一化后的 message view model、当前 expanded/collapsed 状态。
- Session Trace：展示 session ownership、streaming/busy/aborted、pending permissions、pending questions、active tool calls。
- Terminal Diagnostics：展示 PTY pid、cols/rows、last resize、连接状态、最近 exit code、scrollback size。
- Screenshot Probe：提供触发 visual smoke runner 或本地 visual probe 的入口，记录 viewport、theme、drawer state 和 artifact path。Playwright 可用时优先使用 Playwright；当前环境不支持 Playwright 时必须可切换到本地替代 runner。

Debug Drawer 的设计要求：

- 不阻塞主交互，不改变 chat 或 TUI 的运行逻辑。
- 不依赖 React DevTools 才能使用。
- 所有 debug 面板都必须能按 session_id 过滤。
- 默认隐藏敏感字段，允许显式展开 raw payload。

### 事件录制与 replay

前端应实现轻量事件录制器：

```text
diagnostics/events/<session-id>/<turn-id>.jsonl
```

每条记录至少包含：

- `timestamp`
- `source`: `http`、`sse`、`ipc_ws`、`tui_ws`、`reducer`、`renderer`
- `session_id`
- `turn_id`
- `message_id`
- `event_type`
- `payload_summary`
- `payload_raw`，仅 debug 模式保存

录制器用途：

- 回放 SSE/WS event，复现前端 reducer 和 renderer 问题。
- 对比 Rust 后端输出与前端显示状态。
- 在 agent 调试时快速定位“后端没发、前端没收、reducer 丢了、renderer 没画”的责任边界。

### 对话执行轨迹

普通 Web chat 每一轮必须有 trace id。建议字段：

```text
session_id
turn_id
request_id
message_id
tool_use_id
permission_request_id
started_at
completed_at
status: queued | streaming | waiting_permission | waiting_question | running_tool | aborted | completed | error
```

前端需要显示：

- 当前 turn 状态。
- 正在运行的 tool 名称和耗时。
- pending permission/question。
- 最近一次 backend event 时间。
- abort 是否已发送、后端是否确认。

后端需要在 debug mode 下提供对应 trace snapshot，避免前端只能靠猜测重建执行状态。

### 显示效果测试

构建 Web UI 时必须同步建立显示效果测试通道。测试接口必须 runner-neutral，不能把 Playwright 写死为唯一执行器：

- Debug Actions 负责快速把系统置于目标状态，减少 agent 通过 UI 慢速点击和等待的成本。
- Browser visual runner desktop viewport：至少覆盖 1440x900、1280x720。
- Browser visual runner mobile viewport：至少覆盖 390x844。
- Light/dark theme 各一组。
- Chat states：empty、streaming、tool use、tool result、permission dialog、long output、error。
- Drawer states：closed、Files、TUI、Inspector、Debug。
- TUI states：connected、resized、large output、disconnected/error。

### 无 Playwright 环境下的验证策略

当前环境不支持 Playwright，因此计划中所有 screenshot、DOM 检查和 xterm pixel smoke 都必须通过一个可替换的 visual smoke runner 暴露统一命令，例如：

```text
npm run visual:smoke -- --runner=auto
npm run visual:smoke -- --runner=cdp
npm run visual:smoke -- --runner=manual
npm run visual:smoke -- --runner=playwright
```

runner 选择规则：

- `auto`：优先检测 Playwright；不可用时检测本机 Chrome/Chromium 的 DevTools Protocol 或 `--headless --screenshot` 能力；仍不可用时降级到 manual probe。
- `playwright`：仅在依赖和浏览器安装完整时启用，作为 CI 或支持环境的增强路径。
- `cdp`：不依赖 Playwright npm package，使用本机 Chrome/Chromium headless 截图、`--dump-dom`、DevTools Protocol 或轻量脚本完成截图、console 收集、DOM 标记检查和 xterm 非空像素检查。
- `manual`：启动 dev server 和 debug fixtures，生成可打开的 probe URL、fixture state、expected checklist 和 artifact manifest，由开发者或 agent 在真实浏览器中手动截图后放入 `test-results/`。

无 Playwright 环境下的最低验收组合：

- `npm run typecheck`、`npm run lint`、`npm run build` 覆盖前端静态正确性。
- 前端 unit tests 覆盖 SSE parser、message reducer、renderer view model、session stream isolation。
- Rust route/debug tests 覆盖 `/api/state`、`/api/chat`、`/api/debug/*`、`/api/tui/ws` 协议边界。
- replay tests 使用 JSONL fixture 验证 reducer 和 renderer state，不依赖真实浏览器点击路径。
- visual smoke runner 至少产出 DOM snapshot、computed layout probe、console log、network/event JSONL 和 screenshot artifact。若只能使用 manual runner，必须在 artifact manifest 中明确 `runner=manual`、浏览器、viewport、theme、fixture id 和人工检查结果。
- xterm smoke 在无浏览器自动化时分两层：hook/unit 测试验证 socket、resize、fit lifecycle；manual/CDP probe 验证 terminal DOM/canvas 非空、resize 后仍可见。

Debug Drawer 和 Debug Actions 必须支持这种降级路径：agent 先通过 debug action 建立场景，再通过 runner 或 probe 页面读取 DOM/layout 状态、事件日志和截图路径。

推荐测试模式：

1. 调用 `/api/debug/actions/fixtures/load` 或其他 action 建立场景。
2. visual smoke runner 打开页面或刷新目标 route；Playwright 可用时用 Playwright，不可用时用 CDP/headless Chromium 或 manual probe。
3. runner 做截图、DOM 检查、xterm 非空检查和基本交互确认；manual runner 至少输出检查清单和 artifact manifest。
4. 失败时导出 debug events、session trace、console、network 和 screenshot。

每次 visual smoke 应输出：

```text
/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes-web/test-results/
  screenshots/
  traces/
  console.log
  network-events.jsonl
  reducer-events.jsonl
```

测试标准：

- 页面不能空白。
- 主 chat、drawer、modal、composer 不能互相重叠。
- 长文本和长工具输出不能撑破容器。
- TUI canvas/DOM 必须非空，resize 后仍可见。
- Debug Drawer 能显示当前 session trace 和 raw events。

### TUI 调试记录

`/api/tui/ws` 需要保留 dev-only TUI diagnostics：

- PTY spawn command summary，不记录完整敏感 env。
- PTY pid、cwd、cols、rows。
- resize history。
- input byte count 和 output byte count。
- 最近 N 条 lifecycle events。
- exit code / signal / error。
- 可选 terminal output capture，默认脱敏或仅保存 ring buffer。

注意：TUI diagnostics 不能把 PTY 输出解释成普通 chat message，也不能改变 TUI 运行过程。

### 隐私和默认关闭

Debug 通道可能包含 prompt、tool input、tool result、路径、环境变量摘要和终端输出。默认必须关闭 raw payload 持久化。

推荐开关：

```text
ALLTHECODES_WEB_DEBUG=1
ALLTHECODES_WEB_DEBUG_RAW_EVENTS=1
ALLTHECODES_WEB_DEBUG_DIR=<path>
NEXT_PUBLIC_ALLTHECODES_DEBUG=1
```

默认行为：

- UI Debug Drawer 关闭。
- raw event 持久化关闭。
- 只保留内存 ring buffer。
- 用户或测试显式开启后才落盘。

## 后端接口计划

### 已有接口继续使用

当前 `crates/allthecodes-web/src/mod.rs` 已定义：

- `POST /api/chat`
- `POST /api/abort`
- `GET /api/state`
- `POST /api/settings`
- `POST /api/command`
- `GET /api/sessions`
- `POST /api/sessions/new`
- `GET /api/sessions/{id}`
- `POST /api/sessions/{id}/resume`

第一阶段前端只依赖这些接口即可完成普通 Web chat MVP。

### 新增 `/api/tui/ws`

目标：为 xterm.js 提供真实 TUI/terminal 展示。

语义边界：

- 这是 PTY 透传通道，不是结构化聊天通道。
- 服务端只转发 terminal bytes、resize 和生命周期事件。
- 服务端不得对 TUI 的 ANSI 输出做消息级解析、DOM 化、补全、过滤、重排或内容改写。
- 前端不得把 TUI 输出写入普通 chat message store；最多可以保存 terminal scrollback，且 scrollback 只属于 terminal UI。
- TUI tab 的交互结果以 `allthecodes` 进程和 session storage 为准，Web UI 不额外推导一份对话状态。

请求：

```text
GET /api/tui/ws?cwd=<path>&session_id=<optional>&mode=tui
```

客户端到服务端消息：

```json
{"type":"input","data":"..."}
{"type":"resize","cols":120,"rows":34}
{"type":"close"}
```

服务端到客户端消息：

```json
{"type":"output","data":"..."}
{"type":"exit","code":0}
{"type":"error","message":"..."}
```

后端行为：

- 验证 cwd 必须存在且在允许工作区范围内。
- 创建 PTY。
- 启动当前 allthecodes binary。
- 如提供 session_id，可追加 `--continue <session_id>`；否则启动新 TUI。
- 将浏览器输入写入 PTY writer。
- 将 PTY reader 输出写入 WebSocket。
- 收到 resize 后调整 PTY size。
- WebSocket 断开时终止或保留 PTY 的策略要显式配置；MVP 默认终止。

session ownership：

- MVP 建议同一 session 同时只允许一个 active writer：普通 Chat SSE turn 或 TUI PTY。
- TUI 连接建立后，后端应把该 session 标记为 `owned_by=tui`，普通 Chat tab 对该 session 的发送按钮禁用或要求显式接管。
- 普通 Chat 正在 streaming 时，打开同一 session 的 TUI tab 应只允许只读连接或直接拒绝写入。
- 如后续需要同时观察，可以实现 read-only mirror，但 read-only mirror 不发送 PTY input。

安全边界：

- 默认仅绑定 `127.0.0.1`。
- 不允许浏览器任意传入 executable。
- 不允许 query string 直接拼接 shell command。
- cwd 需要 canonicalize。
- 生产前加本地 token 或 origin 校验。

### 新增 `/api/ipc/ws`（Phase 2+）

目标：让 Web 前端直接使用 `FrontendMessage` / `BackendMessage`，覆盖 permission、question、completion、agent/team、subsystem 等完整交互能力。

```text
GET /api/ipc/ws?session_id=<optional>
```

客户端发送：

- `submit_prompt`
- `abort_query`
- `permission_response`
- `question_response`
- `slash_command`
- `resize`
- `query_subsystem_status`
- `request_completions`
- agent/team/subsystem commands

服务端发送：

- `ready`
- `stream_start`
- `stream_delta`
- `thinking_delta`
- `tool_use`
- `tool_result`
- `permission_request`
- `question_request`
- `assistant_message`
- `usage_update`
- `status_line_update`
- agent/team/subsystem events

实现建议：

- 不复制 headless JSONL loop。
- 抽象一个 transport-independent runtime：输入是 `FrontendMessage` stream，输出是 `BackendMessage` sink。
- 让 stdio JSONL 和 WebSocket 都复用同一个 dispatch/runtime。
- 当前 `FrontendSink` 只有 stdout/memory writer，Phase 2 可扩展 channel sink 或 trait sink。

### 新增 `/api/debug/*`（dev-only）

目标：为开发者和 agent 提供后端侧诊断通道，帮助定位 Web UI 构建、渲染、事件流和执行状态问题。

启用条件：

- 仅当 `ALLTHECODES_WEB_DEBUG=1` 或测试 harness 显式开启。
- 默认只监听 `127.0.0.1`。
- raw payload 和 terminal output 持久化需要额外开启 `ALLTHECODES_WEB_DEBUG_RAW_EVENTS=1`。

建议接口：

```text
GET  /api/debug/health
GET  /api/debug/sessions/{session_id}/trace
GET  /api/debug/sessions/{session_id}/events
GET  /api/debug/sessions/{session_id}/ownership
GET  /api/debug/turns/{turn_id}
GET  /api/debug/tui/{connection_id}
POST /api/debug/events/export
POST /api/debug/replay/validate
```

响应内容：

- `trace`：session、turn、message、tool、permission、question 的状态快照。
- `events`：后端 ring buffer 中的 recent SSE/IPC/PTY lifecycle events。
- `ownership`：当前 writer、只读连接、takeover 状态。
- `turns`：单轮请求从 submit 到 result 的生命周期。
- `tui`：PTY pid、cwd、terminal size、byte counters、resize history、exit state。
- `events/export`：把内存 ring buffer 导出到 debug dir。
- `replay/validate`：读取一段 JSONL event replay，验证前端期望的 event ordering contract。

### Debug Actions

为了加快 agent 调试速度，关键交互必须提供 dev-only debug action 触发方式。visual smoke runner 不必总是通过慢速点击路径把 UI 驱动到某个状态；测试和 agent 可以先调用 debug action 设置场景，再用 Playwright、CDP/headless Chromium 或 manual probe 做最终显示验证。

建议接口：

```text
POST /api/debug/actions/chat/submit
POST /api/debug/actions/chat/abort
POST /api/debug/actions/session/new
POST /api/debug/actions/session/resume
POST /api/debug/actions/session/ownership
POST /api/debug/actions/fixtures/load
POST /api/debug/actions/fixtures/clear
POST /api/debug/actions/tui/open
POST /api/debug/actions/tui/input
POST /api/debug/actions/tui/resize
POST /api/debug/actions/tui/close
POST /api/debug/actions/ui/state
POST /api/debug/actions/permissions/respond
POST /api/debug/actions/questions/respond
```

典型用途：

- 快速创建新 session、恢复指定 session、切换 ownership。
- 快速提交测试 prompt 或 abort 当前 turn。
- 注入 fixture：empty、streaming、tool use、tool result、permission dialog、long output、error。
- 触发 TUI 连接、输入、resize、关闭，不必手动点击终端。
- 设置 UI state：打开 drawer、切换 Debug tab、展开指定 message/tool、切换 theme、模拟 mobile layout。
- 对 pending permission/question 直接提交响应，验证后续状态。

Debug Actions 的约束：

- 仅 debug mode 启用，生产构建必须不可用。
- 每个 action 必须返回 `action_id`、`session_id`、`trace_ref` 和执行结果。
- 会改变 session/engine/TUI 状态的 action 必须显式标记 `mutates_runtime=true`。
- fixture action 默认只写入 isolated debug session，不能污染真实用户 session。
- UI state action 只能影响 debug/test harness 或当前浏览器连接，不能写入全局业务配置。
- visual smoke runner 可以依赖 Debug Actions 建立测试前置状态，但最终仍要用真实浏览器渲染产物、截图或 DOM/layout probe 验证真实显示。

约束：

- Debug API 不能成为业务状态权威来源。
- Debug inspection API 不能改变 QueryEngine、TUI 或 session storage 行为；Debug Actions 是唯一允许改变状态的 debug 入口，且必须显式标记。
- Debug API 返回 raw payload 时必须带 `debug_raw_enabled=true` 明确标记。
- 测试和 agent 可以依赖 Debug API 做定位，但普通 UI 流程不能依赖它才能工作。

## 数据模型

前端内部建议维护三层状态。

### Server state

来自 Rust 后端，可重新 fetch：

- sessions list。
- session detail。
- app state。
- settings。
- commands。
- tools。

### Stream state

按 session_id 隔离：

- active stream id。
- current assistant draft。
- current thinking draft。
- pending tool uses。
- pending permissions。
- latest usage。
- busy/error/aborted。
- ownership：`none`、`chat_stream`、`tui_pty`、`ipc_ws`。

所有 SSE/WebSocket callback 必须捕获发送时的 session_id，避免切换 session 后串流污染当前会话。

当 ownership 为 `tui_pty` 时，普通 Chat composer 默认只读；当 ownership 为 `chat_stream` 时，TUI tab 默认不允许写入同一 session。解除 ownership 必须由 stream result、abort、PTY exit 或显式 takeover 完成。

### UI state

只存在浏览器：

- sidebar collapsed。
- drawer active tab。
- selected message。
- expanded tool result ids。
- theme。
- composer draft。
- terminal tab layout。

UI state 可存 localStorage；server state 不以 localStorage 为权威来源。

### Diagnostics state

只在 debug mode 开启，默认使用内存 ring buffer：

- captured SSE events。
- captured WebSocket frames。
- captured PTY lifecycle events。
- reducer actions。
- renderer commit markers。
- layout probe results。
- screenshot artifact refs。
- latest backend trace snapshot。

Diagnostics state 的规则：

- 不能参与业务决策。
- 不能影响 message reducer 的输出。
- ring buffer 必须有上限，避免长会话拖垮浏览器。
- 落盘必须显式开启 raw events。

## 构建和集成

### Next.js 输出

建议使用 static export：

```js
// /data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes-web/next.config.mjs
const nextConfig = {
  output: "export",
  distDir: "dist",
  trailingSlash: true,
}

export default nextConfig
```

注意：

- `next/image` 不适合无 server static export 的复杂优化路径，MVP 避免使用。
- App Router 可以使用，但 realtime 逻辑全部在 client components。
- 需要确保静态资源路径可被 Axum fallback 正确服务。

### 开发模式

```text
Terminal A:
  cargo run -p allthecodes -- --web --web-port 3001 --no-open

Terminal B:
  cd /data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes-web
  npm run dev
```

Next dev server 将 `/api/*` 和 WebSocket 代理到 `127.0.0.1:3001`。

### 调试开发模式

```text
Terminal A:
  ALLTHECODES_WEB_DEBUG=1 \
  ALLTHECODES_WEB_DEBUG_DIR=target/web-debug \
  cargo run -p allthecodes -- --web --web-port 3001 --no-open

Terminal B:
  cd /data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes-web
  NEXT_PUBLIC_ALLTHECODES_DEBUG=1 npm run dev
```

需要原始 payload 落盘时再开启：

```text
ALLTHECODES_WEB_DEBUG_RAW_EVENTS=1
```

调试模式必须输出或可导出：

- 浏览器 console log。
- network/SSE/WebSocket events JSONL。
- reducer events JSONL。
- session trace snapshot。
- TUI PTY lifecycle JSONL。
- visual smoke screenshots/traces；Playwright 可用时包含 Playwright trace，不可用时包含 CDP/manual artifact manifest。

### 生产模式

```text
cd /data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes-web
npm ci
npm run build

cargo build -p allthecodes --features web-ui --release
target/release/allthecodes --web --web-port 3001
```

如果 root package feature 没有向 `crates/allthecodes-web` 的 `web-ui` feature 透传，需要补 Cargo feature wiring。`allthecodes` 仓库内 `crates/allthecodes-web` 的 `rust-embed` folder 应指向同级前端项目构建产物，例如相对 crate manifest 的 `../../../allthecodes-web/dist`。

## 分阶段实施计划

### Phase 0：协议和边界整理

目标：在动代码前固定 Web 端的运行边界。

工作：

- 确认 `allthecodes-web` 的现有 REST/SSE 路由响应 shape。
- 梳理 `SdkMessage` 到前端 message view model 的映射表。
- 梳理 `BackendMessage` 到 typed renderer 的映射表。
- 定义 `/api/tui/ws` 的最小协议。
- 定义 Web UI 的 session-scoped stream state 规则。
- 明确 `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes-web/dist` 嵌入路径和 Cargo feature wiring。
- 明确 `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes-web` 是 Next.js 前端项目的唯一实现归属，allthecodes 仓库内不新增 `web-ui/` 或 `crates/allthecodes-web/web-ui/`。
- 定义 debug event schema、trace id 规则、ring buffer 上限和 raw event 落盘开关。
- 定义 Debug Actions schema，明确哪些 action 只改 UI state，哪些 action 会 mutate runtime。
- 定义显示效果测试矩阵：viewport、theme、chat state、drawer state、TUI state。

验收：

- 有 Web API contract 文档或 TypeScript types 初稿。
- 普通 chat 与 TUI 两条通道的职责清晰，不混用。
- 有 diagnostics contract 初稿，明确 agent 如何查看 raw event、session trace、renderer state 和测试截图。
- 有 Debug Actions contract 初稿，agent 能通过接口快速触发关键交互和测试状态。

### Phase 1：Next.js 前端骨架

目标：能从 Rust Web server 打开一个真实 React 页面。

工作：

- 在 `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes-web` 创建 Next.js + React + TypeScript 项目。
- 初始化 shadcn/ui，生成 `components.json` 和 `src/components/ui/**` 基础组件。
- 引入首批 shadcn/ui 组件：button、input、textarea、dialog、sheet、tabs、scroll-area、dropdown-menu、tooltip、command、badge、separator、resizable。
- 配置 static export 到 `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes-web/dist`。
- 建立 `AppShell`、Sidebar、ChatPanel、WorkspaceDrawer 基础布局。
- 建立 dev-only `DebugDrawer` 空壳和 diagnostics store。
- 基于 shadcn/ui 的 CSS variables 建立 Pure Ink light/dark theme。
- 配置 dev rewrite/proxy 到 Rust server。
- 配置 `package.json` scripts：`dev`、`build`、`lint`、`typecheck`。
- 增加 runner-neutral visual smoke 基线，至少截图 empty shell 的 desktop/mobile/light/dark；当前环境先支持 CDP 或 manual runner，Playwright 作为可选增强。
- 增加 `/api/debug/actions/ui/state` 或等价 mock action，用于快速打开 drawer、Debug tab、切换 theme。

验收：

- `npm run build` 生成 `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes-web/dist`。
- `allthecodes --web --web-port 3001` 能服务静态 UI。
- `src/components/ui/**` 存在首批 shadcn/ui 基础组件，业务组件通过这些基础组件搭建。
- 页面在 desktop/mobile viewport 下不出现明显重叠。
- Debug Drawer 可以打开，显示当前 route、viewport、theme、session placeholder 和 renderer probe。
- visual smoke 截图工件写入 `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes-web/test-results/screenshots/` 或 allthecodes workspace `target/web-ui-test-results/screenshots/`。
- visual smoke runner 能通过 debug action 快速切换 drawer/theme/debug state 后截图；若使用 manual runner，artifact manifest 必须记录对应 action id 和人工检查结果。

### Phase 2：普通 Web chat MVP

目标：实现可用的网页端对话系统。

工作：

- `GET /api/state` 初始化模型、session、工具、权限模式、usage。
- `GET /api/sessions` 渲染 session sidebar。
- `POST /api/sessions/new` 新会话。
- `GET /api/sessions/{id}` 会话预览和历史加载。
- `POST /api/sessions/{id}/resume` 恢复会话。
- `POST /api/chat` 发起 SSE stream。
- `POST /api/abort` 停止当前 generation。
- 实现 SSE parser，将 `SdkMessage` 映射为前端 message view model。
- 实现 SSE event recorder 和 reducer action recorder。
- 实现 ChatInput：Enter 提交、Shift+Enter 换行、IME safe、运行中 Stop。
- 实现 MessageList 的接近底部自动滚动策略。
- Debug Drawer 显示当前 turn status、raw SSE events、message reducer 输出和 session ownership。
- 增加 `/api/debug/actions/chat/submit`、`/api/debug/actions/chat/abort`、`/api/debug/actions/fixtures/load`。

验收：

- 可打开 Web 页面发送一轮 prompt 并看到流式 assistant 输出。
- 可 abort 正在运行的请求。
- 可创建/恢复 session。
- 切换 session 时旧 stream 不污染新 session。
- 一轮对话完成后可以导出 JSONL 事件记录，并能用 replay 验证 reducer 输出。
- 测试失败时保留页面截图、console log、network events 和 reducer events。
- Agent 能通过 debug action 直接建立 streaming、long output、error 等状态，再用 visual smoke runner 验证显示。

### Phase 3：typed renderers 和 command/settings

目标：把 coding agent 关键事件做成可读 UI，而不是纯文本流。

工作：

- 渲染 assistant text 和 markdown。
- 渲染 tool use / tool result。
- 渲染 thinking block。
- 渲染 compact boundary。
- 渲染 retry/error/system info。
- 渲染 usage and cost。
- 接入 `/api/settings`：model、permission mode、thinking、fast mode、effort。
- 接入 `/api/command`：slash command palette 初版。
- 为 `/clear`、session switch、普通 output 等 command result 做前端处理。
- Renderer State Panel 显示每条消息的 raw blocks、normalized view model、renderer component name 和折叠状态。
- 增加 renderer fixture tests，覆盖长工具输出、权限请求、thinking、error、compact boundary。

验收：

- 常见工具调用能以折叠组件展示。
- 设置修改能反映到 `/api/state`。
- Slash command 可执行已有非 query 类型命令。
- Debug Drawer 可以定位“raw payload 存在但 renderer 未显示”的问题。

### Phase 4：xterm.js TUI 展示

目标：在 Web drawer 中真实展示 allthecodes TUI。

本阶段的核心约束是透明性：浏览器中的 TUI 必须等价于在本地终端中运行 `allthecodes`，Web 层只提供 terminal transport，不改变对话过程。

工作：

- 后端新增 `/api/tui/ws`。
- 后端实现 PTY spawn、read/write、resize、exit。
- 后端实现 session ownership 检查，避免同一 session 被 Chat 和 TUI 同时写入。
- 前端实现 `XtermPane`、`useXterm`、`usePtySocket`。
- xterm 支持 fit addon、web links、paste、copy、resize observer。
- WorkspaceDrawer 新增 TUI tab。
- TUI tab 打开时连接，关闭时断开。
- Chat composer 根据 session ownership 显示只读/接管状态。
- Debug Drawer 显示 PTY connection id、pid、cols/rows、byte counters、resize history、exit status。
- 增加 xterm screenshot/pixel smoke，验证 terminal 非空、resize 后仍可见。
- 增加 `/api/debug/actions/tui/open`、`input`、`resize`、`close`，用于快速驱动 TUI 场景。

验收：

- 浏览器中能看到真实 allthecodes TUI。
- Web TUI 通道不解释、不重排、不改写 TUI 输出；除终端尺寸和字体差异外，对话过程与本地终端一致。
- 输入、回车、Ctrl+C、resize 可用。
- 断开连接能清理子进程。
- TUI 连接失败能显示明确错误。
- 同一 session 正在 TUI 写入时，普通 Chat tab 不会再发送 prompt 到该 session。
- TUI debug trace 能说明连接建立、resize、输入、输出、断开和进程退出过程。
- Agent 能通过 debug action 触发 TUI resize/large output/error 场景，再用 visual smoke runner 做 xterm 非空和布局验证；无 Playwright 时先用 CDP/manual probe 加 hook/unit 测试兜底。

### Phase 5：WebSocket IPC 完整交互

目标：补齐普通 SSE chat 不足，支持 permission/question/completion/agent/team/subsystem。

工作：

- 抽象 headless runtime transport，复用 `FrontendMessage` / `BackendMessage`。
- 新增 `/api/ipc/ws`。
- 前端实现 IPC client。
- 前端实现 permission dialog。
- 前端实现 ask-user question dialog。
- 前端实现 completions/slash command suggestion。
- 前端订阅 agent/team/subsystem events。
- 对 status line update 建立 footer/status widget。
- IPC Raw Event Log 显示 `FrontendMessage` 和 `BackendMessage`，支持按 request_id、tool_use_id、permission id 过滤。
- 后端 `/api/debug/sessions/{id}/trace` 覆盖 pending permission/question 和 IPC connection 状态。
- 增加 `/api/debug/actions/permissions/respond` 和 `/api/debug/actions/questions/respond`。

验收：

- 需要权限的 tool call 可以在 Web UI 中审批并继续执行。
- AskUserQuestion 可以在 Web UI 中回答。
- Agent/team/subsystem 状态能实时更新。
- IPC replay 能复现 permission/question 流程的前端状态变化。
- Agent 能通过 debug action 快速推进 pending permission/question，验证后续 event 顺序。

### Phase 6：Files / Inspector / 工作区增强

目标：把 Web UI 从“聊天页”提升为 coding workspace。

工作：

- 右侧 drawer 增加 Files tab。
- 文件搜索复用已有后端 search capability 或 IPC `SearchFiles`。
- Tool result 中的文件路径可跳转到 Files/Inspector。
- Diff renderer 支持文件编辑摘要。
- Inspector 显示 selected message、tool input/output、raw JSON。
- 支持复制 message、复制 tool input/output。
- Inspector 与 Debug Drawer 可互相跳转：从 message 找 raw event，从 raw event 找 renderer。

验收：

- 用户可以从 tool result 快速定位文件。
- 长工具输出不会撑爆布局。
- Inspector 能帮助调试工具调用。
- Agent 可以通过 Inspector 判断工具结果是后端缺失、前端截断还是 renderer 折叠。

### Phase 7：可靠性、安全和发布

目标：达到可长期使用的本地 Web UI。

工作：

- 增加本地访问 token 或 origin 校验。
- 处理端口占用和启动提示。
- 明确 `--web --no-open` 与 auto-open 行为。
- 增加 runner-neutral visual smoke tests；当前环境必须支持非 Playwright runner。
- 增加 xterm viewport screenshot/pixel smoke。
- 增加 Rust route tests：`/api/tui/ws` protocol、session busy、abort、resume。
- 增加前端 unit tests：message reducer、SSE parser、session stream isolation。
- 增加 diagnostics tests：event recorder、raw export、debug endpoints disabled-by-default、session trace snapshot。
- 增加 replay tests：使用录制的 SSE/WS JSONL 重放并验证 message view model。
- 增加 debug action tests：disabled-by-default、schema validation、mutation labeling、fixture isolation。
- 补齐 docs：运行方式、开发方式、故障排查。

验收：

- `cargo test -p allthecodes-web` 通过。
- `npm run typecheck` 通过。
- `npm run lint` 通过。
- `npm run build` 通过。
- visual smoke runner 覆盖 chat MVP 和 xterm smoke；Playwright 覆盖作为支持环境的增强项，不是当前环境的唯一验收路径。
- 失败测试会保留截图、trace、console log、network events、reducer events。
- Debug API 默认关闭；开启后 agent 可以查询 session trace、recent events、TUI diagnostics。
- Debug Actions 默认关闭；开启后 agent 可以快速触发关键交互，但每个 mutating action 都有 trace 和隔离边界。

## 关键映射表

### `SdkMessage` 到 Web message view model

| `SdkMessage` | 前端处理 |
| --- | --- |
| `system_init` | 更新 session/model/tools/permission 初始状态 |
| `assistant` | 添加或 finalize assistant message，抽取 tool use |
| `user_replay` | 添加用户消息或 tool result replay |
| `stream_event` | 更新当前 assistant draft / thinking draft |
| `compact_boundary` | 插入 compact boundary 系统消息 |
| `api_retry` | 插入 retry notice |
| `tool_use_summary` | 插入 tool summary 或附着到相关 tool group |
| `tombstone` | 删除 abandoned draft |
| `result` | 标记 stream complete，更新 usage/cost/error |

### `BackendMessage` 到 typed renderer

| `BackendMessage` | 前端处理 |
| --- | --- |
| `ready` | 初始化 IPC session |
| `stream_start` / `stream_delta` / `stream_end` | 流式 assistant 文本 |
| `thinking_delta` | 流式 thinking block |
| `assistant_message` | assistant final content blocks |
| `tool_use` | tool call renderer |
| `tool_result` | tool result renderer |
| `tool_progress` | running tool live output |
| `permission_request` | permission dialog |
| `question_request` | question dialog |
| `conversation_replaced` | session/history replace |
| `usage_update` | usage footer |
| `status_line_update` | status footer |
| `suggestions` | composer suggestions |
| `agent_event` / `team_event` | agent/team panels |
| subsystem events | settings/status panels |

## 风险和缓解

### Next.js static export 与实时能力

风险：Next SSR/Server Components 不适合承载本地实时 agent UI。

缓解：

- Next 只作为 React app 构建工具和静态导出工具。
- 所有实时逻辑放在 client components、hooks 和 stores。
- 后端 API/WS 全部由 Rust Axum 提供。

### xterm 与普通 chat 混淆

风险：把 TUI 字节流当成普通消息，会导致不可搜索、不可结构化、不可审批。

缓解：

- 普通 chat 永远走 `SdkMessage` / `BackendMessage` typed renderer。
- xterm 只用于真实 shell/TUI tab。
- `/api/tui/ws` 必须保持 PTY 透明传输，不解析、不重排、不写入 chat message store。

### TUI 和 Chat 同时驱动同一 session

风险：用户在 TUI tab 和普通 Chat tab 同时向同一个 session 发送输入，会造成 history 顺序、权限请求、abort 状态和 engine busy 状态竞争。

缓解：

- 每个 session 维护 ownership。
- TUI PTY、Chat SSE、IPC WS 同一时间只允许一个 active writer。
- 非 owner 视图只能只读观察，或要求用户显式 takeover。
- takeover 必须先 abort/close 当前 writer，并重新加载 session state。

### PTY 生命周期泄漏

风险：浏览器断开后遗留子进程。

缓解：

- MVP 默认 WebSocket 断开即 kill PTY child。
- 后续如需 detachable session，必须引入 session registry、heartbeat、explicit reconnect policy。

### Debug 通道泄露敏感内容

风险：raw events、tool input/output、terminal output 和 session trace 可能包含 prompt、文件路径、环境变量摘要、密钥片段或业务代码。

缓解：

- Debug API 和 Debug Drawer 默认关闭。
- raw payload 落盘需要 `ALLTHECODES_WEB_DEBUG_RAW_EVENTS=1`。
- 默认只保存 payload summary 和 bounded ring buffer。
- 导出 debug bundle 前做敏感字段标记和可选脱敏。
- 测试 fixture 使用 synthetic data，不依赖真实用户会话。

### Debug 逻辑影响产品行为

风险：调试 recorder、trace panel 或 replay hook 影响 message reducer、stream timing 或 TUI PTY 透传，导致“为了调试改变了被调试对象”。

缓解：

- Diagnostics state 不能参与业务决策。
- Recorder 只旁路观察 event，不 mutate event。
- replay 只能在测试 harness 或 isolated store 中运行。
- Debug API 不能改变 QueryEngine/TUI/session storage 状态。
- 对 debug-off 和 debug-on 各跑一组 smoke，确认主流程行为一致。

### Debug Actions 误用为业务 API

风险：为了调试速度暴露的 action endpoint 可能绕过正常 UI 约束，被普通功能依赖，或在非 debug 环境中改变真实 session 状态。

缓解：

- Debug Actions 默认关闭，生产不可用。
- Debug Actions 与 inspection Debug API 分命名空间或明确 action 前缀。
- 每个 mutating action 必须写 trace，返回 `mutates_runtime=true`。
- fixture action 默认创建 isolated debug session。
- 正常 UI 代码不能调用 debug actions；只允许测试 harness、Debug Drawer 和 agent 调试脚本调用。
- CI 需要测试 debug actions disabled-by-default。

### shadcn/ui 默认风格覆盖产品风格

风险：直接堆 shadcn/ui 默认组件会让界面变成通用 dashboard，而不是 allthecodes 的 Pure Ink coding workspace。

缓解：

- shadcn/ui 只作为基础组件模板，不作为最终视觉规范。
- 所有业务组件通过 `src/components/ui/**` 和 wrapper 组合，不直接散落大量一次性 class。
- 主题变量在 Phase 1 就收敛到 allthecodes token：背景、边框、muted、accent、danger、warning、success。
- Phase 3 后对高频组件进行二次封装，例如 `ToolCard`、`PermissionDialog`、`InspectorPanel`、`TerminalShell`。
- visual smoke 同时覆盖 shadcn 默认组件状态和定制后的业务组件状态，防止定制破坏可访问性和布局；Playwright 不可用时使用 CDP/manual runner 产物作为阶段验收依据。

### 权限和本地安全

风险：Web UI 暴露 shell/TUI 后，如果被跨站请求利用会很危险。

缓解：

- 默认监听 `127.0.0.1`。
- 加 origin check。
- 加随机 local token。
- cwd canonicalize。
- 不允许客户端指定 executable。

### 许可证边界

风险：`claudecodeui` 为 AGPL 系项目，直接复制实现会污染 Apache-2.0 项目。

缓解：

- 只借鉴架构和交互模式。
- 业务 React 组件、hooks、样式在 allthecodes-web 中重新实现；基础控件可以使用 shadcn/ui 生成的本地组件模板，并按项目需要定制。
- 文档中保留“参考模式，不复制源码”的约束。

## 推荐优先级

最小可用路径：

1. Next.js static shell。
2. `/api/state` + `/api/chat` SSE chat。
3. session list/new/resume。
4. tool/thinking/result typed renderer。
5. `/api/tui/ws` + xterm TUI drawer。
6. `/api/ipc/ws` 完整交互。

不要先做：

- 文件编辑器。
- plugin marketplace UI。
- 多 provider 外部 session scanner。
- detached terminal session。
- 云端多用户权限模型。

## 完成定义

第一版 allthecodes Web 端可认为完成，当满足：

- `target/release/allthecodes --web --web-port 3001` 能打开 Web UI。
- 普通 Web chat 可以完成多轮对话、工具调用展示、abort、session resume。
- TUI tab 可以展示真实 allthecodes TUI 并支持输入和 resize。
- TUI tab 只是 PTY 透传，不改变 TUI 对话过程；输出不被 React message renderer 解释、重排或改写。
- 关键状态按 session_id 隔离。
- 同一 session 的 Chat、IPC、TUI writer 有明确 ownership，避免并发写入。
- Agent/debug 通道可用：能查看 raw events、session trace、renderer state、TUI diagnostics 和测试截图。
- Debug 通道默认关闭，开启后不改变正常 chat、IPC 或 TUI 行为。
- Debug Actions 可用：agent 能通过 dev-only action 快速触发 submit、abort、fixture、permission、question、TUI input/resize 等关键交互，并获得 trace。
- `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes-web/dist` 的前端 static build 可被 Rust `web-ui` feature 嵌入。
- 基础测试覆盖 SSE parser、message reducer、Rust web route、xterm smoke。
