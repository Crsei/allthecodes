# KAIROS MCP 工具安全画像计划

> 状态：计划中
>
> 目标：当 allthecodes 设置新的 MCP 插件或 MCP server 配置时，由 KAIROS 常驻助手模式自动分析 MCP tools 的安全属性，并用同一份安全画像驱动权限配置与 TUI 语义操作显示。

## 1. 背景与问题

当前 MCP 集成已经支持 server binding、权限枚举和 TUI tool operation 展示，但安全语义仍停留在粗粒度层面：

- `McpBinding` 只支持 server 级 `connect`、`list_tools`、`call_tools`、`read_resources`。
- `McpToolWrapper::is_read_only()` 固定返回 `false`，因为运行时不知道 MCP tool 是否有副作用。
- `McpManager::can_call_tool()` 只检查 server 是否有 `call_tools`，无法做到 per-tool allow/ask/deny。
- `allthecodes-tool-display` 目前把 MCP tools 泛化为 `OperationKind::System + OperationSubtype::Mcp`，TUI 只能显示类似 `MCP: fs_read` 的低语义标签。

这会导致一个安全边界问题：如果只为了允许一个只读工具而授予 server 级 `call_tools`，同一 MCP server 上的写入、删除、执行类工具也会被一并放行。

本计划新增一层 MCP tool 安全画像：先分析每个 MCP tool，再按 tool 级策略配置权限，并让 TUI 按画像显示用户可理解的语义操作，例如：

```text
Read with allthecodes-boost: fs_read
Search with allthecodes-boost: search_grep
Modify with github-mcp: update_issue
```

## 2. 设计决策

- 自动分析方式：先用确定性规则分析名称、描述、JSON schema、server/plugin 元数据；规则无法高置信判断或结果冲突时，再交给 KAIROS 做语义判断。
- 自动应用范围：只自动放行高置信只读/查询类工具；写入、删除、执行、凭据、外部网络、低置信和未知工具默认 `ask` 或 `deny`。
- 用户提醒方式：配置写入后复用现有 `SystemInfo` / in-app notice 通道提醒用户“配置已完成，需要重启后应用”。
- 权限边界：per-tool 策略必须接入 `check_permissions()` 和最终 `call()` 强制校验，不能只影响 TUI 显示。
- 路径隔离：所有持久化配置继续使用 `~/.allthecodes/` 和 `.allthecodes/`，不得使用原版 Codex 的路径。
- 与既有 Kairos tips 计划关系：`development/toolsearch/search-discovery-kairos-tips-plan.md` 中的 tips 仍是 advisory；本计划只对“用户已经安装/配置的 MCP server”生成权限画像，不自动安装、启用或执行新插件。

## 3. 公共类型与协议变更

在 MCP 类型层新增安全画像与 tool 策略：

```rust
pub struct McpToolSafetyProfile {
    pub server_id: String,
    pub tool_name: String,
    pub schema_hash: String,
    pub operation_kind: McpToolOperationKind,
    pub risk: McpToolRisk,
    pub confidence: McpToolConfidence,
    pub read_only: bool,
    pub target_fields: Vec<String>,
    pub recommended_policy: McpToolPolicyDecision,
    pub reason: String,
    pub source: McpToolSafetySource,
}

pub struct McpToolPolicy {
    pub tool_name: String,
    pub schema_hash: Option<String>,
    pub decision: McpToolPolicyDecision,
    pub profile: Option<McpToolSafetyProfile>,
}

pub enum McpToolPolicyDecision {
    Allow,
    Ask,
    Deny,
}
```

`McpBinding` 增加：

```rust
pub tool_policies: Vec<McpToolPolicy>
```

兼容规则：

- `tool_policies` 为空时，保留现有 server 级 `permissions` / `read_only` 行为。
- `tool_policies` 非空时，`call_tools` 不再代表整台 server 全量可调用；`can_call_tool()` 必须先查 per-tool policy。
- 旧配置无需迁移即可读取；写入新配置时包含 `tool_policies`。

新增 MCP IPC event：

```rust
ToolSafetyAnalyzed {
    server_name: String,
    profiles: Vec<McpToolSafetyProfile>,
    applied_count: usize,
    ask_count: usize,
    deny_count: usize,
    restart_required: bool,
}
```

同时发送用户可见 notice：

```text
MCP safety analysis completed for `<server>`: <n> safe tools enabled, <m> tools require approval. Restart allthecodes to apply the new MCP permissions.
```

