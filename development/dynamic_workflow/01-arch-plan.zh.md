# DynamicWorkflow 实现计划

> 参考实现：OpenHands SDK (`/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/software-agent-sdk`)
> - 例：`examples/01_standalone_sdk/52_dynamic_workflow.py`
> - 定义：`openhands-tools/openhands/tools/workflow/definition.py`
> - 实现：`openhands-tools/openhands/tools/workflow/impl.py`
> - TaskManager：`openhands-tools/openhands/tools/task/manager.py`

---

## 1. 动机与目标

### 现状

现有 `Workflow` 工具（`crates/allthecodes-tools/src/workflow/mod.rs`）是一个**静态多步骤工作流**：

- 父模型手动规划步骤列表（`steps: [{id, prompt, agent_type, depends_on}]`）
- 模型逐步骤调用 `Workflow action=advance` 推进执行
- 没有脚本编排能力，不能动态地 fan-out / reduce

### 目标

引入 **DynamicWorkflow**：父模型写一段编排脚本（Python/Rust DSL），该脚本在沙箱中执行，通过 wf context 对象调用子 agent，实现：

- `wf.run_agent(prompt, subagent_type)` — 跑一个子 agent
- `wf.map_agents(items, prompt, subagent_type, max_concurrency)` — fan-out
- `wf.reduce_agent(items, prompt, subagent_type)` — 归约汇总
- `wf.pipeline(items, *stages)` — 无 barrier 的流水线

与 OpenHands SDK 的 key differentiator：allthecodes 已有 `Agent` tool 和 `fork` 机制，DynamicWorkflow 不需要重新实现子代理创建逻辑，而是复用 `AgentTool` 的 `dispatch` 路径或 `run_fork`。

---

## 2. 设计决策

### 决策 1：脚本语言 — Python（通过 Rhai / 嵌入式 JS） vs Rust DSL vs 模型生成 + Rust exec

**选择：Rust `execute_workflow_script()` + AST 校验（类似 OpenHands）**

OpenHands 的做法（Python AST 校验 + `exec` 沙箱执行）是成熟的，但 allthecodes 是 Rust 项目，引入 Python 解释器或完整的 V8 引擎都太重。

**方案：用 Rust 原生实现 WorkflowContext + 模型生成的 JSON 序列化脚本**

模型不是输出 Python 源码，而是输出一个 JSON 结构，包含：

```json
{
  "stages": [
    {"type": "map", "items": ["...", "..."], "prompt": "...", "subagent_type": "general-purpose"},
    {"type": "reduce", "prompt": "Synthesize the results"}
  ]
}
```

或者更灵活：模型输出一段 **Rhai 脚本**（嵌入式 Rust 脚本语言），暴露 `wf` 对象。但这种方式调试困难。

**折中方案 — Phase 1：JSON DAG 定义（无脚本）**

Phase 1 支持模型一次性定义多步骤工作流，步骤间支持 fan-out：

```
DynamicWorkflow {
  steps: [
    { id: "a1", type: "agent", prompt: "...", subagent_type: "general-purpose" },
    { id: "b1", type: "agent", prompt: "...", subagent_type: "general-purpose" },
    { id: "c1", type: "reduce", prompt: "Synthesize {{a1}} and {{b1}}", depends_on: ["a1", "b1"] }
  ]
}
```

**Phase 2：模型生成的 Rust 闭包链（或 DSL 字符串）**

将 `WorkflowContext` 的方法暴露为 `Arc<dyn Fn>`，模型生成一个描述符字符串，在沙箱中解析并执行。

### 决策 2：子 agent 复用路径 — `AgentTool` 还是 `run_fork`

**选择：复用 `AgentTool` 的 `dispatch` 路径**

`AgentTool` 已经支持：
- `subagent_type` 选择内置 agent
- `run_in_background` 模式
- 结果收集和错误处理
- 权限系统

`run_fork` 太轻量（无持久化、无 IPC 树注册），而 DynamicWorkflow 的子 agent 应当像正常的 Agent 调用一样可观察（dashboard 事件、日志）。

### 决策 3：并发控制

**选择：`tokio::sync::Semaphore` + `JoinSet`（参考 OpenHands 的 `asyncio.Semaphore`）**

