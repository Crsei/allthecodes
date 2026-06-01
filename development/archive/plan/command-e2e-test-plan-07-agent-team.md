# 命令 E2E 测试计划 07：代理与团队命令

> 目标文件：`crates/claude-code-rs/tests/pty_tui_e2e/commands_agent_team.rs`
> 离线和在线测试混合。

## 涵盖的命令

| 命令 | 别名 | 返回类型 | 模式 |
|---|---|---|---|
| `/agents` | -- | `Output` | 离线 |
| `/agents list` | -- | `Output` | 离线 |
| `/agents show <name>` | -- | `Output` | 离线 |
| `/agents sources` | -- | `Output` | 离线 |
| `/team` | `/teams` | `Output` | 离线 |
| `/team status` | -- | `Output` | 离线 |
| `/team list` | -- | `Output` | 离线 |
| `/team create <name>` | -- | `Output` | 离线 |
| `/team-onboarding` | `/teamonboarding` | `Output` | 离线 |
| `/tasks` | -- | `Output` | 离线 |
| `/tasks show <id>` | -- | `Output` | 离线 |
| `/tasks stop/delete <id>` | -- | `Output` | 离线 |
| `/btw` | -- | `Output` | 在线 |
| `/simplify` | -- | `Output` | 在线 |
| `/coordinator` | `/coord` | `Output` | 离线 |
| `/plan` | -- | `Output` | 离线 |

## 测试用例

### T01：`/agents` 列出代理
```rust
fn agents_list() {
    // 步骤：
    // 1. SkipTrustGate
    // 2. Command("agents")
    // 3. Wait(2s)
    // 4. AssertScreenContains("agent") 或 AssertScreenContains("built-in")
    // 5. Snapshot("agents_list")
    // 断言：按来源分组显示代理列表
}
```

### T02：`/agents list`
```rust
fn agents_list_subcommand() {
    // 步骤：
    // 1. Command("agents list")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：列出代理
}
```

### T03：`/agents show <name>`
```rust
fn agents_show() {
    // 步骤：
    // 1. Command("agents show default")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 4. Snapshot("agent_detail")
    // 断言：显示代理详情或 "not found"
}
```

### T04：`/agents sources`
```rust
fn agents_sources() {
    // 步骤：
    // 1. Command("agents sources")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：显示代理加载路径
}
```

### T05：`/team` 状态
```rust
fn team_status() {
    // 步骤：
    // 1. Command("team")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：显示团队状态
}
```

### T06：`/team list`
```rust
fn team_list() {
    // 步骤：
    // 1. Command("team list")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：列出团队或显示 "none"
}
```

### T07：`/team create` 和 `/team delete`
```rust
fn team_create_and_delete() {
    // 步骤：
    // 1. Command("team create test-e2e-team E2E test team")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 4. Command("team delete test-e2e-team")
    // 5. Wait(2s)
    // 6. AssertNoPanic
    // 断言：创建和删除均可用
}
```

### T08：`/team-onboarding`
```rust
fn team_onboarding() {
    // 步骤：
    // 1. Command("team-onboarding")
    // 2. Wait(3s)
    // 3. AssertNoPanic
    // 断言：生成入手指南或显示帮助
}
```

### T09：`/tasks` 列表
```rust
fn tasks_list() {
    // 步骤：
    // 1. Command("tasks")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：显示任务列表或 "no tasks"
}
```

### T10：`/tasks show` 无效 ID
```rust
fn tasks_show_invalid() {
    // 步骤：
    // 1. Command("tasks show nonexistent-task-123")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：显示 "not found" 或错误
}
```

### T11：`/tasks delete` 无效 ID
```rust
fn tasks_delete_invalid() {
    // 步骤：
    // 1. Command("tasks delete nonexistent-task-123")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：显示 "not found" 或错误
}
```

### T12：`/coordinator` 状态
```rust
fn coordinator_command() {
    // 步骤：
    // 1. Command("coordinator")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：显示协调器状态
}
```

### T13：`/coordinator` 别名 `/coord`
```rust
fn coordinator_alias() {
    // 步骤：
    // 1. Command("coord")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：别名可用
}
```

