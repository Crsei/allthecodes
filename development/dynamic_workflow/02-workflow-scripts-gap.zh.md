# Workflow Scripts 上游对齐补充清单

> 记录日期：2026-07-04
> 对照来源：
> - 上游说明：`/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-bun/docs/features/workflow-scripts.md`
> - 上游实现：`packages/builtin-tools/src/tools/WorkflowTool/`
> - 本项目设计：`development/dynamic_workflow/01-arch-plan.zh.md`
> - 本项目现状：`crates/allthecodes-tools/src/workflow/`、`crates/allthecodes-tools/src/workflow_dynamic/`

## 结论

`01-arch-plan.zh.md` 已经覆盖了 `DynamicWorkflow` 的 JSON DAG、多 agent fan-out/reduce、并发控制和 AgentTool 复用路径；但它没有覆盖上游 `WORKFLOW_SCRIPTS` 的另一条核心能力：**用户在项目目录中放置 workflow 脚本文件，通过 `/workflows` 或自动生成的 slash command 发现并顺序推进 workflow run**。

因此后续设计需要把“文件型 workflow scripts”作为独立补充，不应直接并入现有 DynamicWorkflow DAG 设计，也不应和 `/plan` 的 plan workflow 混淆。

## 上游基线

上游 `WORKFLOW_SCRIPTS` 目前实际行为如下：

- Feature gate：`FEATURE_WORKFLOW_SCRIPTS`。
- 用户 workflow 目录：`.claude/workflows`。
- workflow run 状态目录：`.claude/workflow-runs`。
- 文件扩展名：`.yml`、`.yaml`、`.md`。上游说明文档提到 JSON，但当前 `constants.ts` 没有把 `.json` 列为可发现扩展名。
- `getWorkflowCommands(cwd)` 扫描 workflow 目录，把每个文件变成同名 slash command，例如 `release.md` -> `/release`。
- `/workflows` 命令只列出可用 workflow scripts。
- `WorkflowTool` schema 是 `{ workflow, args?, action?, run_id? }`，action 包含 `start | status | advance | cancel | list`。
- `start` 不自动执行所有步骤，只解析 workflow 文件，创建 run 记录，把第一个 step 标记为 `running`，并返回当前 step prompt 和后续需要调用 `advance` 的提示。
- `advance` 标记当前 step 完成，推进下一个 step；没有下一个 step 时把 run 标记为 `completed`。
- `cancel` 把 run 和未完成 step 标记为 `cancelled`。
- Markdown parser 支持 checklist、bullet、numbered list；YAML parser 支持 `name` 和 `prompt|run|command` 的轻量 step 提取；无法提取步骤时把整个文件作为单步 workflow。
- `LocalWorkflowTask`/`WorkflowDetailDialog` 主要负责任务面板展示、kill、skip/retry agent action 壳和 orphan cleanup。
- `bundled/index.ts` 当前是 no-op，但作为内置 workflow 初始化 hook 保留。

## 本项目现状

本项目已有三类相近但不同的 workflow 概念：

- `Workflow` tool：`crates/allthecodes-tools/src/workflow/mod.rs`
  - 输入是 `name/goal/steps/workflow_id/step_id`，不是上游的 `workflow/run_id`。
  - 持久化 project-local JSON record 到 `.allthecodes/workflows/*.json`，run 摘要到 `.allthecodes/workflow-runs/*.json`。
  - 由模型显式创建步骤并手动 `advance`，不读取用户 workflow script 文件。
- `DynamicWorkflow` tool：`crates/allthecodes-tools/src/workflow_dynamic/`
  - 已实现 JSON DAG 校验、`agent/map/reduce`、拓扑执行、Semaphore 并发、reduce 输入截断、Agent deferred dispatch。
  - 不持久化 run，不提供 `status/advance/cancel/list`，不扫描文件，不注册 slash commands。
- `/plan` plan workflow：`crates/allthecodes-commands/src/plan*.rs`
  - 只管理 plan mode、审批和 implementation task evidence，属于计划流程，不是 workflow scripts。

另外，Rust TUI 当前有 `workflow_detail_dialog.rs`，但只是把 `TaskStatus` 列表渲染成文本；还没有上游 `LocalWorkflowTaskState` 那种 workflowName、workflowFile、output、agentCount、pendingAgentAction 的任务状态模型。

## 历史工作树实现复核（2026-07-04，已过时）

> 2026-07-05 更新：本节记录的是实现前复核，已不代表当前主工作树状态。当前状态见下一节“当前工作树实现状态（2026-07-05）”。

对照 `.worktrees/workflow-scripts-gap/development/dynamic_workflow/01-arch-plan.zh.md` 的“已知剩余缺口：文件型 workflow 动态 slash command”补充，当前主工作树的实际状态需要更保守地判断。

