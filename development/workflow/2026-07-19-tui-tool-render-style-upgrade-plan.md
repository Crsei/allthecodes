# TUI 工具调用样式升级 + TaskList 浮起渲染计划 — 动词短句折叠 + Spinner 顶部 ✔/◻ 列表

> 计划日期：2026-07-19
> 任务 slug：`tui-tool-render-style-upgrade`
> 触发来源：用户反馈 allthecodes 当前 TUI 工具调用展示风格 `● N reads/searches/operations ⋯`，目标是切换到 **claude-code-bun 的动词短句 + `…` 后缀风格**（`Read 2 files` / `Searched for N patterns…` / `Ran N bash commands…`），并以 **bun 模式**呈现 task 列表（消息流只留 `● TaskUpdate …` 一行，`✔/◻` 列表在 Spinner 区上方靠 `Ctrl+T` 浮起展开）。
> 上游姊妹计划：本任务与 `2026-07-19-tui-task-render-port-plan.md`（轨道 1：TaskCreate/Update 字面名 → ✔/◻ 列表）正交，本计划覆盖"非字面短句折叠 + 浮起展开模式"两个维度；轨道 1 的字符 / 状态映射、阶段 F（TodoWrite 收敛）等仍在原计划文件维护。

## 1. 重要事实修正（来自本次两份并行调查）

claude-code-bun 实际渲染样式与最初假设有偏差，决定本计划前必须确认：

| 维度 | claude-code-bun（TS/Bun）真实 | allthecodes（Rust，现状） |
|---|---|---|
| 折叠后缀字符 | `…` (U+2026 普通省略号) | `⋯` (U+22EF MIDLINE HORIZONTAL ELLIPSIS) |
| 折叠行 label | 动词短句：`Read 2 files` / `Searched for N patterns…` / `Ran N bash commands…`，每类 inline 三元运算符选复/单数 | 路径名词复数：`N reads` / `N searches` / `N operations` / `N edits`（`build_batch_label`） |
| 分组 key | "相邻 + 工具 `isSearchOrReadCommand` flush 算法"，无 enum | `(OperationKind, Option<OperationSubtype>)` enum 二元组严格分桶，必须连续，阈值 = 2 |
| 字面工具名行 | `● TaskUpdate …` — 默认 fallback（TaskUpdate 不参与 collapse，没 renderGroupedToolUse） | `● TaskUpdate ⋯` — classifier 把它兜到 `Unknown`，label 是字面工具名 |
| 展开机制 | (1) `Ctrl+O` 全局 transcript verbose；(2) 鼠标 click / 光标选中行 单独展开该行 | **没有展开分支**，只有折叠摘要。"verbose" 是另一套完全不同的旧式渲染，不是 batch 的展开版 |
| TaskList 展示 | `Spinner` 区上方的 `TaskListV2`，靠 `expandedView` 三态 + `Ctrl+T` cycle；消息流里仅一行 `● TaskUpdate …` | 没有；TUI 完全没接通 task 数据到 chat render，TaskUpdate/TaskCreate 只走字面工具名 |

claude-code-bun 没有 `OperationKind` / `OperationSubtype`，没有"ToolGrouping"组件；分组是 `groupToolUses.ts::applyGrouping`（按 `message.id:toolName` 合并同名 ≥2，目前只有 AgentTool 实现 `renderGroupedToolUse`）+ `collapseReadSearch.ts::collapseReadSearchGroups`（flush 算法）两段函数式 transform。

## 2. 目标（本计划只规划，不落代码）

按"动词短句风格 + bun 模式浮起"重新定义 allthecodes TUI 的工具调用展示：

1. 折叠行 label 从名词复数 `N reads / N searches` 切换为动词短句 `Read N files / Searched for N patterns / Ran N bash commands / Launched N agents…`，活跃态动词变原形进行时（`Reading N files…`）、完成态变过去时（`Read N files`）—— 与 bun 一致。
2. 后缀字符 `⋯` (U+22EF) 统一切换为 `…` (U+2026)，与 bun 主流字符一致。darwin 是否保留 `BLACK_CIRCLE = '⏺'` 的平台分支延用既有 `figures.ts` 习惯。
3. TaskList 渲染采用 **bun 模式**：Task* 工具在消息流里仍只显示 `● TaskUpdate …`（或 `● TaskCreate …` 等单行字面），但当 `expanded_view == Tasks` 时在 Spinner 上方浮起 `TaskListV2` 风格的 `✔/◼/◻` 列表。`Ctrl+T` 在三态 `None ↔ Tasks ↔ Teammates`（无 teammate 时二态）间 cycle。Task 全部完成 5s 后自动折叠回 `None`。
4. 折叠批次支持**逐行展开**：默认单行摘要，click / 光标选中后展开为逐条 `● <label> <状态码>` 列表（启用当前已是 `dead_code` 钩子的 `render_operation_detail_line`）。`Ctrl+O` 暂不移植（bun 全局 transcript 模式），用 chat 内逐行展开替代以减小改动面。

