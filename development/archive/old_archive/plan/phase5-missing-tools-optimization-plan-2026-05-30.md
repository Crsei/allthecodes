# Phase 5 缺失工具优化计划

> 日期：2026-05-30
> 目标文档：`development/archive/plan/missing-tools-adaptation-plan-2026-05-28.md`
> 参考实现：`/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-bun`、`/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/codex`
> 当前实现：`/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes`

## 0. 执行状态（2026-05-30）

本计划的 P0/P1/P2 优化项已按阶段落地到当前 worktree：

- Deferred loading 改为 wrapper-only：API schema 与 system prompt 使用同一 visible tool set，discovered tools 只用于 `ExecuteExtraTool` guard 和 compact/resume 恢复。
- `SearchExtraTools` / `ExecuteExtraTool` 使用当前 session catalog，覆盖 root-owned providers、MCP 命名工具和 plugin runtime tools；`ExecuteExtraTool` 通过 canonical dispatch 进入目标工具权限、hook、audit、auto-classifier 路径。
- LocalMemoryRecall、VaultHttpFetch、apply_patch、Goal、ViewImage、workflow、PushNotification、multi-agent v2 的 schema、安全边界、兼容 alias、feature/model/session gates、UI/headless preview 已补齐。
- SDK/headless/web SSE 的 compact boundary metadata 只输出 public copy，并通过 `internal_metadata_hidden` 标记被隐藏的内部字段。
- `development/archive/plan/missing-tools-adaptation-plan-2026-05-28.md` 已同步 Phase 5 implementation snapshot、验收状态和 Intentional divergence。

保留的 intentional divergence：

- `ApplyPatch` CamelCase JSON `{patch}` 作为 legacy alias；Codex Responses provider 使用 lower-case `apply_patch` custom/freeform grammar，其他 provider 降级 JSON function。
- `Workflow` Rust-native durable workflow spec 保留，lower-case `workflow` 是兼容入口。
- `VaultHttpFetch` 仍使用 allthecodes 隔离 credentials file 的 transitional vault store，不宣称 encrypted vault parity。
- `worker` / `statusline-setup` 这类工作型 subagent 可由 agent definition 显式授予 Bash/Edit/Write；递归 agent spawning 默认由 session gate 隐藏，Explore/Plan/code-reviewer 仍只读。

## 1. 结论

Phase 5 当前不是空缺状态，`SearchExtraTools` / `ExecuteExtraTool`、`phase5::tools()`、`multi_agent_v2`、API 请求过滤、compact metadata 都已有首版实现。但三个 subagent 的对照结论一致：当前实现更接近“可运行首版”，还没有达到上游完整语义。

需要优先修的不是“工具是否注册”，而是：

- 延迟工具加载策略与上游 TypeScript 当前行为不一致。
- `ExecuteExtraTool` 未进入统一工具执行边界，存在权限、hook、audit 语义漂移。
- `SearchExtraTools` / `ExecuteExtraTool` 使用全局 registry，无法完整覆盖运行时 MCP/plugin 工具。
- 系统提示词看到的是未过滤工具集，API 请求看到的是过滤工具集，模型可见能力不一致。
- 多个 Phase 5 工具 schema 与上游不兼容，安全边界也更弱。
- Codex 侧 `apply_patch`、Goal、view_image、multi-agent v2 的运行时语义明显比当前 allthecodes 首版深。

本计划按 Full Build 规则处理：默认对齐上游完整实现；若保留 allthecodes 自有简化或增强，必须标为 `Intentional divergence`，并补测试和文档。

## 2. Subagent 对照摘要

### 2.1 claude-code-bun

关键文件：

- `src/tools.ts`
- `src/constants/tools.ts`
- `src/utils/searchExtraTools.ts`
- `packages/builtin-tools/src/tools/SearchExtraToolsTool/`
- `packages/builtin-tools/src/tools/ExecuteTool/`
- `src/services/searchExtraTools/toolIndex.ts`
- `packages/builtin-tools/src/tools/LocalMemoryRecallTool/`
- `packages/builtin-tools/src/tools/VaultHttpFetchTool/`
- `packages/builtin-tools/src/tools/WorkflowTool/`
- `packages/builtin-tools/src/tools/AgentTool/`
- `src/tasks/LocalAgentTask/`

核心发现：

