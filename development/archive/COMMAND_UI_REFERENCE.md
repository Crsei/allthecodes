# TUI Command UI Reference

日期: 2026-05-21

本文按当前 Rust TUI 源码整理 slash command 的 UI 行为，重点回答“哪些命令会打开
设置面板、选择器、向导、外部页面/App，或触发确认/审批”。

主要来源:

- `crates/claude-code-rs/src/ui/command_surface/mod.rs`
- `crates/claude-code-rs/src/ui/command_surface/surfaces/*.rs`
- `crates/claude-code-rs/src/ui/command_palette/*.rs`
- `crates/cc-commands/src/*.rs`
- `crates/claude-code-rs/src/ui/permissions/**`
- `crates/claude-code-rs/src/ui/mcp/**`

## 约定

- 同一个命令可能同时属于多个分类，例如 `/config` 既是设置类，也包含
  model/theme/effort picker。
- `CommandSurface` 只在“命令无参数”时打开。例: 输入 `/mcp` 会打开 TUI
  MCP 面板，输入 `/mcp status` 会直接走普通命令处理器。
- “设置类”只表示会进入配置/管理界面或修改配置，不代表一定会弹出独立的
  settings React 式窗口。
- “外部页面/App”包含浏览器授权 URL、Chrome/IDE 连接、以及 `$VISUAL`/`$EDITOR`
  打开的外部编辑器。
- 当前 Rust 版 `/session` 明确不显示 TypeScript 版的远程 session QR code，只显示
  文本 session 列表。
- 2026-05-21 cfg-test production wiring 已完成：`CommandSurface`、agent create/edit、
  MCP detail、permissions、tasks/team、dialog/tabs helpers 不再依赖 test-only cfg 才能进入生产构建。

## 一、设置类

这些命令用于查看或修改配置、权限、认证、集成或运行策略。

| 命令 | 当前 TUI 行为 | 配置/管理内容 | 备注 |
| --- | --- | --- | --- |
| `/config` (`/settings`) | 无参数打开 `ConfigSurface` | effective config、sources、schema、model、theme、effort、editorMode | Model/Theme/Effort tab 是 picker；Config tab 可填充 `/config set ...` |
| `/sandbox` | 无参数打开 `SandboxSurface` | sandbox enabled、mode、network | Enter 会提交 `/sandbox on/off`、`/sandbox mode ...`、`/sandbox network ...` |
| `/hooks` | 无参数打开 `HooksSurface` | user/project hook settings scope、hook event tree | 面板本身只读；`/hooks open <layer>` 会打开 settings 文件 |
| `/mcp` | 无参数打开 `McpSurface` | MCP server status/edit/reconnect/remove/add、server kind/settings、tool list/detail | 同时是选择器类；`.mcp.json` approval 另见确认/审批类 |
| `/login` | 无参数打开 `LoginSurface` | claude-code/codex/custom profile 切换、Claude.ai/Console/Codex OAuth、Codex CLI import | TUI 提交具名 `/login ...` 命令；OAuth 会给出外部授权 URL，并用 `/login-code` 完成 |
| `/permissions` (`/perms`) | 无参数打开 `PermissionsSurface` | permission mode、workspace/user/project/local/session allow/ask/deny rules、session grants | Enter 填充 `/permissions mode ...`、`allow/ask/deny ... --scope`、`session-grant`、`clear-session-grants` 等命令；`/permissions full access` 直接映射到 confirmed bypass；实际工具审批由权限 dialog 处理 |
| `/keybindings` | 当前无专用 `CommandSurface`，默认创建并打开文件 | `~/.cc-rust/keybindings.json` | 通过 `$VISUAL`/`$EDITOR` 打开；command palette 会显示 edit target |
| `/statusline` | 当前无专用 `CommandSurface`，走文本命令 | `statusLine.command`、enabled、refresh、timeout、padding | 写入 user settings，并同步当前 TUI runtime snapshot |
| `/plugin` | 当前无专用 `CommandSurface`，走文本命令 | installed/enabled/active plugin 状态 | `/plugin` UI surface 仍是计划项；当前支持 list/status/enable/disable/uninstall |
| `/reload-plugins` | 普通文本命令 | 刷新当前 session 的 plugin registry | plugin enable/disable 后通常需要 reload 才对当前 session 生效 |
| `/model` | 普通文本命令 | active model | `/config` 的 Model picker 是当前交互入口 |
| `/effort` | 普通文本命令 | thinking effort | `/config` 的 Effort picker 是当前交互入口 |
| `/fast` | 普通文本命令 | fast mode | 无单独面板 |
| `/advisor` | 普通文本命令 | advisor model | 无单独面板 |
| `/experimental` | 普通文本命令 | feature gates | 无单独面板 |
| `/ide` | 普通文本命令 | selected IDE + MCP bridge reconnect | `/ide select <id>` 持久化 `selectedIde` |
| `/remote` | 无参数打开 `RemoteSurface` | local gateway status、adapters、runs、events、doctor/stop action | 只表示本地 gateway/control-plane surface；远程 inbound channel session 不在本轮能力内 |
| `/notify` | 普通文本命令 | push notification settings | 受 feature gate/后端能力影响 |
| `/voice` | 普通文本命令 | voiceEnabled/language runtime snapshot | 当前描述为兼容设置，runtime voice 可能不可用 |
| `/memory` | 无参数打开 `MemorySurface` | CLAUDE.md、memory scopes、auto-memory toggle/path | 更像 memory 管理器；`/memory auto on/off` 会写 user settings |

