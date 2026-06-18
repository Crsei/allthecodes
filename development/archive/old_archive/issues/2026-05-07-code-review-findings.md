# 2026-05-07 代码审查问题汇总

本文整理 2026-05-07 发现的代码审查问题。严重程度按“严重 / 高 / 中 / 低”归类；结论使用中文说明，并保留原审查里的 `REQUEST CHANGES`、`BLOCK`、`WATCH` 等状态标识，便于和后续 PR 或提交记录对照。

---

## 一、工具实现阶段问题

审查范围：根据 `.omx/plans/tools-implementation-phase-plan.md` 生成的代码。

审查结论：要求修改（`REQUEST CHANGES`）

架构状态：阻塞（`BLOCK`）

### 严重：`plugins/tools.rs` 当前无法编译

位置：`crates/claude-code-rs/src/plugins/tools.rs:82`

问题：`match permission_ctx.mode` 部分移动了 `permission_ctx.mode`，随后代码又在调用 `set_permission_mode_with_auto_mode_safety()` 时借用了 `permission_ctx`，触发 Rust 所有权错误。

证据：

```text
error[E0382]: borrow of partially moved value: `permission_ctx`
  --> crates\claude-code-rs\src\plugins\tools.rs:87:17
   |
84 |                 other => other,
   |                 ----- value partially moved here
...
87 |                 &mut permission_ctx,
   |                 ^^^^^^^^^^^^^^^^^^^ value borrowed here after partial move
```

风险：`claude-code-rs` 无法完成构建，也无法运行对应目标测试。

建议：对 `&permission_ctx.mode` 做匹配，或在匹配前 clone 出 mode，再把拥有所有权的 `PermissionMode` 传入 `set_permission_mode_with_auto_mode_safety()`。

### 高：Opus 4.7 的 Bedrock 模型映射无效

位置：

- `crates/claude-code-rs/src/api/model_mapping.rs:26`
- `docs/cloud-providers.md:67`

问题：`claude-opus-4-7` 被映射为 `us.anthropic.claude-opus-4-7-v1`，但 AWS Bedrock 文档中的 Opus 4.7 模型 ID 带日期和 `:0` 后缀，例如 `us.anthropic.claude-opus-4-7-20260125-v1:0`。

风险：使用 `claude-opus-4-7` 发起 Bedrock 请求时，会路由到不存在的模型 ID。

建议：把 Rust 映射和文档都改为官方 Bedrock ID；同时确认 Vertex 映射仍与对应 provider 文档一致。

参考：`https://docs.aws.amazon.com/bedrock/latest/userguide/model-card-anthropic-claude-opus-4-7.html`

### 高：旧模型别名被移除但没有兼容路径

位置：`crates/claude-code-rs/src/model_registry.rs:40`

问题：`opus`、`sonnet`、`haiku` 不再作为别名解析。当前实现会把它们当成原始模型 ID，但现有文档和配置示例仍把它们描述为有效别名。

风险：

- 现有 `availableModels: ["sonnet", "haiku"]` 配置不再按文档行为工作。
- `/model sonnet` 可能把当前模型设置为字面量 `sonnet`。
- `docs/claude-code-configuration/` 下导入的 Claude Code 文档会继续和运行时行为不一致。

建议：保留 `opus`、`sonnet`、`haiku` 作为兼容别名；如果决定移除，则需要同时迁移配置与文档表面，并拒绝把这些旧别名字面量直接发送给 provider。

### 中：`/model` 命令参考在别名调整后过期

位置：`development/archive/COMMAND_REFERENCE.md:101`

问题：文档仍说明 `/model` 支持以下别名：

- `opus -> claude-opus-4-20250514`
- `sonnet -> claude-sonnet-4-20250514`
- `haiku -> claude-haiku-3-5-20241022`

风险：用户会按照文档输入已经不再按别名工作的命令。