## 3. 不变量（保留 allthecodes 自身设计）

- 保留 `OperationKind / OperationSubtype` enum（不退化为 flush 算法），保留 `(kind, subtype)` 分桶 + 阈值 = 2。bun 的 flush 算法对 ratatui 无必要增益，泛化分桶 + 动词短句足以达成视觉对齐。
- 保留当前 `from_batch` / `from_operation` + `render_tool_operation_lines` 单行基础设施；只改 label 生成与展开分支接入，不重写整条 pipeline。
- 保留 `TaskCreate/Update/List/Get` 走字面工具名一行（避免与轨道 1 渲染管线重叠），但用 TaskList 浮起区吸收"想看到 task 列表"的需求。

## 4. 实施分层（属实施阶段，本计划仅勾画边界）

### 阶段 G — 折叠行 label 切动词短句（核心）

- 在 `crates/allthecodes/src/ui/messages/tool_operation_content.rs::build_batch_label`（162-184 行）改造：`OperationKind → 动词短句`，按 bun 的 inline 复/单数规则生成。
  - `Read → {active: "Reading"}{done: "Read"} {N} {N==1 ? "file" : "files"}`
  - `Search → {active: "Searching for"}{done: "Searched for"} {N} {N==1 ? "pattern" : "patterns"}`
  - `Create → {active: "Writing"}{done: "Wrote"} {N} {N==1 ? "file" : "files"}`（Create 在 allthecodes 含 Write/FileWrite，按 bun 语义归"file"复数）
  - `Modify → {active: "Editing"}{done: "Edited"} {N} {N==1 ? "file" : "files"}`
  - `Delete → {active: "Deleting"}{done: "Deleted"} {N} {N==1 ? "file" : "files"}`
  - `Execute::Test → {active: "Running"}{done: "Ran"} {N} {N==1 ? "test" : "tests"}`；`Execute::Build → "build"/"builds"`；`Execute::Install → "install"/"installs"`；`Execute::Shell → {active: "Running"}{done: "Ran"} {N} {N==1 ? "bash command" : "bash commands"}`
  - `Delegate → {active: "Launching"}{done: "Launched"} {N} {N==1 ? "agent" : "agents"}`
  - `Plan → {active: "Updating"}{done: "Updated"} {N} {N==1 ? "plan" : "plans"}`
  - `Status → {active: "Updating"}{done: "Updated"} {N} {N==1 ? "status" : "statuses"}`
  - `Network / System / Unknown / Permission`：暂沿用通用动词 `{active: "Running"}{done: "Ran"} {N} operations`，或对号入座 prefix。
- 动词时态由当前 batch 整体状态决定：所有 `Resolved` 用过去时；任一 `InProgress` 用进行时 + 加 `…` 后缀；任一 `Error` 末行加 `[error]`、`Cancelled` 加 `[cancelled]`，沿用现有 `render_tool_operation_lines_with_width` 状态码逻辑。
- 复数规则照搬 bun 的"inline 三元运算符"，不抽 `figures` 包（Rust 端用 `if count == 1 { "file" } else { "files" }`），保持 copy 简洁。

### 阶段 H — 后缀字符 `⋯` → `…`

- 全仓 grep `⋯`（U+22EF），统一替换为 `…`（U+2026）。在不与"折叠可展开"指示冲突的前提下，把 `⋯` 作为 InProgress 状态码改用 `…`。
- 同步更新所有出现 `⋯` 的 snapshot 文件（必须 `INSTA_UPDATE=always cargo test -p allthecodes --test pty_tui_e2e -- <failing-tests>` 一次性批量更新，遵循 CLAUDE.md「测试分层验证 SOP」第 2 条 + 新规 nextest 命令）。

### 阶段 I — 折叠批次可展开

- `crates/allthecodes/src/ui/messages/render/context.rs::ToolOperationBatchRenderRecord`（42-50 行）新增 `expanded: bool` 字段（或借 `MessageRenderContext` 的 selection 状态）。
- `crates/allthecodes/src/ui/messages/render/mod.rs:250-261` 在 `if batch.is_batch` 分支根据 `expanded` 二分：
  - 未展开 → 现有 `from_batch` + `render_tool_operation_lines` 单行摘要。
  - 已展开 → `for op in &batch.operations { render_operation_detail_line(op, theme) }`，启用当前 `#[allow(dead_code)]` 钩子（`tool_operation_content.rs:438-443`）。
