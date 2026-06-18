# 真实 Memory / Skill / MCP / Plugin / LSP / Agent Teams E2E 测试计划

日期：2026-05-24
适用仓库：`claude-code-rust`
目标阶段：Full Build

## 1. 目标

基于真实 PTY TUI e2e harness，构建一组可重复的在线集成测试，验证以下能力不是只在命令列表中可见，而是能被真实 agent/subagent 读到并使用：

- memory file：`CLAUDE.md`、`.cc-rust/memory`、team memory scope 写入后，主 agent 和 team teammate 都能读取到。
- skill：项目级 skill 保存后能被 `/skills` 和 `/agents` 发现；真实对话中能触发并读取引用文件。
- MCP：项目配置或插件贡献的 MCP server 启用后，agent 能看到对应工具并完成一次真实工具调用。
- plugin：本地插件安装/启用/刷新后，其 skill、MCP、command、LSP 声明能进入当前 session。
- LSP 插件：插件提供 `lspServers` 后，`/lsp`、recommendation/status surface 能显示配置，agent 能获得可读诊断或能力状态。
- Agent Teams：`/team create/spawn/send/kill` 能驱动真实 teammate，teammate 能继承必要的 memory、skill、MCP、plugin/LSP 上下文。

本计划补足现有命令冒烟计划的缺口：不只检查 `/memory`、`/skills`、`/mcp`、`/plugin`、`/team` 不 panic，还要检查“应用后 agent 是否能读到并正常使用”。

说明：仓库当前没有名为 `memoryfile` 的独立 skill。这里的 memoryfile 测试按两条路径覆盖：内置 `remember` skill 写入/更新 `CLAUDE.md`，以及 `/memory` 管理的 global/project/team/auto memdir 文件。

## 2. 参考依据

- `crates/claude-code-rs/tests/pty_tui_e2e/README.md`：真实 PTY harness、`TestCase` / `TestStep`、在线 ignored 测试、截图与日志目录。
- `development/archive/COMMAND_REFERENCE.md`：`/memory`、`/skills`、`/agents`、`/mcp`、`/tasks`、`/team`、`/plugin` 命令语义。
- `crates/claude-code-rs/tests/capability_lab_support/mod.rs`：已有 `CapabilityLab` 夹具，包含真实项目、项目级 skill、MCP server 配置、插件 fixture、路径隔离断言。
- `crates/cc-commands/src/lsp_cmd.rs`：`/lsp status`、`/lsp recommendations`、`/lsp recommend <language>` 当前实现入口。
- `crates/cc-plugins/src/lsp.rs`：插件 manifest 的 `lspServers` / `lsp_servers` 声明收集逻辑。
- `crates/cc-commands/src/agents_cmd.rs`：只有 `context: fork` 或声明 `agent` 的 skill 会出现在 `/agents` 的 skill-backed agent 分组。
- `crates/cc-engine/src/skill_tool.rs`：模型侧 `Skill` tool 会执行 inline skill 或 fork skill，是验证“agent 真能使用 skill”的主要观测点。
- `crates/cc-engine/src/mcp_tool_adapter.rs`：模型侧 MCP tool wrapper 调用 `client.call_tool`，可用工具名形如 `mcp__<server>__<tool>`。
- `crates/cc-lsp-service/src/tool.rs`：模型侧 `LSP` tool 覆盖 definition、hover、references、symbols、diagnostics、completion 等操作。

## 3. 测试文件布局

新增测试文件：

```text
crates/claude-code-rs/tests/pty_tui_e2e/capability_lab.rs
```

更新测试入口：

```text
crates/claude-code-rs/tests/pty_tui_e2e/main.rs
```

复用夹具：

```text
crates/claude-code-rs/tests/capability_lab_support/mod.rs
```

必要时扩展夹具，但保持所有写入在临时目录、测试 project、测试 HOME、`CC_RUST_HOME` 下。

## 4. 运行方式

离线命令与发现测试：

