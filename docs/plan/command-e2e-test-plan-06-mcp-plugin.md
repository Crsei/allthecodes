# 命令 E2E 测试计划 06：MCP、插件与 IDE 命令

> 目标文件：`crates/claude-code-rs/tests/pty_tui_e2e/commands_mcp_plugin.rs`
> 所有测试均为**离线**（无需 API 密钥）。

参考claude-code-rust/crates/claude-code-rs/tests/capability_lab_support/mod.rs,必须测试真实的mcp plugin,lsp插件

## 涵盖的命令

| 命令 | 别名 | 返回类型 | 备注 |
|---|---|---|---|
| `/mcp` | -- | `Output` | 无参数时显示帮助/配置示例 |
| `/mcp list` | `/mcp ls` | `Output` | 列出已发现的 MCP 服务器 |
| `/mcp status` | -- | `Output` | 发现视图 |
| `/mcp add <name> ...` | -- | `Output` | 添加 MCP 服务器 |
| `/mcp remove <name>` | -- | `Output` | 移除 MCP 服务器 |
| `/mcp connect/disconnect/reconnect` | -- | `Output` | 连接管理 |
| `/mcp auth start/complete/status/clear` | -- | `Output` | MCP 的 OAuth |
| `/plugin` | `/plugins` | `Output` | 无参数时显示帮助 |
| `/plugin list/ls/status` | -- | `Output` | 列出插件 |
| `/plugin enable/disable <id>` | -- | `Output` | 切换插件 |
| `/reload-plugins` | -- | `Output` | 热刷新插件注册表 |
| `/ide` | -- | `Output` | IDE MCP 桥接检测 |
| `/lsp` | -- | `Output` | LSP 服务卡片 |
| `/chrome` | -- | `Output` | Chrome 集成状态 |

## 测试用例

### T01：`/mcp` 显示帮助/配置
```rust
fn mcp_help_no_args() {
    // 步骤：
    // 1. SkipTrustGate
    // 2. Command("mcp")
    // 3. Wait(2s)
    // 4. AssertScreenContains("mcp") 或 AssertScreenContains("MCP")
    // 5. Snapshot("mcp_help")
    // 断言：显示帮助文本或 mcpServers 配置示例
}
```

### T02：`/mcp list`
```rust
fn mcp_list() {
    // 步骤：
    // 1. Command("mcp list")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：列出服务器或显示 "none"
}
```

### T03：`/mcp ls`（别名）
```rust
fn mcp_ls_alias() {
    // 步骤：
    // 1. Command("mcp ls")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：别名可用
}
```

### T04：`/mcp status`
```rust
fn mcp_status() {
    // 步骤：
    // 1. Command("mcp status")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：显示状态/发现视图
}
```

### T05：`/mcp add` streamable-http
```rust
fn mcp_add_streamable_http() {
    // 步骤：
    // 1. Command("mcp add test-server --transport=streamable-http --url=https://example.com/mcp")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：服务器已添加或显示配置
}
```

### T06：`/mcp remove`
```rust
fn mcp_remove() {
    // 步骤：
    // 1. Command("mcp remove test-server")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：服务器已移除或显示 "not found"
}
```

### T07：`/mcp connect` 和 `disconnect`
```rust
fn mcp_connect_disconnect() {
    // 步骤：
    // 1. Command("mcp connect test-server")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 4. Command("mcp disconnect test-server")
    // 5. Wait(2s)
    // 6. AssertNoPanic
    // 断言：尝试连接/断开连接
}
```

### T08：`/mcp auth status`
```rust
fn mcp_auth_status() {
    // 步骤：
    // 1. Command("mcp auth status test-server")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：显示认证状态或 "not found"
}
```

### T09：`/mcp auth clear`
```rust
fn mcp_auth_clear() {
    // 步骤：
    // 1. Command("mcp auth clear test-server")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：认证已清除或显示 "not found"
}
```

### T10：`/plugin` 显示帮助
```rust
fn plugin_help() {
    // 步骤：
    // 1. Command("plugin")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：显示插件帮助
}
```

### T11：`/plugin list`
```rust
fn plugin_list() {
    // 步骤：
    // 1. Command("plugin list")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：列出插件或显示 "none"
}
```

