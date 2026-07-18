# TUI Task / Todo 渲染移植计划 — 把 TaskCreate 等的 "● TaskCreate" 字面行升级为 claude-code-bun 风格 ✔/◻ 列表

> 计划日期：2026-07-19
> 任务 slug：`tui-task-render-port`
> 触发来源：用户反馈 — allthecodes 运行 TaskCreate / TaskUpdate / TaskList / TaskGet / TodoWrite / DelegateTask 时，TUI 仍然只显示 `● TaskCreate` 这种字面工具名一行，没有像 claude-code-bun 那种 `• Updated Plan` + ✔/◻ 缩进列表的结构化展示。

## 1. 背景与目标

用户期望的样式（来自 claude-code-bun 截屏）：

```
• Updated Plan
  └ 已完成 Codex 当前源码与 allthecodes 对应链路审查；开始固化修复边界和测试方案。
  ✔ 读取两个仓库的约束与相关历史上下文
  ✔ 审查 Codex 的流式请求、超时与重试实现
  □ 对照 allthecodes 当前实现并确定修复与测试边界
  □ 在 development/bugs 编写计划并校验
  □ 按文档任务要求仅提交计划文件
```

claude-code-bun 中这一坨 **不是同一个组件一次性渲染的**，而是两层叠到一张屏上：

| 屏幕行 | 来源组件 | 数据源 |
|---|---|---|
| `• Updated Plan` | `AssistantToolUseMessage.tsx` 的 `●` bullet + `FileEditTool/UI.tsx` `userFacingName()` 返回 `'Updated plan'`（plan 目录下） | 单次 `tool_use` |
| `└ 已完成 …` | `FileEditToolUpdatedMessage` 渲染的 plan 文件 diff/preview | `tool_result` |
| `✔ …` / `□ …` | **`TaskListV2.tsx`** ← 本次移植核心 | v2 `Task[]` 状态 |

allthecodes 当前对应状况（来自对 `crates/allthecodes/src/ui/` 的全量探查）：

- **TaskCreate / TaskUpdate / TaskList / TaskGet / DelegateTask** 在 `allthecodes-tool-display::classifier.rs::classify` 的 `match tool_name { … }` 里**完全没有 arm**，全部落入 `_ => Unknown` 分支 → 走 `ToolOperationBatch` 路径 → 由 `tool_operation_content.rs::render_tool_operation_lines` 渲染成一行 `● TaskCreate` 字面工具名。`user_facing_tool_name` 对这些名字也没有映射，原样回显。
- **TodoWrite** 是当前唯一被特殊对待的 task/todo 类工具，已有两套 rich 路径（`assistant_tool_use_message.rs:164-218` 单条文本路径 + `tool_operation_content.rs:319-415` 批量路径），但用 `[x]/[*]/[ ]` 而不是 `✔/◻`，且状态标题是 `Updated todos` 而非 `Updated Plan`。
- **`ui/` 中无 `✔` (U+2714) / `□` (U+25A1) / `Updated Plan` 字面 / `TaskView` / `TodoView` / `UpdatedPlanWidget` 任何组件**。
- **TUI 进程能拿到 task 数据**：`ui/command_surface/adapters/tasks.rs:23-34` 已经在调用 `allthecodes_tasks::global_store().list()` 把 `TaskEntry` 转 `TaskSurfaceItem` 喂给命令面板。但这条通道**从未进入 chat 消息渲染管线**（`ui/messages/render/`）；`MessageRenderContext` / `RenderableMessage`（`render/context.rs`）里没有 `task_store` / `todo_store` 字段。
- 当前 dirty 文件（`render.rs` / `welcome.rs` / 相关 snapshot）的 diff **与本任务无关**（只动了 cwd 字段 / prompt_mode_indicator / status bar），不在本计划范围。

### 目标（本计划只规划，不落代码）

把 allthecodes Rust TUI 的 Task / Todo 渲染对齐 claude-code-bun `TaskListV2.tsx` 的样式：