```bash
cargo test -p claude-code-rs --test pty_tui_e2e capability_lab_offline -- --nocapture
```

真实在线 agent 测试：

```bash
cargo test -p claude-code-rs --test pty_tui_e2e -- --ignored capability_lab_online --nocapture
```

真实 MCP / plugin 安装测试需要本机具备 `npx`，Git MCP 需要 `uvx`。网络或 package cache 不可用时，测试应 `skip` 或输出明确前置条件失败，不能伪造成功。

## 5. 夹具增强

在 `CapabilityLab` 上增加以下 fixture 数据：

1. Project memory file：
   - `CLAUDE.md` 写入唯一 sentinel：`CCRUST_PROJECT_MEMORY_SENTINEL_20260524`。
   - 内容要求所有总结写到 `docs/agent-readable-report.md`。

2. Memdir memory：
   - `.cc-rust/memory/project-style.json` 或通过 `/memory set project-style ...` 写入：`CCRUST_MEMDIR_SENTINEL_20260524`。
   - team memory scope 写入：`CCRUST_TEAM_MEMORY_SENTINEL_20260524`。

3. Project skill：
   - `.cc-rust/skills/product-brief-writer/SKILL.md` 增加 `context: fork` 或 `agent: product-brief-agent`，确保 `/agents` 能看到 skill-backed agent。
   - `references/style-guide.md` 写入：`CCRUST_SKILL_REFERENCE_SENTINEL_20260524`。

4. Local plugin fixture：
   - `plugin.json` 保留 `skills`、`commands`、`mcp_servers`。
   - 增加 `lspServers`：

```json
{
  "typescript-capability-lsp": {
    "languageId": "typescript",
    "extensions": [".ts", ".tsx"],
    "command": "typescript-language-server",
    "args": ["--stdio"]
  }
}
```

5. Plugin skill：
   - `plugin-review/SKILL.md` 增加 `context: fork` 或 `agent: plugin-review-agent`。
   - `references/rubric.md` 写入：`CCRUST_PLUGIN_SKILL_SENTINEL_20260524`。

## 6. 测试矩阵

| ID | 类型 | 目标 | 关键断言 |
|---|---|---|---|
| CL-OFF-01 | 离线 | `/memory path/list/get/search` 能看到 project/global/team/auto scope | 屏幕包含 sentinel key、无 panic、无上游路径 |
| CL-OFF-02 | 离线 | `/skills reload` 后发现 project skill | `/skills product-brief-writer` 包含 skill 名和 reference 路径提示 |
| CL-OFF-03 | 离线 | `/agents list` 发现 skill-backed agent | 包含 `product-brief-agent` 或 `product-brief-writer`，source 为 project skill |
| CL-OFF-04 | 离线 | `/mcp list/status` 读取项目 `.cc-rust/settings.json` | 包含 filesystem/sequential/playwright，配置错误可见 |
| CL-OFF-05 | 离线 | 本地 plugin 安装、enable、reload | `/plugin list/status` 包含 installed/enabled/active |
| CL-OFF-06 | 离线 | plugin skill 进入 `/skills` 和 `/agents` | 包含 `plugin-review` 与 plugin source |
| CL-OFF-07 | 离线 | plugin LSP 声明进入 `/lsp` | `/lsp status` 或 `/lsp recommendations --all` 包含 TypeScript/LSP 状态或清晰未启动原因 |
| CL-ON-01 | 在线 | 主 agent 读取 memory file + memdir | 回复包含两个 memory sentinel，并按 `CLAUDE.md` 写报告 |
| CL-ON-02 | 在线 | 主 agent 触发 project skill 并读 reference | 回复或报告包含 `CCRUST_SKILL_REFERENCE_SENTINEL_20260524` |
| CL-ON-03 | 在线 | agent 使用 filesystem MCP 读取文档 | 输出说明来自 MCP 读取，报告包含 `Product Brief` 事实 |
| CL-ON-04 | 在线 | agent 使用 plugin-contributed MCP | sequential MCP 工具调用成功或返回可读工具错误，主进程不退出 |
| CL-ON-05 | 在线 | agent 可感知 plugin LSP 状态 | 回复包含 TypeScript LSP 插件状态、可用/不可用原因、无 panic |
| CL-TEAM-01 | 在线 | 创建 team 并 spawn teammate | `/team list` 和 `/tasks` 能看到 teammate/task |
| CL-TEAM-02 | 在线 | teammate 读取 memory + skill | teammate 回复包含 project/team memory sentinel 和 skill reference sentinel |
| CL-TEAM-03 | 在线 | teammate 使用 MCP/plugin context | teammate 能总结 MCP 可用工具或完成一次 filesystem MCP 读取 |
| CL-TEAM-04 | 在线 | kill/delete 清理 | `/team kill`、`/team delete` 后无残留 running task，路径隔离断言通过 |