建议：要么恢复这些别名，要么一次性更新命令参考、CLI 用法示例和配置文档。

### 验证记录

已执行：

```text
cargo test -p claude-code-rs model_mapping
cargo test -p cc-permissions auto_mode_runtime_transition -- --nocapture
cargo test -p cc-permissions active_auto_mode -- --nocapture
git diff --check
```

结果：

- `cargo test -p claude-code-rs model_mapping` 在进入目标测试前失败，原因是 `plugins/tools.rs` 无法编译。
- `cargo test -p cc-permissions auto_mode_runtime_transition -- --nocapture` 通过。
- `cargo test -p cc-permissions active_auto_mode -- --nocapture` 通过。
- `git diff --check` 未发现空白错误，只输出 CRLF 转换 warning。

---

## 二、代理实现映射第 6 阶段问题

审查范围：`architecture/agent-implementation-map.md` 中第 6 阶段声称覆盖的 coordinator / team / tasks、PR webhook、worktree hook 相关实现与测试。

审查结论：要求修改（`REQUEST CHANGES`）

### 高：`phase6.rs` 未纳入版本控制会导致 e2e 测试编译失败

位置：`crates/claude-code-rs/tests/e2e_terminal/main.rs:27`

问题：`main.rs` 已新增 `mod phase6;`，但 `crates/claude-code-rs/tests/e2e_terminal/phase6.rs` 当前仍是 untracked 文件。

风险：如果只提交已跟踪文件，`cargo test --test e2e_terminal` 会因为找不到 `phase6` 模块而失败。

建议：将 `phase6.rs` 加入 git；如果暂时不准备提交该测试，则先移除 `mod phase6;`。

### 中：`WorktreeRemove` hook 收到的 `git_root` 可能不是仓库根目录

位置：`crates/claude-code-rs/src/tools/worktree.rs:542`

问题：`EnterWorktree` 创建时会计算真实 `git_root`，但 `WorktreeSession` 只保存 `original_cwd`；退出时把 `original_cwd` 当成 `git_root` 传给 `run_worktree_remove_hook`。

风险：用户从仓库子目录进入 worktree 时，remove hook 得到的是子目录而不是仓库根目录，可能破坏依赖仓库根路径的 hook 行为。

建议：在 `WorktreeSession` 中保存创建时的 `git_root`，或退出时重新调用 `find_git_root(&original_cwd)` 后再传给 hook。

### 中：worktree 路径边界校验没有处理 symlink / Windows junction 逃逸

位置：`crates/claude-code-rs/src/worktree_hooks.rs:61`

问题：`is_allowed_worktree_path()` 只做语法层面的 `absolute_path(...).starts_with(root)`，没有 canonicalize，也没有检查路径组件中的 symlink / Windows junction。

风险：hook 返回的路径看起来位于 `{CC_RUST_HOME}/worktrees` 下，但实际可能通过链接指向隔离根之外；后续 remove 路径属于潜在破坏性操作。

建议：canonicalize worktree root 和最近存在的父目录，或显式拒绝路径中的 symlink / junction 组件；校验通过后再允许 hook-created / hook-removed worktree。

### 低：e2e 测试入口文件存在编码污染

位置：

- `crates/claude-code-rs/tests/e2e_terminal/main.rs:1`
- `crates/claude-code-rs/tests/e2e_terminal/main.rs:8`

问题：文件开头出现 UTF-8 BOM，原注释里的破折号被写成 mojibake。

风险：通常不影响编译，但属于生成代码污染，会降低文档和 diff 可读性。

建议：移除 BOM，把相关注释恢复为 ASCII `-` 或正确 UTF-8 标点。

### 验证记录

已执行：

- 只读代码检查。
- `git diff --check`。

结果：

- `git diff --check` 只报告 CRLF 转换 warning。

未执行：

- Rust 测试套件。

---

## 三、上下文后续实现问题

审查范围：