`.worktrees/.../01-arch-plan.zh.md` 里写到当前已补齐：

- `Workflow`/`workflow` 工具在输入含 `workflow` 或 `run_id` 时进入文件型 workflow mode。
- 文件发现路径使用 `.allthecodes/workflows`，run 状态持久化到 `.allthecodes/workflow-runs`。
- 支持 `.md`、`.yaml`、`.yml` parser 和 `/workflows` 列表命令。
- 剩余缺口主要是把 `.allthecodes/workflows/release.md` 注册为 `/release` 动态 slash command。

但按当前主工作树核查，这些“已补齐”能力还不能视为落地：

- `crates/allthecodes-tools/src/workflow/file_workflow.rs` 是新增未跟踪文件，当前 `workflow/mod.rs` 只声明了 `pub mod plan;`，没有声明或使用 `file_workflow` 模块；因此该文件不会进入编译，也不会暴露任何工具行为。
- 当前 `WorkflowTool` 的 schema 和校验仍是 `workflow_id/name/goal/steps/step_id` 旧模式；输入 `{ workflow, args?, action?, run_id? }` 仍会被 `validate_input` 拒绝或走不到 file-script mode。
- `FileWorkflowAction` 的注释声称兼容 `{ workflow, args?, action?, run_id? }`，但实际结构体没有 `run_id` 字段；run record 也以 `workflow_id` 作为文件名，不能表达上游“一次 start 生成一个 run_id”的多次运行语义。
- `file_workflow.rs` 目前主要是 record/parser/schema helper，没有真正实现 `start/status/advance/cancel/list` 的工具分发、权限请求、run 文件读写、模型可读返回文本或 `run_id` 指令。
- `/workflows` 命令未在 `crates/allthecodes-commands/src/lib.rs` 的 builtin commands 中出现，也未看到 cwd-aware workflow metadata provider；动态 registry 的 `ExecutionStrategy::Inline` 仍会报 “has no handler”，不能承载 workflow 文件命令。
- 当前 `cargo check -p allthecodes-tools` 在进入 tools crate 前失败于 `crates/allthecodes-types/src/agent_events.rs` 的自 crate 导入：`use allthecodes_types::message::Message` / `use allthecodes_types::agent::ForkContext`。这说明当前工作树整体还未达到可验证状态；该失败与 file workflow 未接入是两个独立问题。

`file_workflow.rs` 如果后续接入模块树，还需要先处理以下实现问题：

- `FileWorkflowRecord::update_ready_steps()` 在 `for step in &mut self.steps` 中再次 `self.steps.iter()` / 切片读取，按当前写法会触发可变借用与不可变借用冲突。
- Markdown parser 的 `[x]` checklist 跳过逻辑不可靠：`match_checkbox()` 对已完成项返回 `None` 后，后续 bullet parser 可能把 `- [x] step` 当作普通 bullet 重新收进步骤。
- numbered list parser 没有严格验证分隔符前全是数字，可能把非编号文本误判为步骤。
- YAML parser 使用 `map.get("steps")` / `map.get("workflow")` / `map.get("name")` 这类字符串 lookup，需要在真实编译中确认 `serde_yaml::Mapping` 是否接受该 key 类型；更稳妥的写法是用 `serde_yaml::Value::String(...)` 查询。
- `FileWorkflowObservation::list(workflows)` 当前丢弃 `workflows` 参数，输出里没有可用 workflow 列表。
- `PathBuf`、`anyhow` 等导入当前未使用；模块接入后会产生 warning，需要按仓库要求清理。

因此，当前主工作树的结论应调整为：**file workflow 的部分类型和 parser 草稿已出现，但工具层、命令层、run 语义、权限、测试和 TUI/API 可观测性尚未真正接入；动态 slash command 不是唯一剩余缺口。**

## 当前工作树实现状态（2026-07-05）

本次已按推荐实现补齐 P0 file-script 兼容层，当前主工作树状态与 `.worktrees/workflow-scripts-gap/development/dynamic_workflow/01-arch-plan.zh.md` 的“已知剩余缺口”基本一致：

- `crates/allthecodes-tools/src/workflow/file_workflow.rs` 已接入 `workflow/mod.rs` 并进入编译。
- `Workflow`/`workflow` 工具在输入含 `workflow`、`run_id` 或 `workflow_script: true` 时进入文件型 workflow mode；旧 `name/goal/steps/workflow_id` static workflow mode 保持不变。
- 文件发现路径为 `.allthecodes/workflows`，run 状态持久化到 `.allthecodes/workflow-runs/{run_id}.json`；默认不读取或写入 `.claude`。
- 支持 `.md`、`.yaml`、`.yml` parser；Markdown 已覆盖 checklist、bullet、numbered list，并跳过 `[x]` 已完成项；YAML 已覆盖 `name/title/prompt/description/run/command`。
- `start/status/advance/cancel/list` 已有工具分发、权限语义、结构化 `data`、模型可读 `model_content` 和独立 `run_id`。
- `/workflows` 已作为 builtin slash command 注册，用于列出项目 `.allthecodes/workflows` 下的 workflow scripts，且不会把旧 `.json` static workflow record 当成脚本。
- `agent_events.rs` 的编译阻塞已修复，并为现有 `AgentEvent::Spawned` 构造点补了 `context: None`。

