---
title: "后台 Agent 选择器"
description: "Agent 和 Task 工具的子代理选择机制，支持内置代理、背景运行和 Worktree 隔离。"
keywords: ["agent", "代理", "子代理", "background", "worktree", "Task"]
---

## 概述

后台 Agent 选择器是 allthecodes 的子代理调度系统。当 Agent/Task 工具被调用时，选择器负责选择合适的子代理类型、配置运行环境（前台/后台、Worktree 隔离）、管理生命周期和事件上报。对应 TypeScript 原版的 `tools/AgentTool/`。

## 架构

### Agent 工具

Agent 工具（`AgentTool`）接收以下输入：

| 参数 | 类型 | 说明 |
|------|------|------|
| `prompt` | string | 子代理执行的任务 |
| `description` | string? | 简短任务描述（3-5 词） |
| `subagent_type` | string? | 特化代理类型 |
| `model` | string? | 模型覆盖（SOTA/MOTA/FOTA 或完整模型 ID） |
| `run_in_background` | boolean | 是否在后台运行 |
| `name` | string? | Agent Teams 队友名 |
| `team_name` | string? | Agent Teams 团队名 |
| `mode` | string? | 权限模式（default/auto/bypass/plan/acceptEdits/dontAsk） |
| `isolation` | string? | 隔离模式（"worktree" 等） |

### 内置代理注册表

`builtin_agents.rs` 定义 6 个内置代理：

| 代理名 | 描述 | 工具集 | 颜色 |
|--------|------|--------|------|
| `general-purpose` | 通用代理：研究复杂问题、搜索代码、执行多步骤任务 | 全部工具 | 无 |
| `Explore` | 代码库探索代理：快速搜索文件、查找代码、回答问题 | Glob, Grep, Read | 青色 |
| `Plan` | 软件架构师代理：设计实现计划、分析架构折中 | Glob, Grep, Read | 紫色 |
| `code-reviewer` | 代码审查代理：审计变更、检查正确性和风格 | Glob, Grep, Read | 橙色 |
| `worker` | 协调模式 Worker：执行团队领导分配的特定任务 | 写工具 + SendMessage | 绿色 |
| `statusline-setup` | 状态行配置代理：帮助用户自定义状态行 | Read, Edit, Write, Bash | 黄色 |

### 调度机制

#### 前台执行

`run_agent_normal()` — 标准执行路径：
1. 验证工作目录可用性
2. 构建子配置（`build_child_config`）
3. 注册代理节点到代理树（`agent_runtime::register_agent_node`）
4. 发送 `AgentEvent::Spawned` 事件
5. 创建子 `QueryEngine` 实例
6. 执行 `submit_message`
7. 收集流式结果并返回

#### 后台执行

`spawn_background_agent()` — 后台代理调度：

1. 工具注册回调：在启动/停止时触发 Hooks
2. 准备工作空间（必要时创建 Worktree）
3. 创建 `TaskEntry`，标记为 `kind: "local_agent"`
4. 注册取消令牌（`CancellationToken`）
5. 注册到 `BackgroundSupervisor`：
   - 任务跟踪
   - 生命周期管理
   - 取消传播
6. 发送代理树快照事件

#### Worktree 隔离

`run_in_worktree()` — Git Worktree 隔离执行：

1. 查找 Git 根目录（`find_git_root`）
2. 创建 Worktree（`run_worktree_create_hook`）
3. 设置子 QueryEngine 的 cwd 为 Worktree 路径
4. 执行代理任务
5. 清理：无变更时自动移除 Worktree
6. 回退：非 Git 仓库或创建失败时回退到正常执行

### 代理树和事件

代理节点类型（`AgentNode`）：
- `agent_id` / `parent_agent_id` — 父子关系
- `description` — 任务描述
- `agent_type` — 代理类型
- `model` — 使用的模型
- `state` — 运行状态（running / completed / error）
- `depth` — 嵌套深度（限制 5 层防递归）
- `is_background` — 是否为后台代理
- `chain_id` — 追踪链 ID
- `spawned_at` / `completed_at` — 时间戳

事件流：
1. `AgentEvent::Spawned` — 代理创建
2. `AgentEvent::TreeSnapshot` — 树快照（用于 Dashboard）
3. 流式执行结果通过 `SdkMessage` 返回

### Dashboard 事件日志

`dashboard.rs` 中的 `emit_subagent_event()` 在后台代理启动/运行/完成时记录结构化事件到 `subagent-events.ndjson`：

```
{
  "ts": "2026-04-14T12:00:00Z",
  "kind": "spawn",
  "agent_id": "agent-xxx",
  "parent_agent_id": "parent-yyy",
  "description": "搜索 API 端点",
  "model": "claude-sonnet-4",
  "depth": 1,
  "background": true
}
```

Feature 门控：`FEATURE_SUBAGENT_DASHBOARD=1`

## 使用方式

```bash
# 前台使用 Agent 工具
# 在对话中输入：使用 Agent 工具研究这个代码库的架构

# 后台运行代理
# 在对话中输入：在后台帮我搜索所有 TODO 注释

# 使用特化代理
# 使用 Explore 代理：/agent Explore 帮我找到所有 API 路由定义

# 启用 Agent Teams
ALLTHECODES_EXPERIMENTAL_AGENT_TEAMS=1 cargo run
```

## 相关文件

| 文件 | 职责 |
|------|------|
| `crates/allthecodes-engine/src/agent/mod.rs` | Agent 工具定义和输入 Schema |
| `crates/allthecodes-engine/src/agent/builtin_agents.rs` | 6 个内置代理注册表 |
| `crates/allthecodes-engine/src/agent/dispatch.rs` | 前台代理调度（run_agent_normal） |
| `crates/allthecodes-engine/src/agent/supervisor.rs` | 后台代理管理（spawn_background_agent） |
| `crates/allthecodes-engine/src/agent/worktree.rs` | Worktree 隔离执行 |
| `crates/allthecodes-engine/src/agent/tool_impl.rs` | Tool trait 实现和 JSON Schema |
| `crates/allthecodes-engine/src/agent/fork.rs` | 代理分支逻辑 |
| `crates/allthecodes/src/dashboard.rs` | Subagent Dashboard 事件日志 |
| `crates/allthecodes-engine/src/agent_runtime.rs` | 代理运行时注册 |
