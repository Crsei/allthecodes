---
title: "什么是 allthecodes - Terminal Native Agentic Coding System 的 Rust 实现"
description: "allthecodes 是一个参考 Claude Code 的交互模型、工具系统和 agent 工作流，用 Rust 重新构建的高性能 AI 编程助手。本文介绍其技术定位、与 TypeScript 原版的架构差异以及项目现状。"
keywords: ["allthecodes", "Claude Code", "Rust", "AI 编程助手", "Agentic Coding", "终端 AI"]
---

## 一句话定义

allthecodes 是一个用 **Rust** 重新实现的、运行在本地终端中的 **agentic coding system**。它不是逐文件翻译 Claude Code 的 TypeScript 源码，而是在对照上游行为的基础上，重新整理工程结构、运行时边界、IPC 协议、Rust TUI、权限模型和工具系统，并融合了作者对 coding agent 的独立理解。

## 技术定位：Rust-native agentic system

allthecodes 继承了 Claude Code 的核心定位，但选择了不同的实现路径：

| 定位关键词 | 含义 |
|-----------|------|
| **Terminal-native** | 原生 CLI 应用，不是 IDE 插件、不是 Web 界面、不是 API wrapper |
| **Agentic** | AI 自主决策工具调用链，不是"一问一答"的聊天模式 |
| **Coding system** | 面向软件工程全流程，不是通用问答工具 |
| **Rust-native** | 完全用 Rust 编写，编译为静态链接的单一二进制文件，零运行时依赖 |

与同类工具的**架构层面**差异：

| 工具 | 架构模式 | 运行位置 | 工具执行 | 运行时 |
|------|----------|----------|----------|--------|
| **allthecodes** | Terminal-native agentic loop | 本地 Rust 进程 | 直接 shell 执行 | **静态二进制** |
| **Claude Code (TS)** | Terminal-native agentic loop | 本地 Node/Bun 进程 | 直接 shell 执行 | Node.js/Bun |
| Cursor / Copilot | IDE-integrated autocomplete + chat | IDE 进程内 | LSP / IDE API | Electron |
| Aider | CLI chat → git patch | 本地进程 | 文件操作为主 | Python |

核心差异：allthecodes 拥有**完整的 shell 访问权**——这意味着它可以做任何你在终端里能做的事，但也需要对应的安全机制来约束这个能力。与此同时，由于使用 Rust 实现，它拥有比 Node.js/Bun 运行时更快的启动速度、更低的内存占用和更强的进程隔离能力。

### Terminal-native 意味着什么

终端不是限制，而是选择。allthecodes 在终端中运行时获得的能力：

- **完整的 shell 访问**：可以运行任何命令行工具（编译器、linter、测试框架、调试器），无需为每个工具写 API 适配层
- **项目原生**：直接在项目目录工作，理解文件系统结构、`.gitignore` 规则、git 状态
- **可组合性**：管道模式（`echo "..." | allthecodes -p`）允许嵌入 CI/CD pipeline 和自动化流程
- **低延迟**：没有 Electron 架构的开销，Rust 编译的二进制启动时间 < 5ms
- **零安装负担**：单一二进制文件，没有 `node_modules`，没有运行时依赖

代价是用户需要适应命令行界面——但也正因如此，它吸引的是需要**真正掌控开发环境**的开发者。

### Agentic system 的具体含义

当用户输入"有个 TypeScript 报错，帮我修一下"时，allthecodes 执行的不是一次"问答"，而是一个自主决策的工具调用链：

| Turn | AI 决策 | 工具调用 | 结果 |
|------|---------|----------|------|
| 1 | 先看报错信息 | `Bash("bun run dev 2>&1 \| head -30")` | TypeScript 错误输出 |
| 2 | 定位到文件 | `Read("src/utils/foo.ts")` | 源代码内容 |
| 3 | 搜索相关类型定义 | `Grep("interface Foo", "src/")` | 类型定义位置 |
| 4 | 修复代码 | `Edit(old, new)` | 代码已修改 |
| 5 | 验证修复 | `Bash("bun run dev 2>&1 \| head -10")` | 编译通过 |