```
map_agents(items):
  semaphore = Semaphore::new(min(max_concurrency, context.max_concurrency))
  tasks = items.map(|item| {
    semaphore.acquire() -> agent_call()
  })
  futures::future::join_all(tasks)
```

### 决策 4：安全性

**选择：静态 JSON Schema 校验（Phase 1）+ AST 校验（Phase 2，若有脚本 DSL）**

Phase 1 中模型输出的 JSON DAG：
- 使用 JSON Schema `validate()` 校验结构
- 不允许 `import`、`eval`、`exec`（因为根本没有脚本）
- 通过 `subagent_type` 白名单限制可用的 agent 类型

---

## 3. 实现方案

### 整体结构

新建 `crates/allthecodes-tools/src/workflow_dynamic/`：

```
crates/allthecodes-tools/src/workflow_dynamic/
├── mod.rs           # tools() 导出 + DynamicWorkflowStages + WorkflowContext
├── definition.rs    # DynamicWorkflowAction + DynamicWorkflowObservation + DynamicWorkflowTool
├── executor.rs      # execute_workflow() + validate_workflow_plan()
└── context.rs       # WorkflowContext — run_agent / map_agents / reduce_agent / pipeline
```

### Phase 1 — JSON DAG（静态编排）

```
DynamicWorkflowInput {
    name: String,
    description: Option<String>,
    max_concurrency: Option<u8>,      // default 8
    stages: Vec<WorkflowStage>,
}

WorkflowStage {
    id: String,
    // agent, map, reduce, pipeline
    kind: WorkflowStageKind,
    prompt: String,                    // map: may contain {item}
    subagent_type: Option<String>,     // default "general-purpose"
    depends_on: Vec<String>,           // stage IDs that must complete first
    // for map only:
    items: Option<Vec<String>>,
    max_concurrency: Option<u8>,
    // for reduce only:
    reduce_prompt: Option<String>,
}
```

**执行流程：**

1. 解析 JSON → 校验 stage DAG 无环（topological sort）
2. 识别 `depends_on` 为空的 stage → ready
3. 逐层执行：每层并行的 ready stages 用 `JoinSet` + semaphore 控制并发
4. 完成一个 stage 后，检查其下游 stage 是否所有依赖都完成 → 变为 ready
5. 返回最终结果

```
Execution:

ready_stages: [a1, b1] (no deps)
  └─ spawn a1 (agent "review module A")
  └─ spawn b1 (agent "review module B")
  ├─ both complete → downstream c1 becomes ready
  └─ c1 (reduce: "Synthesize results from a1 and b1")
```

### Phase 2 — 脚本编排（动态 fan-out）

引入 JSON 中的 `script` 字段，但不引入 Python 解释器。使用模型生成的**序列化中间表示**：

```json
{
  "name": "coverage-audit",
  "operations": [
    {
      "kind": "map",
      "items": ["./crate-a", "./crate-b", "./crate-c"],
      "prompt": "Audit test coverage for {item}",
      "subagent_type": "general-purpose"
    },
    {
      "kind": "reduce",
      "prompt": "Summarize the following coverage reports"
    }
  ]
}
```

Phase 2 的关键扩展：
- `map` 的 `items` 可以由前一个 stage 的输出动态决定
- 支持 `pipeline` 语义（无 barrier 流水线）
- 在 JSON 中用 `{stage.<id>}` 引用之前 stage 的结果

**Phase 2 执行模型：**

```
WorkflowContext.exec_plan(Plan):
  for op in Plan.operations:
    match op.kind:
      "map" => self.map_agents(op.items, op.prompt, op.subagent_type, op.max_concurrency)
      "reduce" => self.reduce_agent(op.items, op.prompt, op.subagent_type)
      "run_agent" => self.run_agent(op.prompt, op.subagent_type)
```

---

## 4. 关键类型定义

### DynamicWorkflowAction

```rust
pub struct DynamicWorkflowAction {
    pub name: String,
    pub plan: serde_json::Value,       // JSON DAG or operation list
    pub subagent_type: Option<String>,  // default subagent for all stages
    pub max_concurrency: u8,            // default 8
}
```

### DynamicWorkflowObservation

