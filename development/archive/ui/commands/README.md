# Slash Command UI Snapshots

日期: 2026-05-10

本目录把 [COMMAND_UI_REFERENCE.md](../../COMMAND_UI_REFERENCE.md) 中的 slash command UI 行为转成 Markdown snapshot 草图。这里的 snapshot 是测试期望草图，不是运行时录屏；后续实现 ratatui/insta snapshot 时应以这些结构作为覆盖清单。

## 约定

- 只记录当前 Rust TUI 预期行为；如果命令当前没有专用 `CommandSurface`，不要画成已有弹窗。
- 无参数 slash command 才打开 `CommandSurface`。带参数的形式直接走普通命令处理器。
- ASCII 图里的 `>` 表示当前选中行，`[...]` 表示当前 tab。
- 需要离开 TUI 的流程标成 external/file，不暗示 TUI 自动打开浏览器。
- “无确认”命令必须显式标注为直接执行，避免误导测试补上不存在的 confirm dialog。

## 文件索引

| 文件 | 覆盖范围 |
| --- | --- |
| [command-surfaces.md](command-surfaces.md) | `/` command palette、无参数 `CommandSurface`、选择器类命令 |
| [text-and-external.md](text-and-external.md) | 普通文本命令、外部编辑器、OAuth/Chrome/IDE 流程、plan 文本流程 |
| [approval-snapshots.md](approval-snapshots.md) | 工具权限、MCP 审批、LSP 推荐、workspace trust、当前无确认的状态变更命令 |

## 覆盖清单

| 命令/入口 | Snapshot 文件 |
| --- | --- |
| 输入 `/` | [command-surfaces.md](command-surfaces.md#command-palette) |
| command palette `Ctrl+E` | [command-surfaces.md](command-surfaces.md#edit-target-picker) |
| `Ctrl+R` history search | [command-surfaces.md](command-surfaces.md#history-search) |
| `/agents` | [command-surfaces.md](command-surfaces.md#agents) |
| `/config`, `/settings` | [command-surfaces.md](command-surfaces.md#config-settings) |
| `/diff` | [command-surfaces.md](command-surfaces.md#diff) |
| `/hooks` | [command-surfaces.md](command-surfaces.md#hooks) |
| `/login` | [command-surfaces.md](command-surfaces.md#login) |
| `/mcp` | [command-surfaces.md](command-surfaces.md#mcp) |
| `/memory` | [command-surfaces.md](command-surfaces.md#memory) |
| `/sandbox` | [command-surfaces.md](command-surfaces.md#sandbox) |
| `/skills` | [command-surfaces.md](command-surfaces.md#skills) |
| `/tasks` | [command-surfaces.md](command-surfaces.md#tasks) |
| `/team`, `/teams` | [command-surfaces.md](command-surfaces.md#team-teams) |
| `/permissions`, `/perms` | [text-and-external.md](text-and-external.md#permissions-perms) and [approval-snapshots.md](approval-snapshots.md#tool-permission-dialogs) |
| `/keybindings` | [text-and-external.md](text-and-external.md#keybindings) |
| `/statusline` | [text-and-external.md](text-and-external.md#statusline) |
| `/plugin` | [text-and-external.md](text-and-external.md#plugin) |
| `/reload-plugins` | [text-and-external.md](text-and-external.md#reload-plugins) |
| `/model` | [text-and-external.md](text-and-external.md#model) |
| `/effort` | [text-and-external.md](text-and-external.md#effort) |
| `/fast` | [text-and-external.md](text-and-external.md#fast) |
| `/advisor` | [text-and-external.md](text-and-external.md#advisor) |
| `/experimental` | [text-and-external.md](text-and-external.md#experimental) |
| `/ide` | [text-and-external.md](text-and-external.md#ide) |
| `/notify` | [text-and-external.md](text-and-external.md#notify) |
| `/voice` | [text-and-external.md](text-and-external.md#voice) |
| `/plan`, `/plan enter`, `/plan open`, `/plan edit`, `/plan approve`, `/plan reject` | [text-and-external.md](text-and-external.md#plan-workflow) and [approval-snapshots.md](approval-snapshots.md#plan-approval) |
| `/permissions mode plan` | [text-and-external.md](text-and-external.md#permissions-mode-plan) |
| `/mcp auth start <name>` | [text-and-external.md](text-and-external.md#mcp-auth-start-name) |
| `/login claude-ai`, `/login console`, `/login codex-oauth`, `/login codex-cli`, `/login bedrock`, `/login vertex`, `/login-code <code>` | [text-and-external.md](text-and-external.md#login-subcommands-and-login-code) |
| `/chrome help`, `/chrome reconnect` | [text-and-external.md](text-and-external.md#chrome) |
| `/hooks open <layer>` | [text-and-external.md](text-and-external.md#hooks-open-layer) |
| `/keybindings open` | [text-and-external.md](text-and-external.md#keybindings-open) |
| `/ide reconnect` | [text-and-external.md](text-and-external.md#ide-reconnect) |
| `/init` | [text-and-external.md](text-and-external.md#init) |
| `/team-onboarding` | [text-and-external.md](text-and-external.md#team-onboarding) |
| `/session` | [text-and-external.md](text-and-external.md#session) |
| `/resume` | [text-and-external.md](text-and-external.md#resume) |
| `/logout`, `/clear`, `/tasks stop <id>`, `/tasks delete <id>`, `/team kill|delete|leave`, `/mcp remove <name>`, `/plugin disable|uninstall <id>`, `/permissions reset` | [approval-snapshots.md](approval-snapshots.md#direct-execute-no-confirm) |