## 7. 测试用例步骤

### CL-OFF-01：memory scopes 可见
```rust
fn capability_lab_offline_memory_scopes_visible() {
    // 步骤：
    // 1. 构建 CapabilityLab，写入 CLAUDE.md、project/global/team/auto memory sentinel。
    // 2. 使用临时 HOME、CC_RUST_HOME、E2E_WORKSPACE 启动 PTY TUI。
    // 3. SkipTrustGate。
    // 4. Command("memory path")。
    // 5. WaitForAny(vec!["CLAUDE.md", "Project Instructions"], QUICK_TIMEOUT)。
    // 6. Command("memory list")。
    // 7. WaitForText("project-style", QUICK_TIMEOUT)。
    // 8. Command("memory get project-style")。
    // 9. WaitForText("CCRUST_MEMDIR_SENTINEL_20260524", QUICK_TIMEOUT)。
    // 10. Command("memory search CCRUST")。
    // 11. AssertNoPanic。
    // 12. Snapshot("memory_scopes_visible")。
    // 断言：memory path/list/get/search 均能看到测试 sentinel，且 lab.assert_path_isolated() 通过。
}
```

### CL-OFF-02：project skill reload 后可发现
```rust
fn capability_lab_offline_project_skill_reload() {
    // 步骤：
    // 1. 构建 CapabilityLab，确保 .cc-rust/skills/product-brief-writer/SKILL.md 存在。
    // 2. SKILL.md frontmatter 包含 name、description、allowed-tools、version。
    // 3. 启动 PTY TUI 并 SkipTrustGate。
    // 4. Command("skills reload")。
    // 5. WaitForAny(vec!["product-brief-writer", "Reloaded", "Available Skills"], QUICK_TIMEOUT)。
    // 6. Command("skills product-brief-writer")。
    // 7. WaitForText("product-brief-writer", QUICK_TIMEOUT)。
    // 8. WaitForAny(vec!["project", "allowed", "Skill"], QUICK_TIMEOUT)。
    // 9. AssertNoPanic。
    // 10. Snapshot("project_skill_reload")。
    // 断言：project skill 已进入 registry，并且详情可读。
}
```

### CL-OFF-03：skill-backed agent 可发现
```rust
fn capability_lab_offline_skill_backed_agent_visible() {
    // 步骤：
    // 1. 构建 CapabilityLab，将 project skill frontmatter 设置为 context: fork 或 agent: product-brief-agent。
    // 2. 启动 PTY TUI 并 SkipTrustGate。
    // 3. Command("skills reload")。
    // 4. WaitForText("product-brief-writer", QUICK_TIMEOUT)。
    // 5. Command("agents list")。
    // 6. WaitForAny(vec!["product-brief-agent", "product-brief-writer"], QUICK_TIMEOUT)。
    // 7. WaitForAny(vec!["Project skills", "project", "fork"], QUICK_TIMEOUT)。
    // 8. Command("agents show product-brief-writer")。
    // 9. WaitForAny(vec!["product-brief-writer", "product-brief-agent", "not found"], QUICK_TIMEOUT)。
    // 10. AssertNoPanic。
    // 11. Snapshot("skill_backed_agent_visible")。
    // 断言：fork/agent skill 会出现在 /agents；若 show 使用 canonical name 不匹配，输出必须是可读 not found 而非 panic。
}
```