- 延迟工具启用后，上游保持稳定 core schema；发现 deferred tool 后主要通过 `ExecuteExtraTool` 包装调用，不把 deferred schema 重新加入 API 请求。
- `SearchExtraTools` 支持 keyword、`select:`、`discover:`，并使用 TF-IDF/keyword 混合索引、searchHint、MCP 名称解析和缓存。
- `LocalMemoryRecall` 是 store/key 型本地记忆工具，不是全文递归搜索。默认 preview，full fetch 需要权限，有每 turn byte budget，并把结果包装为 untrusted data。
- `VaultHttpFetch` 使用本地 vault key name，权限粒度为 `key@host`，HTTPS only、无 redirect、timeout、body cap、secret 派生形式 scrub。
- `Workflow` 上游工具名为小写 `workflow`，读取项目 `.claude/workflows`，支持 `start/status/advance/cancel/list`。
- Agent/Task 的后台生命周期、任务输出文件、notification、resume/queue、权限 bubbling 是可用性的核心，不应简化为只写 mailbox。

### 2.2 Codex

关键文件：

- `codex-rs/core/src/tools/spec_plan.rs`
- `codex-rs/core/src/tools/handlers/apply_patch.rs`
- `codex-rs/core/src/tools/runtimes/apply_patch.rs`
- `codex-rs/core/src/tools/handlers/goal.rs`
- `codex-rs/core/src/goals.rs`
- `codex-rs/core/src/tools/handlers/view_image.rs`
- `codex-rs/core/src/tools/handlers/multi_agents_v2/`
- `codex-rs/core/src/agent/control.rs`

核心发现：

- Codex `apply_patch` 是 FREEFORM/custom grammar 工具，不是 JSON `{patch}`；它接入 sandbox、approval、patch lifecycle event、Guardian/hook 和环境 filesystem。
- Goal 工具是 `get_goal/create_goal/update_goal`，并有 runtime accounting：token/time usage、budget-limited、event emission、complete 时 final usage report。
- multi-agent v2 是 `spawn_agent/send_message/followup_task/wait_agent/close_agent/list_agents`，使用 canonical task path、并发限制、fork 语义、root protection、mailbox notification。
- `view_image` 通过 sandboxed filesystem 读图，检查模型 image input 能力，`detail=original` 受能力门控，输出 image content item。
- 工具暴露按 feature、模型能力、environment mode、subagent 类型和 namespace 配置动态规划。

### 2.3 allthecodes 当前实现

关键文件：

- `crates/allthecodes-tools/src/deferred_tools.rs`
- `crates/allthecodes-engine/src/query/turn_context.rs`
- `crates/allthecodes-engine/src/system_prompt/static_sections.rs`
- `crates/allthecodes-tools/src/phase5/mod.rs`
- `crates/allthecodes-teams/src/multi_agent_v2.rs`
- `crates/allthecodes-startup/src/tool_registry.rs`

当前状态：

- Phase 5 工具已注册，但当前 worktree 中相关代码仍有未提交和 untracked 文件。
- 延迟工具过滤默认关闭，需要 `ALLTHECODES_DEFERRED_TOOL_LOADING` 或 `CC_RUST_DEFERRED_TOOL_LOADING`。
- API request filtering 当前是 `CORE_TOOLS ∪ discovered_tools`。
- `SearchExtraTools` / `ExecuteExtraTool` 查的是全局 registry，不是 engine 当前 tools catalog。
- `ExecuteExtraTool` 在 wrapper 内直接调用目标工具，未进入中央执行边界。
- `CORE_TOOLS` 包含 goal、ViewImage、multi-agent v2 等低频工具，边界比上游更宽。
- Phase 5 工具测试较窄；multi-agent v2 只有工具名测试。

## 3. P0 优化项

### P0-A. 延迟工具加载语义重定线

目标：对齐 claude-code-bun 当前行为，稳定初始 tool schema，发现状态只用于 `ExecuteExtraTool` guard 和 compact 恢复，不在下一轮自动暴露 deferred schema。

实施：

1. 修改 `filter_tools_for_deferred_request()`：
   - 启用 deferred 后只发送稳定 core 工具、`SearchExtraTools`、`ExecuteExtraTool`。
   - `discovered_tools` 继续记录，但不参与 API schema 扩展。
   - 如果决定保留 `CORE ∪ discovered` 设计，必须在计划文档和 `IMPLEMENTATION_GAPS` 标注 `Intentional divergence`，并解释 prompt-cache 与模型直调路径风险。