- 计划：`.omx/plans/context-followup-phase-plan-2026-05-06.md`
- 提交范围：`664a765..5054003`
- 同时检查了当前工作树中仍触及 context 相关文件的 diff。

审查结论：要求修改（`REQUEST CHANGES`）

架构状态：观察（`WATCH`）

### 高：auto-compact 阈值判断重复扣减本地释放的 token

位置：`crates/cc-compact/src/pipeline.rs:141`

问题：`auto_compact_estimated_tokens` 从 `estimated` 中减去了 `total_tokens_freed`，但 `estimated` 已经是对 snip、microcompact、context-collapse 之后的 `current` 消息集重新估算得到的值。这里相当于重复抵扣了一次本地压缩收益。

风险：即使实际即将发送的消息集仍超过 80% 阈值，pipeline 也可能跳过 auto-compact。当前测试还把这个错误行为固化了：它断言 `estimated_tokens > 160_000` 时 `auto_compact_triggered == false`。

建议：auto-compact 阈值应直接比较本地 pipeline 之后的真实估算值；如果仍需要展示“抵扣收益”诊断，应把该指标和调度安全阈值分离，避免它抑制必要压缩。

### 高：exact-count 预检漏掉 system prompt 和 tools

位置：`crates/claude-code-rs/src/engine/lifecycle/deps.rs:363`

问题：auto-compact 的 exact-count 预检只使用 `pipeline_result.messages` 构造 `ModelCallParams`，并把 `system_prompt` 和 `tools` 都置为空。

真实模型请求在 `crates/claude-code-rs/src/query/loop_impl.rs:223` 构造，会包含完整 system prompt 和当前启用的 tools。

风险：接近阈值时，provider exact count 会低估真实请求大小，并可能错误地抑制 auto-compact；随后真实请求仍可能超过阈值。

建议：把真实模型调用使用的 request-shaping 输入传入 exact 预检；或者把 exact 预检移动到已经拿到最终 `ModelCallParams` 的位置。

### 中：Phase 10 记录了必需验证门禁失败

位置：`development/archive/context-phase10-final-verification-2026-05-06.md:30`

问题：计划验收条件要求 `cargo test -p claude-code-rs` 通过，但 Phase 10 文档记录该命令没有 green。

风险：实现被标记为 closed，但计划中的显式验证门禁没有通过。即便这些失败被认为无关，也不符合计划文本里的 merge-ready 标准。

建议：在关闭计划前让 `cargo test -p claude-code-rs` 通过；如果确实要排除无关失败，需要用证据明确缩窄验收条件，说明剩余失败早于本变更且不能覆盖该变更集。

### 架构观察项

提供方精确计数路径、启发式阈值路径和最终请求构造分散在不同层。关闭该阶段前，需要把“估算上下文大小”和“实际提供方请求大小”的边界写清楚，否则后续改动容易继续在两个概念之间漂移。

---

## 四、安全实现映射新增代码问题

审查范围：

- 安全映射文档：`architecture/safety-implementation-map.md`
- 该文档新增标记为已实现 / 部分实现的代码路径：Auto mode、Plan mode `allowedPrompts`、sandbox `allowedCommands` 权限桥、共享 safety classifier。
- 相关提交范围：安全映射文档历史中的 `043d942`、`8e7bfa4`、`4568ff0`、`84c04c7`、`9054a09`、`4a6a5d5`、`780678a`。

审查结论：要求修改（`REQUEST CHANGES`）

### 高：`permissions.enableAutoMode=false` 没有约束全部 Auto mode 入口

位置：

- `crates/claude-code-rs/src/commands/permissions_cmd.rs:293`
- `crates/claude-code-rs/src/startup/runtime_config.rs:123`
- `crates/claude-code-rs/src/commands/config_cmd.rs:457`
- `crates/claude-code-rs/src/web/handlers.rs:261`
- `crates/claude-code-rs/src/plugins/tools.rs:83`