### CL-OFF-04：项目 MCP 配置可发现
```rust
fn capability_lab_offline_mcp_project_settings_visible() {
    // 步骤：
    // 1. 构建 CapabilityLab，并通过 write_project_mcp_settings 写入 filesystem、sequential-thinking、playwright 配置。
    // 2. 启动 PTY TUI 并 SkipTrustGate。
    // 3. Command("mcp list")。
    // 4. WaitForAny(vec!["filesystem", "sequential", "playwright", "MCP"], QUICK_TIMEOUT)。
    // 5. Command("mcp status")。
    // 6. WaitForAny(vec!["filesystem", "mcpServers", "discovery"], QUICK_TIMEOUT)。
    // 7. AssertNoPanic。
    // 8. Snapshot("mcp_project_settings_visible")。
    // 断言：项目 .cc-rust/settings.json 的 MCP server 名称可见；连接失败时错误必须可读。
}
```

### CL-OFF-05：本地 plugin 安装、启用、刷新
```rust
fn capability_lab_offline_plugin_install_enable_reload() {
    // 步骤：
    // 1. 构建 CapabilityLab，并调用 write_plugin_fixture 生成本地 plugin 目录。
    // 2. 启动 PTY TUI 并 SkipTrustGate。
    // 3. Command(format!("plugin install {}", plugin_dir.display()))。
    // 4. WaitForAny(vec!["installed", "Plugin installed", "capability-plugin"], QUICK_TIMEOUT)。
    // 5. Command("plugin enable capability-plugin")。
    // 6. WaitForAny(vec!["enabled", "capability-plugin"], QUICK_TIMEOUT)。
    // 7. Command("reload-plugins")。
    // 8. WaitForAny(vec!["Reloaded", "plugin reload", "capability-plugin"], QUICK_TIMEOUT)。
    // 9. Command("plugin status")。
    // 10. WaitForAny(vec!["installed", "enabled", "active"], QUICK_TIMEOUT)。
    // 11. AssertNoPanic。
    // 12. Snapshot("plugin_install_enable_reload")。
    // 断言：plugin 同时处于 installed、enabled、active；所有路径在 CC_RUST_HOME 下。
}
```

### CL-OFF-06：plugin skill 进入 skills 和 agents
```rust
fn capability_lab_offline_plugin_skill_visible() {
    // 步骤：
    // 1. 构建 CapabilityLab，安装并启用 capability-plugin。
    // 2. Command("reload-plugins")。
    // 3. Command("skills reload")。
    // 4. WaitForText("plugin-review", QUICK_TIMEOUT)。
    // 5. Command("skills plugin-review")。
    // 6. WaitForAny(vec!["plugin-review", "plugin", "Capability Plugin"], QUICK_TIMEOUT)。
    // 7. Command("agents list")。
    // 8. WaitForAny(vec!["plugin-review", "plugin-review-agent", "Plugin-provided"], QUICK_TIMEOUT)。
    // 9. AssertNoPanic。
    // 10. Snapshot("plugin_skill_visible")。
    // 断言：plugin skill 可通过 /skills 查看；声明 fork/agent 后也进入 /agents。
}
```

### CL-OFF-07：plugin LSP 声明可见
```rust
fn capability_lab_offline_plugin_lsp_visible() {
    // 步骤：
    // 1. 构建 CapabilityLab，安装包含 lspServers 的 capability-plugin。
    // 2. Command("reload-plugins")。
    // 3. Command("lsp status")。
    // 4. WaitForAny(vec!["LSP server status", "typescript", "No LSP servers configured"], QUICK_TIMEOUT)。
    // 5. Command("lsp recommendations --all")。
    // 6. WaitForAny(vec!["LSP recommendation", "typescript", "Recommendations"], QUICK_TIMEOUT)。
    // 7. AssertNoPanic。
    // 8. Snapshot("plugin_lsp_visible")。
    // 断言：如果 LSP server 未启动，输出必须给出清晰状态；如果已接线，必须包含 TypeScript language id 或插件来源。
}
```