每一步都是 AI 自主决策——它决定用哪个工具、传什么参数、何时停止。这就是 "agentic" 的含义。allthecodes 的 agentic loop 会持续执行工具调用，直到任务完成、达到最大轮次上限或用户中断。

## 为什么是 Rust 而不是 TypeScript

原版 Claude Code 使用 TypeScript 编写，运行在 Node.js 或 Bun 运行时之上。这种架构带来了几个根本性的挑战：

### TypeScript 原版的挑战

| 挑战 | 具体表现 | 影响 |
|------|---------|------|
| **Node.js 运行时依赖** | 用户需要安装 Node.js 或 Bun | 增加使用门槛，容器环境需要额外层 |
| **内存占用较高** | V8 引擎 + Node.js 核心模块基线 60-120MB | 长时间 agent 会话累积更多 |
| **启动延迟** | 解释执行 + 模块加载 | 冷启动 50-300ms，快速任务感知明显 |
| **包管理复杂度** | npm 依赖树巨大，版本冲突 | 安装失败、CI 缓存管理复杂 |
| **进程隔离困难** | child_process 在高并发场景 | 僵尸进程、信号管理、资源泄露 |
| **类型系统局限** | 编译时擦除全部类型 | 需要 ts-json-schema-generator 等额外工具 |
| **模块边界松散** | 没有编译时强制可见性 | 循环依赖、模块职责模糊 |

### Rust 实现的优势

| 优势 | 具体表现 | 对项目的影响 |
|------|---------|-------------|
| **静态编译单二进制** | 一个可执行文件，零运行时依赖 | 分发简单、容器化容易、CI 零摩擦 |
| **零成本抽象** | 所有权 + 借用检查 | 编译时保证内存安全，运行时无 GC 开销 |
| **内存安全** | 编译时消除空指针、数据竞争、内存泄漏 | 长时间运行的 agent 进程更稳定 |
| **真并发** | tokio 多线程异步运行时 | 工具执行真正的并行，CPU 密集型不阻塞 |
| **强类型系统** | enum + match + trait | 状态机（agentic loop）精确表达，JSON Schema 自动派生 |
| **编译时模块边界** | Cargo workspace + crate 可见性 | 40 个 crate 之间严格的接口契约 |
| **FFI 零开销** | 直接调用 C 库 | Tree-sitter 代码解析无需 N-API 桥接 |
| **进程控制** | tokio::process + PTY | Shell 工具精确的进程组、超时、信号管理 |

### 一个具体的编译对比

```
TypeScript 版本:
  npm install → 下载 500+ 包到 node_modules（~200MB）
  → 需要 Bun 运行时（~100MB 二进制）
  → npx 调用 → V8 解释执行

Rust 版本:
  cargo build → 单一静态二进制（~30MB）
  → ./allthecodes
  → 直接执行（无解释层）
```

这不只是体验差异——在 Docker 镜像、CI pipeline、受限环境中，二进制分发的优势是决定性的。

## 项目起源

allthecodes 的前身是 `claude-code-rust`，一个早期的 Rust 实验项目。经过长期快速迭代后，原项目的工程结构、模块边界和前后端耦合都变得比较混乱。继续在原结构里堆功能会让维护成本越来越高，所以 allthecodes 选择另开一个 Rust workspace，把核心能力重新梳理一遍。

创建 allthecodes 的直接原因：

1. **工程结构重组**：用更清晰的类型边界、更强的路径隔离和更可维护的模块组织来承载后续功能
2. **路径隔离**：与 Claude Code/Codex 等工具共存于同一台机器时，使用独立的持久化路径（`~/.allthecodes/`），凭证和服务名全部隔离
3. **行为对标**：对照上游 TypeScript 原版的完整行为，而不是做精简版
4. **独立演进**：在兼容主流工作流的同时，逐步加入 allthecodes 自己的新功能

### 路径隔离策略

allthecodes 与 Claude Code 可以完全共存，互不干扰：

| 用途 | 原版 Codex | allthecodes（本项目） |
|------|------------|---------------------|
| 全局数据目录 | `~/.Codex/` | `~/.allthecodes/` |
| 项目配置 | `.Codex/settings.json` | `.allthecodes/settings.json` |
| 项目技能 | `.Codex/skills/` | `.allthecodes/skills/` |
| Keychain 服务名 | `Codex` | `allthecodes` |
| 凭据文件 | `~/.Codex/credentials.json` | `~/.allthecodes/credentials.json` |
| 项目指令文件 | `AGENTS.md` | `AGENTS.md`（共享） |