仍未落地的上游兼容能力：

- cwd-aware 动态 slash command：尚未把 `.allthecodes/workflows/release.md` 自动注册为 `/release`。
- Rust TUI / API / WS 的 `LocalWorkflowTask` 级完整任务状态、detail dialog、skip/retry/kill 交互仍未实现。
- startup bundled workflow hook、内置 workflow 与用户 workflow 的冲突优先级、feature gate UI 暴露仍需后续补齐。
- `/workflows` 当前是列表命令；坏文件会在列表中显示 parse error，但还没有 dynamic command 执行路径的坏文件/冲突处理。

## 需要补充的设计

### P0：文件型 Workflow Scripts 兼容层

新增或扩展设计一条上游兼容路径，建议命名为 **FileWorkflow / WorkflowScript mode**，避免和 DynamicWorkflow 混淆。

需要明确：

- allthecodes 路径必须是 `.allthecodes/workflows` 和 `.allthecodes/workflow-runs`，不能使用 `.claude`。
- 是否兼容读取 `.claude/workflows` 只能作为显式迁移/导入策略，不能作为默认持久化路径。
- 支持扩展名至少为 `.md`、`.yaml`、`.yml`；是否增加 `.json` 要标记为 allthecodes 扩展，因为上游当前常量未启用 JSON。
- run record 应包含 `run_id`、`workflow`、`args`、`status`、`created_at`、`updated_at`、`current_step_index`、`steps[]`。
- step record 应包含 `name`、`prompt`、`status`、`started_at`、`completed_at`。

### P0：WorkflowTool schema 兼容策略

当前 `Workflow` tool 已占用上游工具名语义。需要设计兼容策略：

- 推荐：在现有 `Workflow`/`workflow` tool 中增加 file-script mode。
- 判定方式：输入含 `workflow` 字段时走上游兼容模式；输入含 `name/goal/steps` 时保留当前 allthecodes static workflow mode。
- read-only：`status`、`list` 允许；`start`、`advance`、`cancel` 继续走权限请求。
- 输出需要同时保持结构化 `data` 和模型可读文本，且文本应包含 `run_id` 和下一步执行指令。

### P0：workflow 文件发现与 slash command 注册

需要补一套 command 设计：

- `/workflows`：列出 `.allthecodes/workflows` 中的 workflow scripts；空目录时提示创建 `.md` 或 `.yaml` 文件。
- 动态 slash command：扫描 workflow 文件并注册 `/filename` 命令，命令内容等价于上游 `Execute this workflow:\n\n{content}\n\nArguments: {args}`。
- 需要接入现有 `DYNAMIC_REGISTRY` 或命令 metadata provider，避免只在工具层可见。
- 命令发现要处理坏文件、重名、大小限制、隐藏/内置命令冲突。

### P0：轻量 DSL/parser

`01-arch-plan` 的 JSON DAG 不是上游文件 workflow parser。需要补充独立 parser：

- Markdown:
  - `- [ ] step`
  - `- [x] step`
  - `- step`
  - `1. step` / `1) step`
- YAML:
  - `- name: Inspect`
  - `prompt: ...`
  - `run: ...`
  - `command: ...`
- fallback：无步骤时把全文作为单步 `Execute workflow`。
- parser 不应执行 shell command；`run/command` 只是 prompt 字段。

### P1：run 状态与可观测性

上游 workflow scripts 是可恢复/可查询的 run，而 `DynamicWorkflow` 是一次 tool call 内执行完。需要补充：

- `.allthecodes/workflow-runs/{run_id}.json` 的读写、safe parse、损坏文件忽略策略。
- `list` 最近 run 的排序和数量上限。
- `status` 格式应列出当前 step 和所有 step 状态。
- run 状态应进入任务/状态面板，供 TUI 和 API/WS 侧观察。

### P1：LocalWorkflowTask / Rust TUI 设计

如果要对齐上游任务面板，需要新增 Rust 侧任务模型或扩展现有 task status：

- `type = local_workflow`
- `workflow_name`
- `workflow_file`
- `summary`
- `agent_count`
- `output`
- `agent_id`
- `abort_controller` 等价取消句柄
- `pending_agent_action = skip | retry`