问题：`/permissions mode auto` 路径会检查 `is_auto_mode_available == Some(false)`，但启动配置、`config set permissionMode auto`、Web `set_permission_mode` 和插件 read-only 子上下文都会绕过这个 gate，直接调用 `set_permission_mode_with_auto_mode_safety` 或把 Default / Plan 提升到 Auto。

风险：安全策略已经显式关闭 Auto mode 时，持久化配置、Web API 或插件上下文仍可重新进入 Auto mode，导致策略开关只对一个 CLI 命令入口有效。

建议：把 Auto / Bypass 可用性校验集中成一个模式切换 API，所有启动、配置、Web、插件 / 子上下文入口都必须经过它。若配置请求了被禁用的 Auto mode，应 fail closed 到 Default 或返回显式诊断。

### 高：Plan `allowedPrompts` 会在恢复 Auto mode 后追加未过滤的 Bash allow 规则

位置：

- `crates/claude-code-rs/src/plan_workflow.rs:112`
- `crates/claude-code-rs/src/tools/plan_mode.rs:255`
- `crates/claude-code-rs/src/tools/plan_mode.rs:262`
- `crates/claude-code-rs/src/tools/plan_mode.rs:270`
- `crates/claude-code-rs/src/tools/plan_mode.rs:315`
- `crates/claude-code-rs/src/tools/plan_mode.rs:415`

问题：`ExitPlanModeTool` 先生成 `allowed_prompt_rules`，再调用 `approve_and_exit_state` 恢复 `pre_plan_mode`，最后才把规则追加到 `session_allow_rules["plan_allowed_prompts"]`。如果进入 Plan 前是 Auto mode，恢复 Auto 时只会剥离当时已有的危险规则；随后追加的新规则没有再经过 `dangerous_auto_mode_allow_reason` 或 `strip_dangerous_permissions_for_active_auto_mode`。

`looks_like_bash_permission_pattern` 还会接受显式模式（例如 `prefix:`、`*`、`cargo` / `npm` / `python` 等前缀），因此计划批准流程可以把较宽的 Bash allow 规则注入到恢复后的 Auto session。

风险：从 Auto mode 进入 Plan、批准计划后，计划里的 `allowedPrompts` 可能绕过 Auto mode 的危险 allow 规则剥离机制，形成长期会话级自动放行。

建议：在生成 `allowed_prompt_rules` 时拒绝危险或过宽的 Bash permission pattern；追加规则后如果当前模式是 Auto，立即再次执行 `strip_dangerous_permissions_for_active_auto_mode`。补一个回归测试覆盖 Auto -> Plan -> ExitPlanMode，`allowedPrompts` 包含危险 / 宽泛 Bash 规则时不得留下 session allow。

### 高：sandbox `allowedCommands` 预批准不检查真实沙箱可用性

位置：

- `crates/claude-code-rs/src/tools/execution/security.rs:154`
- `crates/claude-code-rs/src/engine/lifecycle/deps.rs:128`
- `crates/claude-code-rs/src/tools/execution/pipeline.rs:242`
- `crates/cc-sandbox/src/runner.rs:69`
- `crates/cc-sandbox/src/runner.rs:573`
- `crates/cc-sandbox/src/availability.rs:83`

问题：`sandbox_allowed_command_applies` 只检查 sandbox enabled、mode 是 Workspace、命令匹配 `allowedCommands`，没有检查 `policy.availability` 是否真的有可用 OS-level sandbox，也没有要求 `fail_if_unavailable=true`。而 sandbox runner 在机制不可用且 `fail_if_unavailable=false` 时会 pass-through。

Windows 下当前 OS-level sandbox 明确不可用；Linux / macOS 缺少 `bwrap` / `sandbox-exec` 时也可能不可用。

风险：文档语义是“workspace sandbox 中匹配命令可预批准”，但实际可能是在沙箱不可用时仍预批准，最后命令以非 sandbox 方式运行。虽然 Rust 侧仍有部分预检，这仍破坏了 `allowedCommands` 作为 sandbox-only 放行桥的安全边界。

