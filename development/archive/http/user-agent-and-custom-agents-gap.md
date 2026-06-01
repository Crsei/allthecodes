# HTTP User-Agent 与用户自定义 Agent 差距记录

> 记录日期：2026-05-18
>
> 对比对象：
> - Rust：`claude-code-rust`
> - Bun：`claude-code-bun`
>
> 本文只记录两个差距面：
> 1. HTTP `User-Agent` header 生成与使用策略
> 2. 用户自定义 Agent 的加载、路径与覆盖规则

## 1. HTTP User-Agent

### Bun 当前行为

Bun 侧已经把 HTTP `User-Agent` 拆成多个场景，并集中在 helper 中生成：

- 通用 Claude API / first-party API 请求：
  - 入口：`claude-code-bun/src/utils/http.ts`
  - 函数：`getUserAgent()`
  - 格式：
    - `claude-cli/${VERSION} (${USER_TYPE}, ${CLAUDE_CODE_ENTRYPOINT ?? 'cli'}...)`
  - 额外包含：
    - `CLAUDE_AGENT_SDK_VERSION` -> `agent-sdk/...`
    - `CLAUDE_AGENT_SDK_CLIENT_APP` -> `client-app/...`
    - workload tag -> `workload/...`
  - 代码注释明确说明 `claude-cli` 字符串被日志过滤依赖，不能随意修改。

- MCP / CCR / telemetry / settings sync 等 Claude Code 自身服务请求：
  - 入口：`claude-code-bun/src/utils/userAgent.ts`
  - 函数：`getClaudeCodeUserAgent()`
  - 格式：
    - `claude-code/${VERSION}`

- MCP 专用 User-Agent：
  - 入口：`claude-code-bun/src/utils/http.ts`
  - 函数：`getMCPUserAgent()`
  - 格式：
    - `claude-code/${VERSION}`
    - 可附加 `CLAUDE_CODE_ENTRYPOINT`、`agent-sdk/...`、`client-app/...`

- WebFetch / VaultHttpFetch 对外部站点请求：
  - 入口：`claude-code-bun/src/utils/http.ts`
  - 函数：`getWebFetchUserAgent()`
  - 格式：
    - `Claude-User (claude-code/${VERSION}; +https://support.anthropic.com/)`
  - 语义：
    - 使用公开 crawler/user-initiated fetch 身份
    - 同时带上 Claude Code 后缀，方便站点区分本地 CLI 流量

### Rust 当前行为

Rust 侧当前没有统一的 HTTP User-Agent helper，主要是分散硬编码：

- MCP client 身份：
  - 入口：`claude-code-rust/crates/cc-types/src/mcp.rs`
  - 常量：
    - `CLIENT_NAME = "claude-code-rs"`
    - `CLIENT_VERSION = "0.1.0"`
  - 使用点：
    - `claude-code-rust/crates/cc-mcp/src/client.rs`
    - `claude-code-rust/crates/cc-mcp/src/auth.rs`
  - 格式：
    - `claude-code-rs/0.1.0`

- WebFetch 对外部站点请求：
  - 入口：`claude-code-rust/crates/cc-tools/src/web_fetch.rs`
  - 当前格式：
    - `ClaudeCode/0.1 (Rust)`

### 主要差距