### CL-ON-01：主 agent 读取 memory file 和 memdir
```rust
#[ignore = "requires real API key and real memory integration"]
fn capability_lab_online_agent_reads_memory_file_and_memdir() {
    // 步骤：
    // 1. 构建 CapabilityLab，写入 CLAUDE.md 和 project memdir sentinel。
    // 2. 启动在线 PTY TUI，设置 HOME、CC_RUST_HOME、E2E_WORKSPACE。
    // 3. SkipTrustGate。
    // 4. SetPermission("full access")。
    // 5. Input("Read project instructions and relevant memory. Reply with PROJECT_MEMORY and MEMDIR_MEMORY exact sentinel values only.")。
    // 6. WaitForText("CCRUST_PROJECT_MEMORY_SENTINEL_20260524", API_TIMEOUT)。
    // 7. WaitForText("CCRUST_MEMDIR_SENTINEL_20260524", API_TIMEOUT)。
    // 8. WaitForAny(vec!["docs/agent-readable-report.md", "REPORT_PATH"], API_TIMEOUT)。
    // 9. AssertNoPanic。
    // 10. Snapshot("agent_reads_memory_file_and_memdir")。
    // 断言：最终回复和生成报告至少一个包含两个 sentinel；路径隔离通过。
}
```

### CL-ON-02：主 agent 触发 project skill 并读取 reference
```rust
#[ignore = "requires real API key and real skill invocation"]
fn capability_lab_online_agent_uses_project_skill_reference() {
    // 步骤：
    // 1. 构建 CapabilityLab，project skill reference 写入 CCRUST_SKILL_REFERENCE_SENTINEL_20260524。
    // 2. 启动在线 PTY TUI 并 SkipTrustGate。
    // 3. SetPermission("full access")。
    // 4. Command("skills reload")。
    // 5. WaitForText("product-brief-writer", QUICK_TIMEOUT)。
    // 6. Input("Use the product brief writer skill to write docs/skill-report.md. Include the exact style-guide sentinel you read.")。
    // 7. WaitForText("CCRUST_SKILL_REFERENCE_SENTINEL_20260524", API_TIMEOUT)。
    // 8. WaitForAny(vec!["docs/skill-report.md", "skill-report"], API_TIMEOUT)。
    // 9. AssertNoPanic。
    // 10. Snapshot("agent_uses_project_skill_reference")。
    // 断言：agent 不能只复述 skill 名称，必须读取 reference sentinel。
}
```

### CL-ON-03：主 agent 使用 filesystem MCP
```rust
#[ignore = "requires real API key, npx, and filesystem MCP"]
fn capability_lab_online_agent_uses_filesystem_mcp() {
    // 步骤：
    // 1. 构建 CapabilityLab，写入 filesystem MCP server 配置。
    // 2. 若 npx 不可用，显式 skip 并记录原因。
    // 3. 启动在线 PTY TUI 并 SkipTrustGate。
    // 4. SetPermission("full access")。
    // 5. Command("mcp status")。
    // 6. WaitForAny(vec!["filesystem", "connected", "MCP"], API_TIMEOUT)。
    // 7. Input("Use the filesystem MCP server, not Bash, to read docs/product-brief.md. Write docs/mcp-read-report.md and mention the MCP server name.")。
    // 8. WaitForAny(vec!["filesystem", "mcp__filesystem", "mcp-read-report"], API_TIMEOUT)。
    // 9. WaitForText("Product Brief", API_TIMEOUT)。
    // 10. AssertNoPanic。
    // 11. Snapshot("agent_uses_filesystem_mcp")。
    // 断言：session log 包含 MCP tool 名或 MCP server 名；报告包含 product brief 事实。
}
```

