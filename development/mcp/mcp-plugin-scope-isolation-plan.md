# MCP Plugin Session/Thread Scope Isolation 实施计划

日期：2026-07-02

状态：已实现（2026-07-02）

## 实现结果

本计划已落地为运行时 MCP binding 隔离：

- 新增 `McpToolScope`、`McpPermission`、`McpBinding`、`McpBindingContext`，settings 支持 `mcpBindings` round-trip。
- 旧 `mcpServers` 保持兼容并生成隐式 binding：user -> `global`，project -> `project`，plugin/ide -> 只读 `global` 来源。
- session/thread binding 写入 `{data_root}/runs/{session_id}/mcp-bindings.json`，拒绝空 `session_id/thread_id`，project binding 使用 canonical workspace root 匹配。
- `McpManager` 提供 context-aware tools/resources/call permission API，engine startup、model-call refresh、agent spawn、skill fork 均按当前 `global/project/session/thread` context 过滤 MCP tools。
- `McpToolWrapper::call()` 在调用 MCP client 前二次校验 `call_tools` 权限。
- `AgentDefinitionEntry.mcp_servers` 已实际生效；内置 read-only agents 默认不获得 MCP tools，除非定义显式允许 server 或具体 `mcp__server__tool`。
- CLI/IPC/Web 已支持 binding list/bind/unbind/permission CRUD；TUI `/mcp` 展示 config source、binding scope 和 permissions，并支持 global/project/session 编辑。thread binding 由 CLI/IPC/Web 编辑，TUI 当前只展示。

验证：

- `cargo test -p allthecodes-mcp --lib`
- `cargo test -p allthecodes-ipc-protocol --lib`
- `cargo test -p allthecodes-engine --lib`
- `cargo test -p allthecodes-commands --lib`
- `cargo test -p allthecodes-web --lib`
- `cargo test -p allthecodes-services --lib`
- `cargo build --workspace --release`

## 背景

当前 MCP 已有配置来源分层，但不是运行时绑定隔离：

- `crates/allthecodes-mcp/src/discovery.rs` 只有 `DiscoveryScope::User | Project | Plugin | Ide`，用于标记配置来源和 UI 展示。
- `crates/allthecodes-mcp/src/lib.rs` 的 `McpServerConfig` 不包含 `scope/projectPath/sessionId/threadId/permissions`。
- plugin manifest 的 `McpServerContribution` 只有 `name/command/args/env`。
- 启动时 `full_init.rs` 连接所有已发现 MCP server，并把 `mgr.all_tools()` 全量合并进当前 session 的工具列表。
- `AgentDefinitionEntry.mcp_servers` 已保留字段，但当前 agent spawn 只按 `tools/disallowed_tools` 过滤工具，没有按 MCP server 绑定连接或隔离。
- Streamable HTTP 的 `MCP-Session-Id` 是 MCP 协议传输会话，不等于 allthecodes 的 session/thread 隔离。

本计划目标是实现类似以下能力：

```ts
type ToolScope = "global" | "project" | "session" | "thread";

type McpBinding = {
  serverId: string;
  scope: ToolScope;
  projectPath?: string;
  sessionId?: string;
  threadId?: string;
  permissions: string[];
};
```

## 目标行为

1. MCP server 可以绑定到 global、project、session、thread 四个运行范围。
2. 同一 server 配置可以存在于多个 scope；运行时只暴露当前上下文允许的 MCP tools。
3. session 级绑定只对当前 session 可见，切换/恢复 session 时按 session id 解析。
4. thread 级绑定只对指定 agent/main thread 可见，子 agent 不应默认继承未授权的 thread MCP server。
5. project 级绑定必须受 project path 限定，不因 cwd 子目录或跨项目 session 误暴露。
6. plugin-contributed MCP server 默认是 plugin/global 来源，但可以被用户显式绑定到 project/session/thread。
7. 权限字段先采用最小可用语义：`connect`、`list_tools`、`call_tools`、`read_resources`；后续再扩展细粒度工具名 allowlist。

## 非目标

- 不改 MCP 协议层 `MCP-Session-Id` 语义。
- 不引入远程多租户隔离；本计划只覆盖本地 runtime 里哪个 session/thread 能看到和调用哪个 MCP server。
- 不改变现有 user/project settings 的默认兼容行为：旧配置继续按当前项目/全局方式加载。
- 不一次性完成 marketplace 权限审批 UI；先保留 manifest 能力和后端策略，UI 可后续增强。