运行时也可通过 `ALLTHECODES_HOME` 环境变量指定全局数据目录位置。

## 当前阶段：全量构建（Full Build）

项目目前处于**全量构建阶段**，目标是与上游 TypeScript 完整版行为对齐。项目的 AGENTS.md 明确了书写和审阅规则：

### 构建原则

- **不再按精简版缩减**：新代码应覆盖上游对应模块的完整行为，不要以"精简版"为由省略分支、截断、错误恢复、沙箱、Rust TUI 细节等
- **已有的缩减实现视为 TODO**：不是既定边界，补齐后迁移到已完成文档
- **上游参考**：对照行为时读取 `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-bun/` 的 TypeScript 原版
- **故意保留的缩减需显式说明**：在 PR 描述中标注，并在文档标记为"故意保留"
- **历史 `Deferred` 清单需重评**：不再默认等于"不做"，触及这些条目时按上游完整实现对齐

### 已完成的核心能力

| 能力领域 | 状态 | 说明 |
|---------|------|------|
| **流式对话引擎** | 完整 | QueryEngine 生命周期实现：消息提交、流式响应、工具调用、预算控制 |
| **Rust TUI** | 完整 | ratatui + crossterm 终端界面，支持 markdown 渲染、语法高亮、分屏面板、多主题 |
| **Headless IPC** | 完整 | JSONL over stdio 协议，5-crate IPC 协议栈（protocol/transport/adapters/client） |
| **文件工具** | 完整 | Read、Write、Edit、Glob、Grep，支持文件快照和 undo |
| **Shell 执行** | 完整 | Bash 执行，PTY 支持，超时控制，进程组管理 |
| **权限与沙箱** | 完整 | auto/acd/bypass 三种权限模式，工具级别 + 沙箱级别控制；auto mode 安全分类器 |
| **Skills 系统** | 完整 | 内置 skill、用户自定义 skill、插件 skill、MCP skill |
| **多后端模型** | 完整 | Anthropic Direct、AWS Bedrock、Google Vertex、OpenAI 兼容、Azure、OpenAI Codex |
| **会话持久化** | 完整 | 会话保存、恢复、--continue 指定 session、--resume 最新 session |
| **上下文压缩** | 完整 | autocompact 管道：budget 评估 → snip → 摘要 → 替换 |
| **Daemon 模式** | 完整 | 常驻后台进程，HTTP API，团队记忆服务器，GitHub PR 活动路由 |
| **Web UI 模式** | 完整 | 浏览器可访问聊天界面 |
| **子 Agent 系统** | 完整 | Agent fork/delegate，监督者模式，深度限制 |
| **Plugin 系统** | 完整 | 插件发现、安装、manifest 解析、LSP 声明、命令注册 |
| **优雅关闭** | 完整 | Phase I shutdown：子进程清理、状态持久化、skill usage 保存 |
| **LSP 集成** | 完整 | LSP 服务器管理、语言推荐、diagnostics 收集 |
| **Computer Use** | 完整 | 截图、鼠标点击、键盘输入、滚轮、拖拽等桌面控制 |
| **Chrome 集成** | 完整 | Chrome native messaging host、MCP 桥接 |
| **遥测与审计** | 完整 | Langfuse 集成、事件审计日志、InteractionSpan |
| **Keybinding 系统** | 完整 | 用户可配置键位绑定，编辑模式支持 |
| **Voice 模式** | 初始 | 语音输入支持 |

## 关键架构差异（与 TypeScript 原版）

### 核心架构对比