## 二、选择器类

这些入口需要手动选择条目，或会弹出 picker/dialog/list surface。

| 入口 | UI 类型 | 选择后行为 |
| --- | --- | --- |
| 输入 `/` | `CommandPalette` | 上下选择命令，Enter 插入 `/<command> ` |
| command palette 的 `Ctrl+E` | edit target picker | 对支持 edit targets 的命令插入目标参数 |
| `/agents` | agent source tabs + list/detail/create/edit wizard | Enter opens detail；`n`/`c` 在 User/Project source 创建 agent；detail `e` 编辑可写 agent；detail Enter 提交 `/agents show <agent>` |
| `/config` | tabbed form + model/theme/effort pickers | Enter 提交 `/config show`、`/config set ...` 等 |
| `/diff` | diff source/file selector + detail view | Enter 从文件列表进入 detail；`b` 返回列表 |
| `/hooks` | settings scope tabs + hook event list | Enter 提交 `/hooks list <event>`；`o` 打开当前 scope |
| `/login` | named login/profile selector | Enter/快捷字母提交 `/login status`、`/login claude-code`、`/login codex` 等 |
| `/mcp` | MCP server list/detail + kind/settings/tool list/tool detail | Enter 按当前 action 提交 status/edit/reconnect/remove；detail panes 展示 redacted settings、transport kind、tools 与 auth/status hints |
| `/memory` | memory file/scope selector | Enter 按当前 action 提交 edit/show/path/open |
| `/permissions` (`/perms`) | permission mode/rules/session-grants surface | Enter 填充 mode/rule/session 命令；workspace entry 可生成当前 cwd scope 的 allow/ask/deny rule |
| `/remote` | gateway status/adapters/runs/events selector | Enter 根据当前 action 填充或提交 status、runs、events、stop、doctor 等 `/remote` 命令 |
| `/sandbox` | tabbed option selector | Enter 提交 sandbox toggle/mode/network command |
| `/skills` | filterable skill list | Enter 提交 `/skills <name>`；`r` reload；`d` diagnostics |
| `/tasks` | background tasks list/detail surface | `o`/Right 打开 detail；Enter 提交 `/tasks show <id>`；`s/k` stop 或 team kill；`d` 删除 tool task；`r` refresh |
| `/team` (`/teams`) | team status + teams/teammate dialog | Enter/Right 打开 detail；`l` list；`c` create；`p` spawn；`s` fill send prompt；`k` kill selected teammate |
| LSP plugin recommendation event | recommendation picker | Yes/No/Never/Disable；Yes 当前填充 `/plugin install <name> `，但当前 `/plugin` handler 尚未列出 `install` 子命令 |
| `Ctrl+R` history search | history search dialog | 过滤历史 prompt，Enter 填回输入框 |