### T14：`/plan` 进入计划模式
```rust
fn plan_command() {
    // 步骤：
    // 1. Command("plan")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：进入计划模式或显示计划信息
}
```

### T15：`/btw` 附带问题（在线）
```rust
#[ignore = "requires real API key"]
fn btw_command() {
    // 步骤：
    // 1. SkipTrustGate
    // 2. Command("btw what is 2+2?")
    // 3. WaitForText("4", API_TIMEOUT)
    // 4. AssertNoPanic
    // 断言：分叉代理用于附带问题
}
```

### T16：`/simplify`（在线）
```rust
#[ignore = "requires real API key"]
fn simplify_command() {
    // 步骤：
    // 1. SkipTrustGate
    // 2. Command("simplify")
    // 3. Wait(5s)
    // 4. AssertNoPanic
    // 断言：开始简化审查
}
```

## 优先级：中
代理/团队命令较为复杂。先从列表/状态冒烟测试开始。

---

## 补充功能说明

以下为各命令的功能描述、典型输出、源码路径与边界情况，供编写测试断言时参考。

---

### `/agents` — 代理列表与详情

#### 功能描述

列出引擎可调度的所有 agent，按来源分组（built-in、bundled、user、project、plugin、MCP、team）。只读浏览器。支持 show/info 查看详情，sources 查看加载路径。

#### 输出示例

`/agents` 树形列表：
```
Agents

Built-in subagent types
  code-reviewer  [built-in] [built-in] Provides code review...
  explore        [built-in] [built-in] Explore file trees...
  general        [built-in] [built-in] General purpose sub-agent...

User skills
  prepare-pr  (path/to/skill)  [user] [fork] Prepares a PR description...
```

`/agents show <name>` 详情：
```
Agent: general
───────────────
  Source:      built-in
  Active:      true
  Execution:   built-in
  Description: General purpose sub-agent

  System prompt:
    You are a general-purpose assistant...
```

`/agents sources`：
```
Agent discovery sources
──────────────────────
  [built-in]   Engine-provided subagent types (always available)
  [bundled]    Skills compiled into cc-rust (`src/skills/bundled.rs`)
  [user]       ~/.cc-rust/skills/
  ...
```

#### 源码路径

- 入口分发：`crates/cc-commands/src/agents_cmd.rs` (`AgentsHandler::execute`)
- 子模块测试：`crates/cc-commands/src/agents_cmd/tests.rs`

#### 边界情况

- 不存在的 agent 名返回 "No agent named '{}'"
- 同名 agent 在不同 source 中存在时，第一个优先级最高，后续标为 "shadowed"
- `show` 缺少名称返回 "Usage: /agents show <name>"
- 未知子命令返回 "Unknown /agents subcommand" 并附带用法

---

### `/team` / `/teams` — 团队管理

#### 功能描述

管理 Agent Teams：创建/激活 team、spawn in-process teammate、发送 mailbox 消息、强制停止 teammate、离开或删除 team。无参数在 Rust TUI 中打开 `TeamSurface`。

#### 输出示例

各子命令的输出由底层 `execute_team_command` 生成，典型输出：
```
Team 'alpha' created and activated.
```
```
Team member 'builder' spawned.
```
```
Sent message to 'builder'.
```
```
Team 'alpha' deleted.
```

#### 源码路径

`crates/cc-commands/src/team_cmd.rs` (`TeamHandler::execute`)
实际路由到 `crate::runtime::execute_team_command()`

#### 边界情况

- `leave` 只清除当前 session 的 active team context，不删除磁盘数据
- `delete` 删除 team config/mailbox 并尝试停止非 lead teammate
- `delete` 不存在/无成员的团队时行为
- 没有可用的 team runtime 时返回 "Team command runtime is unavailable"

---

### `/team-onboarding` / `/teamonboarding` — 团队入手指南

#### 功能描述

生成一份 Markdown 格式的 teammate 入手指南，包含：欢迎语、项目概览（从 CLAUDE.md/README.md/git origin 提取）、常用命令列表、已注册技能、活跃团队、定时任务、风险区域。支持 `save` 命令写入文件。

#### 输出示例