```rust
pub struct DynamicWorkflowObservation {
    pub name: String,
    pub status: Literal<"completed" | "error">,
    pub results: HashMap<String, String>,  // stage_id -> result text
    pub final_result: Option<String>,
}
```

### WorkflowContext

```rust
pub struct WorkflowContext {
    parent_conversation: ...,       // for agent spawning
    max_concurrency: u8,
    semaphore: Semaphore,
    results: HashMap<String, String>,  // completed stage -> result
}
```

---

## 5. 与现有系统集成

### Feature Flag

使用已有 `Feature::WorkflowScripts` gate（对应 `ALLTHECODES_WORKFLOW_SCRIPTS` env var），默认启用。

### 工具注册

在 `allthecodes-tools/src/registry.rs` 中：
- `get_all_tools()` 路径：新增 `crate::workflow_dynamic::tools()`
- 将 `DynamicWorkflowTool` 加入 `WORKFLOW_TOOL_NAMES` 数组（或新增 `DYNAMIC_WORKFLOW_TOOL_NAMES`）

### 系统提示词

在 `engine/src/system_prompt/static_sections.rs` 中：
- 新增 `DynamicWorkflow` 工具的用法描述
- 说明什么时候应该使用 `DynamicWorkflow` vs 现有的 `Workflow`（静态步骤）

### 权限

- `action=list | status` → `PermissionResult::Allow`
- `action=start` → `PermissionResult::Ask`（含 stage 数量、name）
- `action=cancel` → `PermissionResult::Ask`

### 子 agent 复用

WorkflowContext 内部调用 `agent_tool.call()` 或直接调用 `AgentRuntime::spawn_agent()`，复用已有的。
- 子 agent 获得与 `AgentTool` 相同的工具集和权限策略
- 结果通过 `agent_result` 回传

### 已知剩余缺口：文件型 workflow 动态 slash command

记录日期：2026-07-04。

当前已补齐的 file-script 兼容层包括：

- `Workflow`/`workflow` 工具在输入含 `workflow` 或 `run_id` 时进入文件型 workflow mode。
- 文件发现路径使用 `.allthecodes/workflows`，run 状态持久化到 `.allthecodes/workflow-runs`。
- 支持 `.md`、`.yaml`、`.yml` parser 和 `/workflows` 列表命令。

仍未落地的上游兼容能力是：把 `.allthecodes/workflows/release.md` 自动注册为 `/release` 这类动态 slash command。

原因不是 parser 或工具层缺失，而是当前 Rust command metadata/dispatcher 仍是全局快照：

- `DefaultCommandDispatcher::for_full_registry()` 只从 builtin + global `DYNAMIC_REGISTRY` 读取 metadata。
- workflow 文件发现依赖当前 `cwd`，但现有 command metadata provider 没有 `cwd` 参数。
- dynamic registry 的 `ExecutionStrategy::Inline` 没有 handler，不能直接承载“读取 cwd 下 workflow 文件并返回 `CommandResult::Query`”的执行语义。

后续实现建议拆成独立任务：

1. 新增 cwd-aware command metadata 路径，例如 `DefaultCommandDispatcher::for_cwd(cwd)` 或 command metadata provider 增加 `cwd` 参数。
2. 扫描 `.allthecodes/workflows/*.md|*.yaml|*.yml`，生成 workflow command metadata；遇到 builtin command 冲突时 builtin 优先。
3. 执行 `/release args` 时读取对应 workflow 文件，返回模型查询消息，文本等价于上游：`Execute this workflow:\n\n{content}\n\nArguments: {args}`。
4. 保持默认持久化路径隔离，不读取或写入 `.claude/workflows` / `.claude/workflow-runs`。

---

## 6. 实现步骤

### Phase 1: JSON DAG 编排（基线 MVP）