1. TaskCreate / TaskUpdate / TaskList / TaskGet 不再以字面工具名一行展示，而是把当前 task 列表渲染为 `✔/◻/◼` 状态行列表。
2. TodoWrite 已有的 `[x]/[*]/[ ]` checklist 风格保留为兼容回退，但提供与新 task 列表一致的字符与色板（可逐步收敛到 `✔/◻/◼`）。
3. "• Updated Plan" header 仅作为 task 列表的标题来源之一，不要求移植 FileEditTool diff preview（属非目标）。
4. 加 `Ctrl+T` 切 `expandedView` 三态（None ↔ Tasks ↔ Teammates，无 teammates 时退化为二态），并把 task 列表注入 chat render 流。

## 2. 差距判断（来自 §1 调查结论）

主因 **(b) 渲染组件缺失**，次因 **(c) 对接缺失**，(a) 数据通道**非主要缺口**：

- **(a) 数据通道**：部分成立但非主因 — `allthecodes_tasks::global_store()` 已存在且被 `ui/command_surface` 证明可用；缺的是把 TaskStore handle 通过 `MessageRenderContext` 下发到 chat renderer 的连接点。可在 `crates/allthecodes/src/ui/` 单文件层补齐。
- **(b) 渲染组件**：完全成立 — 缺 `RenderableMessage::TaskList` 变体（现 enum 只有 `Message / ToolOperationBatch / TodoList`）、缺 `render_task_*_operation_lines` / `render_updated_plan_lines` 函数、缺 `ui/components/` 下的 task/todo/plan 组件、`classifier.rs` 没识别这 5 个工具名。
- **(c) 对接**：部分成立 — 即使加了组件，也要改 `classifier.rs`（识别 5 个工具名）、`operation_grouping.rs`（路由到新变体）、`render/mod.rs::match RenderableMessage`（加新 arm）三层注册。

## 3. 借鉴对象确认

allthecodes 的 task 工具链（命名、参数、状态机、TodoWrite verification_nudge）整体借鉴自 **`claude-code-bun`（Claude Code 的 TypeScript/Bun 原版）**，本次 TUI 渲染移植也以 claude-code-bun 为蓝本。证据：`crates/allthecodes-engine/src/agent/mod.rs:44-49` 的 `TaskAgentTool` doc 注释明示 "Claude Code's TypeScript surface exposes this capability as `Task`"；schema 描述里反复出现 "Bun-compatible camelCase alias"；`agent_definitions/mod.rs:32` 含 `anthropic/claude-code/blob/main/src/tools/AgentTool/loadAgentsDir.ts` upstream 链接。codex 端没有面向 Agent 的 `TaskCreate/Update/List/Get` 工具集（其 `cloud-tasks` 是面向用户的 TUI），不构成借鉴对象。

## 4. 关键参考文件清单

### claude-code-bun 端（蓝本）

| 路径 | 作用 |
|---|---|
| `src/components/TaskListV2.tsx` | 核心 — `Task[]` 渲染为 `✔/◼/◻` + 数字摘要头；`getTaskIcon(status)` switch (253-265) 是字符/色板真源 |
| `src/hooks/useTasksV2.ts` | `TasksV2Store` 单例 + `useSyncExternalStore`，含 5s 全完成自动清空 + 自动折叠 |
| `src/utils/tasks.ts` | `Task` / `TaskStatus` schema、`isTodoV2Enabled()`、`listTasks()`、`getTaskListId()` 五段优先 |
| `src/hooks/useGlobalKeybindings.tsx:55-82` | `Ctrl+T` 切 `expandedView` |
| `src/state/AppStateStore.ts` | `expandedView: 'none' \| 'tasks' \| 'teammates'` |
| `src/components/Spinner.tsx:371-376` | spinner 在线时内联渲染 `<TaskListV2>`（gated by `expandedView==='tasks'`） |
| `src/screens/REPL.tsx:5863-5866` | 非 spinner 时以 `isStandalone` 模式渲染 `<TaskListV2>` + 摘要头 |
| `packages/builtin-tools/src/tools/FileEditTool/UI.tsx:25-50` | `userFacingName()` 返回 `'Updated plan'`（plan 目录下） |
| `src/components/messages/AssistantToolUseMessage.tsx:136-152` | `●` bullet + bold userFacingName + `(path)` 整体行结构 |
| `src/constants/figures.ts` | `BLACK_CIRCLE = '●'`（darwin 用 `'⏺'`） |
| npm `figures` 包 | `figures.tick='✔'` / `figures.squareSmallFilled='◼'` / `figures.squareSmall='◻'` / `figures.pointerSmall='›'` |

