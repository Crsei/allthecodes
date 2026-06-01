# Codex SDK 功能总结

> OpenAI Codex — 开源 AI 编程代理 CLI/SDK，采用 Rust 核心 + 多语言 SDK 架构。

---

## 目录

- [1. 项目架构](#1-项目架构)
- [2. 多语言 SDK](#2-多语言-sdk)
- [3. 工具执行系统](#3-工具执行系统)
- [4. 沙盒安全机制](#4-沙盒安全机制)
- [5. 审批与权限系统](#5-审批与权限系统)
- [6. MCP 协议支持](#6-mcp-协议支持)
- [7. App-Server JSON-RPC v2 API](#7-app-server-json-rpc-v2-api)
- [8. 会话管理](#8-会话管理)
- [9. 配置系统](#9-配置系统)
- [10. 输出项类型](#10-输出项类型)
- [11. 依赖关系图](#11-依赖关系图)

---

## 1. 项目架构

### 顶层目录结构

```
codex/
├── codex-rs/                 # Rust 主实现
│   ├── core/                 # 核心业务逻辑（Agent 执行、工具调用、沙盒策略）
│   ├── protocol/             # 协议数据结构定义（Function, Content, AskForApproval 等）
│   ├── tui/                  # 交互式终端 UI（基于 Ratatui）
│   ├── exec/                 # 非交互式 CLI（用于 CI/自动化）
│   ├── cli/                  # CLI 多功能分发器
│   ├── app-server/           # JSON-RPC v2 服务器（供 IDE 扩展使用）
│   ├── app-server-protocol/  # JSON-RPC v2 API 定义
│   ├── mcp-server/           # MCP 服务器模式
│   ├── sandboxing/           # 平台沙盒隔离
│   ├── config/               # 配置管理
│   ├── state/                # 会话/状态持久化（SQLite）
│   ├── tools/                # 内置工具实现
│   ├── skills/               # 技能系统
│   └── [其他 40+ 工具 crate]
├── codex-cli/                # 旧版 TypeScript CLI（已弃用）
├── sdk/
│   ├── typescript/           # TypeScript SDK（@openai/codex-sdk）
│   ├── python/               # Python SDK（codex-app-server-sdk）
│   └── python-runtime/       # Python 运行时打包
├── docs/                     # 用户文档
└── scripts/                  # 构建和发布脚本
```

### 整体架构图

```
┌──────────────────────────────────────────────────────────┐
│                      用户界面层                           │
│  ┌──────────────┐  ┌───────────────┐  ┌──────────────┐  │
│  │  TUI (Rust)  │  │  App-Server   │  │  VS Code 等  │  │
│  │  交互式终端   │  │ JSON-RPC v2   │  │  IDE 扩展    │  │
│  └──────┬───────┘  └───────┬───────┘  └──────┬───────┘  │
└─────────┼──────────────────┼─────────────────┼──────────┘
          │                  │                 │
┌─────────┴──────────────────┴─────────────────┴──────────┐
│                      SDK 层                              │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐   │
│  │TypeScript SDK│  │  Python SDK  │  │   Rust API   │   │
│  └──────────────┘  └──────────────┘  └──────────────┘   │
└────────────────────────┬────────────────────────────────┘
                         │
┌────────────────────────┴────────────────────────────────┐
│              核心业务逻辑 (codex-core)                    │
│  ┌─────────────┐  ┌───────────┐  ┌──────────────────┐  │
│  │ Agent 执行   │  │ 工具执行   │  │ 审批工作流       │  │
│  └─────────────┘  └───────────┘  └──────────────────┘  │
└───────┬─────────────────┬──────────────────┬────────────┘
        │                 │                  │
┌───────┴──────┐  ┌───────┴───────┐  ┌──────┴──────────┐
│   沙盒隔离    │  │  配置管理      │  │ 状态/会话持久化  │
│ (平台特定)    │  │               │  │  (SQLite)       │
└──────────────┘  └───────────────┘  └─────────────────┘
```

### CLI 入口模式

| 命令 | 模式 | 说明 |
|------|------|------|
| `codex` | 交互式 | 默认 TUI 模式，通过 `codex-tui` |
| `codex exec` | 非交互式 | 无头模式，输出 JSONL 事件，适合 CI |
| `codex app-server` | 服务器 | JSON-RPC v2 服务器，供 IDE 扩展调用 |
| `codex mcp-server` | MCP 服务器 | 暴露工具给其他 MCP 客户端 |

---

## 2. 多语言 SDK

### TypeScript SDK (`@openai/codex-sdk`)

```typescript
// 主要导出
export { Codex }          // 主客户端类，启动并管理 CLI 进程
export { Thread }         // 对话线程
export type { RunResult, RunStreamedResult }
export type { ThreadEvent, ThreadItem }
export type { ThreadOptions, CodexOptions, TurnOptions }
```

**核心用法：**

```typescript
import { Codex } from "@openai/codex-sdk";

// 初始化
const codex = new Codex({
  codexPathOverride?: string,
  env?: Record<string, string>,
  config?: Record<string, any>,
  baseUrl?: string,
  apiKey?: string,
});

// 创建对话线程
const thread = codex.startThread({
  model?: string,
  sandboxMode?: "read-only" | "workspace-write" | "danger-full-access",
  workingDirectory?: string,
  skipGitRepoCheck?: boolean,
  modelReasoningEffort?: "minimal" | "low" | "medium" | "high" | "xhigh",
  networkAccessEnabled?: boolean,
  webSearchMode?: "disabled" | "cached" | "live",
  approvalPolicy?: "never" | "on-request" | "on-failure" | "untrusted",
  additionalDirectories?: string[],
});

// 同步调用
const result = await thread.run("请帮我重构这个函数");
// result.items: ThreadItem[]
// result.finalResponse: string
// result.usage: Usage | null

// 流式调用
const stream = await thread.runStreamed("请帮我重构这个函数");
for await (const event of stream) {
  // 处理流式事件
}
```

**输入类型：**

```typescript
type Input = string | UserInput[];
type UserInput =
  | { type: "text"; text: string }
  | { type: "local_image"; path: string };
```

**流式事件类型：**

```typescript
type ThreadEvent =
  | ThreadStartedEvent
  | TurnStartedEvent
  | ItemStartedEvent
  | ItemUpdatedEvent
  | ItemCompletedEvent
  | TurnCompletedEvent
  | TurnFailedEvent
  | ThreadErrorEvent;
```

### Python SDK (`codex-app-server-sdk`)

```python
from codex_app_server import Codex

codex = Codex()                              # 自动启动 CLI 进程
thread = codex.thread_start(model="gpt-5")   # 创建对话
result = thread.run("请帮我重构这个函数")      # 同步调用

result.final_response   # Agent 文本回复
result.items            # ThreadItem 列表
```

功能与 TypeScript SDK 对等，底层通过 app-server JSON-RPC 协议通信。

---

## 3. 工具执行系统

### 内置工具

| 工具 | 功能 |
|------|------|
| **Shell 命令执行** | 沙盒化运行 shell 命令，输出流式捕获，退出码处理 |
| **文件读取** | 读取文件内容 |
| **文件写入** | 创建或覆盖文件 |
| **文件编辑 (Patch)** | 语义化 patch，RFC 风格 diff，原子事务 |
| **Git 操作** | 分支/提交感知，仓库分析 |
| **Web 搜索** | live/cached 两种模式 |
| **MCP 工具调用** | 调用外部 MCP 服务器提供的工具 |

### 文件操作特性

- **语义化 Patch** — 使用 RFC 风格 diff 进行精确编辑
- **Tree-sitter AST 感知** — 语言级别的代码理解和编辑
- **原子事务** — 确保文件操作的一致性
- **Git 集成** — 自动跟踪变更上下文

### 技能系统 (Skills)

高级别的 Agent 能力封装：

- **GitHub 技能** — PR 自动化、Issue 分类
- **Web 搜索技能** — 网络信息检索
- **MCP 工具调用** — 外部服务集成

技能通过 `AGENTS.md` 文件或 `config.toml` 配置加载。

---

## 4. 沙盒安全机制

### 沙盒模式

| 模式 | 说明 |
|------|------|
| `read-only` | 仅读取，禁止写入 |
| `workspace-write` | 允许写入工作区，禁用网络 |
| `danger-full-access` | 完全访问（仅限开发环境） |

### 平台特定实现

| 平台 | 技术 | 关键特性 |
|------|------|----------|
| **macOS** | Apple Seatbelt (`sandbox-exec`) | 只读文件系统（除可写根）、默认禁用网络、基于 Profile 的策略 |
| **Linux** | Bubblewrap + Landlock + seccomp | 命名空间隔离、Capability 降级、seccomp 文件系统策略 |
| **Windows** | Restricted Tokens | Token 权限降级、受限读取根、系统默认值继承 |

### 安全默认值

- 默认**禁用网络访问**
- 文件系统写入范围**严格限制**在工作区内
- 所有命令执行均有**完整审计追踪**

---

## 5. 审批与权限系统

### 审批模式

| 模式 | 说明 |
|------|------|
| `suggest` | Agent 建议操作，用户手动执行 |
| `auto-edit` | 文件编辑自动执行，命令需审批 |
| `full-auto` | 所有操作自动执行 |

### 审批策略 (SDK 级别)

| 策略 | 说明 |
|------|------|
| `never` | 从不请求审批 |
| `on-request` | 仅在 Agent 主动请求时 |
| `on-failure` | 操作失败时请求 |
| `untrusted` | 不信任的操作均需审批 |

### 特性

- **按工具粒度**的审批覆盖
- **执行策略控制** — 定义何时允许工具运行
- **MCP 工具审批工作流** — 外部工具调用的独立审批链路

---

## 6. MCP 协议支持

### 作为 MCP 客户端

```
Codex  →  连接外部 MCP 服务器  →  调用其工具
```

- 从 `config.toml` 自动发现 MCP 服务器
- 工具调用遵循审批工作流
- 管理服务器生命周期（启动、健康检查、关闭）

### 作为 MCP 服务器

```
其他 MCP 客户端  →  连接 Codex MCP Server  →  使用 Codex 工具
```

- 通过 `codex mcp-server` 启动
- 暴露 shell 执行、文件操作等工具
- 其他 AI Agent 可调用 Codex 能力

### 配置示例

```toml
# ~/.codex/config.toml
[mcp_servers.example]
command = "npx @example/mcp"
```

---

## 7. App-Server JSON-RPC v2 API

为 IDE 扩展（VS Code）和桌面应用提供标准化接口。

### 请求格式

```json
{"method": "thread/start", "params": {...}, "id": 123}
```

### API 方法

| 方法组 | 方法 | 说明 |
|--------|------|------|
| **初始化** | `initialize` | 握手与能力协商 |
| **线程管理** | `thread/start` | 创建新对话线程 |
| | `thread/resume` | 恢复已有线程 |
| | `thread/list` | 列出所有线程 |
| | `thread/archive` | 归档线程 |
| **轮次控制** | `turn/start` | 开始新的对话轮次 |
| | `turn/interrupt` | 中断当前轮次 |
| **内容读取** | `item/read` | 读取对话项 |
| | `item/update` | 更新对话项 |
| **审批** | `approval/create` | 创建审批请求 |
| | `approval/resolve` | 解决审批请求 |
| **配置** | `config/read` | 读取配置 |
| | `config/write` | 写入配置 |
| **登录** | `login/*` | 认证相关 |

### 服务器推送通知

```
thread/started          — 线程已创建
thread/archived         — 线程已归档
turn/started            — 轮次已开始
turn/completed          — 轮次已完成
turn/failed             — 轮次失败
item/started            — 项开始处理
item/updated            — 项已更新
item/completed          — 项处理完成
item/agentMessage/delta — 流式文本增量
approval/required       — 需要用户审批
```

### 通信方式

- **stdio** — 标准输入输出（默认）
- **WebSocket** — 网络连接

---

## 8. 会话管理

| 功能 | 说明 |
|------|------|
| **持久化存储** | SQLite 数据库，保存完整对话历史 |
| **对话压缩** | 长对话自动 compaction，控制上下文长度 |
| **会话恢复** | 支持通过 `thread/resume` 恢复先前会话 |
| **会话回滚** | Rollout 文件支持回滚到先前状态 |
| **Git 上下文** | 自动跟踪当前分支、commit、remote 信息 |
| **临时会话** | `--ephemeral` 标志用于不保存的一次性会话 |

---

## 9. 配置系统

### 配置文件

**全局配置** — `~/.codex/config.toml`

```toml
model = "gpt-4"
sandbox_mode = "workspace-write"
approval_mode = "auto-edit"

[mcp_servers.example]
command = "npx @example/mcp"

[notify]
script = "terminal-notifier -message '{message}'"
```

**项目级指令** — 项目根目录下的 `AGENTS.md`

### 模型控制

| 参数 | 说明 |
|------|------|
| `model` | 模型选择（per-thread 粒度） |
| `modelReasoningEffort` | 推理强度：`minimal` / `low` / `medium` / `high` / `xhigh` |
| **结构化输出** | 支持 JSON Schema 约束输出格式 |
| **Token 使用量** | 自动跟踪并返回用量信息 |

### 校验

- 自动生成 JSON Schema（`codex-rs/core/config.schema.json`）
- 启动时校验配置合法性

---

## 10. 输出项类型

SDK 返回的 `ThreadItem` 涵盖 Agent 执行过程中的所有产出：

| 类型 | 说明 |
|------|------|
| `AgentMessage` | Agent 的文本回复 |
| `Reasoning` | 模型推理过程（o3/o4 模型可见） |
| `CommandExecution` | Shell 命令执行结果（命令、输出、退出码） |
| `FileChange` | 文件变更 patch（创建、修改、删除） |
| `McpToolCall` | MCP 工具调用及结果 |
| `WebSearch` | Web 搜索结果 |
| `TodoList` | Agent 内部任务列表 |
| `Error` | 非致命错误信息 |

---

## 11. 依赖关系图

```
codex-core（所有组件的基础）
  ├── codex-protocol        （数据类型定义）
  ├── codex-sandboxing      （安全沙盒）
  ├── codex-config          （配置管理）
  ├── codex-state           （会话存储）
  └── codex-tools           （工具定义）

codex-exec（无头 CLI）
  ├── codex-core
  ├── codex-app-server-client
  └── codex-feedback        （遥测）

codex-tui（交互式终端）
  ├── codex-core
  ├── codex-exec
  └── ratatui               （UI 框架）

codex-app-server（JSON-RPC 服务器）
  ├── codex-exec-server
  ├── codex-app-server-protocol
  └── codex-core

codex-mcp-server（MCP 模式）
  ├── codex-core
  └── rmcp                  （MCP SDK）

TypeScript SDK (@openai/codex-sdk)
  └── 启动 @openai/codex CLI，通过 JSONL 通信

Python SDK (codex-app-server-sdk)
  └── 启动 codex CLI，通过 app-server JSON-RPC 通信
```

---

> **总结**：Codex SDK 实现了一个完整的 AI 编程代理框架，涵盖安全沙盒化的工具执行、多语言 SDK 封装、IDE 集成接口、MCP 生态互通、灵活的审批权限系统以及持久化的会话管理，是构建 AI Coding Agent 应用的全栈解决方案。
