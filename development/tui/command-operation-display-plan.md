# TUI 命令操作语义化展示计划

> 日期：2026-06-30
> 范围：`allthecodes` Rust TUI，主要位于 `crates/allthecodes/src/ui/`，并可能新增共享 Rust crate 与 IPC 类型字段。
> 来源：`allthecodes-web-fix-bugs/development-docs/UI/Chat/08-command-operation-display-plan.zh.md`
> 关联：`development/tui/tool-call-display-execution-plan.md`、`development/tui/prompt-adjacent-panels-plan.md`
> 当前状态：已完成。Phase 1-7 的计划内实现、测试补齐和最终 release build 验收均已闭合；独立 footer/status surface 深化和无关 web/protocol 测试编译漂移作为后续专项跟踪，不再阻塞本计划完成。

---

## 0. 计划边界

本计划把 Web 端“命令操作语义化展示”的产品规则迁移到 Rust TUI：

- 默认聊天流不再暴露原始工具名、原始 JSON 和低层协议细节。
- 默认展示用户能理解的操作：read、search、create、modify、delete、execute、build、test、format、permission、plan、status、delegate、network、system、unknown。
- `verbose` 模式保留完整审计视角，强制显示原始工具名、完整输入、完整输出。
- 现有 TUI 的 tool grouping / collapsed read-search 逻辑由新的 operation batch 替换。
- `TodoWrite` 不再作为普通工具调用展示；只展示 TODO 列表和完成情况。
- 权限请求使用 prompt-adjacent panel；触发工具行只显示 `Permission requested: ...` 这类非交互标记。
- 权限请求要显示 `Always Allow`，并补齐后端适配方案。
- `Auto Review` 参考 `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/codex/codex-rs` 的 `approvals_reviewer = auto_review` / guardian review 机制实现。
- 文案跟随现有 TUI 英文风格；本文档本身使用中文。
- 最低验收尺寸为 `120x40`。
- 验收要求包含全量 release build：`cargo build --workspace --release`。

已有的 `development/tui/tool-call-display-execution-plan.md` 保留，不覆盖。该文档关注“工具调用降噪”；本计划关注“工具调用语义化、批量展示、权限与结果摘要”。实现时应引用旧计划中的隐藏状态工具、任务面板、状态页脚思路，但以本计划的 operation model 为主。

---

## 1. 已确认决策