### T12：`/plugin status`
```rust
fn plugin_status() {
    // 步骤：
    // 1. Command("plugin status")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：显示插件状态
}
```

### T13：`/plugin enable/disable` 缺少 ID
```rust
fn plugin_enable_missing_id() {
    // 步骤：
    // 1. Command("plugin enable")
    // 2. Wait(2s)
    // 3. AssertScreenContains("error") 或 AssertScreenContains("plugin")
    // 断言：关于缺少插件 ID 的错误提示
}

fn plugin_disable_missing_id() {
    // 步骤：
    // 1. Command("plugin disable")
    // 2. Wait(2s)
    // 3. AssertScreenContains("error") 或 AssertScreenContains("plugin")
    // 断言：关于缺少插件 ID 的错误提示
}
```

### T14：`/reload-plugins`
```rust
fn reload_plugins() {
    // 步骤：
    // 1. Command("reload-plugins")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：插件已重新加载
}
```

### T15：`/ide` 检测
```rust
fn ide_command() {
    // 步骤：
    // 1. Command("ide")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：显示 IDE 检测结果
}
```

### T16：`/lsp` 服务卡片
```rust
fn lsp_command() {
    // 步骤：
    // 1. Command("lsp")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：显示 LSP 服务信息
}
```

### T17：`/chrome` 状态
```rust
fn chrome_command() {
    // 步骤：
    // 1. Command("chrome")
    // 2. Wait(2s)
    // 3. AssertNoPanic
    // 断言：显示 Chrome 集成状态
}
```

### T18：所有 MCP/插件命令批量测试
```rust
fn mcp_plugin_batch_no_crash() {
    // 执行：/mcp list、/mcp status、/plugin list、/plugin status、
    //       /reload-plugins、/ide、/lsp、/chrome
    // 每个执行后 AssertNoPanic
    // 断言：所有命令均不会使 TUI 崩溃
}
```

## 优先级：中
MCP 和插件管理命令。首先关注不崩溃的冒烟测试。

---

## 补充功能说明

以下为各命令的功能描述、典型输出、源码路径与边界情况，供编写测试断言时参考。

---

### `/mcp` — MCP 服务器管理

#### 功能描述

管理 Model Context Protocol (MCP) 服务器的发现、连接、配置与认证。无参数时显示帮助文本和 `mcpServers` 配置示例。Rust TUI 中无参数时打开 `McpSurface`。

#### 输出示例

无参数帮助输出：
```
MCP (Model Context Protocol) server management (issue #44).

Usage:
  /mcp list                       list discovered MCP servers grouped by scope
  /mcp status                     show live connection status
  /mcp add <name> [flags]         create a new stdio config (user scope by default)
  ...
```

未知子命令：
```
Unknown mcp subcommand: 'xxx'.

MCP (Model Context Protocol) server management ...
```

#### 源码路径

- 入口分发：`crates/cc-commands/src/mcp/mod.rs` (`McpHandler::execute`)
- 帮助文本：`crates/cc-commands/src/mcp/help.rs`
- list/status：`crates/cc-commands/src/mcp/list_status.rs`
- add/remove/edit：`crates/cc-commands/src/mcp/config.rs`
- connect/disconnect/reconnect：`crates/cc-commands/src/mcp/runtime.rs`
- auth 子命令：`crates/cc-commands/src/mcp/auth.rs`

#### 边界情况

- 无参数在普通命令路径输出帮助文本；在 Rust TUI 中打开 `McpSurface`（行为不同）
- 未知子命令返回 `"Unknown mcp subcommand: '{}'"` 格式错误提示并附带帮助
- `help`、`-h`、`--help` 三个变体均等价于无参数
- `list` 和 `ls` 等价

---

### `/mcp list` / `/mcp ls`

#### 功能描述

列出当前发现到的所有 MCP 服务器，按 scope（plugin、user、project）分组显示，包含连接状态和传输类型。

#### 输出示例

有服务器时：
```
Discovered MCP servers (2; 1 browser):

[user]
  context7 -- connected -- streamable-http
  chrome-ext [browser] -- connected -- stdio
```