建议：权限预批准前要求 `policy.availability` 可用，或要求 `fail_if_unavailable=true`；否则回落到正常 Ask / permission flow。新增 Windows / unavailable runner 场景测试，确认 `allowedCommands` 不会在无 sandbox 时自动批准。

### 高：`allowedCommands` 前缀匹配允许 shell 链接命令搭车

位置：

- `crates/cc-sandbox/src/policy.rs:58`
- `crates/cc-sandbox/src/policy.rs:71`
- `crates/claude-code-rs/src/tools/execution/security.rs:181`

问题：`command_matches_any` 对非通配规则采用纯字符串前缀匹配：命令等于规则或以 `规则 + 空格` 开头即命中。因此规则 `cargo test` 会匹配 `cargo test --all && cat ~/.ssh/id_rsa`、`cargo test --all; curl ...` 这类链式 shell 命令。

风险：工具层危险命令检查能拦截一部分删除 / 提权行为，但不能可靠拦截任意读文件、网络外传、管道、重定向、命令替换等 shell 组合。结合上一条“无真实 sandbox 也可预批准”时，影响更大。

建议：`allowedCommands` 用作权限预批准时，应解析 shell 命令并要求它是单个 simple command，argv 前缀匹配 allowlist；默认拒绝包含 `&&`、`;`、`|`、反引号、`$()`、重定向等 shell 控制操作的命令，除非 allowlist 显式包含完整控制结构。

### 高：classifier redaction 漏掉 JSON 字段形式的 secret

位置：

- `crates/claude-code-rs/src/safety/classifier.rs:301`
- `crates/claude-code-rs/src/safety/classifier.rs:525`
- `crates/claude-code-rs/src/safety/classifier.rs:736`

问题：classifier prompt 会把 `tool_input`、`tool_classifier_input`、hook context 和 transcript 渲染为 pretty JSON 后发送给模型；但当前 redaction regex 只覆盖 `password=value` / `password: value` 这类形态。JSON 中常见的 `"password": "hunter2"`、`"apiKey": "secret"`、`"token": "..."` 因 key 后还有引号，无法命中当前 `\b(key)\s*[:=]` 模式。

风险：共享 classifier 接入 Auto mode 后，工具输入或 transcript 中的 JSON secret 可能被原样发送给 classifier model，和 system prompt 中“redacted values are intentionally unavailable”的安全承诺不一致。

建议：增加 JSON-aware redaction pattern，覆盖带引号字段名的 `apiKey` / `api_key` / `token` / `accessToken` / `refreshToken` / `secret` / `password` / `passwd` 等；补嵌套 JSON、hook context、transcript tool_use input 的测试。

### 中：Plan approval UI 没展示将写入的具体 allowed prompt 规则

位置：

- `crates/claude-code-rs/src/tools/plan_mode.rs:214`
- `crates/claude-code-rs/src/tools/plan_mode.rs:224`

问题：批准提示只显示“will also add N transient allowed prompt rule(s)”，没有展示具体会写入的 `Tool(pattern)` rule。

风险：用户可能以为只是批准计划文本，但实际还会批准会话级工具规则。对于 `Bash(...)` 这类规则，审批信息不足。

建议：审批前展示规范化后的规则列表；规则过多时至少展示前几条和总数，并把全部规则写入 structured result / audit record。

### 验证记录

已执行：

- 只读代码审查。
- `cargo test -p claude-code-rs safety::classifier`
- `cargo test -p claude-code-rs test_exit_plan_mode`
- `cargo test -p claude-code-rs central_permission_sandbox_allowed_command`
- `cargo test -p cc-permissions auto_mode`
- `cargo test -p cc-sandbox allowed_command`

结果：