- 扩展 `decorate_selected_message`（`render/mod.rs:197-208`）覆盖 `RenderableMessage::ToolOperationBatch`，selection 触发 `expanded = true`。
- 折叠态行尾追加 `…`（仅当未展开且 batch 状态非 Resolved）作为"可展开"指示，与 InProgress 主动状态码合并到同一字符位置以避免重复。

### 阶段 J — TaskList 浮起（bun 模式）

- `App` 状态新增 `expanded_view: enum { None, Tasks, Teammates }`，初始从 `global_config.show_expanded_todos / show_spinner_tree` 反序列化（不存在则 `None`）。
- `Ctrl+T`（crossterm `KeyEvent` 路由）cycle：无 teammate 二态 `None ↔ Tasks`；有 teammate 三态 `None → Tasks → Teammates → None`。
- 在 `App::render` 主入口（`crates/allthecodes/src/ui/app/render.rs:241 / :690`）Spinner 区域上方插入条件渲染分支：当 `expanded_view == Tasks` 且 `task_items` 非空时，渲染 task 列表（按轨道 1 阶段 B 的 `render_task_list_lines` 输出，蓝本 `TaskListV2.tsx`）。
- 全部 task 完成 5s 后自动折叠回 `None`（移植 `useTasksV2WithCollapseEffect`），ratatui 端用主 loop tick probe `Instant` 替代 RR `setTimeout`。
- Task* 工具调用本身（TaskCreate/Update/List/Get）在消息流里仍只显示 `● TaskUpdate …` 字面工具名（保留阶段 H 切换后的 `…`，不再走 `✔/◻` 列表）。
- 数据源：复用 `crates/allthecodes/src/ui/command_surface/adapters/tasks.rs::task_surface_items` 同源快照（`allthecodes_tasks::global_store().list()`），不在 Spinner 区旁路再开一份 store 读路径。

### 阶段 K — classifier 把 TaskCreate/Update/List/Get 兜底到字面 label 即可（不再升级到 ✔/◻）

- 与轨道 1 阶段 A 解绑：本计划下，`TaskCreate / TaskUpdate / TaskList / TaskGet` **不需要**新增 classifier arm，**保持** 落 `_ => Unknown`、label 是字面工具名 — 因为 TaskList 浮起区已吸收列表展示，消息流里就是想要的 `● TaskUpdate …`。
- 仅需确认 `user_facing_tool_name`（`rendering/tool_activity.rs:288-302`）的 `_ =>` 兜底能让 `TaskUpdate` 原样返回（当前已是这样），无需改动。

## 5. 关键参考文件清单

### claude-code-bun 端（蓝本）

| 路径 | 作用 |
|---|---|
| `src/components/messages/CollapsedReadSearchContent.tsx`（346-517） | 折叠行动词短句渲染：`Read N files / Searched for N patterns… / Ran N bash commands…`，按 search→read→list→repl→mcp→bash→memory 排序拼 part |
| `src/components/messages/AssistantToolUseMessage.tsx`（127-202） | 单行 `● <userFacingName>(<args>)` 默认渲染（`TaskUpdate` 走这里） |
| `src/utils/groupToolUses.ts`（67-208） | `applyGrouping` 按 `(message.id, toolName)` 合并同名 ≥2，仅 `renderGroupedToolUse` 工具参与 |
| `src/utils/collapseReadSearch.ts`（834-1022） | flush 算法（allthecodes enum 路不分桶，但短句文案逻辑抄这里） |
| `packages/builtin-tools/src/tools/AgentTool/UI.tsx`（745-878） | `renderGroupedAgentToolUse`：`Running N agents… / Launched N agents` — `Delegate` 类动词短句蓝本 |
| `packages/builtin-tools/src/tools/FileReadTool/UI.tsx`（170-178） | `userFacingName`：返回 `'Read'` |
| `packages/builtin-tools/src/tools/FileWriteTool/UI.tsx`（81-86） | `userFacingName`：返回 `'Write'` |
| `packages/builtin-tools/src/tools/TaskUpdateTool/TaskUpdateTool.ts`（104-106, 140-143） | `userFacingName` 返回 `'TaskUpdate'`；调用时 `setAppState({...expandedView:'tasks'})` 触发浮起 |
| `src/components/TaskListV2.tsx`（253-265） | `getTaskIcon` 字符 / 色板（与轨道 1 共用，本计划不重复） |
| `src/components/Spinner.tsx`（371-376） | Spinner 上方条件渲染 `<TaskListV2>`，gated by `expandedView === 'tasks'` |
| `src/components/Messages.tsx`（713-725, 813） | `expandedKeys` + `onItemClick` + `isItemExpanded` 逐行展开机制（allthecodes 用 selection 替代 click） |
| `src/components/CtrlOToExpand.tsx` | `(ctrl+o to expand)` hint — 本计划不移植 Ctrl+O，以 chat 内 selection 展开替代 |
| `src/constants/figures.ts`（4） | `BLACK_CIRCLE = '⏺' (darwin) / '●' (else)`；注：`…` (U+2026) 在 bun 仓库内未抽常量，本计划也不抽 |
| `src/state/AppStateStore.ts` | `expandedView: 'none' \| 'tasks' \| 'teammates'` 三态字段 |