TUI detail dialog 需显示 workflow 文件、状态、输出，并支持返回、停止；如果后续 workflow scripts 驱动子 agent，再接入 skip/retry。

### P1：权限 UI 与规则

上游有独立 `WorkflowPermissionRequest`。Rust 侧需要补设计：

- 权限消息要展示 workflow 名称和 args。
- 允许规则应作用于 `Workflow`/`workflow` 命令或工具名。
- `status/list` 不需要询问。
- `cancel` 属于停止运行任务，应确认。

### P1：启动与 bundled hook

虽然上游 `initBundledWorkflows()` 当前 no-op，本项目仍应保留等价 hook：

- feature gate 使用 `ALLTHECODES_WORKFLOW_SCRIPTS`，保持 full-build 默认启用。
- startup 时初始化内置 workflow registry。
- 内置 workflow 与用户 workflow 冲突时需要优先级规则，建议用户项目文件优先。

### P2：DynamicWorkflow 与文件 workflow 的关系

建议分层：

- FileWorkflow 先对齐上游：文件解析为顺序 step run，模型负责执行当前 step，再调用 `advance`。
- DynamicWorkflow 保持 JSON DAG：适合一次性多 agent 编排。
- 后续可以允许 YAML workflow 声明 `kind: dynamic` 或 `stages:`，再编译为 `DynamicWorkflow` plan；这属于 allthecodes 扩展，不是上游 baseline。

## 需要回写到 `01-arch-plan` 的要点

`01-arch-plan.zh.md` 应补一节“与上游 WORKFLOW_SCRIPTS 的关系”：

| 能力 | 上游 Workflow Scripts | 当前 `01-arch-plan` DynamicWorkflow | 需要补充 |
|---|---|---|---|
| 入口 | `.claude/workflows` 文件 + `/workflows` + 动态 slash command | 直接 tool call JSON plan | `.allthecodes/workflows` 文件发现和命令注册 |
| 执行模型 | 顺序 step，模型执行后手动 `advance` | tool 内并发执行 Agent DAG | 两条路径并存 |
| 持久化 | `.claude/workflow-runs/{runId}.json` | 无 run 持久化 | `.allthecodes/workflow-runs/{run_id}.json` |
| DSL | Markdown/YAML 轻量 step | JSON DAG `agent/map/reduce` | 补 Markdown/YAML parser |
| UI | LocalWorkflowTask + detail dialog | 未覆盖 | Rust TUI task 状态与 detail |
| 权限 | WorkflowPermissionRequest | 仅 DynamicWorkflow Ask | file workflow start/advance/cancel 权限语义 |

## 建议实现顺序

0. 先修复当前工作树可编译性：`agent_events.rs` 的自 crate 导入问题需要先处理；随后把 `file_workflow.rs` 接入模块树并修掉借用、warning 和 parser 单测失败。
1. 先实现路径、parser、run record 类型和单元测试，保证不触碰 `.claude`；run record 必须引入独立 `run_id`，不能复用 deterministic `workflow_id` 覆盖同一文件。
2. 扩展 `Workflow` tool 的上游兼容 mode，覆盖 `start/status/advance/cancel/list`；判定逻辑必须支持输入含 `workflow` 或 `run_id` 时进入 file-script mode，同时保留旧 `name/goal/steps` static workflow mode。
3. 增加 `/workflows` 列表命令，确认无文件、坏文件、重名、隐藏文件和内置命令冲突的行为。
4. 按 `.worktrees/workflow-scripts-gap/development/dynamic_workflow/01-arch-plan.zh.md` 的补充，再做 cwd-aware 动态 workflow slash command：`/release args` 读取 `.allthecodes/workflows/release.md`，返回 `CommandResult::Query`，文本等价于 `Execute this workflow:\n\n{content}\n\nArguments: {args}`。
5. 接入 Rust TUI task/detail 状态和 API/WS 可观测性。
6. 最后再考虑把某些 YAML workflow 编译为 `DynamicWorkflow` DAG。

## 验证清单

- `Workflow` file-script mode 在临时项目中只读写 `.allthecodes/workflows` 和 `.allthecodes/workflow-runs`。
- Markdown parser 覆盖 checklist、bullet、numbered。
- YAML parser 覆盖 `name/prompt/run/command`。
- `start` 创建 run 并把第一个 step 设为 `running`。
- `advance` 能推进到完成。
- `cancel` 会取消 running/pending steps。
- `list/status` 在 feature gate 开启时可用且为 read-only。
- `/workflows` 在无文件、有文件、坏文件和重名冲突时行为明确。
- feature gate 关闭时隐藏 `Workflow`、`workflow`、`DynamicWorkflow` 和 workflow commands。
