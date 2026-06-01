# COORDINATOR_MODE — 多 Agent 编排（协调者模式）

> 功能标志：`FEATURE_COORDINATOR_MODE=1`
> 实现状态：编排者完整，worker 为通用 AgentTool
> 源码文件数：3

## 一、功能概述

协调者模式（Coordinator Mode）将 CLI Agent 转变为"编排者"角色。编排者不直接操作文件，而是通过 AgentTool 派发任务给多个 worker 并行执行。适用于大型任务拆分、并行研究、实现+验证分离等场景。

### 核心约束

- 编排者只能使用：`Agent`（派发 worker）、`SendMessage`（继续 worker）、`TaskStop`（停止 worker）
- Worker 可以使用所有标准工具（Bash、Read、Edit 等）+ MCP 工具 + Skill 工具
- 编排者的每条消息都是给用户看的；worker 结果以 `<task-notification>` 形式到达

## 二、用户交互

### 启用方式

```bash
# 设置环境变量启用协调者模式
FEATURE_COORDINATOR_MODE=1 allthecodes
```

同时需要 feature flag 和保护环境变量。环境变量可在会话恢复时自动匹配。

### 典型工作流

```
用户: "修复 auth 模块的问题"

编排者:
  1. 并行派发两个 worker:
     - Agent({ description: "调查 bug", prompt: "..." })
     - Agent({ description: "研究测试", prompt: "..." })

  2. 收到 <task-notification>:
     - Worker A: "在 validate.rs:42 发现..."
     - Worker B: "测试覆盖情况..."

  3. 综合发现，继续 Worker A:
     - SendMessage({ to: "agent-a1b", message: "修复..." })

  4. 收到修复结果，派发验证:
     - Agent({ description: "验证修复", prompt: "..." })
```

## 三、实现架构

### 3.1 模式检测

协调者模式检测集成在系统提示模块中：`crates/allthecodes-engine/src/system_prompt/static_sections.rs`。当 feature flag 启用时，系统提示中注入协调者身份说明，限制编排者可用的工具集。

### 3.2 Agent 定义

Agent 类型通过 `AgentDefinitionEntry` 定义，支持以下内置类型：

| Agent 类型 | 描述 | 工具集 |
|-----------|------|--------|
| `general-purpose` | 通用 Agent | 全部工具 |
| `Explore` | 只读探索 | Glob, Grep, Read |
| `Plan` | 计划模式 | 受限工具集 |

Agent 定义支持：
- **工具白名单/黑名单**：`tools` 和 `disallowed_tools` 列表
- **权限模式**：`Default` / `AcceptEdits` / `BypassPermissions` / `Plan`
- **最大轮次**：`max_turns` 限制
- **模型覆盖**：`model` 指定使用特定模型
- **Worktree 隔离**：`isolation` 设置为 `"worktree"`
- **技能列表**：`skills` 关联的技能

### 3.3 Worker 工具集

Worker 的工具集由以下因素决定：

1. **父级权限模式**：`Auto` / `Bypass` / `AcceptEdits` / `Plan` / `Default`
2. **AgentDefinitionEntry**：通过 `builtin_agent_entries()` 查找匹配的 Agent 类型
3. **工具过滤**：`filter_tools_for_optional_definition()` 根据 allow/disallow 列表过滤

### 3.4 Agent 上下文与链式追踪

每个子 Agent 获得一个 `AgentContext`：

```rust
pub struct AgentContext {
    pub agent_id: String,
    pub query_tracking: QueryChainTracking,  // chain_id + depth
    pub langfuse_session_id: String,
    pub agent_type: Option<String>,
    pub team_context: Option<TeamContext>,
    pub tool_permission_context: Option<ToolPermissionContext>,
}
```

`QueryChainTracking` 提供链式追踪：
- `chain_id` — 同一链条中所有 Agent 共享
- `depth` — 嵌套深度，防止无限递归（上限 5）

### 3.5 数据流

```
用户消息
      │
      ▼
编排者 REPL（受限工具集，仅 Agent/SendMessage/TaskStop）
      │
      ├──→ Agent({ subagent_type: "general-purpose", prompt: "..." })
      │         │
      │         ▼
      │    Worker Agent（完整工具集）
      │    ├── 执行任务（Bash/Read/Edit/...）
      │    ├── 流式事件通过 IPC 通道转发
      │    └── 返回 <task-notification> 结果
      │
      ├──→ SendMessage({ to: "agent-id", message: "..." })
      │         │
      │         ▼
      │    继续已存在的 Worker
      │
      └──→ TaskStop({ task_id: "agent-id" })
                │
                ▼
           停止运行中的 Worker
```

### 3.6 工具解析逻辑

`tool_matches_spec()` 支持灵活的规格匹配：

```rust
fn tool_matches_spec(tool_name: &str, spec: &str) -> bool {
    // 通配符 "*" 匹配所有
    // 后缀通配符 "Read*" 匹配前缀
    // "Bash(git status)" 匹配 Bash 工具
}
```

工具去重确保同一工具不重复注册。

### 3.7 Agent 深度控制

```rust
const MAX_AGENT_DEPTH: usize = 5;
```

在 `build_child_config()` 中，`depth` 从父级的 `current_depth + 1` 计算。当深度超过 `MAX_AGENT_DEPTH` 时，系统拒绝创建新的子 Agent。

## 四、系统提示集成

协调者模式系统提示由 `static_sections` 模块管理，包含：

1. **角色定义**：编排者的职责与约束
2. **工具说明**：Agent/SendMessage/TaskStop 的使用指南
3. **Workers 信息**：Worker 的能力范围
4. **任务流程**：Research → Synthesis → Implementation → Verification
5. **Worker Prompt 编写指南**：自包含 prompt 的最佳实践
6. **示例会话**：完整的多步骤编排示例

## 五、关键设计决策

1. **编排者受限**：只能用 Agent/SendMessage/TaskStop，确保编排者专注于派发而非执行
2. **Worker 不可见编排者对话**：每个 worker 的 prompt 必须自包含所有必要上下文
3. **并行优先**：系统提示鼓励并行派发独立任务
4. **综合而非转发**：编排者必须理解 worker 发现，再写出具体实现指令
5. **深度限制**：`MAX_AGENT_DEPTH = 5`，防止嵌套失控
6. **权限继承清晰**：子 Agent 权限从父级继承，Plan 模式允许子级 Default

## 六、使用方式

```bash
# 基本启用
FEATURE_COORDINATOR_MODE=1 allthecodes
```

## 七、文件索引

| 文件 | 职责 |
|------|------|
| `crates/allthecodes-engine/src/agent/mod.rs` | AgentTool 定义 + 工具过滤 + 深度控制 |
| `crates/allthecodes-engine/src/agent/dispatch.rs` | Agent 分发（SubagentStart/Stop hooks） |
| `crates/allthecodes-engine/src/agent/supervisor.rs` | 后台 Agent 生命周期管理 |
| `crates/allthecodes-engine/src/system_prompt/static_sections.rs` | 协调者模式系统提示 |