已存在但当前未作为 slash command 接线的选择组件:

- `ResumePicker`: Rust `/resume` 当前直接恢复最近 session 或按 id/prefix 恢复，不弹交互选择器。
- `/agent` alias decision: do not add a singular alias; `/agents` remains the canonical list/detail surface to keep help and palette routing unambiguous.

## 三、向导类

当前 Rust TUI 中，真正“一步一步留在同一个 TUI wizard surface 内”的 slash command
很少；多数是文本式多步骤流程。

| 命令/入口 | 向导形态 | 当前步骤 |
| --- | --- | --- |
| `/login` | TUI 方法选择 + 文本式后续步骤 | 选择 API key/OAuth/Codex CLI/Bedrock/Vertex；OAuth 后用 `/login-code <code>` 完成 |
| `/agents` | TUI create/edit wizard | location、method、generation goal、agent name、system prompt、description、tools、model、color、confirm；User/Project agent 通过 `AgentSettingsCommand::Upsert` 保存 |
| `/mcp auth start <name>` | 文本式 OAuth 流程 | 输出授权 URL、redirect URI、state；然后 `/mcp auth complete <name> --code=...` |
| `/plan` / `/plan enter` | durable plan workflow | 进入 plan mode，`/plan open` 编辑，`/plan approve` 或 `/plan reject` 完成审批 |
| `/permissions mode plan` | plan workflow 入口 | 进入 plan permission mode，并写 plan workflow state |

不应误判为向导的命令:

- `/init` 是一次性创建 `.cc-rust/settings.json` 和 `CLAUDE.md`，不是交互向导。
- `/team-onboarding` 是生成 onboarding 文档/保存文件，不是 TUI onboarding wizard。
- `/logout` 会清理 auth/onboarding 派生状态，但没有确认向导。

## 四、外部页面、二维码或 App

这些命令会要求用户离开当前 TUI，访问外部 URL，或打开外部应用/编辑器。

| 命令 | 外部行为 | 备注 |
| --- | --- | --- |
| `/login claude-ai` | 输出 Claude.ai OAuth 授权 URL | 之后用 `/login-code <code>` |
| `/login console` | 输出 Console OAuth 授权 URL | Console flow 还会尝试创建/存储 API key |
| `/login codex-oauth` | 输出 OpenAI Codex OAuth 授权 URL | 之后用 `/login-code <code>` |
| `/login codex-cli` | 读取/刷新 Codex CLI auth | 依赖 `~/.codex/auth.json`，不打开 UI |
| `/login bedrock` | 提示 AWS Bedrock 环境配置 | 可能要求用户在外部 shell/cloud 环境配置凭据 |
| `/login vertex` | 提示 GCP Vertex 配置 | 可能要求用户运行 `gcloud auth application-default login` |
| `/mcp auth start <name>` | 输出 MCP server OAuth 授权 URL | 完成命令是 `/mcp auth complete ...` |
| `/chrome help` | 输出 Chrome Web Store、permissions、troubleshooting URL | 当前命令不直接打开浏览器 |
| `/chrome reconnect` | 重新检测/安装 native host，并提示去 Chrome reconnect URL | 依赖 Chrome subsystem 已启用 |
| `/hooks open <managed|user|project|local>` | 通过 `$VISUAL`/`$EDITOR` 打开 settings 文件 | 无 editor 时只打印路径 |
| `/keybindings` 或 `/keybindings open` | 通过 `$VISUAL`/`$EDITOR` 打开 keybindings 文件 | 文件缺失时先创建模板 |
| `/plan open` 或 `/plan edit` | 通过 `$VISUAL`/`$EDITOR` 打开当前 plan 文件 | 文件缺失时创建模板 |
| transcript export action | 通过 `$VISUAL`/`$EDITOR` 打开导出的 transcript | 这是 TUI key/action，不是 slash command |
| `/ide reconnect` | 重新连接选中的 IDE MCP bridge | 不直接打开 IDE，但会触发外部 IDE bridge 连接 |