| # | 步骤 | 文件 | 描述 |
|---|------|------|------|
| 1 | 新建 crate 模块 | `workflow_dynamic/mod.rs` | tools() 导出 DynamicWorkflowTool |
| 2 | 定义 Action/Observation | `workflow_dynamic/definition.rs` | DynamicWorkflowAction + DynamicWorkflowObservation |
| 3 | 实现 DAG 校验 | `workflow_dynamic/executor.rs` | 拓扑排序、stage 引用校验 |
| 4 | 实现 WorkflowContext | `workflow_dynamic/context.rs` | run_agent, map_agents, reduce_agent |
| 5 | 实现顺序/并行执行 | `executor.rs` + `context.rs` | Phase 1 的 DAG 执行器 |
| 6 | 注册工具 | `registry.rs` | 加入 WORKFLOW_TOOL_NAMES |
| 7 | 系统提示词 | `static_sections.rs` | DynamicWorkflow 用法说明 |
| 8 | E2E 测试 | `tests/` | 测试 basic DAG, map+reduce, 错误场景 |
| 9 | 文档 | `docs/` | 用户文档 + 架构决策记录 |

### Phase 2: 动态脚本编排

| # | 步骤 | 描述 |
|---|------|------|
| 1 | pipeline 支持 | WorkflowContext.pipeline() — 无 barrier 流水线 |
| 2 | 动态 map items | map 的 items 可以引用前序 stage 结果 |
| 3 | 错误处理 | ExceptionGroup 模式（参考 OpenHands impl.py `map_agents` 的异常聚合） |
| 4 | 重试/超时 | 单个 agent 超时 + 重试配置 |
| 5 | 持久化 | worklow run 状态写入 `~/.allthecodes/workflow-runs/{id}.json` |
| 6 | /commands UI | `commands/` 层支持 DynamicWorkflow 状态查询 |

---

## 7. 关键边界与约束

### 与现有 `Workflow` 工具的关系

| | Workflow (现有) | DynamicWorkflow (新建) |
|---|---|---|
| 编排方式 | 模型手动调用 advance | 一次性定义 DAG / 脚本 |
| 步骤数 | 10-20 步 | 任意 |
| 并行执行 | 模型手动控制 | 自动 semaphore |
| 适用场景 | 简单的线性步骤 | 复杂的 fan-out/reduce |
| 子 agent 类型 | 同一种 | 可每个 stage 指定 |

两者互补：简单线性工作沿用 `Workflow`，复杂 DAG 用 `DynamicWorkflow`。

### 限制

- **Phase 1** 不支持条件分支、循环
- **Phase 2** 的脚本安全性依赖 JSON Schema 校验（没有 AST 级别的安全检查 = 没有脚本注入风险，因为根本没有脚本执行引擎）
- 子 agent 数量上限：受 `Semaphore`（最大 64）和总 agent 数量隐式约束
- 子 agent 结果大小上限：`_MAX_REDUCE_INPUT_CHARS`（同 OpenHands 的 12KB），超长自动截断

---

## 8. 参考：OpenHands SDK 关键代码映射

| OpenHands | allthecodes 对应 |
|-----------|-----------------|
| `WorkflowAction(script, max_concurrency)` | `DynamicWorkflowAction(plan, max_concurrency)` |
| `WorkflowContext.run_agent()` | `WorkflowContext.run_agent()` — 复用 `AgentTool::call()` |
| `WorkflowContext.map_agents()` | `WorkflowContext.map_agents()` — JoinSet + semaphore |
| `WorkflowContext.reduce_agent()` | `WorkflowContext.reduce_agent()` — 调用 run_agent + 附加上下文 |
| `WorkflowContext.pipeline()` | Phase 2：无 barrier 流水线 |
| `TaskManager.start_task()` | allthecodes 的 `supervisor::run_subagent()` 或 `agent/dispatch.rs` |
| `validate_workflow_script()` (AST) | Phase 1：JSON Schema validate |
| `_safe_globals()` | Phase 2 不入沙箱，exec 路径不走 |
| `ExceptionGroup` | `tokio::task::JoinError` 聚合 |

---

## 9. 风险和缓解

| 风险 | 缓解 |
|------|------|
| 模型生成的 DAG 有环 | 拓扑排序检测 → 返回校验错误 |
| stage 结果太大 | 截断到 12KB + `... [truncated]` 标记 |
| 子 agent 死循环 | 继承父 conversation 的 `max_iteration_per_run` |
| 权限绕过 | DynamicWorkflow 工具本身走权限系统；子 agent 复用已有 AgentTool 权限 |
| 现有 Workflow 工具不兼容 | 保持独立工具名，两个工具共存 |