### allthecodes 端（落地点）

| 关注点 | 文件:行号 |
|---|---|
| 主 render 入口 | `crates/allthecodes/src/ui/app/render.rs:241` & `:690` |
| RenderableMessage enum | `crates/allthecodes/src/ui/messages/render/context.rs:62-69` |
| TodoList RenderableMessage 派发 | `crates/allthecodes/src/ui/messages/render/mod.rs:262` |
| ToolOperationBatch 派发 | `crates/allthecodes/src/ui/messages/render/mod.rs:250-260` |
| assistant ToolUse → 文本渲染 | `crates/allthecodes/src/ui/messages/render/render_assistant.rs:84-135` |
| TodoWrite 文本级 rich render（参考扩展点） | `crates/allthecodes/src/ui/messages/assistant_tool_use_message.rs:164-218` |
| TodoWrite 批量 rich render（参考扩展点） | `crates/allthecodes/src/ui/messages/tool_operation_content.rs:319-415` |
| tool_display 分类（5 个工具名无 arm） | `crates/allthecodes-tool-display/src/classifier.rs:101-401` |
| TodoWrite 分类为 Status/Todo（参考） | `crates/allthecodes-tool-display/src/classifier.rs:317-336` |
| 连续 TodoWrite → TodoList 批聚合 | `crates/allthecodes/src/ui/messages/render/operation_grouping.rs:43-51` + `:201-254` |
| user_facing_tool_name | `crates/allthecodes/src/ui/rendering/tool_activity.rs:288-302` |
| TUI 拿 TaskEntry 的现有入口（命令面板）| `crates/allthecodes/src/ui/command_surface/adapters/tasks.rs:23-34` |
| TUI 持有 task 状态 | `crates/allthecodes/src/ui/app/runtime_state.rs:13-95` (`RuntimeViewState.task_items`) |
| TaskCreate 工具实现 | `crates/allthecodes-tasks/src/tool_requests.rs:8-85` |
| Task domain store | `crates/allthecodes-tasks/src/store.rs` (`TaskStore`) |
| 全局 store accessor | `allthecodes_tasks::global_store()` |
| 已有 TodoWrite unit test（参考）| `crates/allthecodes/src/ui/messages/assistant_tool_use_message.rs:403-416`、`render/mod.rs:1332-1394` |

## 5. 字符 / 状态映射（移植目标）

| status | 字符 | 颜色 | 文字样式 | 含义 |
|---|---|---|---|---|
| `completed` | `✔` (U+2714) | `success` 绿 | `DIM + CROSSED_OUT`（对齐 bun 的 strikethrough+dimColor） | 完成 |
| `in_progress` | `◼` (U+25FC) | `claude`/品牌蓝 | `BOLD` | 进行中 |
| `pending` | `◻` (U+25FB) | 默认 | 普通 | 等待中 |
| `pending`(blocked) | `◻` + ` › blocked by #2, #3` | 默认 | `DIM`，后缀 blocked by | 阻塞 |

> 字符在 claude-code-bun 来自外部 `figures` 包。allthecodes 端直接用 Rust 字面常量即可（`'✔'`/`'◼'`/`'◻'`/`'›'`）。darwin 平台是否要 fallback 到其它字形（bun 的 `BLACK_CIRCLE` 在 darwin 用 `⏺`）由实施阶段在 `crates/allthecodes/src/ui/rendering/tool_activity.rs` 旁边的常量模块决定；非本计划约束。

状态枚举两边一致（`pending / in_progress / completed`），可直接用 `allthecodes_tasks` 现有 `TaskStatus`，无需新增类型。

## 6. 实施分层（属实施阶段，本计划仅勾画边界）

### 阶段 A — 分类器识别 task 工具名（必做，门槛最低）

- 在 `allthecodes-tool-display/src/classifier.rs::classify` 的 `match tool_name` 加 arm：
  - `TaskCreate` / `TaskUpdate` / `TaskList` / `TaskGet` → `(OperationKind::Status, Some(OperationSubtype::Task), …)`，label 形如 `Update task list`（参照现有 `TodoWrite → "Update TODO list (N items)"` 写法）。
  - 已有 `OperationSubtype::Todo` 模式可类比新增 `OperationSubtype::Task`（若 enum 已有则复用）。