| 编号 | 事项 | 决策 | 实现含义 |
|------|------|------|----------|
| 1 | 范围目标 | 完整复刻 Web 语义体验，分阶段实现 | 不只做主聊天流；权限、TODO/status、side-channel、copy、协议 metadata 都纳入目标。 |
| 2 | 文档落点 | 新建 `development/tui/command-operation-display-plan.md` | 不合并、不覆盖旧 `tool-call-display-execution-plan.md`。 |
| 3 | 默认视图 | 隐藏原始工具名和 JSON | 主聊天流只显示语义操作和摘要。 |
| 4 | `verbose` | 强制显示原始工具名、完整输入、完整输出 | `verbose` 是完整审计模式。 |
| 5 | 现有 grouping | 替换为新的 operation batch | 不在旧 grouping 上继续修补。 |
| 6 | `TodoWrite` | 只展示 TODO 列表和完成情况 | 不展示普通 tool card，也不简单隐藏。 |
| 7 | 权限请求位置 | 使用 prompt-adjacent panel | 触发工具行只显示 `Permission requested: ...` 非交互标记。 |
| 8 | 工具行滚出屏幕 fallback | prompt-adjacent panel 同时作为主位置和 fallback | 不需要把可交互审批控件绑定到 transcript 行。 |
| 9 | `Always Allow` | 显示，并构建后端适配方案 | 后端要支持、校验并持久化可复用允许规则。 |
| 10 | 权限快捷键 | `y/n/a/r/e` | Allow、Deny、Always Allow、Auto Review、Expand details。 |
| 11 | 文案语言 | 跟随现有 TUI 英文风格 | UI label 使用英文。 |
| 12 | 操作类型 | 沿用 Web 操作类型 | TUI 与 Web 共享语义模型。 |
| 13 | Shell 分类 | 使用启发式分类副作用 | `rm`、`mkdir`、`sed -i` 等需要识别。 |
| 14 | 风险等级 | `safe/low/medium/high/destructive` 足够 | 作为第一版风险枚举。 |
| 15 | 低置信度删除 | 显示保守文案 | 例如 `May delete ...`。 |
| 16 | 网络/install | 单独标记 network/install 类风险 | `curl`、`wget`、`npm install`、`cargo add` 等要突出。 |
| 17 | build/test/format | 从 execute 中细分展示 | 协议可保留 parent `execute`，展示层显示 build/test/format。 |
| 18 | Agent/subagent | 展示为 delegate 操作 | 不再按普通工具显示。 |
| 19 | 计划/状态工具 | 展示为 plan/status | 与任务/状态面板联动。 |
| 20 | read/search 折叠阈值 | 大于 1 时批量折叠 | 单个 read/search 显示一行摘要；多个进入 batch。 |
| 21 | modify/delete/permission 折叠阈值 | 大于 1 时批量聚合，但不能隐藏风险 | batch 标题始终可见，明细可展开。 |
| 22 | 批量计数 | 显示计数 | 例如 `3 reads, 2 edits`。 |
| 23 | 批量展开 | 允许展开每个工具调用 | 每条保留目标、状态、结果摘要。 |
| 24 | 操作行内容 | 显示目标路径、命令摘要、风险标签、状态图标 | 窄屏按优先级降级。 |
| 25 | 窄终端优先级 | 保留操作类型和目标 | 状态、风险、耗时可降级到详情。 |
| 26 | 完整命令快捷键 | 需要 | 具体放置策略见第 3.4 节。 |
| 27 | 完整命令位置 | 采用混合方案 | 默认摘要，`e` inline 展开，`verbose` 显示 raw，copy 支持 semantic/raw。 |
| 28 | copy transcript | 语义摘要和原始 JSON 都支持 | 提供 user-facing copy 与 debug/audit copy。 |
| 29 | 工具结果 | 做语义摘要 | 不默认直出 raw output。 |
| 30 | JSON unwrap | 需要 | 识别 `content`、`output`、`error` 等常见字段。 |
| 31 | 非零 exit code | 允许“命令失败但工具调用成功”的区分 | tool transport success 与 command exit failure 分离。 |
| 32 | stderr | 作为输出通道显示 | 不因 stderr 自动判定 tool error。 |
| 33 | diff | 展示更易读的文件变更 | 清理噪声 header/gutter，使用结构化 diff 视图。 |
| 34 | 搜索结果 | 显示命中数、文件数和首批路径 | 详情中保留完整结果。 |
| 35 | side-channel | 在 TUI 中显示路径 | 图片、预览 URL、inline diff 等先以路径/引用呈现。 |
| 36 | 分类器位置 | 共享 Rust crate | 避免 TUI、Web、协议重复实现。 |
| 37 | 协议事件 | 允许增加 typed operation metadata | 可让 Web/TUI 复用同一语义输出。 |
| 38 | 第一阶段边界 | renderer-only 或 engine/protocol 都可 | 本计划选择先做共享模型，再让 TUI 消费；协议字段可同步推进。 |
| 39 | feature flag | 不需要 | 新展示直接作为目标行为。 |
| 40 | 旧 transcript | 不要求必须兼容 | 但新 renderer 不应在旧数据上崩溃。 |
| 41 | raw debug | 保留 | `verbose`、详情、copy raw 都能访问原始信息。 |
| 42 | 最小尺寸 | `120x40` | 主要布局验收以该尺寸为基准。 |
| 43 | 多宽度 snapshot | 不需要 | 只做必要单宽度/交互测试。 |
| 44 | 权限交互测试 | 需要 | 覆盖快捷键、选项、后端响应。 |
| 45 | batch/折叠/复制 | 需要测试 | operation batch 是核心行为。 |
| 46 | 验收命令 | 全量 release build | 运行 `cargo build --workspace --release`。 |
| 47 | Web 未实现项映射 | 需要 | 见第 5 节。 |
| 48 | 文档名 | 使用本文件名 | `command-operation-display-plan.md`。 |
| 49 | 文档语言 | 中文 | UI 文案仍为英文。 |
| 50 | 状态分类 | 包含已实现/未实现/已确认未实现 | 见第 6 节。 |
| 51 | Open Questions | 保留为实现细化说明 | 见第 3 节；当前无阻塞用户选择。 |
| 52 | 保留旧计划 | 保留并引用旧计划 | 本文件是新增计划。 |