2. 收窄 `CORE_TOOLS`：
   - 保留真正核心工具：文件、shell、Agent/Task/Todo、plan mode、WebFetch/WebSearch、LSP/Skill、Sleep、SearchExtraTools、ExecuteExtraTool、StructuredOutput。
   - 将低频 Phase 5 工具默认设为 deferred：`LocalMemoryRecall`、`VaultHttpFetch`、`PushNotification`、`DiscoverSkills`、`workflow`、`ApplyPatch`/`apply_patch`、Goal、ViewImage、multi-agent v2 扩展工具。
3. 同步系统提示词：
   - system prompt 的 enabled tools 必须来自与 API 请求一致的可见工具集。
   - 启用 deferred 时，不提示模型直接调用隐藏的 Cron/WebBrowser/Workflow 等工具。
4. 检查 `crates/allthecodes-query` 是否仍有消费者；如有，补同样过滤和 compact annotation；如无，标注废弃路径。

验收：

- Deferred enabled 时，首轮 API tools 数量稳定，不随 discovered tools 增长。
- `SearchExtraTools("select:WebBrowser")` 后下一轮仍不直接暴露 `WebBrowser` schema，模型应通过 `ExecuteExtraTool` 调用。
- compact/resume 后 `ExecuteExtraTool` 仍识别已发现工具。

### P0-B. 统一 deferred 工具目录和执行边界

目标：`SearchExtraTools` / `ExecuteExtraTool` 使用当前 session 的真实工具目录，并通过目标工具名进入统一权限、hook、audit、auto-classifier 流。

实施：

1. 将 engine 当前 tools catalog 注入 `ToolUseContext` 或新增只读 `ToolCatalog` provider。
2. `SearchExtraTools` 使用当前 catalog：
   - 包括 refreshed MCP tools。
   - 包括 plugin runtime tools。
   - 包括 root-owned providers。
   - 不只查 `allthecodes_tools::registry::get_all_tools()`。
3. `ExecuteExtraTool` 改为 canonical dispatch：
   - 执行时保留目标工具名，而不是只按 `ExecuteExtraTool` 做外层审批。
   - target 的 permission rules、always allow/deny、PreToolUse/PostToolUse hooks、audit log、auto classifier 都必须可观察。
   - wrapper 仅负责 discovery guard 和参数转发。
4. 对 dynamic MCP 工具补测试，确保 `mcp__server__tool` 能被搜索、发现、执行。

验收：

- 对目标工具设置 deny rule 时，即使通过 `ExecuteExtraTool` 调用也被拒绝。
- Pre/PostToolUse hook 收到目标工具名。
- MCP/plugin deferred tool 可被搜索并执行。

### P0-C. VaultHttpFetch 与 LocalMemoryRecall 安全边界

目标：避免把本地凭据和记忆读取做成普通 HTTP/文件搜索工具。

实施：

1. `LocalMemoryRecall` 对齐上游 store/key 模型：
   - schema 改为 `action: list_stores | list_entries | fetch`、`store`、`key`、`preview_only`。
   - 路径使用 allthecodes 隔离目录，例如 `{data_root}/local-memory/`，不得读取 `.claude`、`.Codex`、`.codex`。
   - preview 默认 2KB，full fetch 最大 50KB。
   - full fetch 需要 `LocalMemoryRecall(fetch:store/key)` 粒度权限。
   - 每 turn 总 fetch budget，建议先 100KB。
   - 内容清理控制字符，并包为 untrusted data，明确模型不得把其当指令。
2. `VaultHttpFetch` 对齐上游安全模型：
   - schema 改为 `vault_auth_key`、`auth_scheme`、`auth_header_name`、`reason`。
   - 权限粒度为 `vault_auth_key@host`。
   - HTTPS only。
   - 禁止 redirect，或 redirect 后重新做 host/private network 检查且不转发 auth header。
   - timeout 默认 30s，响应 body cap 默认 1MB。
   - scrub secret 本体及 bearer/basic/base64/header 派生形式。
   - 若暂不实现 encrypted vault，当前 `credential_ref`/`credentials.json` 必须标为 transitional，并禁止宣称完整 Vault parity。