## 4. 分析流程

触发条件：

- 新 MCP plugin 被安装或启用，且 manifest 贡献了 `mcp_servers`。
- `/mcp config` 或 IPC upsert 新增/修改 MCP server。
- MCP server reconnect 后 `tools/list` 返回的 tool set 或 schema hash 发生变化。

处理流程：

1. MCP server 新建或变更时，只授予最小发现权限：`connect + list_tools`。
2. 连接成功后读取 `tools/list` 的 `name`、`description`、`input_schema`、`server_name`。
3. 本地规则生成第一版 `McpToolSafetyProfile`：
   - `read/list/get/fetch/search/query/status/inspect` 倾向 read/search。
   - `write/create/update/edit/delete/remove/run/exec/shell/apply/deploy` 倾向 mutating 或 execute。
   - schema 字段含 `command`、`script`、`env`、`token`、`password`、`secret`、`url`、`method` 等时提高风险。
   - JSON schema 描述了文件路径、query、resource URI、issue id 等时填充 `target_fields`。
4. 本地规则高置信时直接输出画像；低置信、冲突或未知项进入 KAIROS 分析。
5. KAIROS 只接收脱敏元数据，不接收 token/env 值，不调用 MCP tool，不读取 MCP resources。
6. 合并规则结果与 KAIROS 结果：
   - 硬危险规则优先，例如 shell/exec/delete/credential 操作不能被 KAIROS 降级为 safe。
   - 冲突时取更保守结果。
   - 未知默认 `Ask`。
7. 将画像写入 MCP binding 的 `tool_policies`，并发送 `ToolSafetyAnalyzed` 与 notice。

## 5. 权限执行链路

`McpToolWrapper` 需要统一读取安全画像：

- `is_read_only(input)`：查当前 `server_id + tool_name + schema_hash` 的 profile，高置信 read/search 返回 `true`。
- `check_permissions(input, ctx)`：
  - `Allow`：返回 `PermissionResult::Allow`。
  - `Ask`：返回 `PermissionResult::Ask`，message 包含 server、tool、operation、risk 和 reason。
  - `Deny`：返回 `PermissionResult::Deny`。
  - 无 profile：默认 `Ask`，不得静默允许。
- `call(input, ctx, ...)`：在发送 `tools/call` 前再次调用 manager 的 per-tool enforcement；即使绕过 UI permission flow，也不能执行 deny/unknown 工具。

`McpManager::can_call_tool()` 更新为：

- tool policy 存在时，必须 `decision == Allow` 才返回 true。
- tool policy 不存在但 binding 仍是旧格式时，按现有 `CallTools` 行为兼容。
- `Deny` 与 schema hash mismatch 直接拒绝。
- schema hash mismatch 触发重新分析或降级为 `Ask`，避免旧画像误用到变更后的工具。

## 6. TUI 语义显示

`allthecodes-tool-display` 增加 profile-aware MCP classifier：

- 从 `mcp__server__tool` 解析 server/tool。
- 查 `McpToolSafetyProfile`。
- 将 profile 映射为现有 `ToolOperation`：
  - read/search -> `OperationKind::Read` / `OperationKind::Search`，`OperationRisk::Safe`。
  - write/create/update -> `OperationKind::Modify` / `Create`，`OperationRisk::Medium`。
  - delete/exec/credential -> `OperationRisk::High` 或 destructive。
  - unknown -> `OperationKind::Unknown`，low confidence。
- label 格式：
  - `Read with <server>: <tool>`
  - `Search with <server>: <tool>`
  - `Modify with <server>: <tool>`
  - `Execute with <server>: <tool>`
- target 从 profile 的 `target_fields` 对应 input 字段抽取；缺失时不显示 target。

权限弹窗继续复用 `PermissionDialogRequest.operation`，优先显示 profile 提供的 operation kind、risk、target 和 reason；没有 profile 时保留当前 fallback。

## 7. 持久化与重启语义

分析结果写入当前 MCP binding 所在 scope：

- project-scoped binding 写入 `.allthecodes/settings.json` 或对应项目设置。
- global/session/thread scope 按现有 binding 写入路径保存。
- 不写入 `.Codex` 或 `~/.Codex`。

默认行为是“写入后提醒重启”。本计划不要求热替换当前运行中 engine 的 MCP tool set，避免半更新状态：