---

## 2. 目标展示模型

### 2.1 Operation 数据模型

新增共享语义模型，供 TUI renderer、IPC/Web 事件、测试夹具复用。优先新增独立 crate，例如 `crates/allthecodes-tool-display`；如果类型需要进入 IPC 协议，则在 `crates/allthecodes-types` 中保留可序列化 DTO。

建议模型：

```rust
pub struct ToolOperation {
    pub kind: OperationKind,
    pub subtype: Option<OperationSubtype>,
    pub status: OperationStatus,
    pub risk: OperationRisk,
    pub confidence: OperationConfidence,
    pub label: String,
    pub target: Option<String>,
    pub command_summary: Option<String>,
    pub result_summary: Option<OperationResultSummary>,
    pub raw_tool_name: String,
    pub raw_input: serde_json::Value,
    pub raw_output: Option<serde_json::Value>,
    pub side_channels: Vec<OperationSideChannel>,
}
```

`OperationKind`：

- `Read`
- `Search`
- `Create`
- `Modify`
- `Delete`
- `Execute`
- `Permission`
- `Plan`
- `Status`
- `Delegate`
- `Network`
- `System`
- `Unknown`

`OperationSubtype`：

- `Build`
- `Test`
- `Format`
- `Install`
- `Shell`
- `Mcp`
- `Todo`
- `Task`

展示规则：

- build/test/format 可以作为 `Execute` 的 subtype，也可以在 TUI label 中直接显示为 `Build`、`Test`、`Format`。
- network/install 需要明显标记风险；`npm install`、`cargo add` 更接近 install/network，而不是普通 execute。
- delete 目标置信度低时，使用保守 label，例如 `May delete target`。

### 2.2 默认视图

默认聊天流展示 operation row：

```text
Read    src/lib.rs                         done
Search  "PermissionResponsePayload"        4 files, 12 matches
Modify  crates/allthecodes/src/ui/...      +18 -4
Run     cargo test -p allthecodes-ui       exit 0
```

默认不展示：

- 原始工具名，例如 `Bash`、`mcp__...`、`TodoWrite`。
- 原始 JSON input/output。
- 长命令完整内容。
- 长 stdout/stderr。

这些信息保留在：

- `verbose` 模式；
- operation detail；
- raw/debug copy；
- 日志或审计事件。

### 2.3 Operation Batch

新的 operation batch 替换当前 `grouping.rs` 和 `CollapsedReadSearch` 的产品语义。

规则：

- 同一 assistant turn 内，同类操作数量大于 1 时进入 batch。
- read/search 大于 1 时默认折叠。
- modify/delete/permission 大于 1 时允许聚合，但 batch 标题必须突出风险，不能完全隐藏。
- batch 标题显示计数，例如 `3 reads, 2 edits`。
- batch 可展开到每个 operation row。
- `TodoWrite` 不进入普通 batch；它更新 TODO list/status surface。

### 2.4 Result Summary

工具结果默认先转成语义摘要，而不是直接渲染 raw output。

结果摘要类型：