- 以上 targeted tests 均通过。
- 现有测试覆盖了基础 classifier、Plan mode allow rule、Auto dangerous-rule 剥离、sandbox allowed command happy path，但未覆盖本节列出的绕过 / 不可用 / JSON secret 场景。

未执行：

- 全量 `cargo test` / `cargo build --release`。本次是 review 和问题归档，没有修改实现代码。

---

## 五、Extensibility 实现映射文档问题

审查范围：`architecture/extensibility-implementation-map.md`

审查结论：要求修改（`REQUEST CHANGES`）

本节主要是文档与交付状态问题。没有发现需要立即修改运行时代码的新增阻塞项，但当前映射文档里有多处 Phase 5/6 后的旧结论未收口，容易误导后续实现和验收。

### 中：Phase 5/6 闭环结论与旧“部分实现”状态互相冲突

位置：

- `architecture/extensibility-implementation-map.md:12`
- `architecture/extensibility-implementation-map.md:20`
- `architecture/extensibility-implementation-map.md:59`
- `architecture/extensibility-implementation-map.md:61`
- `architecture/extensibility-implementation-map.md:62`
- `architecture/extensibility-implementation-map.md:85`
- `architecture/extensibility-implementation-map.md:129`
- `architecture/extensibility-implementation-map.md:147`
- `architecture/extensibility-implementation-map.md:185`
- `architecture/extensibility-implementation-map.md:186`
- `architecture/extensibility-implementation-map.md:187`

问题：文档顶部说明 active Extensibility runtime 已按当前标准面闭环，并声明 custom-agent safety、MCP transports、OAuth、registry refresh 等已完成；实现映射表和汇总表也把 MCP configuration / protocol 记为 `已实现`。但逐文档结论仍保留旧口径，把 `custom-agents.mdx`、`mcp-configuration.mdx`、`mcp-protocol.mdx` 说成 `部分实现` 或“完整度还不够”。

风险：同一个文档既可被读成“Extensibility 已闭环”，也可被读成“MCP / custom agents 仍是部分实现”。后续计划、PR 审查和 `docs/WORK_STATUS.md` 对齐时会反复重新判断同一批状态。

建议：把 Phase 5/6 之前的旧结论改成“历史结论，已由 Phase 5/6 覆盖”，或统一改为当前口径：active runtime 已实现；future fields / WebSocket 另列为 out-of-scope 或待确认项。

### 中：Phase 实施记录缺少 Phase 5 条目

位置：

- `architecture/extensibility-implementation-map.md:20`
- `architecture/extensibility-implementation-map.md:200`
- `architecture/extensibility-implementation-map.md:203`
- `architecture/extensibility-implementation-map.md:204`
- `development/archive/extensibility-phase5-custom-agent-safety-2026-05-06.md:1`
- `docs/WORK_STATUS.md:8`

问题：文档顶部有 `2026-05-06 Phase 5 Update - Custom Agent Safety`，`development/archive/` 也已有 Phase 5 验证归档，但底部 Phase 实施记录从 Phase 4 直接跳到 Phase 6。

风险：custom-agent safety 是 Extensibility 闭环的关键验收点，缺少实施记录会让 Phase 6 看起来跳过了安全闭环阶段，也不方便追踪验证证据。

建议：在 Phase 4 与 Phase 6 之间补一条 Phase 5 实施记录，引用 `development/archive/extensibility-phase5-custom-agent-safety-2026-05-06.md`，并写清楚 permission inheritance、plugin `permissionMode` ignored、`disallowedTools` 优先、`maxTurns: 0` 拒绝等验收点。

### 中：custom-agent parsed-but-inactive 字段没有落入问题表

位置：