### CL-ON-04：主 agent 使用 plugin-contributed MCP
```rust
#[ignore = "requires real API key, npx, and plugin MCP"]
fn capability_lab_online_agent_uses_plugin_contributed_mcp() {
    // 步骤：
    // 1. 构建 CapabilityLab，安装启用包含 plugin-sequential MCP 的 capability-plugin。
    // 2. 若 npx 不可用，显式 skip 并记录原因。
    // 3. 启动在线 PTY TUI 并 SkipTrustGate。
    // 4. SetPermission("full access")。
    // 5. Command("reload-plugins")。
    // 6. Command("mcp list")。
    // 7. WaitForAny(vec!["plugin-sequential", "sequential", "capability-plugin"], API_TIMEOUT)。
    // 8. Input("Use the plugin-provided sequential-thinking MCP tool to produce a 3 step plan for fixing the issue tracker test.")。
    // 9. WaitForAny(vec!["plugin-sequential", "sequential", "3 step", "Step 1"], API_TIMEOUT)。
    // 10. AssertNoPanic。
    // 11. Snapshot("agent_uses_plugin_contributed_mcp")。
    // 断言：MCP 成功时有工具结果；失败时必须是可读工具错误且主进程保持可交互。
}
```

### CL-ON-05：主 agent 感知 plugin LSP 状态
```rust
#[ignore = "requires real API key and plugin LSP fixture"]
fn capability_lab_online_agent_reads_plugin_lsp_status() {
    // 步骤：
    // 1. 构建 CapabilityLab，安装启用包含 TypeScript lspServers 的 capability-plugin。
    // 2. 启动在线 PTY TUI 并 SkipTrustGate。
    // 3. SetPermission("full access")。
    // 4. Command("reload-plugins")。
    // 5. Command("lsp status")。
    // 6. WaitForAny(vec!["LSP server status", "typescript", "No LSP servers configured"], QUICK_TIMEOUT)。
    // 7. Input("Check plugin and LSP status for this TypeScript project. Reply with plugin id, language id, and active or failed reason.")。
    // 8. WaitForAny(vec!["capability-plugin", "typescript", "LSP"], API_TIMEOUT)。
    // 9. AssertNoPanic。
    // 10. Snapshot("agent_reads_plugin_lsp_status")。
    // 断言：agent 能把 plugin id 与 TypeScript LSP 状态联系起来；不可用时说明原因。
}
```

### CL-TEAM-01：创建 team 并 spawn teammate
```rust
#[ignore = "requires real API key and agent teams feature"]
fn capability_lab_team_create_and_spawn_teammate() {
    // 步骤：
    // 1. 构建 CapabilityLab，设置 CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1。
    // 2. 启动在线 PTY TUI 并 SkipTrustGate。
    // 3. SetPermission("full access")。
    // 4. Command("team create capability-team Capability lab team")。
    // 5. WaitForAny(vec!["capability-team", "created", "active"], QUICK_TIMEOUT)。
    // 6. Command("team spawn capability-reader Read project status and wait for follow-up")。
    // 7. WaitForAny(vec!["capability-reader", "spawned", "running"], API_TIMEOUT)。
    // 8. Command("team list")。
    // 9. WaitForText("capability-reader", QUICK_TIMEOUT)。
    // 10. Command("tasks")。
    // 11. WaitForAny(vec!["capability-reader", "team", "running", "completed"], QUICK_TIMEOUT)。
    // 12. AssertNoPanic。
    // 13. Snapshot("team_create_and_spawn_teammate")。
    // 断言：team file、member 和 task surface 都能观察到 teammate。
}
```