- 新会话或重启后读取新 `tool_policies`。
- 当前会话如果继续使用旧 tool list，仍按旧权限运行。
- notice 明确说明 `Restart allthecodes to apply the new MCP permissions.`。

## 8. 实施步骤

1. 增加类型与序列化测试：
   - 添加 `McpToolSafetyProfile`、`McpToolPolicy`、`McpToolPolicyDecision`。
   - 扩展 `McpBinding` serde roundtrip。
   - 新增 `ToolSafetyAnalyzed` IPC event 序列化测试。

2. 增加画像存储与查询：
   - 在 MCP binding 加载/保存层处理 `tool_policies`。
   - 在 manager 内提供 `tool_policy(server, tool, schema_hash)` 查询。
   - 保留旧 binding 行为兼容。

3. 实现本地规则分析器：
   - 输入 `McpToolDef` 与 plugin/server 元数据。
   - 输出 profile 和 confidence。
   - 为 allthecodes-boost 风格工具覆盖 read/search/status 等常见场景。

4. 接入 KAIROS fallback：
   - 仅在 `FEATURE_KAIROS=1` 且规则低置信/冲突时调用。
   - KAIROS 分析 prompt 只包含脱敏元数据。
   - KAIROS 结果必须经过保守合并器。

5. 接入触发点：
   - MCP config upsert/reconnect 后排队分析。
   - plugin enable/install 后发现 mcp server 时排队分析。
   - `tools/list` schema hash 变化时重新分析。

6. 接入权限链路：
   - 更新 `McpToolWrapper::is_read_only()`。
   - 实现 `McpToolWrapper::check_permissions()`。
   - 更新 `McpManager::can_call_tool()` 和 `call()` 前二次校验。

7. 接入 TUI 显示：
   - 更新 shared classifier 支持 MCP profile。
   - 权限弹窗显示 profile reason/risk。
   - 发送 `SystemInfo` notice 和 `ToolSafetyAnalyzed` event。

8. 文档收尾：
   - 在 MCP/KAIROS 相关 docs 中记录新行为、重启要求和安全默认值。
   - 如有缩减实现遗留，按 Full Build 规则标注 TODO 或补齐。

## 9. 测试计划

Unit tests：

- 本地规则能正确分类 read/search/write/delete/exec/unknown。
- unknown、低置信、冲突项默认 `Ask`。
- `McpBinding` 旧格式和新 `tool_policies` 格式都能 deserialize。
- `McpManager::can_call_tool()` 在 per-tool policy 下只允许 `Allow` 工具。
- schema hash mismatch 降级为拒绝或重新分析，不复用旧 allow。
- `McpToolWrapper::is_read_only()`、`check_permissions()`、`call()` 使用同一策略。
- `ToolClassifier` 对 `mcp__allthecodes-boost__fs_read` 输出 read/safe/高置信 label。
- `ToolSafetyAnalyzed` IPC event serde roundtrip。

Integration / E2E：

- 假 MCP server 同时暴露 `read_file`、`search_docs`、`delete_file`、`run_command`。
- 启用 plugin 或 upsert config 后自动分析。
- read/search 被自动写入 allow。
- delete/run 被写入 ask 或 deny，调用时触发 permission dialog 或拒绝。
- TUI 显示语义 operation，不再只显示泛化 `MCP: <tool>`。
- 分析完成后出现重启 notice。

Verification：

```bash
export CARGO_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo
export RUSTUP_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/rustup
export CARGO_TARGET_DIR=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.tmp/allthecodes-target
export PATH="$CARGO_HOME/bin:$PATH"

cargo check --workspace
cargo test -p allthecodes-mcp
cargo test -p allthecodes-tool-display
cargo test -p allthecodes-ipc-protocol
cargo test -p allthecodes --lib mcp
```

最终提交前按仓库规则修复新增 warning。

## 10. 验收标准

- 新 MCP 插件/server 首次连接后能生成 per-tool 安全画像。
- 只读/查询工具可以被自动配置为允许调用。
- 写入、删除、执行、凭据、未知工具不会被自动静默放行。
- TUI 对 MCP tools 展示具体语义操作和风险，不再只有泛化 MCP 标签。
- 权限弹窗和最终 tool call enforcement 使用同一份策略。
- 用户收到“配置已完成，需要重启应用”的 notice。
- 旧 MCP binding 配置仍可读取并按原行为工作。