| 维度 | Bun | Rust | 差距 |
| --- | --- | --- | --- |
| 统一 helper | 有 `getUserAgent()` / `getClaudeCodeUserAgent()` / `getMCPUserAgent()` / `getWebFetchUserAgent()` | 无统一 helper | Rust 修改 User-Agent 需要逐点查找，容易漏改 |
| 主 API User-Agent | `claude-cli/${VERSION} (...)`，含 user type / entrypoint / SDK / client app / workload | 未发现同构实现 | 缺少可观测性字段和 SDK consumer 标识 |
| MCP User-Agent | `claude-code/${VERSION}`，可带 entrypoint / SDK / client app | `claude-code-rs/0.1.0` | 名称、版本来源、附加字段均不一致 |
| WebFetch User-Agent | `Claude-User (claude-code/${VERSION}; +https://support.anthropic.com/)` | `ClaudeCode/0.1 (Rust)` | Rust 不符合 Bun 的公开 fetch 身份语义 |
| 版本来源 | `MACRO.VERSION`，与 package 构建版本绑定 | `CLIENT_VERSION = "0.1.0"` 硬编码 | Rust 版本容易与 crate/package 真实版本漂移 |
| 可观测性 | User-Agent 含日志过滤和 workload 语义 | MCP/WebFetch 仅有固定字符串 | Rust 对流量归因、日志过滤、SDK 调试支持不足 |

### 影响

- MCP / OAuth / SSE / streamable HTTP 请求的客户端身份与 Bun 不一致。
- 外部站点看到的 WebFetch 身份不一致，可能影响 robots / allowlist / audit 规则。
- Rust 侧未来接入 SDK、remote、bridge、workload 或多 entrypoint 后，缺少统一扩展点。
- 固定 `0.1.0` 容易让线上排障误判版本。

### Rust 补齐建议

建议新增一个集中模块，例如：

- `crates/cc-http/src/user_agent.rs`，如果已有 HTTP 公共 crate
- 或 `crates/cc-config/src/user_agent.rs`，如果希望先避免新增 crate

建议提供函数：

- `claude_code_user_agent() -> String`
  - 对齐 Bun `getClaudeCodeUserAgent()`
  - 格式建议：`claude-code/${version}` 或保留迁移期 `claude-code-rs/${version}`，但需要明确兼容策略

- `api_user_agent() -> String`
  - 对齐 Bun `getUserAgent()`
  - 包含 entrypoint、SDK version、client app、workload

- `mcp_user_agent() -> String`
  - 对齐 Bun `getMCPUserAgent()`
  - 供 `cc-mcp` auth/client 统一调用

- `web_fetch_user_agent() -> String`
  - 对齐 Bun `getWebFetchUserAgent()`
  - 建议替换当前 `ClaudeCode/0.1 (Rust)`

还需要把版本来源改为构建期 crate/package version，避免单独硬编码：

- `env!("CARGO_PKG_VERSION")`
- 或 workspace 构建脚本生成的统一版本常量

## 2. 用户自定义 Agent

### Bun 当前行为

Bun 侧用户自定义 Agent 与官方 Claude Code 配置目录保持一致：

- 配置根：
  - `CLAUDE_CONFIG_DIR`
  - 默认 `~/.claude`
  - 入口：`claude-code-bun/src/utils/envUtils.ts`

- 用户 Agent 路径：
  - `${CLAUDE_CONFIG_DIR}/agents/*.md`
  - 默认 `~/.claude/agents/*.md`

- 项目 Agent 路径：
  - 从当前 cwd 向上查找 `.claude/agents`
  - 停止边界通常是 git root 或 home
  - 对 git worktree 有 fallback：worktree 缺少 `.claude/agents` 时，可加载 canonical main repo 的 `.claude/agents`
  - 入口：`claude-code-bun/src/utils/markdownConfigLoader.ts`
  - 函数：`loadMarkdownFilesForSubdir('agents', cwd)`

- Managed / policy Agent：
  - 从 managed settings 路径下的 `.claude/agents` 加载
  - source 为 `policySettings`

- Plugin Agent：
  - 从启用插件的 `agents/` 或 manifest 指定路径加载
  - 入口：`claude-code-bun/src/utils/plugins/loadPluginAgents.ts`

- CLI flag / JSON Agent：
  - 通过 JSON schema 解析
  - source 为 `flagSettings`

### Bun Agent 覆盖规则

Bun 的 active agent 计算在：

- `claude-code-bun/packages/builtin-tools/src/tools/AgentTool/loadAgentsDir.ts`
- 函数：`getActiveAgentsFromList()`

