---
title: "状态行"
description: "可脚本化的终端状态行系统，支持自定义命令、JSON 输入和实时刷新。"
keywords: ["status line", "状态行", "脚本", "终端", "自定义"]
---

## 概述

状态行（Status Line）系统为 allthecodes 终端提供可脚本化的自定义底部状态栏。用户可以通过配置自定义命令，让状态行显示 Git 分支、模型信息、token 消耗、工作区状态等实时信息。默认提供内置状态栏，开启用户命令后使用自定义脚本输出。

## 架构

状态行系统分为两层：

### 引擎层（allthecodes-engine）

`crates/allthecodes-engine/src/status_line/` 包含核心逻辑：

#### Payload（`payload.rs`）

`StatusLineSnapshot` 是所有状态数据的聚合结构：

| 字段 | 类型 | 说明 |
|------|------|------|
| `session_id` | `Option<String>` | 当前会话 ID |
| `model_id` | `&str` | 当前模型 ID |
| `backend` | `Option<&str>` | 模型后端 |
| `cwd` | `&Path` | 当前工作目录 |
| `input_tokens` | `u64` | 输入 token 数 |
| `output_tokens` | `u64` | 输出 token 数 |
| `cache_read_tokens` | `u64` | 缓存读取 token 数 |
| `cache_creation_tokens` | `u64` | 缓存创建 token 数 |
| `total_cost_usd` | `f64` | 总费用（美元） |
| `api_calls` | `u64` | API 调用次数 |
| `session_duration_secs` | `Option<u64>` | 会话持续时长 |
| `resolved_output_style_name` | `Option<String>` | 输出样式名 |
| `editor_mode` | `Option<&str>` | 编辑器模式（vim 等） |
| `worktree` | `Option<WorktreeStatus>` | Worktree 状态 |
| `streaming` | `bool` | 是否正在流式输出 |
| `message_count` | `usize` | 消息数量 |

`build_payload_from_snapshot()` 函数将这些字段组装为 `StatusLinePayload` JSON，通过管道传递给用户配置的命令。

#### Runner（`runner.rs`）

`StatusLineRunner` 负责执行用户配置的状态行命令：

- **子进程管理**：将 Payload JSON 写入 stdin，捕获 stdout
- **节流控制**：`refreshIntervalMs` 设置刷新间隔，避免高频刷新
- **取消机制**：新刷新开始时自动取消正在运行的子进程
- **超时保护**：命令超时时回退到默认状态栏
- **失败处理**：捕获 spawn 错误、非零退出码、超时，显示错误信息

输出结构（`StatusLineOutput`）：
- `stdout` — 捕获的命令输出
- `error` — 错误信息（非空时 TUI 回退到默认状态栏）
- `updated_at` — 更新时间戳

### 根 crate 层（allthecodes）

`crates/allthecodes/src/ui/status/status_line_resolver.rs` 提供引擎层无法直接访问的依赖注入：

- `resolve_output_style_name()` — 解析输出样式名称（需要 engine::output_style）
- `current_worktree_status()` — 获取当前 worktree 状态（需要 allthecodes_worktree）

## 配置

### 基础配置

```json
{
  "statusLine": {
    "command": "echo '{{model.id}} | {{context.tokens}} tokens'",
    "refreshIntervalMs": 5000
  }
}
```

但实际运行时，状态行命令接收的是 JSON payload 通过 stdin 传入，而不是简单模板替换。推荐使用 shell 脚本处理：

```json
{
  "statusLine": {
    "command": "jq -r '\"\(.model.id) | $\(.cost.total_usd | round)\"'",
    "refreshIntervalMs": 10000
  }
}
```

### 状态行代理

内置的 `statusline-setup` 代理可以自动帮助用户配置状态行：

```
名称: statusline-setup
描述: 配置 allthecodes 状态行设置
工具: Read, Edit, Write, Bash
```

## 通信协议

TUI 层每轮渲染时，通过 `StatusLineRunner` 获取最新状态。引擎在工具调用或事件更新后触发状态刷新。整个通信链路：

```
TUI 渲染 tick
      │
      ▼
StatusLineRunner::tick()
      │
      ├── 检查节流时间窗口
      ├── 检查 Payload 指纹（跳过无变化的刷新）
      │
      ▼
写入 stdin JSON → 子进程
      │
      ▼
捕获 stdout → StatusLineOutput
      │
      ▼
TUI 渲染状态行
  ├── output.is_usable() → 显示自定义状态行
  └── !is_usable()      → 显示默认状态行
```

## 默认状态行

当未配置自定义状态行或命令失败/超时时，TUI 回退到内置渲染。

## 使用方式

```bash
# 使用内置状态行代理配置
# 对话中输入：帮我配置状态行

# 手动配置
# 编辑 ~/.allthecodes/settings.json 或 .allthecodes/settings.json
# 设置 statusLine.command 为你的命令

# 查看状态行状态
# /statusline status

# 测试状态行命令
echo '{"model":{"id":"claude-sonnet-4"},"cost":{"total_usd":0.05}}' | your-command
```

## 相关文件

| 文件 | 职责 |
|------|------|
| `crates/allthecodes-engine/src/status_line/payload.rs` | Payload JSON 结构和构建函数 |
| `crates/allthecodes-engine/src/status_line/runner.rs` | 状态行命令执行器（子进程、节流、取消） |
| `crates/allthecodes-engine/src/status_line/mod.rs` | 模块入口和类型重导出 |
| `crates/allthecodes/src/ui/status/status_line_resolver.rs` | 根 crate 依赖注入（输出样式、worktree） |
| `crates/allthecodes/src/ui/app/status.rs` | TUI 状态栏渲染 |
| `crates/allthecodes/src/ui/tui.rs` | TUI 主循环 |
| `crates/allthecodes/src/ui/app.rs` | 应用渲染入口 |
| `crates/allthecodes-engine/src/agent/builtin_agents.rs` | statusline-setup 代理定义 |