| 维度 | Claude Code (TypeScript) | allthecodes (Rust) |
|------|------------------------|---------------------|
| **运行时** | Node.js 或 Bun | 无（静态二进制） |
| **启动方式** | `npx @anthropic-ai/claude-code` | `./allthecodes` |
| **TUI 框架** | React/Ink（虚拟 DOM + Yoga Flexbox 布局） | ratatui + crossterm（即时模式渲染，无虚拟 DOM） |
| **模块系统** | npm 包 + TypeScript 模块 | Cargo workspace + 40 独立 crate |
| **IPC 协议** | 内置（前后端耦合） | 显式 JSONL 协议（4 层独立 IPC crate） |
| **工具定义** | 类 + 装饰器模式 | Trait 对象 + 工厂函数 + JSON Schema 派生 |
| **权限模型** | 工具级别 + 命令级别 | 工具级别 + 沙箱级别 + Auto Mode 安全分类器 |
| **System Prompt** | 函数式组合 | 静态段 + 动态段 + Hook 注入 |
| **上下文压缩** | 迭代式裁剪 + 摘要 | 结构化压缩管道（budget → snip → compact → collapse） |
| **状态管理** | React state + 可变对象 | AppState 结构化数据 + Arc 共享 + update_app_state() |
| **并发模型** | Node.js 事件循环 + Promise | tokio 多线程异步运行时 + async/await |
| **进程管理** | child_process + exec | tokio::process + PTY + 信号链 + kill_on_drop |
| **错误处理** | try/catch（运行时类型） | Result<T, E> + ? 操作符（编译时类型） |
| **分发方式** | npm 包（npx） | GitHub Releases + npm tarball（二进制） |
| **配置层** | 单层 settings.json | 多层（managed → user → project → local + env 覆盖） |

### 工具系统的实现差异

TypeScript 原版的工具定义使用类和装饰器：

```typescript
class BashTool extends Tool<BashInput, BashOutput> {
    @toolMethod
    async call(input: BashInput): Promise<BashOutput> {
        // ...
    }
}
```

Rust 版本使用 trait + 工厂函数：

```rust
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn input_schema(&self) -> serde_json::Value;
    async fn validate_input(&self, input: Value) -> Result<()>;
    async fn check_permissions(&self, ctx: &PermissionContext) -> Result<()>;
    async fn call(&self, input: Value) -> Result<ToolResult>;
}
```

### TUI 的实现差异

TypeScript 版使用 React/Ink，它维护虚拟 DOM 并在终端中渲染 diff：

```typescript
// React/Ink: 声明式组件
function ChatMessages({ messages }: { messages: Message[] }) {
    return (
        <Box flexDirection="column">
            {messages.map(msg => <Message key={msg.id} msg={msg} />)}
        </Box>
    );
}
```

allthecodes 使用 ratatui + crossterm，在每个渲染周期中直接绘制：

```rust
// ratatui: 即时模式渲染
fn render(frame: &mut Frame, state: &AppState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(3)])
        .split(frame.area());

    // 直接绘制消息列表
    frame.render_widget(ChatWidget::new(&state.messages), chunks[0]);
    // 直接绘制输入行
    frame.render_widget(InputWidget::new(&state.input), chunks[1]);
}
```

两种方式各有优劣：React/Ink 的声明式风格更容易维护复杂 UI 状态（如消息列表增量更新），而 ratatui 提供了更大的渲染控制权（精确控制每个像素的字符和颜色），且没有虚拟 DOM diff 的计算开销。

## 核心工作流

allthecodes 的完整生命周期分为三个阶段：

```
Phase A: 参数解析 + 快速路径
    │
    ├── 快速路径: --version → 输出版本退出
    │              --chrome-native-host → Chrome 桥接
    │              --dump-system-prompt → 打印提示词退出
    │              --export-ui-snapshots → 导出 UI 截图
    │              --daemon-worker → 守护进程工作模式
    │
    └── 完整初始化 (Phase B)
         │
         ├── 加载环境变量、日志初始化
         ├── 加载多层设置（managed → user → project → local）
         ├── 权限模式解析
         ├── 插件初始化
         ├── 工具注册（50+ 基础工具）
         ├── Skills 加载（内置 + 项目 + 插件）
         ├── MCP 服务器发现与连接
         ├── Chrome 集成
         ├── Computer Use 工具注册
         ├── 模型选择（CLI → config → provider → 默认）
         ├── AppState 创建
         ├── 会话恢复（--resume / --continue）
         └── QueryEngine 创建
              │
              └── 模式选择
                   ├── TUI    → ratatui 终端界面
                   ├── Headless → JSONL IPC
                   ├── Daemon → axum HTTP 后台服务
                   ├── Web    → HTTP Web UI
                   ├── Print  → 单次问答输出
                   └── JSON   → 结构化 JSON 输出

Phase I: 关闭与清理
    ├── 优雅关闭 (graceful_shutdown)
    ├── 子进程清理
    ├── 技能使用持久化
    ├── Langfuse 关闭
    └── 退出码返回
```