### CL-TEAM-02：teammate 读取 memory 和 skill
```rust
#[ignore = "requires real API key, agent teams, and skill integration"]
fn capability_lab_team_teammate_reads_memory_and_skill() {
    // 步骤：
    // 1. 构建 CapabilityLab，写入 project memory、team memory、project skill reference sentinel。
    // 2. 设置 CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1 和 FEATURE_TEAMMEM=1。
    // 3. 启动在线 PTY TUI 并 SkipTrustGate。
    // 4. SetPermission("full access")。
    // 5. Command("skills reload")。
    // 6. Command("team create capability-team Capability lab team")。
    // 7. Command("team spawn capability-reader Read project memory, team memory, and product brief skill reference. Reply with exact sentinels.")。
    // 8. WaitForText("CCRUST_PROJECT_MEMORY_SENTINEL_20260524", API_TIMEOUT)。
    // 9. WaitForText("CCRUST_TEAM_MEMORY_SENTINEL_20260524", API_TIMEOUT)。
    // 10. WaitForText("CCRUST_SKILL_REFERENCE_SENTINEL_20260524", API_TIMEOUT)。
    // 11. AssertNoPanic。
    // 12. Snapshot("teammate_reads_memory_and_skill")。
    // 断言：teammate 继承/读取 project memory、team memory 和 skill reference。
}
```

### CL-TEAM-03：teammate 使用 MCP/plugin context
```rust
#[ignore = "requires real API key, agent teams, npx, and plugin MCP"]
fn capability_lab_team_teammate_uses_mcp_plugin_context() {
    // 步骤：
    // 1. 构建 CapabilityLab，写入 filesystem MCP 配置并安装 capability-plugin。
    // 2. 若 npx 不可用，显式 skip。
    // 3. 设置 CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1。
    // 4. 启动在线 PTY TUI 并 SkipTrustGate。
    // 5. SetPermission("full access")。
    // 6. Command("reload-plugins")。
    // 7. Command("team create capability-team Capability lab team")。
    // 8. Command("team spawn capability-reader Use available MCP/plugin context to read docs/product-brief.md and report MCP status.")。
    // 9. WaitForAny(vec!["filesystem", "plugin-sequential", "MCP"], API_TIMEOUT)。
    // 10. WaitForText("Product Brief", API_TIMEOUT)。
    // 11. Command("tasks")。
    // 12. WaitForAny(vec!["capability-reader", "completed", "running"], QUICK_TIMEOUT)。
    // 13. AssertNoPanic。
    // 14. Snapshot("teammate_uses_mcp_plugin_context")。
    // 断言：teammate 能看到 MCP/plugin context；失败路径必须显示可读 MCP/plugin 错误。
}
```

### CL-TEAM-04：team cleanup
```rust
#[ignore = "requires real API key and agent teams feature"]
fn capability_lab_team_cleanup() {
    // 步骤：
    // 1. 构建 CapabilityLab，设置 CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1。
    // 2. 启动在线 PTY TUI 并 SkipTrustGate。
    // 3. Command("team create capability-team Cleanup test")。
    // 4. Command("team spawn capability-reader Wait until killed")。
    // 5. WaitForAny(vec!["capability-reader", "spawned", "running"], API_TIMEOUT)。
    // 6. Command("team kill capability-reader")。
    // 7. WaitForAny(vec!["killed", "stopped", "capability-reader"], QUICK_TIMEOUT)。
    // 8. Command("team delete capability-team")。
    // 9. WaitForAny(vec!["deleted", "capability-team"], QUICK_TIMEOUT)。
    // 10. Command("tasks")。
    // 11. AssertScreenNotContains("running capability-reader")。
    // 12. AssertNoPanic。
    // 13. Snapshot("team_cleanup")。
    // 14. lab.assert_path_isolated()。
    // 断言：teammate 被停止，team 被删除，没有 running team task 残留。
}
```

## 8. 推荐测试脚本形态

优先使用 `TestCase` / `TestStep`，每个阶段保留截图：

