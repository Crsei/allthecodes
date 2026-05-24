# claude-code-bun UI 结构梳理

根目录：`F:\AIclassmanager\cc\claude-code-bun`

来源：`gpt-5.5` subagent 只读梳理，并由当前会话做轻量目录核对。未修改源项目文件，未启动构建或测试。

## 入口与边界

- `F:\AIclassmanager\cc\claude-code-bun\src\main.tsx`：CLI 主入口，解析命令并进入交互 TUI。
- `F:\AIclassmanager\cc\claude-code-bun\src\replLauncher.tsx`：REPL/TUI 启动封装。
- `F:\AIclassmanager\cc\claude-code-bun\src\interactiveHelpers.tsx`：创建 Ink root、渲染一次性对话框/错误/设置屏幕。
- `F:\AIclassmanager\cc\claude-code-bun\src\screens\REPL.tsx`：主交互界面，消息流、输入框、权限弹窗、滚动区、快捷键都汇聚在这里。
- `F:\AIclassmanager\cc\claude-code-bun\src\components\App.tsx`：TUI 应用外层组件。
- `F:\AIclassmanager\cc\claude-code-bun\packages\@ant\ink\src\index.ts`：本地 Ink 终端 React 渲染框架导出入口。
- `F:\AIclassmanager\cc\claude-code-bun\packages\@ant\ink\src\core\root.ts`：终端 React root 核心。
- `F:\AIclassmanager\cc\claude-code-bun\packages\@ant\ink\src\core\ink.tsx`：终端 React renderer 核心。
- `F:\AIclassmanager\cc\claude-code-bun\packages\remote-control-server\web\src\main.tsx`：浏览器端 React 入口。
- `F:\AIclassmanager\cc\claude-code-bun\packages\remote-control-server\web\src\App.tsx`：远程控制 Web UI 应用入口。

## UI 文件树概览