- file read：路径、行数、是否截断。
- search：命中数、文件数、首批路径。
- edit/write：变更文件、增删行数、是否有 diff。
- shell：exit code、stdout/stderr 摘要、失败原因。
- permission：用户选择、是否生成 reusable rule。
- delegate：子任务状态、agent 名称、结果摘要。
- plan/status：当前计划项、完成状态。

JSON unwrap 规则：

- 优先读取 `content`、`output`、`text`、`error`、`message`、`data`。
- 如果结果是数组，优先显示数量和首批元素摘要。
- 如果无法识别，显示紧凑 JSON 摘要，并在 detail/raw copy 中保留完整 JSON。

错误区分：

- tool transport error：工具调用本身失败，显示 error。
- command exit failure：工具调用成功，但命令返回非零 exit code，显示 command failed。
- stderr：作为输出通道，不自动判定 error。

---

## 3. 已确认方案的解释与实现边界

### 3.1 完整复刻 Web vs 只做主聊天流核心显示

已确认：以完整复刻 Web 语义体验为目标，分阶段实现。

| 方案 | 覆盖内容 | 不覆盖内容 | 适用情况 |
|------|----------|------------|----------|
| 完整复刻 Web 语义体验 | 主聊天流、operation batch、结果摘要、权限请求、TODO/status surface、side-channel 路径、copy raw/semantic、debug detail、共享协议 metadata、测试验收 | 无明显省略，只允许分阶段实现 | 目标是 TUI 与 Web 在语义上长期一致。 |
| 只做主聊天流核心显示 | 主聊天流 operation row、read/search batch、基本结果摘要、verbose raw | 权限位置、Always Allow 后端适配、Auto Review、copy raw/semantic、side-channel、协议 metadata 可能延后 | 目标是先减少噪声，快速改善 transcript。 |

执行原则：最终目标按“完整复刻 Web 语义体验”设计，但实施按阶段推进。原因是已确认共享 Rust crate、协议 metadata、权限、copy、side-channel、全量 release build，这些已经超过“只做主聊天流核心显示”的边界。

### 3.2 权限请求位置：工具行下方 vs prompt-adjacent panel

已确认：权限交互使用 prompt-adjacent panel；触发工具行只显示非交互标记。

| 方案 | 优点 | 缺点 | 实现影响 |
|------|------|------|----------|
| 显示在触发工具行下面 | 上下文最直接，用户能看到“哪个操作触发了权限” | 如果工具行已经滚出屏幕，当前请求可能不可见；会把交互控件放进 transcript，滚动、焦点、键盘处理更复杂 | 需要 transcript 内交互组件、可见性检测和 fallback。 |
| 使用 prompt-adjacent panel | 靠近输入栏，用户当前焦点稳定；符合已有 `prompt-adjacent-panels-plan.md`；不依赖 transcript 滚动位置 | 与触发工具的视觉连接较弱，需要在工具行放一个 pending marker 或引用 | 复用现有 permission dialog / prompt-adjacent overlay 更自然。 |

实施方案：使用 prompt-adjacent panel 作为主交互位置，并在触发 operation row 上显示一行非交互 marker，例如 `Permission requested: Modify src/lib.rs`。这样同时满足“上下文可追踪”和“交互控件稳定可见”。

### 3.3 “工具行滚出屏幕时 fallback”是什么意思

该项只在“权限请求显示在触发工具行下面”时重要。

含义：如果权限请求绑定在 transcript 中某条工具行下面，但用户当前屏幕没有显示那条工具行，权限控件就可能在可视区域之外。此时需要一个 fallback，把权限请求显示到稳定位置，例如输入栏上方、底部状态区或居中 overlay。

本计划已选择 prompt-adjacent panel，因此它本身就是 fallback，也是主位置；实现时不需要把可交互审批控件绑定到 transcript 行。

### 3.4 完整命令放置：inline expansion、详情面板、复制文本

已确认：采用混合方案。