当前没有 Rust slash command 会显示 QR code。`/session` 源码注释明确说明 TypeScript 版
会显示 remote session QR code，Rust CLI 当前改为文本 session 列表。

## 五、非设置界面的确认/审批类

这些不是 settings 界面，但会要求用户确认、审批，或执行带审批语义的命令。

| 入口 | UI/命令 | 决策 |
| --- | --- | --- |
| 工具权限请求 | `PermissionDialog` | Allow / Deny / Always Allow；Esc 等价 Deny |
| Shell/PowerShell 权限 | `bash_permission_request` / `power_shell_permission_request` | 允许或拒绝命令执行，可带 shell-specific options |
| 文件写入/编辑权限 | `file_write_permission_request`、`file_edit_permission_request`、`sed_edit_permission_request` 等 | 展示目标/patch/diff 后允许或拒绝 |
| Web fetch 权限 | `web_fetch_permission_request` | 对 URL 访问进行审批 |
| Computer-use 权限 | `computer_use_approval` | 对浏览器/桌面自动化动作审批 |
| AskUserQuestion | `ask_user_question_permission_request` | 用户选择/输入回答后工具继续 |
| 进入/退出 plan mode | `enter_plan_mode_permission_request`、`exit_plan_mode_permission_request` | 计划审批相关的确认 UI |
| `/plan approve` / `/plan reject` | 普通 slash command | 明确批准或驳回当前 plan workflow |
| team plan approval | `SendMessage` plan approval request/response | teammate 等待 leader approve/reject |
| MCP `.mcp.json` 单 server | `mcp_server_approval_dialog` | approve current、approve all future project servers、reject |
| MCP `.mcp.json` 多 server | `mcp_server_multiselect_dialog` | Space toggle，Enter apply，Esc reject all |
| LSP recommendation | `LspRecommendationSurface` | Yes / No / Never / Disable |
| workspace trust gate | startup trust prompt | 信任当前工作目录或退出/拒绝 |

需要注意的“会改变状态但当前没有额外确认 dialog”的命令:

- `/tasks stop <id>`、`/tasks delete <id>`
- `/team kill <name>`、`/team delete <name>`、`/team leave`
- `/logout`
- `/clear`
- `/mcp remove <name>`
- `/plugin disable <id>`、`/plugin uninstall <id>`
- `/permissions reset`

这些命令当前由用户显式输入后直接执行。后续如果补更完整的 TUI surface，可以把它们作为
confirm dialog 的候选对象，但不要在文档中暗示当前已经有确认弹窗。

## 快速索引

当前无参数会打开 `CommandSurface` 的 slash commands:

```text
/agents
/config
/diff
/hooks
/login
/mcp
/memory
/permissions
/perms
/remote
/sandbox
/skills
/tasks
/team
/teams
```

当前会在 command palette 里显示 `Edit:` 路径/目标信息的 commands:

```text
/config
/hooks
/keybindings
/mcp
/memory
/plan
/plugin
/skills
```

其中 `Ctrl+E` 会打开目标 picker 并插入参数的 commands:

```text
/config
/hooks
/keybindings
/mcp
/memory
/plan
```

当前会打开 `$VISUAL`/`$EDITOR` 的 commands:

```text
/hooks open <layer>
/keybindings
/keybindings open
/plan open
/plan edit
```