```text
F:\AIclassmanager\cc\claude-code-bun
├─ src
│  ├─ main.tsx - CLI/TUI 主入口。
│  ├─ replLauncher.tsx - 启动 REPL 屏幕。
│  ├─ interactiveHelpers.tsx - Ink 渲染辅助与一次性 UI 流程。
│  ├─ dialogLaunchers.tsx - 动态加载各类弹窗。
│  ├─ screens
│  │  ├─ REPL.tsx - 主 TUI 会话界面。
│  │  ├─ Doctor.tsx - doctor 命令 UI。
│  │  └─ ResumeConversation.tsx - 会话恢复选择 UI。
│  ├─ components
│  │  ├─ App.tsx - TUI 顶层应用组件。
│  │  ├─ Messages.tsx / Message*.tsx / MessageRow.tsx - 对话消息渲染。
│  │  ├─ PromptInput/*.tsx - 主输入框、底栏、模式、提示、语音状态。
│  │  ├─ permissions/**/*.tsx - Bash/File/Web/Plan/Skill 等权限请求弹窗。
│  │  ├─ tasks/**/*.tsx - 后台任务、远程会话、shell/agent 任务详情 UI。
│  │  ├─ agents/**/*.tsx - Agent 列表、编辑器、创建向导、工具/模型选择 UI。
│  │  ├─ mcp/**/*.tsx - MCP 服务/工具列表、设置、重连、elicitation UI。
│  │  ├─ design-system/*.tsx - 本项目 TUI 设计系统组件。
│  │  ├─ CustomSelect/*.tsx - 终端选择器组件。
│  │  ├─ LogoV2/*.tsx - 欢迎页/Logo/公告区域。
│  │  ├─ Spinner/*.tsx - 加载与 teammate 动画。
│  │  ├─ diff/*.tsx / StructuredDiff*.tsx - diff 查看 UI。
│  │  ├─ shell/*.tsx - shell 输出与进度行 UI。
│  │  ├─ memory/*.tsx - 记忆文件选择/更新提示。
│  │  ├─ sandbox/*.tsx - sandbox 设置页。
│  │  ├─ teams/*.tsx - team 状态/对话框。
│  │  ├─ wizard/*.tsx - 通用向导布局。
│  │  └─ 其他顶层 *.tsx - 状态行、设置、登录、反馈、主题、更新、IDE、远程环境弹窗。
│  ├─ commands
│  │  └─ **/*.tsx - slash/CLI 子命令的本地 Ink UI，例如 model.tsx、theme.tsx、mcp.tsx、plugin/*.tsx、statusline.tsx。
│  ├─ hooks
│  │  ├─ use*.ts / use*.tsx - REPL 输入、快捷键、通知、权限、任务、远程会话、插件、IDE、终端尺寸等 UI hooks。
│  │  ├─ notifs/**/*.tsx - 启动/模型/插件/速率限制等通知 hooks。
│  │  └─ toolPermission/**/*.ts - 交互权限处理 hooks。
│  ├─ vim
│  │  ├─ motions.ts - Vim motion 逻辑。
│  │  ├─ operators.ts - Vim operator 逻辑。
│  │  ├─ textObjects.ts - Vim text object 逻辑。
│  │  ├─ transitions.ts - Vim 模式/状态转移。
│  │  └─ types.ts - Vim 输入模式类型。
│  └─ utils
│     ├─ ink.ts / terminal.ts / terminalPanel.ts - TUI/终端适配工具。
│     ├─ renderOptions.ts / staticRender.tsx / exportRenderer.tsx - 渲染配置与静态导出。
│     ├─ markdown.ts / textHighlighting.ts / sliceAnsi.ts / horizontalScroll.ts - 文本、ANSI、Markdown、滚动处理。
│     └─ theme.ts / systemTheme.ts / systemThemeWatcher.ts / logoV2Utils.ts - 主题与视觉辅助。
├─ packages
│  ├─ @ant\ink
│  │  ├─ src\index.ts - Ink 框架公共 API。
│  │  ├─ src\components\*.tsx - Box/Text/Button/ScrollBox/AlternateScreen 等终端 UI 原语。
│  │  ├─ src\core\*.ts(x) - reconciler、renderer、screen、output、layout、terminal I/O、selection、cursor、ANSI 渲染核心。
│  │  ├─ src\core\events\*.ts - 输入、键盘、鼠标、焦点、resize、paste 事件模型。
│  │  ├─ src\core\termio\*.ts - ANSI/CSI/DEC/OSC 终端控制序列解析与生成。
│  │  ├─ src\hooks\*.ts - useInput/useApp/useTerminalSize/useSelection/useTerminalTitle 等终端 hooks。
│  │  ├─ src\keybindings\*.ts(x) - 快捷键解析、匹配、上下文和安装。
│  │  ├─ src\theme\*.tsx - ThemedBox/Text、Dialog、Tabs、Pane、Spinner、FuzzyPicker 等主题组件。
│  │  ├─ src\types\*.d.ts - JSX/元素类型声明。
│  │  ├─ docs\*.md - Ink 使用、布局、主题、滚动、输入、终端集成文档。
│  │  └─ tsconfig.json / package.json - 包配置。
│  ├─ builtin-tools\src\tools
│  │  ├─ **\UI.tsx - 各内置工具的 TUI 展示组件。
│  │  ├─ AgentTool\AgentTool.tsx / UI.tsx - Agent 工具 UI。
│  │  ├─ BashTool\BashTool.tsx / BashToolResultMessage.tsx / UI.tsx - Bash 工具与输出 UI。
│  │  ├─ PowerShellTool\PowerShellTool.tsx / UI.tsx - PowerShell 工具 UI。
│  │  ├─ AskUserQuestionTool\AskUserQuestionTool.tsx - 用户提问工具 UI。
│  │  ├─ WorkflowTool\WorkflowPermissionRequest.tsx - workflow 权限 UI。
│  │  └─ testing\TestingPermissionTool.tsx - 测试权限工具 UI。
│  └─ remote-control-server\web
│     ├─ index.html - Web UI HTML 宿主。
│     ├─ src\main.tsx - Web React 挂载入口。
│     ├─ src\App.tsx - Web 应用路由/状态入口。
│     ├─ src\components\*.tsx - 控制栏、会话列表、任务面板、权限视图等 Web 组件。
│     ├─ src\components\chat\*.tsx - 聊天视图、输入、消息、工具调用、计划和权限面板。
│     ├─ src\components\ui\*.tsx - Web 基础 UI 控件。
│     ├─ src\components\ai-elements\*.tsx - AI 消息/推理/工具/代码块展示组件。
│     ├─ src\pages\Dashboard.tsx / SessionDetail.tsx - Web 页面。
│     ├─ src\hooks\*.ts - Web 数据与状态 hooks。
│     ├─ src\api\*.ts / src\acp\*.ts - Web API/ACP 客户端。
│     ├─ src\index.css - Web 样式入口。
│     ├─ vite.config.ts / tsconfig.json - Web 构建配置。
│     └─ src\__tests__\*.test.ts - Web 单元测试。
├─ docs
│  ├─ terminal-rendering-guide.md - 终端渲染指南。
│  ├─ terminal-rendering-exploration-report.md - 终端渲染调研报告。
│  ├─ tui-component-implementation-guide.md - TUI 组件实现指南。
│  ├─ ink-terminal-*.md - Ink/终端集成文档。
│  ├─ images\*.png - 文档图片资源。
│  └─ logo\*.svg / favicon.svg - 文档/品牌资源。
└─ tests
   └─ integration\*.test.ts - 集成测试；未看到专门 UI 快照测试入口。
```