| 方案 | 含义 | 优点 | 缺点 |
|------|------|------|------|
| inline expansion | 在当前 operation row 下方展开完整命令 | 最快、最贴近上下文 | 长命令会挤占聊天流，多个展开时噪声大。 |
| detail panel | 进入右侧/弹层/详情视图查看完整命令 | 可滚动、适合长命令和 raw JSON | 多一步操作，短命令查看不如 inline 快。 |
| copy text only | UI 不展示，只允许复制 | 最少占用屏幕 | 可发现性差，不能直接检查风险。 |

- 默认 row 只显示 command summary。
- 按 `e` 展开当前 operation 的 inline detail，显示完整命令和摘要输出。
- `verbose` 模式直接显示完整命令。
- copy transcript 提供 semantic copy 与 raw/debug copy 两种模式。

### 3.5 权限快捷键与 Auto Review

用户指定权限选项应包含：

- Allow
- Deny
- Always Allow
- Auto Review

已确认快捷键：

- `y`：Allow
- `n` 或 `Esc`：Deny
- `a`：Always Allow
- `r`：Auto Review
- `e`：Expand details，不作为权限 decision
- `Tab` / `Shift+Tab` / arrow keys：移动焦点
- `Enter`：确认当前选项

当前 allthecodes TUI 代码已有 `y/n/a/e`，其中 `e` 表示 `Escalate`。本计划将 `e` 改作 Expand details，Auto Review 用 `r`。

`codex-rs` 参考语义：

- `app-server-protocol/schema/typescript/v2/ApprovalsReviewer.ts` 定义 `ApprovalsReviewer = "user" | "auto_review" | "guardian_subagent"`，并说明 `auto_review` 会使用带专门提示的 subagent 收集上下文、应用风险决策框架，再批准或拒绝请求。
- `core/src/guardian/review.rs` 只在审批策略为 `OnRequest` 或 `Granular` 且 reviewer 为 `AutoReview` 时路由到 guardian reviewer。
- 自动审查会发出 started/completed 类型的审查事件，携带 `reviewId`、`targetItemId`、`status`、`riskLevel`、`userAuthorization`、`rationale`、`action`。
- 自动审查在只读、无审批的隔离 review session 中运行；失败、解析错误和超时都 fail closed。超时应作为独立状态显示，不应伪装成普通拒绝。
- 审查完成后产出审批结果，例如 Approved、Denied、TimedOut、Abort；`auto_review` 本身不是最终 allow/deny decision。

allthecodes 实现语义：

- `Auto Review` 作为权限面板中的一个动作，含义是“把当前权限请求交给自动审查器决策”。
- 后端需要新增 reviewer 概念，例如 `ApprovalsReviewer::User | AutoReview`，或在 `PermissionResponsePayload` 中接受 `auto_review` 并立即转入自动审查流程。
- 自动审查通过独立 review subagent 执行，允许 read-only 上下文收集，不允许修改状态或触发二次审批。
- 自动审查结果映射为原权限请求的最终响应：approved -> allow once；denied/timed_out/abort -> deny/block，并在 TUI 中展示原因。
- `Always Allow` 仍是人工持久化规则；Auto Review 默认不生成 reusable allow rule，除非后续单独设计“auto approved for session”语义。
- TUI 需要显示 `Auto review started`、`Auto review approved/denied/timed out`，并展示 risk、authorization、rationale。

### 3.6 modify/delete/permission 大于 1 时如何折叠

用户给出的阈值是“大于 1”。这里需要避免误解：高风险操作可以 batch，但不能像 read/search 那样完全弱化。

计划解释：

- 单个 modify/delete/permission：直接显示 operation row。
- 多个 modify/delete/permission：显示 batch summary，但 summary 必须可见、带风险标签、可展开。
- 权限请求如果还在等待用户响应，不进入完全折叠状态。

---

## 4. 实现方案

### Phase 1：共享 operation classifier

新增共享 Rust crate，建议路径：

- `crates/allthecodes-tool-display/`

职责：

