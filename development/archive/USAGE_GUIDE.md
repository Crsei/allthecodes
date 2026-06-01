# Claude Code Rust Lite — 使用指南

> 版本: 0.1.0-lite | 最后更新: 2026-04-06
>
> 当前 CLI 参数、模式优先级与 REPL 命令注册表请优先参考 [`CLI_REFERENCE.md`](CLI_REFERENCE.md)。本文件中的部分历史示例和说明已经落后于当前实现。
> 每个斜杠命令的详细子参数和示例见 [`COMMAND_REFERENCE.md`](COMMAND_REFERENCE.md)。

---

## 目录

1. [构建与安装](#构建与安装)
2. [快速开始](#快速开始)
3. [命令行参数](#命令行参数)
4. [运行模式](#运行模式)
5. [TypeScript SDK](#typescript-sdk)
6. [Python SDK](#python-sdk)
7. [斜杠命令](#斜杠命令)
7. [内置工具](#内置工具)
8. [Skills 技能系统](#skills-技能系统)
9. [权限系统](#权限系统)
10. [配置管理](#配置管理)
11. [会话管理](#会话管理)
12. [CLAUDE.md 项目���令](#claudemd-项目指令)
13. [键盘快捷键](#键盘快捷键)
14. [故障排查](#故障排查)

---

## 构建与安装

### 前置要求

- **Rust 工具链**: 1.70+ (推荐 stable 最新版)
- **系统依赖**:
  - Linux: `pkg-config`, `libssl-dev`, `cmake`
  - macOS: Xcode Command Line Tools
  - Windows: Visual Studio C++ Build Tools

### 构建

```bash
cd cc/rust
cargo build --release
```

生成的二进制文件位于 `target/release/claude-code-rs`。

### 运行测试

```bash
cargo test
```

---

## 快速开始

### 1. 配置 `.env` (推荐)

在项目根目录创建 `.env` 文件，启动时自动加载：

```env
# 提供商 API Key (任选一个)
OPENROUTER_API_KEY=sk-or-v1-...

# 模型 (可选，不设则使用提供商默认模型)
CLAUDE_MODEL=anthropic/claude-sonnet-4
```

也可以用传统 `export` 方式或在 REPL 中登录：

```bash
# 方式 2: export
export ANTHROPIC_API_KEY="sk-ant-api03-..."

# 方式 3: REPL 中直接输入 API Key
> /login sk-ant-api03-...

# 方式 4: OAuth 登录 (Claude.ai Pro/Max 订阅用户)
> /login claude-ai
# → 打印授权 URL → 浏览器中授权 → 复制 code
> /login-code eyJhb...

# 方式 5: OAuth 登录 (Console API 计费用户)
> /login console
# → 同上流程，登录后自动创建 API Key 存入 Keychain

# 查看认证状态
> /login status
```

### 2. 启动交互式 REPL

```bash
claude-code-rs
```

进入交互模式后，输入自然语言指令或斜杠命令即可。输入 `/exit` 或按 `Ctrl+D` 退出。

### 3. 单次查询 (Print 模式)

```bash
claude-code-rs -p "解释这段代码的作用"
```

输出模型回复后立即退出，适合管道和脚本集成。

### 4. 带初始提示启动

```bash
claude-code-rs "帮我重构这个函数"
```

启动 REPL 并自动发送第一条消息。

---

## 命令行参数

| 参数 | 短写 | 说明 | 默认值 |
|------|------|------|--------|
| `--version` | `-V` | 打印版本号并退出 | — |
| `--print` | `-p` | Print 模式: 输出回复后退出 | false |
| `--resume` | — | 恢复最近一次会话 | false |
| `--continue <ID>` | — | 恢复指定会话 ID | — |
| `--max-turns <N>` | — | 最大对话轮数限制 | 无限 |
| `--cwd <PATH>` | `-C` | 指定工作目录 | 当前目录 |
| `--model <MODEL>` | `-m` | 指定模型 | `claude-sonnet-4-20250514` |
| `--system-prompt <TEXT>` | — | 替换默认系统提示词 | 内置提示词 |
| `--append-system-prompt <TEXT>` | — | 追加到系统提示词末尾 | — |
| `--permission-mode <MODE>` | — | 权限模式: `default`, `auto`, `bypass` | `default` |
| `--verbose` | `-v` | 启用详细日志输出 | false |
| `--max-budget <USD>` | — | 最大消费预算 (美元) | 无限 |
| `--output-format <FMT>` | — | 输出格式: `text`, `json`, `stream-json` | text |

### 隐藏参数 (开发/调试用)

| 参数 | 说明 |
|------|------|
| `--dump-system-prompt` | 打印完整系统提示词并退出 |
| `--init-only` | 仅执行初始化然后退出 |
| `--headless` | Headless 模式: 无 TUI，通过 stdin/stdout JSONL 与外部 UI 通信 |
| `--daemon` | Daemon 模式: 启动 KAIROS 常驻助手 HTTP 服务 (需要 `FEATURE_KAIROS=1`) |
| `--port <PORT>` | Daemon HTTP 端口 (默认: 19836) |

---

## 运行模式

### OpenTUI UI (推荐)

```bash
cd ui
./run.sh
```

基于 OpenTUI (`@opentui/core` + `@opentui/react`) 的终端前端，通过 IPC 与 Rust 后端通信：

- **60fps 渲染**，double-buffering + 同步更新
- **流式 Markdown** — `<Markdown streaming>` 组件实时渲染
- **虚拟列表** — 支持大量消息高效渲染
- **Vim 模式** — `Ctrl+G` 切换，支持 Normal/Insert/Visual 模式
- **权限弹窗** — 工具调用时自动弹出，支持 y/n/a 快捷键
- **Flexbox 布局** — 自适应终端尺寸

前端依赖: Bun/Node.js 20+, 在 `ui/` 目录执行 `bun install`。

环境变量:
- `CC_RUST_BINARY` — 指定 Rust 二进制路径 (不设则自动查找 target/ 下)

架构详情: [`architecture/ink-terminal-frontend.md`](../architecture/ink-terminal-frontend.md)

### 交互式 TUI (legacy)

```bash
claude-code-rs
```

原有 ratatui + crossterm 全屏终端界面：

- 顶部显示欢迎信息 (版本、模型、会话 ID)
- 消息区域可滚动浏览对话历史
- 底部输入框输入文本发送给模型
- 状态栏显示模型名称、消息数、费用、当前状态
- 输入斜杠命令执行本地操作
- 模型可调用工具执行文件操作、命令运行等
- 流式输出期间显示 Braille 点阵旋转动画
- 按 `Ctrl+C` 中断当前流式输出
- 按 `Ctrl+D` 退出

### Print 模式

```bash
echo "解释 main.rs" | claude-code-rs -p
claude-code-rs -p "列出所有 TODO"
```

- 非交互式，适合脚本和管道
- 输出纯文本到 stdout
- 返回 0 (成功) 或 1 (失败)

### JSON 输出模式 (SDK 用)

```bash
# 通过参数���入 prompt
claude-code-rs --output-format json -p "你好"

# 通过 stdin 传入 prompt
echo "你好" | claude-code-rs --output-format json -p
```

- 输出 JSONL (每行一个 JSON 对象) 到 stdout
- 用于 TypeScript SDK 的进程通信协议
- 每次调用以一个 `{"type": "result", ...}` 消息结束
- 详见 [`sdk/typescript/README.md`](../sdk/typescript/README.md)

### KAIROS 常驻助手模式 (Daemon)

KAIROS 模式将 cc-rust 变为常驻后台助手，支持自主执行、外部消息接入和推送通知。

#### 启动 Daemon

```bash
# 最小启用 (常驻助手 + Brief 结构化输出)
FEATURE_KAIROS=1 FEATURE_KAIROS_BRIEF=1 cc-rust --daemon

# 全功能启用
FEATURE_KAIROS=1 \
FEATURE_KAIROS_BRIEF=1 \
FEATURE_KAIROS_CHANNELS=1 \
FEATURE_KAIROS_PUSH_NOTIFICATION=1 \
FEATURE_KAIROS_GITHUB_WEBHOOKS=1 \
FEATURE_PROACTIVE=1 \
cc-rust --daemon

# 指定端口
cc-rust --daemon --port 19837
```

#### Feature Flag 说明

| 环境变量 | 说明 | 依赖 |
|----------|------|------|
| `FEATURE_KAIROS` | 主开关，启用 daemon 模式 | — |
| `FEATURE_KAIROS_BRIEF` | BriefTool 结构化输出 | 需要 KAIROS |
| `FEATURE_KAIROS_CHANNELS` | 外部 MCP Channel 消息接入 | 需要 KAIROS |
| `FEATURE_KAIROS_PUSH_NOTIFICATION` | 推送通知 (Windows Toast + Webhook) | 需要 KAIROS |
| `FEATURE_KAIROS_GITHUB_WEBHOOKS` | GitHub Webhook 事件 | 需要 KAIROS |
| `FEATURE_PROACTIVE` | 自主 tick 循环 (30s 间隔) | 独立，KAIROS 隐含启用 |

依赖规则: 子功能未设置父功能时自动禁用并输出 warn 日志。

#### HTTP API 端点

Daemon 在 `127.0.0.1:19836` 提供以下端点:

| 端点 | 方法 | 说明 |
|------|------|------|
| `/health` | GET | 健康检查 |
| `/api/submit` | POST | 提交用户消息 `{text, id}` |
| `/api/abort` | POST | 中止当前查询 |
| `/api/status` | GET | Daemon 状态 (kairos/proactive/sleeping/clients) |
| `/api/events` | GET | SSE 事件流 (支持 `client_id` + `last_event_id` 断线重连) |
| `/api/attach` | POST | 前端连接 `{client_id, last_seen_event}` |
| `/api/detach` | POST | 前端断开 |
| `/api/command` | POST | 执行斜杠命令 `{raw}` |
| `/api/permission` | POST | 权限响应 `{tool_use_id, decision}` |
| `/webhook/github` | POST | GitHub Webhook (HMAC-SHA256 验证) |
| `/webhook/slack` | POST | Slack Webhook (签名验证) |
| `/webhook/generic` | POST | 通用 Webhook |

#### Proactive Tick 机制

启用 `FEATURE_PROACTIVE` 后，daemon 每 30 秒触发一次 tick:
- 检查 `is_query_running` 和 `is_sleeping`，跳过繁忙/休眠状态
- 向 QueryEngine 提交 `<tick_tag>` 消息 (含本地时间 + 终端焦点状态)
- 模型根据系统提示决定: 执行任务 / 询问用户 / 调用 Sleep 休眠
- 首次 tick 问候并询问任务；后续 tick 寻找有用工作
- `terminal_focus: false` (无前端连接) 时高度自主

#### 推送通知配置

在 `settings.json` 中配置:

```json
{
  "notifications": {
    "windows_toast": { "enabled": true, "only_when_detached": true },
    "webhook": {
      "enabled": true,
      "url": "https://api.day.app/YOUR_KEY",
      "headers": {},
      "events": ["task_complete", "agent_done", "error"]
    }
  }
}
```

支持: Bark (iOS)、Pushover、钉钉 Robot、企业微信、任意 POST URL。

#### 每日日志

KAIROS 模式下自动在 `~/.cc-rust/logs/YYYY/MM/YYYY-MM-DD.md` 追加日志。
使用 `/dream` 命令可将近期日志蒸馏为结构化记忆。

### 会话恢复模式

```bash
# 恢复最近的会话
claude-code-rs --resume

# 恢复指定会话
claude-code-rs --continue 550e8400-e29b-41d4-a716-446655440000
```

---

## TypeScript SDK

提供类型��全的 TypeScript 封装，通过子进程与 `claude-code-rs` 交互。

### 安装与构建

```bash
cd sdk/typescript/
npm install && npm run build
```

### 快速使用

```typescript
import { ClaudeCode } from "claude-code-rs-sdk";

const client = new ClaudeCode();
const session = client.startSession({ permissionMode: "auto" });

// 非流式
const turn = await session.run("列出文件");
console.log(turn.finalResponse);

// 流式
const { events } = await session.runStreamed("分析代码");
for await (const event of events) {
  // 处理 session.started, stream.delta, item.completed, turn.completed 等事��
}
```

### 架构

SDK 不直接调用 API，而是 spawn `claude-code-rs --output-format json` 子进程，通过 stdin/stdout JSONL 通信。与 OpenAI Codex TypeScript SDK 架构一致。

完整文档: [`sdk/typescript/README.md`](../sdk/typescript/README.md)

---

## Python SDK

提供类型安全的 Python 封装，通过子进程与 `claude-code-rs` 交互。与 TypeScript SDK 功能对等。

### 前置要求

- Python >= 3.10
- `claude-code-rs` 二进制（已构建）
- 零运行时依赖

### 安装

```bash
# 开发安装
pip install -e sdk/python/

# 或免安装使用
cd sdk/python/ && PYTHONPATH=src python your_script.py
```

### 快速使用

```python
from claude_code_rs import ClaudeCode, SessionOptions

client = ClaudeCode()
session = client.start_session(SessionOptions(permission_mode="auto"))

# 非流式
turn = session.run("列出文件")
print(turn.final_response)

# 流式
streamed = session.run_streamed("分析代码")
for event in streamed.events:
    # 处理 SessionStartedEvent, StreamDeltaEvent, ItemCompletedEvent, TurnCompletedEvent 等事件
    pass
```

### 架构

与 TypeScript SDK 一致: spawn `claude-code-rs --output-format json` 子进程，通过 stdin/stdout JSONL 通信。

完整文档: [`sdk/python/BUILD.md`](../sdk/python/BUILD.md)

---

## 斜杠命令

在 REPL 中输入 `/` 前缀的命令执行本地操作。

### 基础

| 命令 | 别名 | 说明 |
|------|------|------|
| `/help` | `/h`, `/?` | 列出所有可用命令 |
| `/clear` | — | 清空当前对话历史 |
| `/exit` | `/quit`, `/q` | 退出 REPL |
| `/version` | `/v` | 显示版本号 |

### 模型控制

| 命令 | 别名 | 说明 |
|------|------|------|
| `/model` | — | 查看或切换当前模型 |
| `/cost` | `/usage` | 显示 token 用量和费用 |
| `/fast` | — | 切换快速模式 |
| `/effort` | — | 设置 thinking 深度 (low/medium/high) |

### 配置 & 权限

| 命令 | 别名 | 说明 |
|------|------|------|
| `/config` | `/settings` | 查看或修改配置 |
| `/permissions` | `/perms` | 查看或修改工具权限 |
| `/init` | — | 初始化项目配置 (`.cc-rust/settings.json`) |

### 认证

| 命令 | 说明 |
|------|------|
| `/login` | 认证引导 — 选择具名 profile / OAuth / cloud providers |
| `/login claude-code` | 切换到 Claude Code / Anthropic-compatible profile |
| `/login claude-ai` | Claude.ai OAuth 登录 (Pro/Max 订阅用户，Bearer Token 模式) |
| `/login console` | Console OAuth 登录 (API 计费用户，自动创建 API Key) |
| `/login codex` | 切换到 OpenAI Codex profile |
| `/login codex-oauth` | OpenAI Codex OAuth 登录 |
| `/login codex-cli` | 从 `~/.codex/auth.json` 导入 / 刷新 |
| `/login custom` | 切换到已有 `authProfiles.custom` |
| `/login bedrock` | 当前会话启用 AWS Bedrock provider 并显示缺失环境变量 |
| `/login vertex` | 当前会话启用 GCP Vertex AI provider 并显示缺失环境变量 |
| `/login-code <code>` | 完成 OAuth 登录 — 粘贴授权后获得的 code |
| `/login status` | 查看当前认证状态 |
| `/logout` | 清除所有认证凭据 (Keychain + OAuth tokens) |

### Git

| 命令 | 别名 | 说明 |
|------|------|------|
| `/commit` | — | 创建 git commit |
| `/diff` | — | 显示当前 git 变更 |
| `/branch` | `/br` | 查看或切换分支 |

### 会话 & 上下文

| 命令 | 别名 | 说明 |
|------|------|------|
| `/session` | — | 查看会话信息或列出已保存会话 |
| `/resume` | — | 恢复之前的会话 |
| `/context` | `/ctx` | 显示上下文使用信息 |
| `/files` | — | 列出对话中引用的文件 |
| `/status` | — | 查看会话状态 |
| `/copy` | `/cp` | 复制最后回复到剪贴板 |

### 技能 & 记忆

| 命令 | 别名 | 说明 |
|------|------|------|
| `/skills` | — | 列出可用技能 |
| `/memory` | `/mem` | 查看和管理 CLAUDE.md 项目指令 |

### KAIROS 模式 (需要对应 FEATURE_* 环境变量)

| 命令 | 别名 | 说明 | 依赖 Feature |
|------|------|------|-------------|
| `/sleep` | — | 设置 proactive 休眠时长 (秒) | `PROACTIVE` |
| `/assistant` | `/kairos` | 查看助手模式状态 | `KAIROS` |
| `/daemon` | — | 查看/控制 daemon 进程 (status/stop) | `KAIROS` |
| `/notify` | — | 推送通知配置 (status/test/on/off) | `KAIROS_PUSH_NOTIFICATION` |
| `/channels` | — | 查看已连接的外部 Channel | `KAIROS_CHANNELS` |
| `/dream` | — | 将每日日志蒸馏为结构化记忆 | `KAIROS` |

---

## 内置工具

模型在对话中可以调用以下 8 个工具来完成任务:

### 文件操作

| 工具 | 说明 |
|------|------|
| **Read** | 读取文件内容，支持 `offset` 和 `limit` 参数按范围读取 |
| **Write** | 写入文件，会覆盖已有内容 |
| **Edit** | 文件内精确字符串替换，支持 fuzzy 匹配和 `replace_all` 全局替换 |

### 搜索

| 工具 | 说明 |
|------|------|
| **Glob** | 文件名模式匹配 (如 `**/*.rs`)，按修改时间排序 |
| **Grep** | 正则表达式内容搜索，支持上下文行和多种输出模式 |

### 系统

| 工具 | 说明 |
|------|------|
| **Bash** | 执行 shell 命令，支持超时控制和输出截断 (head 200 + tail 100) |

### 交互

| 工具 | 说明 |
|------|------|
| **AskUser** | 向用户提问并等待回复 |

### KAIROS 工具 (Feature Gate 控制)

| 工具 | 说明 | 依赖 Feature |
|------|------|-------------|
| **Brief** | 向用户发送结构化消息 (markdown + 附件 + status)。Brief 模式下是唯一的用户输出通道 | `KAIROS_BRIEF` |
| **Sleep** | 设置 proactive tick 休眠 (1-3600 秒)。不阻塞线程，仅标记休眠状态 | `PROACTIVE` 或 `KAIROS` |

### 技能

| 工具 | 说明 |
|------|------|
| **Skill** | 调用内置或用户自定义技能 |

---

## Skills 技能系统

Skills 是可复用的提示词模板，通过 Skill 工具调用。技能系统是 Lite 版保留的核心扩展机制。

### 技能来源

| 来源 | 路径 | 说明 |
|------|------|------|
| 内置 | 程序打包 | 随程序发布的预定义技能 |
| 全局 | `~/.cc-rust/skills/*.md` | 用户级自定义技能 |
| 项目 | `.cc-rust/skills/*.md` | 项目级自定义技能 |

### 技能文件格式

```markdown
---
name: my-skill
description: 一句话说明这个技能的用途
---

技能的提示词内容写在这里。

支持的变量替换:
- $ARGUMENTS — 调用时传入的参数
- ${ARG_NAME} — 命名参数
- ${CLAUDE_SKILL_DIR} — 技能文件所在目录
- ${CLAUDE_SESSION_ID} — 当前会话 ID
```

### 使用技能

技能由模型通过 Skill 工具自动调用，也可以通过 `/skills` 命令查看所有可用技能。

### 创建自定义技能

1. 创建技能目录: `mkdir -p .cc-rust/skills/`
2. 创建技能文件: `.cc-rust/skills/my-skill.md`
3. 编写 frontmatter (name, description) 和提示词内容
4. 重启后即可使用

示例 — 代码审查技能:

```markdown
---
name: review
description: 对指定文件进行代码审查
---

请对以下文件进行代码审查，关注:
1. 潜在 bug
2. 性能问题
3. 代码风格

文件: $ARGUMENTS
```

---

## 权限系统

权限系统控制模型对工具的调用权限，保护用户免受意外操作。

### 权限模式

| 模式 | CLI 参数 | 行为 |
|------|---------|------|
| **Default** | `--permission-mode default` | 每次工具调用都需用户确认 |
| **Auto** | `--permission-mode auto` | 自动批准安全操作，危险操作仍需确认 |
| **Bypass** | `--permission-mode bypass` | 跳过所有权限检查 (危险) |
| **Plan** | `--permission-mode plan` | 只读模式，所有写操作需确认 |

### 危险命令检测

Bash / PowerShell 的“危险命令”硬拦截，当前覆盖范围是:

- 未闭合引号
- 引号中的多行字符串
- `rm -rf /`
- `rm -rf ~`
- `rm -rf /*`
- `git push --force`
- `git push -f`
- `git reset --hard`
- `dd if=...`
- `mkfs*`
- `chmod 777 /`
- fork bomb (`:(){ :|:& };:`)
- 写入块设备 (`> /dev/sd*`、`> /dev/nvme*`)
- `curl | sh` / `curl | bash`
- `wget | sh` / `wget | bash`
- `curl | sudo sh` / `curl | sudo bash`
- `wget | sudo sh` / `wget | sudo bash`
- 覆写 `/etc/passwd`
- 覆写 `/etc/shadow`

补充说明:

- 执行前的硬拦截发生在 `src/tools/execution/security.rs`，命中后直接返回 `Dangerous command blocked`
- `BashTool::check_permissions()` 里也会做复合命令拆分，但那里返回的是 `Ask`，不是硬拒绝
- 这套检测是固定 regex + 启发式检查，不是完整 shell 语法树分析

这些命令即使在 Auto 模式下也会触发确认提示。

### 配置权限规则

在 `settings.json` 中设置:

```json
{
  "allowed_tools": ["Read", "Glob", "Grep"],
  "hooks": {
    "before_tool_use": {}
  }
}
```

---

## 配置管理

### `.env` 文件 (推荐)

项目根目录下的 `.env` 文件在启动时自动加载（基于 [dotenvy](https://crates.io/crates/dotenvy)）：

```env
# === 提供商 API Key (任选一个) ===
OPENROUTER_API_KEY=sk-or-v1-...
# 或 ANTHROPIC_API_KEY, OPENAI_API_KEY, DEEPSEEK_API_KEY, ...

# === 模型 (可选) ===
CLAUDE_MODEL=anthropic/claude-sonnet-4
```

### 模型优先级

```
CLI -m 参数 > CLAUDE_MODEL 环境变量/.env > settings.json model > 提供商默认模型
```

### 支持的提供商

程序自动检测已设置 API Key 的提供商（按检测顺序）：

| 提供商 | 环境变量 | 默认模型 |
|--------|---------|---------|
| Anthropic | `ANTHROPIC_API_KEY` | `claude-sonnet-4-20250514` |
| OpenAI | `OPENAI_API_KEY` | `gpt-4o` |
| Google | `GOOGLE_API_KEY` | `gemini-2.0-flash` |
| Groq | `GROQ_API_KEY` | `llama-3.3-70b-versatile` |
| OpenRouter | `OPENROUTER_API_KEY` | `anthropic/claude-sonnet-4` |
| DeepSeek | `DEEPSEEK_API_KEY` | `deepseek-chat` |
| 智谱 AI | `ZHIPU_API_KEY` | `glm-4-flash` |
| 通义千问 | `DASHSCOPE_API_KEY` | `qwen-plus` |
| 月之暗面 | `MOONSHOT_API_KEY` | `moonshot-v1-8k` |
| 百川 | `BAICHUAN_API_KEY` | `Baichuan4-Air` |
| MiniMax | `MINIMAX_API_KEY` | `MiniMax-Text-01` |
| 零一万物 | `YI_API_KEY` | `yi-lightning` |
| 硅基流动 | `SILICONFLOW_API_KEY` | `deepseek-ai/DeepSeek-V3` |
| 阶跃星辰 | `STEPFUN_API_KEY` | `step-2-16k` |
| 讯飞星火 | `SPARK_API_KEY` | `generalv3.5` |

> 通过 OpenRouter 可使用所有提供商的模型，只需一个 `OPENROUTER_API_KEY`，在 `CLAUDE_MODEL` 中指定模型 ID。

### 配置文件

| 层级 | 路径 | 说明 |
|------|------|------|
| `.env` | 项目根目录 | 启动时自动加载 |
| 全局 | `~/.cc-rust/settings.json` | 用户级默认配置 |
| 项目 | `.cc-rust/settings.json` | 项目级配置 (从 CWD 向上查找) |

### `settings.json` 格式

```json
{
  "model": "claude-sonnet-4-20250514",
  "theme": "dark",
  "verbose": false,
  "permission_mode": "default",
  "allowed_tools": ["Bash", "Read", "Grep"],
  "system_prompt": "你是一位 Rust 专家...",
  "hooks": {},
  "api_key": "sk-ant-..."
}
```

### 环境变量

| 变量名 | 说明 |
|--------|------|
| `ANTHROPIC_API_KEY` | API 密钥 (推荐方式) |
| `ANTHROPIC_AUTH_TOKEN` | 外部认证 Token |
| `CLAUDE_MODEL` | 覆盖默认模型 |
| `CLAUDE_VERBOSE` | 详细模式 (`true`/`1`) |
| `CLAUDE_PERMISSION_MODE` | 权限模式覆盖 |

---

## 会话管理

### 存储位置

```
~/.cc-rust/sessions/{session_id}.json
```

每个会话以 UUID 命名，存储为 JSON 文件。

### 对话记录 (Transcript)

除了会话 JSON，系统还以 NDJSON 格式追加写入对话记录:

```
~/.cc-rust/sessions/{session_id}.ndjson
```

### 会话操作

```bash
# 恢复最近的会话
claude-code-rs --resume

# 恢复指定会话
claude-code-rs --continue <session-id>

# REPL 中查看/恢复会话
> /session
> /resume
```

---

## CLAUDE.md 项目指令

在项目中放置 `CLAUDE.md` 文件，可以为模型注入自定义指令。

### 查找规则

系统从当前工作目录开始，向上遍历所有祖先目录，收集找到的所有 `CLAUDE.md` 文件。根目录优先加载，CWD 最近的文件最后加载 (可覆盖通用指令)。

### 用途建议

```markdown
# CLAUDE.md

## 项目概述
这是一个 Rust CLI 工具...

## 编码规范
- 使用 snake_case
- 错误处理统一使用 anyhow::Result

## 构建命令
cargo build --release
cargo test
```

---

## 键盘快捷键

### 全局

| 快捷键 | 场景 | 说明 |
|--------|------|------|
| `Ctrl+C` | 流式输出中 | 中断当前回复 |
| `Ctrl+C` | 空闲时 | 退出程序 |
| `Ctrl+D` | 任何时候 | 退出程序 (EOF) |
| `Page Up` / `Shift+Up` | 任何时候 | 向上滚动 |
| `Page Down` / `Shift+Down` | 任何时候 | 向下滚动 |

### 输入编辑

| 快捷键 | 说明 |
|--------|------|
| `Enter` | 提交输入 |
| `Up` / `Down` | 浏览输入历史 |
| `Left` / `Right` | 移动光标 |
| `Home` / `End` | 光标移到行首/行尾 |
| `Ctrl+A` / `Ctrl+E` | 行首/行尾 |
| `Ctrl+U` | 清除整行 |
| `Ctrl+W` | 删除光标前一个词 |
| `Ctrl+K` | 删除光标到行尾 |

### 权限对话框

| 快捷键 | 说明 |
|--------|------|
| `Y` / `N` | 允许 / 拒绝 |
| `A` | 始终允许 |
| `Enter` | 确认当前选项 |
| `Esc` | 拒绝 |

---

## 故障排查

### 编译问题

```bash
# 清理并重新编译
cargo clean && cargo build

# 查看详细编译输出
cargo build -vv
```

### 运行时日志

```bash
# 启用详细日志
claude-code-rs -v

# 环境变量控制日志级别
RUST_LOG=debug claude-code-rs
```

### 数据文件

```bash
# 会话文件
ls ~/.cc-rust/sessions/

# 配置文件
cat ~/.cc-rust/settings.json

# 技能文件
ls ~/.cc-rust/skills/
```

### 系统提示词调试

```bash
claude-code-rs --dump-system-prompt
```

### 常见问题

**Q: Windows 上 git2 编译失败**
A: 确保安装了 Visual Studio C++ Build Tools 和 cmake。也可以尝试: `set LIBGIT2_NO_VENDOR=1`

**Q: 权限模式没有生效**
A: 检查优先级: CLI 参数 > 环境变量 > 项目配置 > 全局配置。使用 `--verbose` 查看配置来源。

**Q: 编译时间很长**
A: 首次编译需下载所有依赖。后续增量编译会快很多。开发时使用 `cargo build` (不加 `--release`)。