## 主要模块组成

- 主 TUI 应用：`src/main.tsx`、`src/replLauncher.tsx`、`src/screens/REPL.tsx`、`src/components/App.tsx`。
- TUI 组件层：`src/components/**`，覆盖消息、输入框、权限、任务、agents、MCP、diff、shell、memory、sandbox、team、wizard、设置和反馈等界面。
- 命令 UI：`src/commands/**/*.tsx`，用于 slash/CLI 子命令里的本地 Ink 渲染。
- UI hooks：`src/hooks/**`，处理输入、快捷键、通知、权限、任务、远程会话、插件、IDE 和终端尺寸。
- Vim 输入模式：`src/vim/*.ts`，负责 motion、operator、text object 和状态转移。
- 终端渲染工具：`src/utils/ink.ts`、`terminal.ts`、`terminalPanel.ts`、`staticRender.tsx`、`exportRenderer.tsx`、Markdown/ANSI/主题辅助。
- 本地 Ink 框架：`packages/@ant/ink/src/**`，提供终端 React renderer、组件原语、事件、键位、主题组件和 terminal I/O。
- 内置工具 UI：`packages/builtin-tools/src/tools/**` 中的 `UI.tsx`、权限请求、结果展示和工具专属渲染组件。
- Web 远程控制 UI：`packages/remote-control-server/web/**`，是浏览器端 React/Vite 应用，与主 TUI 并列存在。

## 测试与资源位置

- TUI/组件测试主要散落在 `src/components/**/__tests__`、`src/hooks/**/__tests__`、`src/commands/**/__tests__`、`src/utils/**/__tests__`。
- 内置工具 UI/展示相关测试在 `packages/builtin-tools/src/tools/**/__tests__`。
- Web UI 测试在 `packages/remote-control-server/web/src/__tests__`。
- 未发现常规 `*.snap` 快照文件；“snapshot”主要是代码语义文件，例如 `SnapshotUpdateDialog.test.tsx`、`agentMemorySnapshot.ts`、`ShellSnapshot.ts`。
- 静态资源主要在 `docs/images`、`docs/logo`、`docs/favicon.svg`。
- Web UI 样式入口是 `packages/remote-control-server/web/src/index.css`。

## 规模核对

轻量目录计数：

- `src/components` 下 TS/TSX 文件约 607 个。
- `packages/@ant/ink/src` 下 TS/TSX/声明文件约 145 个。
- `packages/remote-control-server/web/src` 下 TS/TSX/CSS 文件约 39 个。
- `packages/builtin-tools/src/tools` 中 UI/权限/结果展示相关 TSX 文件约 37 个。

这些数量说明该项目 UI 面很大，本报告按 UI 边界和文件族记录组成结构，未把所有 600+ 组件逐项展开为超长清单。

## 无法确认的项

- 没有逐行审阅所有 UI 文件；大量文件职责按文件名和目录结构推断。
- `src/components/src/**`、`src/screens/src/**`、`src/hooks/src/**` 看起来是局部抽取或内联依赖副本，具体生成来源未确认。
- 未运行测试或构建，因此未验证这些 UI 入口在当前工作树是否全部可编译。