- `architecture/extensibility-implementation-map.md:15`
- `architecture/extensibility-implementation-map.md:30`
- `crates/claude-code-rs/src/ipc/agent_settings.rs:216`
- `crates/claude-code-rs/src/ipc/agent_settings.rs:223`
- `crates/claude-code-rs/src/ipc/agent_settings.rs:226`
- `crates/claude-code-rs/src/ipc/agent_settings.rs:229`
- `crates/claude-code-rs/src/ui/agents/types.rs:81`
- `crates/claude-code-rs/src/ui/agents/types.rs:82`
- `crates/claude-code-rs/src/ui/agents/types.rs:83`

问题：文档说明 custom-agent 的 `skills` / `hooks` / `plugin` / `mcpServers` 是 parsed-but-inactive future fields，源码也确实只解析、写回或保留 UI 类型字段。但后面的“未实现 / 部分实现 / 待确认”表没有把这些字段单独列为待确认 / future activation 项。

风险：读者容易把“custom agents 已实现”理解成这些字段也具备运行时语义。后续如果直接激活字段，可能绕过 Phase 5 已建立的 inherited permission context 约束。

建议：新增一行 `custom-agent future fields`，状态标为 `待确认` 或 `parsed inactive`，明确它们当前只 round-trip；任何 runtime activation 都必须复用父级 `ToolPermissionContext`，并补对应回归测试。

### 低：WebSocket unsupported/custom 与“没有故意裁剪”结论不一致

位置：

- `architecture/extensibility-implementation-map.md:17`
- `architecture/extensibility-implementation-map.md:43`
- `architecture/extensibility-implementation-map.md:61`
- `architecture/extensibility-implementation-map.md:62`
- `architecture/extensibility-implementation-map.md:137`
- `architecture/extensibility-implementation-map.md:145`
- `architecture/extensibility-implementation-map.md:189`
- `architecture/extensibility-implementation-map.md:193`
- `architecture/extensibility-implementation-map.md:203`

问题：文档多处把 WebSocket 记录为 unsupported/custom 或非当前标准 transport，但状态说明又声明这次核查没有找到 `故意裁剪` 项。

风险：如果 WebSocket 被视为上游文档的一部分，当前文本没有明确说明它是“故意不做”还是“待实现”；如果它不属于当前标准 MCP transport，也应从未实现项中剥离并明确 out-of-scope。

建议：二选一收口：若 WebSocket 确认为非当前标准面，状态列为 `故意裁剪` / out-of-scope，并说明原因；若仍需对齐上游自定义 transport，则列入 `待确认`，不要同时写“unsupported/custom”和“没有故意裁剪”。

### 低：已实现汇总遗漏 MCP 与 custom-agent safety 闭环

位置：

- `architecture/extensibility-implementation-map.md:171`
- `architecture/extensibility-implementation-map.md:173`
- `architecture/extensibility-implementation-map.md:181`
- `architecture/extensibility-implementation-map.md:193`

问题：“已实现汇总”只列出 hooks、skills、custom-agents 配置与运行时主路径，没有把 MCP configuration / protocol 的 Phase 6 closure 和 custom-agent safety 放进同一汇总；而后续动作又说 active runtime 主路径已完成。

风险：读者先看汇总会低估已完成范围，后看后续动作又得到相反结论。

建议：把“已实现汇总”改成 Phase 6 后的最终汇总，列出 hooks、skills、custom-agent active runtime + safety、MCP configuration/protocol active runtime；future fields 与 WebSocket 另列为边界项。

### 验证记录

已执行：

- 只读检查 `architecture/extensibility-implementation-map.md`。
- `rg` 定位 Phase 5/6、状态、WebSocket、parsed-but-inactive、custom-agent future fields。
- 交叉检查 `development/archive/extensibility-phase5-custom-agent-safety-2026-05-06.md` 与 `docs/WORK_STATUS.md` 是否已有 Phase 5 记录。
- 交叉检查 `agent_settings.rs`、`engine/agent/*`、`ui/agents/types.rs` 中 custom-agent future fields 的解析 / 类型保留情况。

未执行：

- Rust 测试套件。本节只整理映射文档问题，没有修改实现代码。