- 风险/置信度先按 `TodoWrite` arm 的同档设置（Low / 较高置信）。
- `DelegateTask` 是引擎内部工具（非 LLM 外显工具），**不在 classifier 范围**，保持现状。

### 阶段 B — 渲染组件（核心）

新增（建议文件）`crates/allthecodes/src/ui/messages/task_list_content.rs`，类比 `tool_operation_content.rs::render_todo_operation_lines` 写一个：

- `pub fn render_task_list_lines(/* tasks: &[TaskEntry], theme, state */) -> Lines`
- 内部按 `status` 字段 dispatch 到 `✔/◼/◻` + 主题色 + `Modifier`（DIM/CROSSED_OUT/BOLD）。
- 头行：`  ● Updated tasks`（resolved）/ `  ● Updating tasks`（in_progress）/ `  ● Update tasks`（queued），`isStandalone` 模式额外加摘要头 `N tasks (X done, Y in progress, Z open)`。
- 截断策略：先按 claude-code-bun `TaskListV2.tsx:142-198` 的优先级（`recentCompleted(30s) → in_progress → pending(by blockedBy) → olderCompleted`）做 `max_display` 截断；多出部分一行 dim `… +N in progress, M pending, K completed`。
- `TaskCreate` 单次调用若带 `subject/description`，可在 tool_use 文本路径类比 `assistant_tool_use_message.rs::render_todo_write_tool_use_message`（164-218）写一个 `render_task_create_tool_use_message`，把 `subject` 立刻显示为一个新行（pending 状态字符）。

### 阶段 C — 对接分组与派发

- `ui/messages/render/context.rs` 给 `RenderableMessage` 增加变体 `TaskList(TaskListRenderRecord)`（参照 `TodoList` 写法）。
- `ui/messages/render/operation_grouping.rs::group_by_operation` 增加分支：连续 `OperationKind::Status && OperationSubtype::Task` 聚合为 `RenderableMessage::TaskList`。
- `ui/messages/render/mod.rs` 的 `match RenderableMessage` 加 arm 调用阶段 B 的 `render_task_list_lines`。
- `ui/messages/render/operation_grouping.rs::is_suppressed_operation_tool` 评估是否需要把 task 工具从被合并的 batch 里豁免（参照 TodoWrite 当前豁免逻辑）。

### 阶段 D — 数据通道接通

- 把 `allthecodes_tasks::global_store().list()` 的结果（或当前 `RuntimeViewState::task_items` 已缓存的快照）通过 `MessageRenderContext` 传给 chat renderer。
- 优先复用现有 `command_surface/adapters/tasks.rs::task_surface_items` 的同源数据，避免在 chat renderer 旁路再开一份 store 读路径。
- 考虑懒加载：仅在 `RenderableMessage::TaskList` 出现时拉一次快照（task 列表是一致的进程内全局态）。

### 阶段 E — 折叠/展开与键位

- `App` 状态新增 `expanded_view: enum { None, Tasks, Teammates }`，初始从 `global_config.show_expanded_todos / show_spinner_tree` 反序列化（如该配置不存在则默认 `None`）。
- `Ctrl+T` 在 crossterm `KeyEvent` 路由里 cycle：无 teammates 时 `None ↔ Tasks`；有 teammates 时 `None → Tasks → Teammates → None`。
- 全 task 完成后 5s 自动把 `expanded_view` 抹回 `None`（移植 `useTasksV2WithCollapseEffect`）；ratatui 端用主 loop tick 里 probe `Instant` 替代 RR `setTimeout`。

### 阶段 F — TodoWrite 字符收敛（可选，单独 commit）

- 把 `assistant_tool_use_message.rs::todo_status_marker` 和 `tool_operation_content.rs::render_todo_operation_lines` 里的 `[x]/[*]/[ ]` 替换为 `✔/◼/◻`，与 task 列表统一字符表。
- 同步更新对应已存在的 snapshot（`assistant_tool_use_message.rs:403-416` 测试、`render/mod.rs:1332-1394` 测试及其它 snapshot 文件）。需要 `INSTA_UPDATE=always` 一次性批量更新（参见 CLAUDE.md「测试分层验证 SOP」第 2 条）。
- 若决定保留 `[x]/[*]/[ ]` 风格，本阶段跳过。