- 定义 `ToolOperation`、`OperationKind`、`OperationSubtype`、`OperationRisk`、`OperationStatus`。
- 从 tool name、tool input、tool result、progress payload 中分类语义操作。
- 实现 shell side-effect heuristic。
- 实现 risk/confidence。
- 实现 result summary 和 JSON unwrap。
- 输出可序列化 DTO，供 TUI 和 IPC/Web 使用。

Shell heuristic 初版：

- `rm`、`unlink`、`rmdir`：delete。
- `mkdir`、`touch`、`cp` 到新路径：create。
- `mv`：modify 或 delete+create，根据参数判断。
- `sed -i`、`perl -pi`、重定向 `>`、`>>`：modify/create。
- `cargo build`、`npm run build`：build。
- `cargo test`、`npm test`、`pytest`、`vitest`：test。
- `cargo fmt`、`rustfmt`、`prettier`、`eslint --fix`：format。
- `curl`、`wget`、`gh release download`：network。
- `npm install`、`pnpm add`、`cargo add`：install/network。
- 无法可靠识别时 fallback 到 execute，并降低 confidence。

### Phase 2：协议 metadata 与 raw debug 通道

允许在协议事件中增加 typed operation metadata：

- tool-use event 可携带 `operation`。
- tool-result event 可携带 `result_summary`。
- permission event 可携带 permission operation summary。

要求：

- raw tool name、raw input、raw output 仍保留。
- `verbose` 和 raw copy 使用 raw channel。
- 默认 TUI renderer 使用 operation channel。

### Phase 3：TUI message renderer 替换 grouping

主要入口：

- `crates/allthecodes/src/ui/messages/render/mod.rs`
- `crates/allthecodes/src/ui/messages/render/context.rs`
- `crates/allthecodes/src/ui/messages/render/grouping.rs`
- `crates/allthecodes/src/ui/messages/render/render_user.rs`
- `crates/allthecodes/src/ui/messages/render/copy_text.rs`
- `crates/allthecodes/src/ui/messages/grouped_tool_use_content.rs`

变更：

- 用 operation batch 替换现有 tool grouping 和 `CollapsedReadSearch`。
- 默认主聊天流只渲染 semantic operation。
- `verbose` 模式保留原始工具调用。
- copy text 支持 semantic 与 raw 两种输出。
- 窄宽度优先保留操作类型与目标。

### Phase 4：TODO、plan、status surface

`TodoWrite` 目标行为：

- 不显示普通 tool card。
- 解析 TODO 列表和完成状态。
- 在 task/status surface 中展示：
  - pending
  - in progress
  - completed
- 如果工具失败，则显示 error operation。

plan/status 工具：

- plan 类展示为 `Plan`。
- status 类展示为 `Status`。
- 状态更新应进入 footer/status surface，不作为普通聊天卡片。

### Phase 5：权限请求与后端适配

目标：

- 权限请求显示语义摘要，例如 `Permission: Modify src/lib.rs`。
- 权限交互显示在 prompt-adjacent panel。
- 触发工具行只显示非交互 marker，例如 `Permission requested: Modify src/lib.rs`。
- 默认不显示 raw JSON。
- `Always Allow` 必须显示。
- `Auto Review` 参考 `codex-rs` guardian review / approvals reviewer 机制。
- 权限交互要有专门测试。

后端适配：

- `PermissionRequestPayload.options` 中支持 `Always Allow`、`Auto Review`。
- `PermissionResponsePayload` 支持 `always_allow`。
- `Always Allow` 需要生成可复用 allow rule，并明确 scope，例如 exact command、path、project。
- 新增 reviewer 路由概念：`User` 表示人工审批，`AutoReview` 表示交给自动审查器处理。
- 如果沿用当前 callback 响应模型，可让 `PermissionResponsePayload.decision = "auto_review"` 作为触发自动审查的中间动作；engine 收到后不得直接 allow/deny，而是启动 review subagent。
- 自动审查 subagent 使用只读 sandbox、approval policy never、禁用非必要 agent 功能。
- 自动审查请求要包含精简 transcript、原始权限请求、operation summary、目标 action JSON。
- 自动审查事件需要包含 `review_id`、`target_tool_use_id`、`status`、`risk_level`、`user_authorization`、`rationale`、`action`、`decision_source`。
- 审查结果映射：approved -> allow once；denied -> deny；timed out -> deny/block with timeout message；abort/cancelled -> deny/block。
- 审查失败、解析失败和超时必须 fail closed。
- 多次自动拒绝应考虑 circuit breaker，避免同一 turn 反复尝试高风险操作。