无服务器时：
```
No MCP servers discovered.

Add servers to ~/.cc-rust/settings.json or .cc-rust/settings.json, or run:
  /mcp add <name> --command=<cmd> [--arg=<arg> ...]
```

#### 源码路径

`crates/cc-commands/src/mcp/list_status.rs` (`handle_list`)

#### 边界情况

- 空注册表返回 "No MCP servers discovered" 并附带添加指引
- 浏览器 MCP 服务器会被标记 `[browser]`
- 连接状态包含：connected、disconnected、pending、error、disabled、unknown

---

### `/mcp status`

#### 功能描述

输出 MCP 服务器的运行态发现视图，展示每个服务器的连接状态、工具数量和资源数量。

#### 输出示例

```
MCP server status (2):

  context7 -- connected (12 tools, 0 resources)
  chrome-ext -- error (0 tools, 0 resources) -- connection refused
```

#### 源码路径

`crates/cc-commands/src/mcp/list_status.rs` (`handle_status`)

#### 边界情况

- 无服务器时返回 "No MCP servers discovered"
- discovery 失败时返回 state="error"，error 字段包含详细信息
- disabled 服务器 state 显示 "disabled"，tools/resources 均为 0

---

### `/mcp add`

#### 功能描述

创建新的 MCP 服务器配置条目。支持 `streamable-http`（标准）、`sse`（遗留）和 `stdio` 三种传输方式。默认写入 user scope。

#### 输出示例

```
MCP server 'test-server' added to user scope.
  transport: streamable-http
  url: https://example.com/mcp
```

#### 源码路径

`crates/cc-commands/src/mcp/config.rs` (`handle_add`)

#### 边界情况

- 重复添加同名服务器的行为（应报错或覆盖，取决于 scope）
- `--scope=project` 时写入项目级 `.cc-rust/settings.json`
- 缺少必要参数（如 `--url`）时返回用法提示

---

### `/mcp remove`

#### 功能描述

从可编辑 scope 中删除指定的 MCP 服务器配置。

#### 输出示例

成功时：
```
Removed MCP server 'test-server' from user scope.
```

未找到时：
```
No MCP server named 'test-server' found.
```

#### 源码路径

`crates/cc-commands/src/mcp/config.rs` (`handle_remove`)

#### 边界情况

- 删除不存在的服务器返回 "not found" 而非 panic
- `--scope=user|project` 控制从哪个 scope 删除
- 别名：`rm`、`delete`

---

### `/mcp connect` / `/mcp disconnect` / `/mcp reconnect`

#### 功能描述

管理 MCP 服务器的运行时连接。`connect` 建立连接，`disconnect` 断开，`reconnect` 先断后连。

#### 输出示例

```
Connecting to MCP server 'test-server'...
```

#### 源码路径

`crates/cc-commands/src/mcp/runtime.rs` (`handle_connect`, `handle_disconnect`, `handle_reconnect`)

#### 边界情况

- 连接不存在的服务器返回错误提示
- 对已连接的服务器重复 connect 的行为
- 对未连接的服务器 disconnect 的行为

---

### `/mcp auth start` / `complete` / `status` / `clear`

#### 功能描述

手动 OAuth PKCE 认证流程管理。`start` 生成授权 URL，`complete` 用回调 code 存储 token，`status` 查询脱敏凭证状态，`clear` 删除存储的 token。Token 存储在 `~/.cc-rust/` 下。

#### 输出示例

`auth start`：
```
OAuth authorization started for MCP server `context7`.
Open this URL in a browser:
https://...

Redirect URI: http://localhost:3000/callback
Then run: /mcp auth complete context7 --code=<code> --state=abc123
Token store: ~/.cc-rust/mcp_tokens/
```

`auth status`：
```
OAuth status for MCP server `context7`: configured=true authorized=true expired=false refreshable=true
Token store: ~/.cc-rust/mcp_tokens/
```

`auth clear`：
```
Cleared OAuth credentials for MCP server `context7`.
```

#### 源码路径

`crates/cc-commands/src/mcp/auth.rs` (`handle_auth`)

#### 边界情况