加载后按下面顺序写入同一个 `Map<agentType, AgentDefinition>`：

1. `built-in`
2. `plugin`
3. `userSettings`
4. `projectSettings`
5. `flagSettings`
6. `policySettings`

因为后写覆盖先写，所以实际优先级为：

`policySettings` > `flagSettings` > `projectSettings` > `userSettings` > `plugin` > `built-in`

展示层也有统一 source 分组：

- `User agents`
- `Project agents`
- `Local agents`
- `Managed agents`
- `Plugin agents`
- `CLI arg agents`
- `Built-in agents`

入口：

- `claude-code-bun/packages/builtin-tools/src/tools/AgentTool/agentDisplay.ts`

### Bun Agent 文件能力

Bun markdown / JSON agent 支持的字段更完整，包括：

- `name`
- `description`
- `tools`
- `disallowedTools`
- `model`
- `effort`
- `permissionMode`
- `mcpServers`
- `hooks`
- `maxTurns`
- `skills`
- `initialPrompt`
- `memory`
- `background`
- `isolation`

并且有额外行为：

- memory agent 自动注入 `Read` / `Write` / `Edit` 工具
- `requiredMcpServers` 可用于 MCP 可用性过滤
- plugin agent 会忽略部分高权限字段，避免插件侧绕过安全边界
- source 可被设置开关禁用
- 支持同 inode 去重，避免 symlink / hardlink 重复加载

### Rust 当前行为

Rust 侧使用隔离配置目录，不直接复用 Bun 的 `.claude` 路径：

- 数据根：
  - `CC_RUST_HOME`
  - 默认 `~/.cc-rust`
  - 入口：`claude-code-rust/crates/cc-config/src/paths.rs`

- UI 侧 Agent 目录常量：
  - `AGENT_FOLDER_NAME = ".cc-rust"`
  - `AGENTS_DIR = "agents"`
  - 入口：`claude-code-rust/crates/claude-code-rs/src/ui/agents/types.rs`

- User Agent 路径：
  - `${CC_RUST_HOME}/agents/*.md`

- Project Agent 路径：
  - `${cwd}/.cc-rust/agents/*.md`

- 当前 service 侧加载入口：
  - `claude-code-rust/crates/cc-services/src/agent_definitions/mod.rs`
  - 函数：`list_all_agents(cwd)`
  - 当前只加载：
    - built-in
    - user
    - project

Rust UI 类型里已经存在这些 source：

- `User`
- `Project`
- `Local`
- `Policy`
- `Flag`
- `BuiltIn`
- `Plugin`

但 service 侧实际 loader 尚未完整加载 Local / Policy / Flag / Plugin。

### Rust Agent 覆盖与展示差异

Rust UI 工具里有 source 排序：

- 入口：`claude-code-rust/crates/claude-code-rs/src/ui/agents/utils.rs`
- 函数：`source_rank()`

当前排序为：

1. `Project`
2. `Local`
3. `User`
4. `Policy`
5. `Flag`
6. `Plugin`
7. `BuiltIn`

这只是展示排序，不等价于 Bun 的 active override 规则。

当前 Rust service 侧 `list_all_agents()` 返回所有 agent，但没有看到与 Bun `getActiveAgentsFromList()` 同构的 active-agent 覆盖计算。UI 类型里有 `overridden_by` 字段，列表也能显示 `shadowed by ...`，但 service 加载和 winner 选择还没有 Bun 那样完整。

### 主要差距