打印到终端：
```
# project-name — Teammate Onboarding

## Welcome

Hi — here's the short tour.
...

## Common slash commands

- `/help` — list every slash command
- `/plan` — enter plan mode
...
```

`save` 输出：
```
Wrote teammate onboarding guide to /path/to/ONBOARDING_TEAM.md (2530 bytes).
```

#### 源码路径

`crates/cc-commands/src/team_onboarding.rs` (`TeamOnboardingHandler::execute`, `build_guide`)

#### 边界情况

- CLAUDE.md / README.md 不存在时，项目概览部分为空
- "Risk areas" 覆盖：首次运行状态、认证未完成、缺少 `.cc-rust/`、缺少 CLAUDE.md
- `save` 默认路径为 `cwd/ONBOARDING_TEAM.md`，支持相对/绝对路径
- 所有部分都是空值抑制的（empty-suppressed），无数据时不会留空标题
- 未知子命令返回 "Unknown /team-onboarding subcommand"

---

### `/tasks` — 后台任务管理

#### 功能描述

聚合 tool-driven tasks 和 in-process teammate tasks 的列表/详情/停止/删除。tool task 持久化在磁盘上，team task 是运行时态的。

#### 输出示例

`/tasks` 列表：
```
Background tasks

Tool tasks (2)
  task-abc  Implement feature X    [shell:pending]      2025-01-01 12:00:00 | output 0 bytes
  task-def  Refactor module Y      [agent:in_progress]  2025-01-01 12:30:00 | output 1234 bytes

Team tasks (1)
  task-ghi  builder (team-alpha)   [team:running]        prompt: Finish UI wiring
```

`/tasks show <id>` 详情：
```
Tool task task-abc
──────────────────
  Subject:     Implement feature X
  Description: (empty)
  Kind:        shell
  Status:      pending
  Created:     2025-01-01 12:00:00
  Updated:     2025-01-01 12:00:00
  Output:      0 bytes
  Retained log: (empty)
```

`/tasks stop <id>`：
```
Cancelled tool task 'Implement feature X' (now Cancelled).
```

`/tasks delete <id>`：
```
Deleted persisted tool task 'Implement feature X'.
```

#### 源码路径

`crates/cc-commands/src/tasks_cmd.rs` (`TasksHandler::execute`)

#### 边界情况

- `show` 缺少 id 返回 "Usage: /tasks show <id>"
- `show` 不存在的 id 返回 "No task with id '{}'"
- `stop` team task 提示 "use `/team kill <name>` to stop a teammate"
- `delete` team task 提示 "team tasks are runtime-only and cannot be deleted"
- 列表始终包含 "Tool tasks" 和 "Team tasks" 两个分组
- 空列表显示 "(none)" 子节点

---

### `/btw` — 侧边问题（在线）

#### 功能描述

fork 一个无工具、单轮的子引擎来快速回答一个辅助问题，不修改主会话历史记录。答案通过 `CommandResult::Output` 返回，不会污染对话 transcript。

命令格式：`/btw <question>`

#### 输出示例

成功：
```
/btw (forked agent, 1234 ms)

The `fold` pattern in Rust...
```

空参数：
```
Usage: /btw <question>

Ask a side question without interrupting the current task. The question runs as a tool-free, single-turn fork.
```

#### 源码路径

`crates/cc-commands/src/btw.rs` (`BtwHandler::execute`)

#### 边界情况

- 参数为空或仅空白字符时返回 "Usage: /btw <question>"
- 需要真实 API key（标记为 `#[ignore = "requires real API key"]`）
- fork 失败时返回 "/btw error: failed to run fork: ..."
- forked 代理的 system prompt 限制为无工具、1-3 句回答

---

### `/simplify` — 多代理简化审查（在线）

#### 功能描述

通过 fork 并行审查代理进行多代理代码简化审查。默认并行运行三个审查角度：reuse（复用）、quality（质量）、efficiency（效率），汇总后返回。支持 `--single` 单代理模式和文件/目录限定。

命令格式：
- `/simplify` — 审查近期变更的代码
- `/simplify <file-or-dir>` — 限定审查范围
- `/simplify --single` — 单代理模式