## 数据模型

新增统一绑定类型，建议放在 `crates/allthecodes-types/src/mcp.rs` 或 `crates/allthecodes-ipc-protocol/src/subsystem_types.rs`，再由 MCP crate 引用轻量版本：

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum McpToolScope {
    Global,
    Project,
    Session,
    Thread,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpBinding {
    pub server_id: String,
    pub scope: McpToolScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(default)]
    pub permissions: Vec<McpPermission>,
}
```

存储建议：

- User/global：`~/.allthecodes/settings.json` 的 `mcpBindings`。
- Project：`{project}/.allthecodes/settings.json` 的 `mcpBindings`，并写入规范化 `projectPath`。
- Session：`{data_root}/runs/{session_id}/mcp-bindings.json`，避免污染项目配置。
- Thread：同 session 文件内以 `threadId` 区分，或 `{data_root}/runs/{session_id}/threads/{thread_id}/mcp-bindings.json`。

兼容规则：

- 旧 `mcpServers` 不带绑定时按来源生成隐式绑定：
  - user settings -> `global`
  - project settings -> `project`
  - plugin/ide -> `global`，但标记为 read-only source
- 显式 `mcpBindings` 优先于隐式绑定。
- `serverId` 使用现有 server name；如果未来支持同名多来源 server，应升级为 `{source_scope}:{source_id}:{name}` 的稳定 id。

## 运行时设计

新增 `McpBindingContext`，由 engine 在每次构建工具快照时传入：

```rust
pub struct McpBindingContext {
    pub cwd: PathBuf,
    pub project_root: Option<PathBuf>,
    pub session_id: String,
    pub thread_id: String,
}
```

thread id 规则：

- 主线程使用稳定值 `main`，或 `session:{session_id}`；需要全仓统一一个表示。
- agent/subagent 使用现有 `AgentContext.agent_id` 作为 thread id。
- UI 的 `current_agent_thread_id` 只能作为展示选择，不应作为后端权限来源；后端调用时以实际 engine/agent context 为准。

MCP manager 需要从“单全局 manager”升级为“按可见 server set 管理连接”：

- `McpRuntimeRegistry` 持有所有 server config、binding 和连接实例。
- 连接 key 至少包含 `server_id + scope + session_id/thread_id`，避免 thread 绑定复用 global 连接时泄漏状态。
- 对 stateless stdio server 可复用 global/project 连接；session/thread 绑定默认独立连接，除非配置显式允许共享。
- `McpManager::all_tools()` 增加带上下文版本：

```rust
pub fn tools_for_context(&self, ctx: &McpBindingContext) -> Vec<McpToolDef>;
```

## 工具暴露路径改造

第一阶段先保持模型工具名格式 `mcp__{server}__{tool}`，但工具过滤必须按 context 做：

- `full_init.rs` 启动时不再无条件 `mgr.all_tools()` 全量并入所有工具。
- 初始化 QueryEngine 时，用当前 session/main thread context 生成可见 MCP tools。
- `lifecycle/deps/model_call.rs` 的 MCP refresh 改为 `tools_for_context(ctx)`。
- `agent/mod.rs` 创建子 engine 时，用 child agent thread id 构建 context，再解析该 thread 可见 MCP tools。
- `skill_tool.rs` fork context 也要用当前 skill fork 的 thread context，避免直接从 `agent_runtime::all_tools()` 取全局 MCP tools。

为了不破坏现有工具名 allowlist：

- `AgentDefinitionEntry.tools` 继续支持 `mcp__server__tool` 精确匹配。
- 新增可选语法 `mcp:server` 或读取 `AgentDefinitionEntry.mcp_servers`，用于声明 agent 允许连接哪些 MCP server。
- `mcp_servers` 字段优先用于 server 级授权，`tools` 仍用于最终工具名过滤。

## 权限模型

最小权限枚举：

- `connect`：允许启动/连接 server。
- `list_tools`：允许列出工具并暴露给模型。
- `call_tools`：允许调用工具。
- `read_resources`：允许读取 MCP resources / MCP skills。

默认策略：

- 旧配置隐式绑定给 `connect/list_tools/call_tools/read_resources`，保持兼容。
- session/thread 绑定必须显式创建，不从 global 自动降级复制。
- thread 级绑定不继承 sibling thread 的绑定。
- 子 agent 默认继承 project/global 可见 MCP；session/thread 继承需由 parent policy 或 agent definition 明确允许。

调用时检查：

- `McpToolWrapper::call()` 在调用前再次检查 binding context 和 `call_tools` 权限。
- 不仅在工具列表生成时过滤，避免旧工具对象被缓存后越权调用。

## IPC / CLI / Web 控制面

新增 IPC 类型：

- `McpCommand::ListBindings`
- `McpCommand::BindServer { binding }`
- `McpCommand::UnbindServer { server_id, scope, session_id, thread_id }`
- `McpCommand::SetBindingPermissions { ... }`
- `McpEvent::BindingsUpdated`
- `McpEvent::BindingError`

CLI 建议：

- `/mcp bind <server> --global`
- `/mcp bind <server> --project`
- `/mcp bind <server> --session`
- `/mcp bind <server> --thread <thread-id>`
- `/mcp unbind <server> --session|--thread ...`
- `/mcp bindings`

Web REST 建议：

- `GET /api/mcp-bindings`
- `POST /api/mcp-bindings`
- `PATCH /api/mcp-bindings/{id}`
- `DELETE /api/mcp-bindings/{id}`

UI 可先只展示和编辑 global/project/session，thread 级通过 agent/thread detail 后续接入。

## 实施阶段

### Phase 0：基线测试与文档冻结

改动范围：

- `crates/allthecodes-mcp/src/discovery.rs`
- `crates/allthecodes-mcp/src/manager.rs`
- `crates/allthecodes/src/full_init.rs`
- `crates/allthecodes-engine/src/mcp_tool_adapter.rs`
- `crates/allthecodes-engine/src/agent/mod.rs`

任务：

- 增加当前行为测试：旧 `mcpServers` 仍可加载并生成工具。
- 增加负向测试草案：agent/thread 不应看到未绑定的 session/thread MCP tools。
- 在文档标注现状没有 session/thread binding，避免后续误判为已完成。

验收：

- `cargo test -p allthecodes-mcp --lib`
- `cargo test -p allthecodes-engine --lib mcp`

### Phase 1：数据模型与存储

改动范围：

- `crates/allthecodes-types/src/mcp.rs`
- `crates/allthecodes-ipc-protocol/src/subsystem_types.rs`
- `crates/allthecodes-config/src/settings/*`
- `crates/allthecodes-session/src/storage.rs` 或新建 session binding storage 模块

任务：

- 增加 `McpToolScope`、`McpPermission`、`McpBinding`。
- settings 支持 `mcpBindings` round-trip。
- session/thread binding 文件读写，路径必须在 `~/.allthecodes/runs/{session_id}` 下。
- 规范化 project path，拒绝空 session/thread id。

验收：

- settings serde round-trip。
- session binding storage create/list/delete。
- path isolation 测试确认不写 `~/.Codex`。

### Phase 2：Discovery 合并绑定

改动范围：

- `crates/allthecodes-mcp/src/discovery.rs`
- `crates/allthecodes/src/app_subsystem_handlers/snapshot.rs`
- `crates/allthecodes-web/src/handlers/mcp_servers.rs`

任务：

- `ScopedMcpServer` 保留来源 scope，同时输出稳定 `server_id`。
- 新增 `discover_mcp_bindings_scoped(cwd, session_id)`。
- 为旧 `mcpServers` 生成隐式 binding。
- `/mcp list` 和 Web 列表展示 server source scope 与 binding scope，避免混淆。

验收：

- user/project/plugin/ide 来源仍按当前优先级合并。
- 同名 server 多来源能在 config entries 中保留独立行。
- binding scope 不覆盖 config source scope。

### Phase 3：Runtime Context 与工具过滤

改动范围：

- `crates/allthecodes-mcp/src/manager.rs`
- `crates/allthecodes-mcp/src/runtime.rs`
- `crates/allthecodes-engine/src/mcp_tool_adapter.rs`
- `crates/allthecodes-engine/src/lifecycle/deps/model_call.rs`
- `crates/allthecodes/src/full_init.rs`

任务：

- 增加 `McpBindingContext`。
- `McpManager::all_tools()` 保留兼容，新增 `tools_for_context()`。
- tool wrapper 保存 binding metadata，并在 `call()` 时做二次权限检查。
- engine refresh MCP tools 时传入当前 session/thread context。
- 默认 main thread 工具列表和旧行为保持一致。

验收：

- session A 绑定的 MCP tool 不出现在 session B。
- thread A 绑定的 MCP tool 不出现在 thread B。
- 旧 global/project 配置仍出现在默认工具列表。
- 越权直接调用 wrapper 返回明确错误。

### Phase 4：Agent / Skill / Thread 接入

改动范围：

- `crates/allthecodes-engine/src/agent/mod.rs`
- `crates/allthecodes-engine/src/agent/builtin_agents.rs`
- `crates/allthecodes-engine/src/skill_tool.rs`
- `crates/allthecodes-ipc-protocol/src/subsystem_types.rs`

任务：

- agent spawn 使用 `AgentContext.agent_id` 作为 thread id。
- 实现 `AgentDefinitionEntry.mcp_servers` 解析：
  - string: server name/server id
  - object: 未来兼容 inline config
- agent `tools` allowlist 在 binding 过滤后再应用。
- skill fork context 不再直接继承全局 MCP tools。

验收：

- custom agent 只看到自身 `mcp_servers` 允许的 MCP server tools。
- builtin Explore 等 read-only agent 不因 MCP tools 默认 side-effect unknown 而误获得 MCP tools，除非显式允许。
- parent thread MCP binding 不泄漏给 sibling agent。

### Phase 5：控制面与 UX

改动范围：

- `crates/allthecodes-commands/src/mcp/*`
- `crates/allthecodes/src/app_subsystem_handlers/mcp.rs`
- `crates/allthecodes-ipc-protocol/src/subsystem_events.rs`
- `crates/allthecodes-web/src/handlers/mcp_servers.rs`
- `crates/allthecodes/src/ui/mcp/*`

任务：

- CLI 支持 bind/unbind/list bindings。
- IPC 支持 binding CRUD。
- Web REST 支持 binding CRUD。
- TUI `/mcp` 页面展示：
  - config source: user/project/plugin/ide
  - binding scope: global/project/session/thread
  - permissions

验收：

- CLI 创建 session binding 后当前 session 可见，重启/恢复 session 后仍可见。
- CLI 创建 thread binding 后只有目标 thread 可见。
- Web/IPC CRUD round-trip 不泄漏 token/env secret。

### Phase 6：回归与 release build

必跑：

```bash
export CARGO_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo
export RUSTUP_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/rustup
export PATH="$CARGO_HOME/bin:$PATH"

cargo test -p allthecodes-mcp --lib
cargo test -p allthecodes-ipc-protocol --lib
cargo test -p allthecodes-engine --lib
cargo test -p allthecodes-commands --lib
cargo test -p allthecodes-web --lib
cargo build --workspace --release
```

验收标准：

- 无新增 warning。
- 旧 MCP 配置无需迁移即可继续工作。
- session/thread 绑定隔离有单元测试和至少一个集成级路径覆盖。
- 文档补充到 `docs/WORK_STATUS.md` 或对应 completed 文档。

## 风险与决策点

- `serverId` 是否只用 server name：短期可行，长期同名多来源会冲突。建议第一阶段内部使用 `ResolvedMcpServerId`，UI 仍显示 name。
- thread id 定义必须统一：建议后端主导，不依赖 UI 当前选中 thread。
- session/thread 独立连接会增加资源占用；需要限制并发连接数和断开策略。
- MCP tools 当前 `is_read_only=false`，read-only agent 默认不应获得 MCP tools，除非 server/tool metadata 后续能证明只读。
- plugin manifest 是否允许直接声明 binding scope 需要产品决策。保守做法是 manifest 只贡献 server，用户/项目再显式绑定。

## 完成定义

本计划完成时，应满足：

1. 数据模型存在 `global/project/session/thread` 四级 binding。
2. 当前 engine/session/thread 只暴露匹配 binding 的 MCP tools。
3. MCP tool call 路径有权限二次校验。
4. agent 的 `mcp_servers` 字段实际生效。
5. CLI/IPC 至少能创建、列出、删除 binding。
6. 旧 `mcpServers` 配置保持兼容，且不会误认为已经是 session/thread 隔离。