```rust
let case = TestCase::new("capability_lab_memory_skill_agent")
    .cols(140)
    .rows(44)
    .env("HOME", lab.home_dir.to_string_lossy())
    .env("CC_RUST_HOME", lab.cc_rust_home.to_string_lossy())
    .env("E2E_WORKSPACE", lab.project_dir.to_string_lossy())
    .env("CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS", "1")
    .env("FEATURE_TEAMMEM", "1")
    .timeout(Duration::from_secs(240))
    .step(TestStep::SkipTrustGate)
    .step(TestStep::Command("memory path".into()))
    .step(TestStep::AssertNoPanic)
    .step(TestStep::Command("skills reload".into()))
    .step(TestStep::Command("agents list".into()))
    .step(TestStep::Snapshot("discovery".into()))
    .step(TestStep::Input(
        "Read project memory and use the product brief skill. Reply with the exact memory and skill sentinels you found.".into(),
    ))
    .step(TestStep::WaitForText("CCRUST_PROJECT_MEMORY_SENTINEL_20260524".into(), API_TIMEOUT))
    .step(TestStep::WaitForText("CCRUST_SKILL_REFERENCE_SENTINEL_20260524".into(), API_TIMEOUT))
    .step(TestStep::Snapshot("agent_read_memory_skill".into()));
```

需要精确验证工具调用时，直接使用 `PtySession` 并检查 `current_text()` / `session_full.log` 中的 tool event 文本；不要只看最终自然语言。

## 9. 真实 Agent Prompts

主 agent memory/skill prompt：

```text
Read the project instructions and relevant memory files. Use the product brief writer skill if it is available. Reply with only:
PROJECT_MEMORY=<sentinel>
MEMDIR_MEMORY=<sentinel>
SKILL_REFERENCE=<sentinel>
REPORT_PATH=<path you wrote>
```

MCP prompt：

```text
Use the filesystem MCP server, not Bash, to read docs/product-brief.md. Then write docs/mcp-read-report.md with the first requirement and mention which MCP server was used.
```

Plugin/LSP prompt：

```text
Check the active plugin and LSP status for this TypeScript project. Use plugin-provided capabilities if available. Reply with the plugin id, LSP language id, and whether the server is active or failed with a readable error.
```

Team prompt：

```text
Create/spawn a teammate named capability-reader. Ask it to read project memory, team memory, the product brief skill reference, and MCP availability. It must reply with the exact sentinels and one sentence about MCP status.
```

## 10. 验收标准

- 每个在线测试都必须标记 `#[ignore = "requires real API key and real integration dependencies"]`。
- 所有测试使用临时 `HOME` 与 `CC_RUST_HOME`，结束后调用 `lab.assert_path_isolated()`。
- 断言必须包含唯一 sentinel，不能只断言 “success” 或 “done”。
- MCP/plugin/LSP 前置依赖缺失时必须显示具体缺失项：`npx`、`uvx`、`typescript-language-server`、网络、API key。
- 插件安装后必须执行 `/reload-plugins` 或等价 runtime refresh，再断言当前 session active。
- LSP command reference 当前落后于实现；补测试时同步更新 `development/archive/COMMAND_REFERENCE.md` 的 `/lsp` 与 `/plugin install/marketplace/uninstall/validate` 条目。
- Agent Teams 测试必须带 `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1`；需要 team memory 时同时带 `FEATURE_TEAMMEM=1`。
- Agent Teams 测试完成后必须清理 team，并确认 `/tasks` 不再显示 running teammate。

## 11. 实施顺序

1. 扩展 `CapabilityLab`：sentinel memory、fork skill、local plugin LSP fixture、辅助安装/skip 检查。
2. 新增离线发现测试：memory、skills、agents、plugin、lsp、mcp status。
3. 新增在线主 agent 测试：memoryfile + memdir + project skill。
4. 新增真实 MCP 测试：filesystem 优先，sequential/playwright 作为 ignored 扩展。
5. 新增 plugin/LSP 测试：local plugin 安装、reload、skill/MCP/LSP 可见性。
6. 新增 Agent Teams 测试：team spawn、teammate context 继承、mailbox send、kill/delete 清理。
7. 跑对应 filtered e2e；若触及共享代码，再跑 `cargo build --workspace --release` 并清理 warning。