### allthecodes 端（落地点）

| 关注点 | 文件:行号 |
|---|---|
| build_batch_label（动词短句改造点） | `crates/allthecodes/src/ui/messages/tool_operation_content.rs:157-185` |
| render_tool_operation_lines_with_width（状态码 + 展开 / 折叠分支接入点） | `crates/allthecodes/src/ui/messages/tool_operation_content.rs:229-313`、`:438-443`（dead_code 钩子） |
| OperationKind / OperationSubtype 枚举 | `crates/allthecodes-types/src/tool_operation.rs:10-74` |
| classifier match（Task* 落 Unknown 不动） | `crates/allthecodes-tool-display/src/classifier.rs:388-400` |
| 分组二元组 + 阈值 = 2 | `crates/allthecodes/src/ui/messages/render/operation_grouping.rs:54-199`（保留不动） |
| ToolOperationBatchRenderRecord（加 expanded 字段）| `crates/allthecodes/src/ui/messages/render/context.rs:42-50` |
| render mod batch 分支（加展开分支）| `crates/allthecodes/src/ui/messages/render/mod.rs:250-261` |
| decorate_selected_message（覆盖 ToolOperationBatch）| `crates/allthecodes/src/ui/messages/render/mod.rs:197-208` |
| user_facing_tool_name（_ => 兜底确认） | `crates/allthecodes/src/ui/rendering/tool_activity.rs:288-302` |
| 主 render 入口（Spinner 区上方插 TaskList 浮起）| `crates/allthecodes/src/ui/app/render.rs:241 / :690` |
| TUI 拉 task 数据现有入口（复用）| `crates/allthecodes/src/ui/command_surface/adapters/tasks.rs:23-34` |
| RuntimeViewState.task_items（浮起来源）| `crates/allthecodes/src/ui/app/runtime_state.rs:13-95` |

## 6. 验证策略（实施阶段遵循，按 CLAUDE.md 最新 PTY 规）

1. `cargo fmt --all --check`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. 非-PTY crate：`cargo test --workspace --exclude allthecodes --lib`
4. UI 单测：`cargo test -p allthecodes --lib ui::messages` 与 `cargo test -p allthecodes --lib ui::components`
5. PTY 端到端：`cargo nextest run -p allthecodes --test pty_tui_e2e --no-fail-fast`（依赖 `.config/nextest.toml` 的 `tui_pty_e2e.max-threads = 10`，约 2.5min/轮）；用 libtest 时仍须 `cargo test -p allthecodes --test pty_tui_e2e -- --test-threads=1`
6. snapshot 批量更新：`INSTA_UPDATE=always cargo test -p allthecodes --test pty_tui_e2e -- <failing-tests>` 一次性更新所有 `.snap.new`，统一肉眼审阅后一次提交
7. 全仓 release：`cargo build --workspace --release`（≤2 次；超过降级到分 crate 跑）

新增 unit test 至少覆盖：

- `build_batch_label` 各 `OperationKind + 状态(tense)` 输出符合 bun 风格短句（含单复数）。
- `→ …` 后缀仅在 active 时附加；状态码 `[error]/[cancelled]/✔` 逻辑不变。
- 折叠 / 展开切换：未展开输出 1 行；展开输出 N 行 `● <label> <状态>`，每条一行。
- `Ctrl+T` 三态 / 二态 cycle 正确；全 task 完成 5s 自动回 `None`。
- `expanded_view = Tasks` 时 Spinner 上方出现 `✔/◻` 列表（与轨道 1 共享 `render_task_list_lines`）；消息流仍只显示 `● TaskUpdate …`。
- 新 PTY e2e：模拟 `TaskCreate → TaskUpdate → 全部完成` 序列，验证浮起列表出现 → 出现 `✔` → 5s 后折叠。
- 新 PTY e2e：连续 2 条 `Read` 折叠为 `Read 2 files`，逐行展开后变 2 行 `Read <file>`。