- 缺少服务器名称时返回 "Usage: /mcp auth start|complete|status|clear <name>"
- `auth complete` 缺少 `--code` 参数时返回用法提示
- `auth clear` 对未存储凭证的服务器返回 "No stored OAuth credentials found"
- 服务器名不存在时返回 "No MCP server named `{}` found"

---

### `/plugin` — 插件管理

#### 功能描述

管理插件的三层状态：Install（磁盘存在）、Enablement（启用/禁用）、Active（内存中活跃）。无参数或 `list`/`ls` 时列出所有插件的三层视图。

#### 输出示例

无参数/list：
```
Plugins (2):

  plugin                             version    installed  enabled  active
  ----------------------------------- ---------- --------- -------- -------
  alpha                              1.0.0      yes        yes      yes
  beta                               1.0.0      yes        no       no
```

无插件时：
```
No plugins registered.
```

#### 源码路径

`crates/cc-commands/src/plugin_cmd.rs` (`PluginHandler::execute`)

#### 边界情况

- `enable`/`disable` 缺少 plugin-id 时返回真正的 error（`bail!`），不是普通文本
- `enable`/`disable` 操作后如果 session 与磁盘不同步，输出包含 "Session drift" 提示并建议 `/reload-plugins`
- `installed`、`disabled`、`errors` 三个过滤器互斥
- 状态摘要 (`status`) 包含 "in sync" 或 "Session drift" 诊断

---

### `/plugin list` / `/plugin ls` / `/plugin status`

#### 功能描述

`list`/`ls` 显示所有插件的分层表格视图，包含 installed/enabled/active 三列。`status` 显示汇总统计和 session drift 诊断。

#### 输出示例

`plugin status`：
```
Plugin status summary:
  - total on disk: 3
  - active in session: 2
  - installed (enabled): 2
  - disabled: 1
  - error: 0

Session is in sync with disk.
```

有 drift 时：
```
Session drift: status changed (foo); added on disk (bar) — run /reload-plugins to bring this session back in sync.
```

#### 源码路径

`crates/cc-commands/src/plugin_cmd.rs` (`handle_list`, `handle_status`)

#### 边界情况

- 空注册表返回 "No plugins registered"
- list 输出包含 skills/tools/mcp 子行（如果有）
- session drift 提示在 list 和 status 中都会出现

---

### `/plugin enable` / `/plugin disable`

#### 功能描述

切换插件的启用/禁用状态。写入 `~/.cc-rust/plugins/installed_plugins.json`，同时尝试同步内存状态。

#### 输出示例

```
Plugin 'github' enabled.
```

带 drift 时：
```
Plugin 'github' disabled.
Session drift: status changed (github) — run /reload-plugins to apply.
```

#### 源码路径

`crates/cc-commands/src/plugin_cmd.rs` (`handle_set_enabled`)

#### 边界情况

- 缺少 plugin-id 时返回 error（`bail!("Usage: /plugin enable <plugin-id>")`），不是 `CommandResult::Output`
- 不存在的 plugin-id 返回 "Plugin '{}' not found in installed plugins"
- 启用新插件后提示 "run /reload-plugins to refresh contributed tools/skills/mcp"

---

### `/reload-plugins` — 热刷新插件注册表

#### 功能描述

清空内存中的插件注册表，从 `~/.cc-rust/plugins/installed_plugins.json` 重新加载，同时刷新技能包。输出重载报告（插件数、耗时、错误信息）。

#### 输出示例

成功时：
```
Reloaded 3 plugin(s) in 42ms.
Reloaded 5 skill package(s) at revision 7.
```

有错误时：
```
Reloaded 2 plugin(s) in 11ms.
  - broken@local: manifest parse failed
1 plugin(s) failed to load.
Reloaded 3 skill package(s) at revision 7.
Skill diagnostics: 0 warning(s), 1 error(s).
```

零插件时：
```
Reloaded 0 plugin(s) in 7ms.
Reloaded 0 skill package(s) at revision 0.
```

#### 源码路径

`crates/cc-commands/src/reload_plugins_cmd.rs` (`ReloadPluginsHandler::execute`, `format_report`)

#### 边界情况

- 零插件不算错误（不包含 "failed"）
- 插件加载失败和全局元数据/缓存诊断是分开的两类错误
- 如果 `ReloadPluginsRuntime` 未安装，返回 "Reload plugins runtime is unavailable"
- 报告格式固定以 "Reloaded N plugin(s) in Nms." 开头