3. 修 UTF-8 截断：
   - 禁止使用 byte index 直接切 `String`。
   - 统一引入 UTF-8 safe truncate helper。

验收：

- private/localhost/redirect-to-private 均被拒绝。
- full memory fetch 未授权时必须 ask/deny。
- 非 ASCII 记忆和 HTTP body 截断不 panic。
- secret 不出现在 data、error、headers、audit 中。

### P0-D. Codex `apply_patch` 对齐

目标：当前 `ApplyPatch` JSON `{patch}` 只是过渡实现；需要补 Codex-compatible freeform/custom grammar 工具。

实施：

1. 新增或迁移为 Codex-compatible `apply_patch`：
   - 支持 FREEFORM/custom grammar。
   - 保留 `*** Begin Patch` / `*** End Patch`、add/delete/update/move、EOF marker、context hunk。
   - 使用正式 parser，而不是 ad hoc line split。
2. 接入安全边界：
   - 解析后先生成变更摘要。
   - update/delete 要求 read-before-write 或等价 stale guard。
   - 写入路径按 allthecodes permission sandbox 校验。
   - move destination 单独校验。
   - approval 按文件路径/环境粒度缓存。
3. 接入 UI/headless event：
   - patch begin/update/end。
   - permission preview 显示 diff summary。
   - audit 保留目标文件和操作，不泄漏无关内容。
4. `ApplyPatch` CamelCase JSON 工具如保留，只能作为兼容 alias，并标注 `Intentional divergence`。

验收：

- Codex patch parser 测试用例可迁移通过。
- add/update/delete/move 均有权限和 stale guard。
- 非法 patch 不产生部分写入。

## 4. P1 优化项

### P1-A. 工具 schema/name 兼容层

目标：减少模型提示和 transcript 兼容漂移。

实施：

- `DiscoverSkills`：提供上游兼容 schema `description` / `limit`；当前 `query/source/max_results` 可作为 allthecodes 扩展。
- `LocalMemoryRecall`：按 P0-C 改为 store/key action schema。
- `VaultHttpFetch`：按 P0-C 改为 vault key schema。
- `PushNotification`：提供 `title/body/priority: normal|high` 兼容入口；当前 `message/urgent/webhook` 如保留，标为 allthecodes provider extension。
- `VerifyPlanExecution`：兼容 `plan_summary/verification_notes/all_steps_completed`；allthecodes 更强 read-only verification 可作为扩展 mode。
- `workflow`：新增上游小写 `workflow` 工具，读取项目 `.allthecodes/workflows`，run 状态写 `.allthecodes/workflow-runs`；当前 `Workflow` Rust-native durable spec 若保留，标为 `Intentional divergence`。
- Codex 工具名：补小写 `get_goal/create_goal/update_goal/view_image/spawn_agent/send_message/followup_task/wait_agent/close_agent/list_agents` 兼容层，或在系统提示和 docs 中明确 CamelCase 是 allthecodes 约定。

验收：

- 上游常见 tool call JSON 可直接映射到 allthecodes。
- CamelCase 和 lower_case alias 不造成 duplicate registration 或权限规则绕过。

### P1-B. Goal runtime accounting

目标：Goal 不只是 `goals/{session}.json` CRUD。

实施：

1. 记录 goal fields：
   - objective
   - status: active / complete / blocked / budget_limited
   - token_budget
   - tokens_used
   - time_used_seconds
   - created_at / updated_at
2. 接入 query loop：
   - turn started/finished 更新 token 和 wall-clock accounting。
   - usage limit 或 budget limit 触发状态和提示。
   - `UpdateGoal(status=complete)` 返回 completion budget report。
3. 事件：
   - headless/TUI 输出 goal updated event。
   - resume 时恢复 active goal。

验收：

- 创建带 token budget 的 goal 后，完成时能输出 final usage。
- 超预算时有可测试状态。
- 不能在 active goal 未完成时创建新 goal。

### P1-C. Multi-agent v2 runtime 化

目标：从 team-file/mailbox 首版升级到可观察的 agent runtime lifecycle。

实施：

1. 对齐 Codex v2 工具：
   - `spawn_agent`
   - `send_message`
   - `followup_task`
   - `wait_agent`
   - `close_agent`
   - `list_agents`