## 7. 流程步骤（per-session worktree workflow）

1. 本计划文件单独 commit 到主分支 `allthecodes`（即本次提交）。
2. `git worktree add -b worktree/tui-tool-render-style-upgrade .worktrees/tui-tool-render-style-upgrade allthecodes`。
3. 在 worktree 内按阶段 G → H → I → J → K 顺序实施，每阶段单 commit；commit 描述一句话直接说明本次目的。
4. 写 artifact `development/worktree-workflow-artifacts/2026-07-19-tui-tool-render-style-upgrade.html`（含任务目标 / 流程步骤 / 改动列表 / 本计划文件路径 / commit 列表 / 验证依据）→ commit。
5. `git merge --ff-only worktree/tui-tool-render-style-upgrade` → `git push origin allthecodes`（按 CLAUDE.md 推送规范，必要时 Clash 代理 + `GIT_ASKPASS`） → `git worktree remove .worktrees/tui-tool-render-style-upgrade` + `git branch -d worktree/tui-tool-render-style-upgrade`。
6. 跨 worktree target 隔离：worktree 内 `export CARGO_TARGET_DIR=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.tmp/atc-tui-tool-render-style-upgrade`。
7. 若与轨道 1 worktree（`tui-task-render-port`）并行：两 worktree 共用 `render_task_list_lines`，约定轨道 1 先落地该函数；本任务在 worktree 内通过 cherry-pick 或 rebase 接入。优先串行执行（轨道 1 先合并入主分支再开本 worktree）以避免冲突。

## 8. 验证依据（本计划文件自身）

- 本计划仅写文档，不动任何代码 / 产物，git diff 只追加一个新文件 `development/workflow/2026-07-19-tui-tool-render-style-upgrade-plan.md`，不触当前 dirty 的 `render.rs` / `welcome.rs` / snapshot / `app/tests.rs`。
- 内容覆盖：背景、目标、不变量、实施分层、参考文件清单、验证策略、流程步骤、与轨道 1 的边界划分。
- 落地范围符合「文档更新按任务拆分」「计划文件先上主分支」两条硬性约束。

## 9. 与轨道 1 的边界

- **轨道 1（`2026-07-19-tui-task-render-port-plan.md`）**：负责把 TaskCreate/Update 字面名升级到 `✔/◻` 列表的渲染管线（classifier arm + `render_task_list_lines` + RenderableMessage::TaskList + 数据通道）。**已被本计划修订**：在 bun 模式下，TaskList 不在 chat 消息流里 inline 渲染，而在 Spinner 区浮起 → 轨道 1 阶段 B 产出的 `render_task_list_lines` 仍需要，但阶段 C（RenderableMessage::TaskList 变体 + grouping + render mod inline 对接）改为接入 Spinner 上方浮起分支（本计划阶段 J）。
- **本计划（轨道 2）**：负责动词短句风格（G/H）+ 折叠可展开（I）+ Spinner 浮起机制（J）。`render_task_list_lines` 复用轨道 1；classifier 把 Task* 兜底到字面 label 这一选项明确豁免（K），避免与轨道 1 阶段 A 重叠。
- **执行顺序建议**：轨道 1 先做（先落 `render_task_list_lines` 这个共享件）；轨道 2 在轨道 1 合并入主分支后起 worktree，复用该函数。两者**不在同一 worktree**。

## 10. 非目标

- 不移植 bun 的 `Ctrl+O` 全局 transcript verbose 切换（用 chat 内逐行 selection 展开替代）。
- 不移植 bun 的 `unfold groupToolUses`（同名 tool `(message.id, toolName)` 合并 ≥2）—— allthecodes 的 `(kind, subtype)` 分桶已能覆盖目标样式，避免引入第二套分组机制。
- 不改 `(OperationKind, OperationSubtype)` 分桶算法本身（只改 label 文案，不改分组 key 与阈值）。
- 不移植 plan 文件 diff / preview（用户样例里 `└ 已完成 …` 那一行属 FileEditToolUpdatedMessage，与 task 系统无关，留待独立任务）。
- 不在 chat 流里 inline 渲染 `✔/◻` 列表（已切换为 bun 浮起模式）。
- 不动 Per-Session Worktree Workflow 主流程文档。
- 不改 `DelegateTask` 引擎内部工具定位（不在 LLM 外显工具集）。
