# 权限治理当前实现情况

日期：2026-07-02

范围：权限模式、规则来源、hook、auto review、sandbox allow、工具执行前安全校验、交互式授权和 Web/TUI 表面。

## 总体状态

权限治理已经集中到 `allthecodes-permissions` 的决策引擎，并由 `QueryEngine` 工具执行层统一调用。当前实现包括：

- 多种 permission mode。
- 持久 allow/deny/ask 规则。
- session-level grants。
- hook permission decision。
- auto review/classifier。
- dangerous command 检测。
- 文件路径边界检查。
- Plan mode 工具限制。
- sandbox allowed commands 集成。
- TUI/Web 交互式授权。

权限治理不是只靠 UI 弹窗。工具执行前会先经过安全校验和中心化权限决策，再决定允许、拒绝或询问用户。

## 权限上下文

核心类型在 `crates/allthecodes-types/src/permissions.rs`。

当前 `PermissionMode` 包含：

- `Default`
- `Auto`
- `Bypass`
- `Plan`
- `AcceptEdits`
- `DontAsk`

`ToolPermissionContext` 记录运行时权限上下文，主要包含：

- 当前 mode。
- additional working directories。
- always allow rules。
- always deny rules。
- always ask rules。
- session allow rules。
- Auto mode 下被剥离的持久规则和 session 规则。
- bypass/auto 可用性。
- pre-plan mode。

启动阶段在 `crates/allthecodes/src/full_init.rs` 中解析 CLI/settings，构造权限上下文。`--no-network` 会影响 sandbox network 配置。settings map 会记录 permission mode 和 sandbox 的来源。

## 决策顺序

核心决策在 `crates/allthecodes-permissions/src/decision.rs`。

当前常规顺序是：

1. hook deny。
2. deny/ask/allow 规则匹配。
3. hook ask/allow。
4. session-level grant。
5. mode fallback。

规则语义是 deny 优先，其次 ask，再其次 allow。规则来源包括 managed、project、local、user、CLI、session；显示时有来源优先级，但决策时主要按规则语义处理。

mode fallback 当前行为：

- `Default`：询问用户。
- `Plan`：询问用户，并额外限制非只读工具。
- `Auto`：无 classifier 时默认允许；有 classifier 时按 classifier verdict。
- `Bypass`：直接允许。
- `AcceptEdits`：自动允许文件编辑类工具，其他工具询问。
- `DontAsk`：静默拒绝。

在 `crates/allthecodes-engine/src/lifecycle/deps/permission.rs` 中还有运行时整合逻辑：

- Plan mode 下允许写计划文件。
- 普通 Ask 结果在满足 sandbox allowed command 时可以降级为 allow，原因标记为 `sandbox_allowed_command`。
- verbose 模式下可以发出 permission decision debug event。
- hook decision、auto review、拒绝消息和反馈消息都在这里转换成引擎事件。

## 工具执行前安全校验

工具运行前安全校验在 `crates/allthecodes-engine/src/tool_runtime/execution/security.rs`。

当前检查包括：

- `Bypass` mode 跳过安全校验。
- `Plan` mode 阻止非只读工具，计划文件写入例外。
- Bash/PowerShell 执行前做 dangerous command 检测。
- Write/Edit/FileWrite/FileEdit 等文件工具做路径校验。
- 文件路径不允许 traversal/null 等非法输入。
- 写入路径必须位于 cwd 或 additional working directories 内。
- sandbox `allowedCommands` 只在 workspace sandbox 场景下作为预批准来源，不适用于 `dangerouslyDisableSandbox`。

这层校验发生在具体工具执行前，用于补齐工具声明和权限规则之外的安全边界。

## Auto Mode 安全处理

Auto mode 的危险规则处理在 `crates/allthecodes-permissions/src/dangerous/auto_mode.rs`。

进入 Auto mode 时会剥离危险的 allow/session 规则，离开 Auto mode 时再恢复。当前被视为危险的规则包括：

- Agent 类 blanket allow。
- shell blanket allow。
- shell 代码执行。
- shell 提权。
- PowerShell 代码执行。
- PowerShell 提权。

测试覆盖了如 `Bash`、`PowerShell(*)`、`Agent(*)`、`Bash(prefix:python)`、`Bash(prefix:npm)`、`ssh`、`sudo`、`PowerShell(Invoke-Expression*)` 等危险规则剥离；也覆盖保留较窄规则，如 `Bash(cargo test*)`、`Bash(prefix:git)`、`Read`、`PowerShell(Get-ChildItem*)`。

Auto classifier 会返回 allow/deny/ask verdict，并携带 reason、model、stage、thinking、unavailable、transcript_too_long 等调试信息。Denial tracker 会在连续或累计拒绝过多时触发 fallback。

## 交互式授权与事件

交互式回调定义在 `crates/allthecodes-types/src/callbacks.rs`。

权限请求 payload 包含：

- `tool_use_id`
- `tool_name`
- `input`
- `message`
- `options`
- `operation`

响应支持：

- allow
- deny
- always_allow
- auto_review
- optional feedback

权限事件类型在 `crates/allthecodes-types/src/permission_events.rs`，包括：

- hook permission decision event。
- permission decision debug event。
- permission auto review event。

TUI 权限展示位于 `crates/allthecodes/src/ui/permissions/`，已经按 Bash、PowerShell、文件写入/编辑、notebook、web fetch、computer use、sandbox、skill、ask-user、rules 等工具类型提供专门渲染。

Web 侧已有权限响应入口，例如 `/api/chat/permissions/{tool_use_id}/response`，并有 admin settings patch 可以切换 permission mode。切换 Auto mode 时会检查 `permissions.enableAutoMode` 并走安全处理逻辑。

## 当前边界与待补齐点

- `Bypass` 当前是强允许路径，会跳过常规安全校验和规则评估，应只在明确受信任场景使用。
- 权限 debug event 主要在 verbose 场景暴露，不是默认审计输出。
- auto classifier 可能不可用，当前会按 fallback 行为处理。
- sandbox allowed command 是执行前权限整合的一部分，但不会覆盖 deny/ask 规则的优先处理。
- durable record/replay 已能记录 permission request/response，但工具完成记录还没有统一内嵌最终权限结论。
- 新会话会清空 session-level grants；持久 allow/deny/ask 规则仍来自配置文件，不随会话自动隔离。