2. 使用 canonical target：
   - 支持 agent id、nickname、task path。
   - root/team lead 不能被 close/followup。
   - task path 唯一且可 list。
3. 明确 send 语义：
   - `send_message` 只入队，不触发 turn。
   - `followup_task` 触发目标下一轮。
   - `wait_agent` 等最终状态或 mailbox notification，不返回大量子 agent transcript。
4. close 语义：
   - 发送 shutdown request。
   - 等 runtime acknowledgement 或标记 pending close。
   - 不只写 inactive。
5. 与现有 Agent/Task system 合并：
   - 不新增第二套 agent registry。
   - 后台 agent 输出文件、task notification、resume/queue 行为与上游一致。

验收：

- list/spawn/followup/wait/close 形成端到端测试。
- close root 被拒绝。
- wait timeout 与 completion 可区分。
- stopped agent 可 resume 或返回明确错误。

### P1-D. SearchExtraTools / DiscoverSkills 搜索质量

目标：从 contains score 升级为上游级别可用搜索。

实施：

- 引入 tool index：
  - tool name CamelCase 分词。
  - MCP `mcp__server__tool` 分词。
  - searchHint。
  - prompt/description。
  - exact-name fast path。
  - `+required` terms。
  - TF-IDF + keyword weighted merge。
  - CJK token 支持。
  - deferred tools cache invalidation。
- DiscoverSkills 复用 skill index：
  - bundled/user/project/plugin/MCP skills。
  - allthecodes path isolation：`.allthecodes/skills`、`~/.allthecodes/skills`。
  - 不读 `.claude/skills`，除非做显式 migration/import。

验收：

- exact tool name、MCP server name、action keyword 都能稳定命中。
- search result 排名有 fixture 测试。

## 5. P2 优化项

### P2-A. UI/headless 渲染与隐藏 attachment

实施：

- `deferred_tools_delta`、compact discovered metadata、verify reminders 默认隐藏，不在用户 UI 里裸露 JSON。
- LocalMemory result 用 untrusted data render。
- Vault result 显示 status/header/body summary，永不显示 secret。
- Workflow start/advance/status/cancel 有专用渲染和 permission preview。
- PushNotification 区分 delivered/not delivered/provider disabled。
- Agent/Task/multi-agent v2 渲染 spawn/send/wait/close lifecycle。
- view_image 历史项显示路径和预览状态，不把 base64 文本铺到 UI。

验收：

- headless JSONL 和 Rust TUI 均能区分 hidden/internal attachment 与用户可见消息。
- UI permission router 覆盖 ApplyPatch、Vault、LocalMemory full fetch、workflow mutating action、CloseAgent。

### P2-B. Tool exposure gating

实施：

- Feature gates：
  - deferred tool loading。
  - workflow scripts。
  - push notification remote bridge。
  - goal tools。
  - multi-agent v2。
- Model capability gates：
  - view_image only when model supports image input。
  - original image detail only when model supports it。
  - apply_patch freeform only when provider supports custom/freeform grammar；否则降级 wrapper/JSON，并标注。
- Session gates：
  - non-interactive/background agent 默认不得弹权限，除非 bubble permission 明确可用。
  - review/subagent mode 禁用递归/危险工具。

验收：

- 禁用 gate 时工具不出现在 API schema 和系统提示词中。
- text-only model 不暴露 view_image。

## 6. 测试计划

### 6.1 单元测试

- `SearchExtraTools`：
  - keyword / `select:` / `discover:`。
  - exact name。
  - MCP prefix。
  - already loaded/core tool no-op。
  - pending MCP server message。
- `ExecuteExtraTool`：
  - undiscovered guard。
  - core tool refused。
  - target deny rule 生效。
  - hook/audit target name 生效。
  - MCP/plugin tool execution。
- `LocalMemoryRecall`：
  - list stores/list entries/fetch preview/full。
  - invalid store/key。
  - full fetch permission。
  - per-turn budget。
  - UTF-8 truncation。
  - untrusted wrapper escaping。
- `VaultHttpFetch`：
  - non-HTTPS reject。
  - localhost/private/link-local reject。
  - redirect reject。
  - timeout/body cap。
  - secret scrub。
  - key@host permission.
- `apply_patch`：
  - add/update/delete/move。
  - invalid grammar no write。
  - stale guard。
  - outside allowed dir ask/deny。