| 维度 | Bun | Rust | 差距 |
| --- | --- | --- | --- |
| 配置根 | `CLAUDE_CONFIG_DIR` / `~/.claude` | `CC_RUST_HOME` / `~/.cc-rust` | 路径隔离明确，但不兼容 Bun / 官方 Claude Code agent 目录 |
| 项目目录 | `.claude/agents`，支持向上查找 | `${cwd}/.cc-rust/agents` | Rust 只看当前 cwd 下的 `.cc-rust/agents`，缺少 Bun 的层级查找 |
| git worktree fallback | 有 canonical main repo fallback | 未发现同构实现 | worktree/sparse checkout 下 agent 可见性不同 |
| Managed / Policy | 已加载 `policySettings` | 类型存在，service 未完整加载 | 管理策略 agent 不同构 |
| Plugin | 已加载 enabled plugins 的 agents | 类型存在，service 未完整加载 | 插件 agent 不同构 |
| CLI flag / JSON | 支持 `flagSettings` JSON agent | 类型存在，service 未完整加载 | CLI 注入 agent 不同构 |
| 覆盖规则 | 明确 active winner：policy > flag > project > user > plugin > built-in | 未发现同构 active winner 计算 | 同名 agent 冲突处理可能不一致 |
| 去重 | 基于 inode 去重 | 未发现同构实现 | symlink/hardlink 场景可能重复 |
| agent 字段 | 支持完整 markdown / JSON 字段 | parser 覆盖较多字段，但 hooks/mcpServers 只支持单行 JSON，生态行为较少 | 配置兼容性不完整 |
| source 开关 | 可按 setting source 禁用 | 未发现同构实现 | enterprise / restricted mode 行为不同 |

### 影响

- 现有 Bun / 官方 Claude Code 用户的 `~/.claude/agents` 不会自动被 Rust 读取。
- 项目中已有 `.claude/agents` 的 agent 配置不会被 Rust 读取，除非迁移到 `.cc-rust/agents`。
- 同名 agent 在 Rust 侧的 winner 可能与 Bun 不同。
- plugin / policy / CLI 注入 agent 在 Rust 侧还不能作为完整可用面。
- worktree 中运行 Rust 时，agent 可见性和 Bun 不一致。

### Rust 补齐建议

需要先明确产品取舍：

1. 如果坚持路径隔离：
   - 保留 `.cc-rust/agents`
   - 文档中明确这是 Rust 专属配置面
   - 增加迁移工具或兼容导入命令，例如从 `.claude/agents` 复制到 `.cc-rust/agents`

2. 如果追求 Bun / 官方 Claude Code parity：
   - 增加可选兼容读取 `.claude/agents`
   - 读取顺序需要避免与 `.cc-rust/agents` 产生不可解释的覆盖关系
   - 建议用 feature/env 控制，例如 `CC_RUST_LOAD_CLAUDE_AGENTS=1`

建议实现顺序：

1. 抽出 Rust 侧统一 `AgentDefinitionsLoader`
   - 输入：cwd、home/data root、managed root、plugin registry、CLI flag definitions
   - 输出：`all_agents`、`active_agents`、`failed_files`

2. 实现 Bun 同构 source priority
   - `built-in`
   - `plugin`
   - `user`
   - `project`
   - `flag`
   - `policy`
   - winner 规则与展示排序分离

3. 补齐 source 加载
   - Local
   - Policy
   - Flag / JSON
   - Plugin

4. 补齐 project directory discovery
   - 从 cwd 向上查找 agent 目录
   - git root / home stop boundary
   - worktree canonical repo fallback

5. 补齐去重与 parse error reporting
   - inode / canonical path 去重
   - 保留 failed file 列表，供 UI 展示

6. 明确 `.cc-rust` 与 `.claude` 的兼容策略
   - 这是最容易影响用户预期的差异，应在实现前先定规则

## 总结

- HTTP `User-Agent`：Rust 当前是分散硬编码，Bun 是集中 helper + 多场景身份。Rust 应优先补一个统一 user-agent 模块，再替换 MCP / WebFetch / API 请求使用点。
- 用户自定义 Agent：Rust 当前偏 `.cc-rust` 隔离实现，只覆盖 built-in/user/project 基础加载；Bun 已有完整 source、覆盖、插件、policy、CLI flag、worktree fallback 和去重机制。Rust 若要 parity，需要先补 loader 架构和覆盖规则，再决定是否兼容 `.claude/agents`。