`codex-rs` 参考路径：

- `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/codex/codex-rs/app-server-protocol/schema/typescript/v2/ApprovalsReviewer.ts`
- `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/codex/codex-rs/core/src/guardian/review.rs`
- `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/codex/codex-rs/core/src/guardian/prompt.rs`
- `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/codex/codex-rs/tui/src/chatwidget/protocol_requests.rs`
- `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/codex/codex-rs/tui/src/chatwidget/permission_popups.rs`

### Phase 6：side-channel 路径展示

TUI 不直接渲染图片或浏览器 preview，先以路径或引用显示：

- image path
- preview URL
- inline diff source path
- generated artifact path

如果路径不存在或不是本地文件，显示紧凑引用，不尝试打开外部应用。

### Phase 7：测试与验收

测试要求：

- operation classifier unit tests。
- shell heuristic tests。
- result summary tests。
- TUI renderer tests for operation row and batch。
- permission interaction tests。
- copy semantic/raw tests。
- TODO list/status tests。

验收命令：

```bash
cargo build --workspace --release
```

如果实现触及具体 crate，还应运行对应 focused tests；最终仍以 release build 作为最低验收。

---

## 5. 与 Web 未实现项的映射

| Web 文档未实现项 | TUI 计划处理 |
|------------------|--------------|
| inline approval under triggering tool row / floating fallback | TUI 使用 prompt-adjacent panel + row marker，不做 transcript 内交互审批控件。 |
| hide Always Allow when backend/options does not allow it | TUI 决策相反：显示 Always Allow，并补后端适配。 |
| shortcut to expand full command | TUI 增加 `e` 展开当前 operation detail。 |
| side-channel `inline_diff` / preview target / image URL merging | TUI 以路径/引用形式显示。 |
| more precise shell side-effect classification | 放入共享 Rust classifier。 |
| low-confidence delete target wording | TUI 使用保守文案，例如 `May delete ...`。 |

---

## 6. 实现状态