> 实施 **不在本计划内**：本计划只锁定文件清单、分层边界、字符表、验证策略；具体代码改动在后续 worktree 任务里完成。

## 7. 验证策略（实施阶段遵循）

按 CLAUDE.md「测试分层验证 SOP」执行：

1. `cargo fmt --all --check`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. 非-PTY crate：`cargo test --workspace --exclude allthecodes --lib`
4. UI 单测：`cargo test -p allthecodes --lib ui::messages` 与 `cargo test -p allthecodes --lib ui::components`
5. PTY 端到端：`cargo test -p allthecodes --test pty_tui_e2e -- --test-threads=1`（必要时按测试分层 SOP 第 2 条用 `INSTA_UPDATE=always` 一次性更新 snapshot）
6. 全仓 release：`cargo build --workspace --release`（≤2 次；超过降级到分 crate 跑）

新增 unit test 至少覆盖：

- `render_task_list_lines` 把 `pending/in_progress/completed/blocked` 四种状态映射到正确字符与色板。
- 截断优先序列在 `tasks.len() > max_display` 时正确按 `recentCompleted → in_progress → pending(by blockedBy) → olderCompleted` 排序并输出 `… +N` 占位行。
- `classifier` 把 `TaskCreate/Update/List/Get` 正确分到 `Status/Task`，不再落 `_ => Unknown`。
- `Ctrl+T` 在有/无 teammates 两种情况下 cycle 正确三态/二态。
- 新增 PTY e2e：模拟一次 `TaskCreate` + `TaskUpdate` 调用，截屏校验出现 `✔/◻` 而不是 `● TaskCreate`。

## 8. 流程步骤（per-session worktree workflow）

1. 本计划文件单独 commit 到主分支 `allthecodes`（即本次提交）。
2. `git worktree add -b worktree/tui-task-render-port .worktrees/tui-task-render-port allthecodes`。
3. 在 worktree 内按阶段 A → F 顺序实施，每阶段单 commit；commit 描述一句话直接说明本次目的。
4. 写 artifact `development/worktree-workflow-artifacts/2026-07-19-tui-task-render-port.html`（含任务目标 / 流程步骤 / 改动列表 / 本计划文件路径 / commit 列表 / 验证依据）→ commit。
5. `git merge --ff-only worktree/tui-task-render-port` → `git push origin allthecodes`（按 CLAUDE.md 推送规范，必要时套用 Clash 代理 + `GIT_ASKPASS`） → `git worktree remove .worktrees/tui-task-render-port` + `git branch -d worktree/tui-task-render-port`。
6. 跨 worktree target 隔离：worktree 内 `export CARGO_TARGET_DIR=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.tmp/atc-tui-task-render-port`。

## 9. 验证依据（本计划文件自身）

- 本计划仅写文档，不动任何代码 / 产物，git diff 只追加一个新文件 `development/workflow/2026-07-19-tui-task-render-port-plan.md`，不触当前 dirty 的 `render.rs` / `welcome.rs` / snapshot / `app/tests.rs`。
- 内容覆盖：背景、差距判断、借鉴对象、参考文件清单、字符表、实施分层、验证策略、流程步骤。
- 落地范围符合「文档更新按任务拆分」「计划文件先上主分支」两条硬性约束。

## 10. 非目标

- 不在本计划落地任何代码改动（代码改动属后续 worktree 任务）。
- 不移植 `FileEditToolUpdatedMessage` 的 plan 文件 diff/preview（用户样例里 `└ 已完成 …` 那一行），该渲染与 task 系统无关。
- 不引入 Agent Teams swarm 的 owner 颜色映射（`AGENT_COLOR_TO_THEME_COLOR`），allthecodes 当前无 swarm，暂保留 dimColor 默认即可。
- 不动 v1 `TodoWrite` 的 verification_nudge 触发条件（已与 bun 端对齐，无需改）。
- 不动 Per-Session Worktree Workflow 主流程文档。
- 不改变 `DelegateTask` 的引擎内部工具定位（它不在 LLM 外显工具集，不进 classifier / chat render）。