---

### `/ide` — IDE 检测与 MCP 桥接

#### 功能描述

检测本机安装的 IDE（vscode、cursor、intellij 等），管理 IDE 选择和 MCP 桥接连接。无参数时显示 IDE 状态概览和操作提示。支持的 IDE：vscode、cursor、intellij、goland、pycharm、rubymine、webstorm。

#### 输出示例

无参数：
```
IDE status (no selection):

  * vscode     Visual Studio Code       installed=yes running=no  connection=disconnected
    cursor     Cursor                   installed=no  running=no  connection=disconnected
    intellij   IntelliJ IDEA            installed=no  running=no  connection=disconnected

  * = currently selected
  Select with: /ide select <id>
```

选中后：
```
IDE status (selected: vscode):

  * vscode     Visual Studio Code       installed=yes running=yes connection=connected
  ...
```

#### 源码路径

`crates/cc-commands/src/ide_cmd.rs` (`IdeHandler::execute`)

#### 边界情况

- 无参数等价于显示 picker（status + 操作提示）
- `select` 缺少 id 时返回 "Usage: /ide select <id>"
- `select` 未知 id 返回 "Failed to select IDE"
- `reconnect` 在未选择 IDE 时返回 "Reconnect failed"
- `clear` 返回 "Cleared IDE selection."
- 未知子命令返回 "Unknown ide subcommand" 并附带帮助

---

### `/lsp` — LSP 服务器状态

#### 功能描述

显示已配置的 LSP 服务器卡片（语言 ID、状态、打开文件数、扩展），以及 LSP 插件推荐设置和项目级推荐。

#### 输出示例

`/lsp status`：
```
LSP server status

[rust]
  state: running
  open files: 3
  extensions: rust-analyzer
```

无服务器时：
```
LSP server status
No LSP servers configured.
```

`/lsp recommendations`：
```
LSP recommendation settings
  disabled: false
  muted plugins: (none)

Recommendations are shown when the backend emits a real LSP RecommendationRequest.
```

#### 源码路径

`crates/cc-commands/src/lsp_cmd.rs` (`LspHandler::execute`)

#### 边界情况

- `status`、`servers` 和无参数均等价
- `recommendations --all` 显示所有推荐（含 dismissed/installed/new 分组）
- `recommend <language>` 按语言过滤推荐
- 推荐被禁用时返回 "LSP recommendations are disabled"
- 未知子命令返回 "Unknown /lsp subcommand" 并附带用法

---

### `/chrome` — Chrome 集成状态

#### 功能描述

显示 Claude in Chrome 第一方集成的状态：扩展是否安装、连接状态（Disabled/Enabled/Connecting/Connected/Error）、浏览器类型。`reconnect` 重新运行检测和 native-host 安装。

#### 输出示例

禁用时：
```
Claude in Chrome (first-party Chrome integration)

  Platform: UNSUPPORTED
  Claude in Chrome is only available on macOS, Linux, and Windows.
```

或：
```
Claude in Chrome (first-party Chrome integration)

  Status: disabled

  Extension: NOT detected
    Install: https://...

  Onboarding:
    1. Start cc-rust with `--chrome` or set `CLAUDE_CODE_ENABLE_CFC=1`.
    2. Install the Chrome extension: https://...
    3. Run `/chrome reconnect` after enabling the subsystem.
```

已连接时：
```
  Status: connected
  Extension: installed (Google Chrome)
  Connected via Google Chrome — browser tools are live.
```

#### 源码路径

`crates/cc-commands/src/chrome_cmd.rs` (`ChromeHandler::execute`)

#### 边界情况

- 不支持的平台（非 macOS/Linux/Windows）直接返回 "UNSUPPORTED"
- 禁用状态下 `reconnect` 返回 "Chrome subsystem is disabled"
- 未知子命令返回 "Unknown /chrome subcommand" 并附带帮助
- `help`/`?` 显示 CLI flags、环境变量和安装链接
- 连接状态五种：Disabled、Enabled、Connecting、Connected{browser}、Error{msg}