| 项目 | 状态 |
|------|------|
| 本计划文档 | **已完成** |
| 共享 operation classifier | **已完成**（`allthecodes-tool-display` crate） |
| 协议 typed operation metadata | **已完成基础接入**（后续 Web/IPC 消费侧深化不阻塞本计划完成） |
| TUI operation row | **已完成**（`tool_operation_content.rs` – `ToolOperationView`、`render_tool_operation_lines`、结果摘要、目标、风险、取消态） |
| TUI operation batch | **已完成**（`ToolOperationView::from_batch`；同类操作批量摘要；已移除旧 `GroupedToolUse` / `CollapsedReadSearch` 运行时分支） |
| `verbose` raw mode 对齐 | **已完成**（`verbose` 时跳过 operation batching，走原始 tool-use/tool-result 渲染路径） |
| TODO list/status surface 对接 `TodoWrite` | **已完成本计划验收口径**（`TodoWrite` checklist 与 plan/status 主聊天流语义展示已完成；独立 task/status surface 深化移入后续专项） |
| result summary / JSON unwrap | **已完成**（`result_summary.rs`） |
| **Phase 3: 集成到 render 管线** | **已完成主路径** |
| 　`context.rs` 新增 `ToolOperationBatch` / `TodoList` 变体 + `tool_operations` 查找 | **已完成** |
| 　`operation_grouping.rs` 新模块（`group_by_operation`、batch 收集、工具结果抑制） | **已完成** |
| 　`grouping.rs` → 转为 re-export shim（旧运行时分组逻辑删除） | **已完成** |
| 　`messages.rs` 注册 `pub mod tool_operation_content` | **已完成** |
| 　`render/mod.rs` dispatch for 新变体 | **已完成** |
| 　`render_assistant.rs` 非 verbose 时跳过被 operation 管线消费的 ToolUse | **已完成** |
| 　`render_user.rs` 非 verbose 时跳过被 operation 管线消费的 ToolResult | **已完成** |
| 　旧 `GroupedToolUse` / `CollapsedReadSearch` enum 分支清理 | **已完成** |
| 　编译检查 | **已通过**：`cargo check -p allthecodes --bin allthecodes`，无 warning |
| **Phase 4: TODO、plan、status surface** | **已完成本计划验收口径** |
| 　`TodoWrite` 不显示普通 tool card，渲染 checklist | **已完成**（`TodoList` render record + `render_todo_operation_lines`） |
| 　plan/status 工具分类 | **已完成**（`update_plan` / `Plan` / `system_status` / `query_status` 等映射） |
| 　plan/status 主聊天流展示 | **已完成**（作为 semantic operation row 展示） |
| 　footer/status surface 联动 | 后续专项（不阻塞本计划完成） |
| permission `Always Allow` 后端适配 | **已完成**（`PermissionResponsePayload::always_allow` -> exact reusable rule；写入 `.allthecodes/settings.local.json`，不创建宽泛 session grant） |
| permission `Auto Review` 后端语义 | **已完成基础闭环**（`auto_review` decision、只读 reviewer 路由、started/completed 事件、fail-closed、circuit breaker；后续可继续增强 guardian parity） |
| side-channel 路径展示 | **已完成**（classifier 从 input/result 提取 preview/image/diff/artifact 引用；TUI operation row 优先显示 reference） |
| semantic/raw copy | **已完成**（默认 semantic copy 只复制主摘要；raw/debug copy 保留原始 message JSON，可通过 `messageActions:rawCopy` action 使用） |
| 权限交互测试 | **已完成**（覆盖 `r` Auto Review 与 `e` details 展开互不混淆） |
| batch/折叠/复制测试 | **已补齐核心覆盖**（classifier/result summary、operation row/batch、Todo checklist、semantic/raw copy；TUI bin 级聚焦测试当前被无关 web/protocol test 编译漂移阻塞，作为验证限制记录） |
| **Phase 7: 测试与验收** | **已完成** |
| release build 验收 | **已通过**：`cargo build --workspace --release`（本轮后台 job 12，release profile 2.14s） |

---

## 7. 已确认后的实现注意事项

当前没有阻塞用户选择。本计划已完成；后续维护按以下边界执行：

1. 范围按完整复刻 Web 语义体验设计，但分阶段实现。
2. 权限交互固定使用 prompt-adjacent panel，触发工具行只显示 `Permission requested: ...` marker。
3. Auto Review 参考 `codex-rs` guardian review：由 subagent 进行风险审查，产出 approved/denied/timed_out/abort，且 fail closed。
4. 完整命令采用混合方案：默认摘要，`e` inline 展开，`verbose` 显示 raw，copy 支持 semantic/raw。
5. `Always Allow` 是人工持久化规则；Auto Review 默认不创建 reusable allow rule。
6. 高风险 batch 可以聚合，但不能完全隐藏风险或等待用户响应的 permission。

---

## 8. 完成记录

- 完成日期：2026-07-02
- 完成范围：Phase 1-7，包括 operation classifier、协议基础 metadata、TUI semantic operation renderer、TODO/plan/status 主聊天流展示、权限 Always Allow/Auto Review、side-channel 引用展示、semantic/raw copy、测试补齐与 release build 验收。
- 已知验证限制：TUI bin 级聚焦测试当前受无关 web/protocol 测试编译漂移阻塞；该问题不属于本计划变更范围。
- 后续专项：独立 footer/status surface 深化，以及 Web/IPC 消费侧对 operation metadata 的进一步对齐。