### 生命周期状态机

```
Phase A (CLI) ──快速路径──→ 立即退出
     │
     └──→ Phase B (初始化) ──→ 就绪
                                  │
                           ┌──────┼──────┐
                           ↓      ↓      ↓
                       TUI   Headless  Daemon/Web
                           │      │      │
                           └──────┴──────┘
                                  │
                                  ↓
                            Phase I (关闭)
                                  │
                                  ↓
                             退出
```

## 设置系统

allthecodes 的设置加载是分层的，支持从多个来源合并：

```
加载顺序（后层覆盖前层）:
  1. Managed settings（企业托管）
  2. User settings（~/.allthecodes/settings.json）
  3. Project settings（.allthecodes/settings.json，项目目录）
  4. Local overrides（.allthecodes/settings.local.json，不提交版本控制）
  5. 环境变量覆盖
  6. CLI 参数（最高优先级）
```

每个设置项可以追踪来源，`/config show` 命令可以展示每个配置项来自哪个层。

## 权限系统

allthecodes 的权限系统比 TypeScript 原版多了一层沙箱控制：

```
权限决策链路:

工具调用请求
    │
    ├── 1. validate_input()    → 参数合法性检查
    │
    ├── 2. check_permissions() → 权限模式决策
    │      │
    │      ├── Auto 模式: 安全分类器判断（高风险 → 提示用户；低风险 → 自动放行）
    │      ├── ACD 模式: 始终提示用户确认
    │      └── Bypass 模式: 自动放行所有操作
    │
    ├── 3. Sandbox 策略       → 网络隔离、文件系统限制
    │
    └── 4. 执行               → 实际工具调用
```

权限规则从 5 个来源汇聚（session → project → user → managed → default），支持工具名匹配、命令模式、路径前缀等多种匹配方式。

## 运行模式

allthecodes 支持多种运行模式，适应不同的使用场景：

| 模式 | CLI 标志 | 典型场景 |
|------|---------|---------|
| **TUI** | 默认 | 交互式开发，推荐日常使用 |
| **Print** | `-p "prompt"` | 单次问答，管道集成 |
| **JSON** | `--output-format json -p "prompt"` | 程序化调用，SDK 集成 |
| **Headless** | `--headless` | 自定义前端，自动化测试 |
| **Daemon** | `--daemon` | 后台常驻，团队协作 |
| **Web** | `--web --web-port 3001` | 浏览器访问，远程开发 |
| **Resume** | `--resume` | 恢复最近的会话 |
| **Continue** | `--continue <session_id>` | 恢复指定会话 |

## Tech Stack 总览

```
语言:     Rust (edition 2021, toolchain 1.91.1)
异步:     tokio (multi-thread runtime) + futures
CLI:      clap (derive)
TUI:      ratatui 0.29 + crossterm 0.28
HTTP:     reqwest + axum 0.8
序列化:   serde + serde_json
协议:     JSONL (IPC), SSE (API streaming), MCP (工具)
认证:     Keychain (keyring), OAuth, API Key
搜索:     ignore (gitignore-aware), walkdir, glob
解析:     tree-sitter (AST), pulldown-cmark (markdown)
缓存:     lru
差异:     similar (text diff)
日志:     tracing + tracing-subscriber + OpenTelemetry
构建:     Cargo workspace, 40 crates, release profile with debug info
测试:     assert_cmd + assert_fs + insta (snapshot) + tempfile
```

## 它不是另一个 Claude Code

- **不是逐文件翻译**：allthecodes 没有照搬 TypeScript 源码，而是在理解行为的基础上重新实现
- **不是精简版**：目标是完整功能覆盖，不以 Lite 或 Demo 为边界
- **不是官方产品**：这是独立开发的开源项目，与 Anthropic 无关联
- **不是原地重构**：创建新仓库而非在原仓库上重构，是为了彻底解决工程结构问题