#### 输出示例

多代理模式：
```
/simplify (multi-agent review)

━━━ reuse ━━━
Found duplicated logic in src/main.rs...
(1234 ms, agent-0)

━━━ quality ━━━
Naming suggestions...
(1100 ms, agent-1)

━━━ efficiency ━━━
Avoidable allocations...
(980 ms, agent-2)
```

单代理模式：
```
/simplify (single-agent fork, 2345 ms)

Code review results...
```

#### 源码路径

`crates/cc-commands/src/simplify.rs` (`SimplifyHandler::execute`)

#### 边界情况

- 需要真实 API key（标记为 `#[ignore = "requires real API key"]`）
- 缺少 bundled `simplify` skill 时返回错误提示
- `--single`/`-1` 标志切换到单代理模式
- 多代理模式中某个 fork 失败不影响其他 fork 的结果
- fork 错误会显示 "(fork failed)" 标记和错误消息

---

### `/coordinator` / `/coord` — 协调器模式

#### 功能描述

启用/停用 coordinator 模式。开启时会自动绑定一个 Agent Team，将工具策略切换为仅限 `Agent` / `SendMessage`（无 `Bash` 等）。支持 status/start/stop 子命令。

#### 输出示例

`/coordinator status`：
```
Coordinator mode: ON
Active team: alpha
Lead: lead@alpha
Visible teammates: 2
Team task status: 1 running (0 idle), 2 total
Tool policy: Agent, SendMessage
```

`/coordinator start`：
```
Coordinator mode enabled and team 'alpha' is active. Spawn workers with Agent(name=..., prompt=...) or /team spawn.
```

`/coordinator stop`：
```
Coordinator mode disabled for this session. Active team state was left intact.
```

#### 源码路径

`crates/cc-commands/src/coordinator.rs` (`CoordinatorHandler::execute`)

#### 边界情况

- 关闭（stop）时不清除 team context
- `start` 会同时启用 `agent_teams` feature flag
- 如果 team create 失败，返回 "Failed to create coordinator team"
- `start` 对已有同名 team 不会重复创建，直接绑定
- 未知子命令返回 "Unknown /coordinator subcommand"

---

### `/plan` — 计划模式

#### 功能描述

进入/退出计划模式。计划模式下将 permission mode 切换为 `Plan` 并生成一个计划文件（`.cc-rust/plan.md`）。支持 enter/show/open/edit/status/trace/approve/reject/link/classify 多个子命令，以及持久化 workflow 记录。

#### 输出示例

`/plan` 进入计划模式：
```
**Plan mode** (entered) - /path/to/.cc-rust/plan.md
Status: InProgress Approval: Pending ...

(empty plan - use `/plan open` to draft one)
```

`/plan status`：
```
Plan workflow: uuid...
Status: InProgress
Approval: Pending
Plan file: /path/to/.cc-rust/plan.md
Owner: slash_command
Linked tasks: (none)
Updated: 2025-01-01T12:00:00Z
```

`/plan approve`：
```
Plan approved. Normal operations restored.
Status: Completed Approval: Approved ...
```

`/plan path`：
```
Plan file: /path/to/.cc-rust/plan.md
Workflow file: /path/to/.cc-rust/plan_workflow.json
```

`/plan classify <prompt>`：
```
Plan classifier: should_enter=true
Reason: ...
Matched rule: (none)
```

#### 源码路径

`crates/cc-commands/src/plan.rs` (`PlanHandler::execute`)
计划 workflow 逻辑：`crates/cc-commands/src/plan_workflow.rs`

#### 边界情况

- `approve` 恢复先前的 permission mode（pre_plan_mode）
- 重复 `/plan` 不覆盖 pre_plan_mode（幂等）
- `open` 在 `$EDITOR`/`$VISUAL` 不可用时提示但不报错
- 未知子命令返回 "Unknown subcommand: `{}`" 并附带完整用法
- `link` 缺少 task-id 返回 "Usage: /plan link <task-id> [summary]"
- `classify` 缺少 prompt 返回 "Usage: /plan classify <prompt>"
- 无 workflow 时 status/trace 提示 "No plan workflow exists yet"