- Goal：
  - lifecycle。
  - duplicate active goal reject。
  - budget accounting。
  - complete report。
- Multi-agent v2：
  - spawn/list/send/followup/wait/close。
  - root close reject。
  - timeout vs completed。

### 6.2 集成测试

建议命令：

```bash
export CARGO_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo
export RUSTUP_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/rustup
export PATH="$CARGO_HOME/bin:$PATH"

cargo test -p allthecodes-tools deferred_tools -- --nocapture
cargo test -p allthecodes-tools phase5 -- --nocapture
cargo test -p allthecodes-teams multi_agent_v2 -- --nocapture
cargo test -p allthecodes-startup tool_registry -- --nocapture
cargo test -p allthecodes-engine deferred -- --nocapture
cargo build --workspace --release
```

必须在构建后清 warning。已有已知 warning 可以按 `AGENTS.md` 记录，但本任务新增 warning 必须修。

## 7. 实施顺序

### Slice 1：冻结契约与 divergence 文档

产出：

- 更新 Phase 5 状态文档。
- 列出每个工具的上游兼容 schema、allthecodes 扩展 schema、Intentional divergence。
- 决定 deferred 策略：推荐切到上游 wrapper-only。

风险：

- 如果不先冻结契约，后续实现会继续在 CamelCase/lower_case、direct schema/wrapper execution 之间摇摆。

### Slice 2：Deferred runtime/catalog 修复

产出：

- 当前 session tools catalog 注入。
- API schema 稳定 core-only。
- system prompt 使用同一 visible set。
- dynamic MCP/plugin deferred search/execute 测试。

### Slice 3：ExecuteExtraTool 中央执行边界

产出：

- target tool canonical dispatch。
- permission/hook/audit target name 可见。
- wrapper permission 不覆盖目标工具权限。

### Slice 4：LocalMemory/Vault 安全重做

产出：

- store/key memory tool。
- key@host vault permission。
- redirect/timeout/body cap/scrub。
- UTF-8 safe truncation helper。

### Slice 5：Codex 工具语义补齐

产出：

- `apply_patch` freeform/custom grammar。
- Goal accounting。
- `view_image` model capability + sandboxed read。
- lower_case multi-agent v2 compatibility tools。

### Slice 6：Workflow/Push/Verify schema 兼容

产出：

- 小写 `workflow` 上游兼容入口。
- `Workflow` Rust-native spec 标为 allthecodes extension 或迁移。
- PushNotification remote bridge gating；local audit/webhook 标为 provider extension。
- VerifyPlanExecution 上游 schema 兼容，同时保留 allthecodes stronger verification mode。

### Slice 7：UI/headless 和 e2e

产出：

- 隐藏 internal attachments。
- permission preview。
- agent/task lifecycle rendering。
- headless JSONL fixtures。
- Rust TUI 快照/command surface 测试。

## 8. 验收门槛

Phase 5 不能只用“工具已注册”判定完成。完成门槛应改为：

- Deferred enabled 时 API schema 稳定，prompt 与 schema 可见集一致。
- Dynamic MCP/plugin 工具可 deferred search/execute。
- `ExecuteExtraTool` 不绕过目标工具权限、hook、audit。
- LocalMemory/Vault 达到上游安全边界或明确标为未完成。
- Codex `apply_patch` freeform、Goal accounting、view_image capability gate 有等价实现或明确 divergence。
- Multi-agent v2 有真实 runtime lifecycle，不只是 team file 状态。
- 每个 Phase 5 工具有 schema 兼容测试、权限测试、错误路径测试、UI/headless 渲染测试。
- 所有持久化路径符合 allthecodes 隔离：`~/.allthecodes/`、`.allthecodes/`、Keychain service `allthecodes`。

## 9. 当前首个优化 PR 建议

第一批 PR 不建议碰所有工具。建议只做：

1. `SearchExtraTools` / `ExecuteExtraTool` 使用当前 session tools catalog。
2. `ExecuteExtraTool` 改进到 target tool permission/hook/audit 边界。
3. system prompt 使用 filtered visible tools。
4. 增加 deferred API filtering、MCP deferred、target deny rule 的测试。

这批改完后，后续 Phase 5 工具安全和 Codex 语义补齐会有稳定执行底座。
