# Text Command And External Flow Snapshots

来源: [COMMAND_UI_REFERENCE.md](../../COMMAND_UI_REFERENCE.md)

这些命令当前主要通过文本结果、文件编辑器、OAuth URL 或外部桥接完成，不应画成已有专用 TUI surface。

## Permissions Perms

触发: `/permissions` 或 `/perms`

```text
Permissions
mode: ask

Rules
  allow Bash(cargo test*)        project   .cc-rust/settings.json
  deny Write(secrets/**)         local     .cc-rust/settings.local.json

Session grants
  Bash(cargo fmt)                expires at session end

Commands:
  /permissions mode <ask|allow|deny|plan>
  /permissions allow <matcher>
  /permissions deny <matcher>
  /permissions reset
```

关键期望:

- 当前无专用 `CommandSurface`。
- 工具审批弹窗另见 [approval-snapshots.md](approval-snapshots.md#tool-permission-dialogs)。

## Keybindings

触发: `/keybindings`

```text
Keybindings
target: ~/.cc-rust/keybindings.json

File did not exist; created template.
Opening with $VISUAL/$EDITOR...

If no editor is configured:
  ~/.cc-rust/keybindings.json
```

关键期望:

- 默认创建并打开 keybindings 文件。
- command palette 可以显示 edit target。

## Keybindings Open

触发: `/keybindings open`

```text
Open keybindings
target: ~/.cc-rust/keybindings.json
editor: code --wait

status: launched
```

## Statusline

触发: `/statusline`

```text
Status line
enabled: true
command: scripts/statusline.ps1
refresh: 1000ms
timeout: 2000ms
padding: 1

Runtime snapshot updated for current TUI session.
```

## Plugin

触发: `/plugin`

```text
Plugins
installed: 3
enabled: 2
active in session: 2

> github      enabled   active
  teams       enabled   active
  browser     disabled  inactive

Commands:
  /plugin list
  /plugin status <id>
  /plugin enable <id>
  /plugin disable <id>
  /plugin uninstall <id>
```

关键期望:

- 当前没有完整 `/plugin` UI surface。
- `install` 可能由 LSP recommendation 填充 prompt，但 handler 支持面以命令实现为准。

## Reload Plugins

触发: `/reload-plugins`

```text
Plugins reloaded
registry: refreshed
active session: updated

2 enabled plugin(s) available.
```

## Model

触发: `/model`

```text
Model
active: claude-opus-4-20250514
backend: native

Interactive picker:
  use /config, then open the Model tab.
```

## Effort

触发: `/effort`

```text
Effort
active: medium
budget: 10240 tokens

Interactive picker:
  use /config, then open the Effort tab.
```

## Fast

触发: `/fast`

```text
Fast mode
enabled: true
model override: fast lane
scope: current session
```

## Advisor

触发: `/advisor`

```text
Advisor
model: gpt-5.4-mini
status: enabled

Commands:
  /advisor set <model>
  /advisor clear
```

## Experimental

触发: `/experimental`

```text
Experimental features
  browser_mcp: enabled
  remote_control: disabled
  voice: disabled

Commands:
  /experimental enable <flag>
  /experimental disable <flag>
```

## Ide

触发: `/ide`

```text
IDE integration
selectedIde: vscode
bridge: connected

Commands:
  /ide select <id>
  /ide reconnect
```

## Ide Reconnect

触发: `/ide reconnect`

```text
IDE bridge reconnect
selectedIde: vscode
action: reconnect MCP bridge
status: reconnect requested
```

关键期望:

- 不直接打开 IDE。
- 触发外部 IDE MCP bridge 重连。

## Notify

触发: `/notify`

```text
Notifications
enabled: true
backend: osc9

Commands:
  /notify status
  /notify test
  /notify on
  /notify off
```

## Voice

触发: `/voice`

```text
Voice
voiceEnabled: false
language: default
runtime backend: unavailable

This is a compatibility setting; runtime voice may be unsupported.
```

## Plan Workflow

触发: `/plan`, `/plan enter`

```text
Plan mode
state: active
file: .cc-rust/plans/current.md

Next actions:
  /plan open
  /plan approve
  /plan reject
```

触发: `/plan open` 或 `/plan edit`

```text
Open plan
target: .cc-rust/plans/current.md
editor: $VISUAL/$EDITOR

status: launched
```

触发: `/plan approve`

```text
Plan approved
state: approved
next: implementation may continue
```

触发: `/plan reject`

```text
Plan rejected
state: rejected
next: revise plan before implementation
```

## Permissions Mode Plan

触发: `/permissions mode plan`

```text
Permissions
mode: plan
workflow: plan mode entered

No file edits should be made until the plan is approved.
```

## MCP Auth Start Name

触发: `/mcp auth start <name>`

```text
MCP OAuth
server: docs
authorization_url: https://mcp.example.com/oauth/authorize?state=...
redirect_uri: http://127.0.0.1:...
state: generated

Complete with:
  /mcp auth complete docs --code=<code>
```

关键期望:

- 文本式 OAuth 流程。
- 输出授权 URL，但不暗示 TUI 内嵌浏览器。

## Login Subcommands And Login Code

触发: `/login claude-ai`

```text
Claude.ai OAuth
authorization_url: https://claude.ai/oauth/authorize?...

After authorization:
  /login-code <code>
```

触发: `/login console`

```text
Console OAuth
authorization_url: https://console.anthropic.com/oauth/authorize?...
will attempt: create/store API key

After authorization:
  /login-code <code>
```

触发: `/login codex-oauth`

```text
OpenAI Codex OAuth
authorization_url: https://auth.openai.com/oauth/authorize?...

After authorization:
  /login-code <code>
```

触发: `/login codex-cli`

```text
Codex CLI auth
source: ~/.codex/auth.json
status: imported or refreshed
```

触发: `/login bedrock`

```text
AWS Bedrock login
status: external environment required

Expected setup:
  AWS_PROFILE / AWS_REGION / credential chain
```

触发: `/login vertex`

```text
GCP Vertex login
status: external environment required

Expected setup:
  gcloud auth application-default login
```

触发: `/login-code <code>`

```text
OAuth completion
code: received
token: stored under ~/.cc-rust/
status: authenticated
```

## Chrome

触发: `/chrome help`

```text
Chrome integration
Chrome Web Store: <url>
Permissions: <url>
Troubleshooting: <url>

No browser is opened automatically.
```

触发: `/chrome reconnect`

```text
Chrome reconnect
native host: checked
install status: ok
reconnect_url: chrome-extension://.../reconnect
```

## Hooks Open Layer

触发: `/hooks open <managed|user|project|local>`

```text
Open hooks settings
layer: project
target: .cc-rust/settings.json
editor: $VISUAL/$EDITOR

status: launched
```

关键期望:

- 没有编辑器时只打印路径。

## Init

触发: `/init`

```text
Init
created:
  .cc-rust/settings.json
  CLAUDE.md

status: project initialized
```

关键期望:

- 一次性创建文件。
- 不是交互式 wizard。

## Team Onboarding

触发: `/team-onboarding`

```text
Team onboarding
generated: onboarding document
saved: .cc-rust/team-onboarding.md

status: complete
```

关键期望:

- 生成文档/保存文件。
- 不是 TUI onboarding wizard。

## Session

触发: `/session`

```text
Sessions
current: 2026-05-10T04-31-00

Recent sessions
> 2026-05-10T04-31-00  current project
  2026-05-09T11-28-00  previous project

Rust TUI does not show the TypeScript remote session QR code.
```

## Resume

触发: `/resume`

```text
Resume
target: most recent session
status: restored

With argument:
  /resume <id-or-prefix>
```

关键期望:

- 当前不弹 `ResumePicker`。
- 直接恢复最近 session 或按 id/prefix 恢复。
