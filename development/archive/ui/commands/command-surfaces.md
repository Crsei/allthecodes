# Command Surface Snapshots

来源: [COMMAND_UI_REFERENCE.md](../../COMMAND_UI_REFERENCE.md)

这些 snapshot 覆盖会打开 TUI 选择器、列表或 `CommandSurface` 的入口。

## Command Palette

触发: 输入 `/`

```text
+ Commands --------------------------------------------------------------+
| /init          Initialize project config and CLAUDE.md                 |
| /add-dir       Add a new working directory                             |
| /advisor       Show, set, or clear the advisor model                   |
| /agents        Browse agent definitions with source visibility         |
| /audit-export  Export session as verifiable audit record               |
|                                                                       |
| Usage: /init                                                          |
| Example: /init                                                        |
| Enter selects the command; type arguments after the inserted space.    |
+-----------------------------------------------------------------------+
```

关键期望:

- 过滤输入保留在 command palette 内。
- Enter 插入 `/<command> `，不会直接运行带参数流程。
- 支持 edit target 的命令在辅助区域显示 `Edit:` 目标信息。

## Edit Target Picker

触发: command palette 中按 `Ctrl+E`

适用命令: `/config`, `/hooks`, `/keybindings`, `/mcp`, `/memory`, `/plan`

```text
+ Edit target -----------------------------------------------------------+
| command: /hooks                                                       |
|                                                                       |
| > user settings      ~/.cc-rust/settings.json                         |
|   project settings   ./.cc-rust/settings.json                         |
|   local settings     ./.cc-rust/settings.local.json                   |
|                                                                       |
| Enter insert target | Up/Down navigate | Esc cancel                   |
+-----------------------------------------------------------------------+
```

关键期望:

- 只负责插入目标参数，不直接打开编辑器。
- 目标名称必须和普通 slash command 参数兼容。

## History Search

触发: `Ctrl+R`

```text
+ History search --------------------------------------------------------+
| filter: cargo                                                         |
|                                                                       |
| > cargo test -p claude-code-rs --test e2e_terminal                    |
|   cargo clippy --workspace --all-targets -- -D warnings               |
|   cargo build --release                                               |
|                                                                       |
| Enter fill prompt | Up/Down navigate | Esc close                      |
+-----------------------------------------------------------------------+
```

关键期望:

- 不是 slash command 本身，但要在命令 UI 快照中覆盖。
- Enter 把历史 prompt 填回输入框，不自动提交。

## Agents

Trigger: `/agents`

List mode:

```text
Agents
[Agents]  Built-in agents  User agents  Project agents

Built-in agents:
> general-purpose - default model
  debugger - default model
  code-reviewer - default model

Left/Right switch source tabs | Up/Down navigate | Enter detail | Esc close
```

Detail mode:

```text
Agent detail: general-purpose
...agent definition details...

Backspace/b return to list | Enter submit `/agents show general-purpose` | Esc close
```

Key expectations:

- `/agents` is a list/detail surface, not a selector-only surface.
- `/agents show <name>` text behavior is preserved from detail mode.
- The create-agent wizard entry stays hidden until save/cancel can use the existing safe `AgentSettingsCommand` User/Project settings paths.
- `/agent` is intentionally not an alias; `/agents` is the canonical list/detail command to keep help and palette routing unambiguous.

## Config Settings

触发: `/config` 或 `/settings`

Status tab:

```text
Config
[Status]  Model   Theme   Effort   Config   Editor
> Show effective config - model=custom-model; backend=native
  Show setting sources - which layer set each key
Left/Right switch tabs | Up/Down navigate | Enter select | Esc close
```

Model picker:

```text
Config
 Status  [Model]  Theme   Effort   Config   Editor
Current effort: medium (10240 tokens)
Model / Filter...
> custom-model - configured; current; effort=medium
  SOTA (claude-opus-4-20250514) - configured; effort=medium
Left/Right switch tabs | Type filter | Up/Down navigate | Enter select | Esc close
```

Theme picker:

```text
Config
 Status   Model  [Theme]  Effort   Config   Editor
Theme / Filter...
  Auto - match terminal when supported
  Dark mode - default ratatui palette
> Light mode - light terminal palette; current
  Solarized - low-contrast terminal palette
  Monokai - high-contrast editor palette
  Nord - cool low-saturation palette
Left/Right switch tabs | Type filter | Up/Down navigate | Enter select | Esc close
```

Effort picker:

```text
Config
 Status   Model   Theme  [Effort]  Config   Editor
Effort / Filter...
  Auto - model default; 10240 tokens
  Low - shorter thinking budget; 4096 tokens
> Medium - balanced thinking budget; 10240 tokens; current
  High - deeper thinking budget; 24576 tokens
  Max - largest fixed thinking budget; 32768 tokens
Left/Right switch tabs | Type filter | Up/Down navigate | Enter select | Esc close
```

Config tab:

```text
Config
 Status   Model   Theme   Effort  [Config]  Editor
> Show raw layers - managed/user/project/local settings
  Show schema - JSON schema for settings.json
  Set custom model - fill prompt with /config set model
  Set custom theme - fill prompt with /config set theme
Left/Right switch tabs | Up/Down navigate | Enter select | Esc close
```

关键期望:

- Model/Theme/Effort 是 picker。
- Config tab 的 set 操作应填充 `/config set ...` prompt。
- Editor tab 提交 `/config set editorMode vim|normal`。

## Diff

触发: `/diff`

List view:

```text
Uncommitted changes
1 file/line changed +3 -1

> src/main.rs                                            +3 -1

Enter view | [esc] close
```

Detail view:

```text
Uncommitted changes / src/main.rs

@@ src/main.rs
  fn main() {
-     old_call();
+     new_call();
+     extra_call();
  }

b back | [esc] close
```

关键期望:

- Left/Right 在 list mode 切换 diff source。
- Enter 从文件列表进入 detail。
- `b` 从 detail 返回 list。

## Hooks

触发: `/hooks`

```text
Hooks
[User settings]  Project settings

> PreToolUse     matchers=2 commands=2
  PostToolUse    matchers=1 commands=1
  Notification   matchers=0 commands=0
  Stop           matchers=0 commands=0

Left/Right switch settings scope | Up/Down navigate | Enter select | o open scope | Esc close
```

关键期望:

- 面板本身只读。
- Enter 提交 `/hooks list <event>`。
- `o` 打开当前 scope，等价 `/hooks open user|project`。

## Login

触发: `/login`

```text
[Status]  Claude Code key  Claude.ai  Console  Codex  Codex CLI
Login methods
> s. Status     - show current authentication source
  1. Claude Code key - paste a Claude Code / Anthropic-compatible API key
  2. Claude.ai  - start Claude.ai OAuth for Pro/Max accounts
  3. Console    - start Console OAuth for API billing
  4. Codex      - start OpenAI Codex OAuth for ChatGPT accounts
  5. Codex CLI  - check or import ~/.codex/auth.json

Left/Right switch action tabs | Up/Down navigate | Enter select | s status | 1-5 select | Esc close
```

关键期望:

- API key 选择填充 `/login sk-ant-api03-`。
- OAuth 选择提交 `/login claude-ai`、`/login console`、`/login codex-oauth` 并输出外部授权 URL。
- Codex CLI 选择提交 `/login codex-cli`。

## MCP

触发: `/mcp`

```text
[Status]  Edit  Reconnect  Remove
MCP servers (3)
  database       stdio   connected  tools=1
  uvx db-mcp
> docs           remote  failed     tools=0
  https://mcp.example.com
  agent-tools    agent   connecting tools=0

Left/Right switch action tabs | Up/Down navigate | Enter select | a add | Esc close
```

关键期望:

- Status tab 提交 `/mcp status`。
- Edit tab 填充 `/mcp edit <name> `。
- Reconnect/Remove tab 直接提交对应命令。
- `a` 填充 `/mcp add `。

## Memory

触发: `/memory`

```text
[Edit]  Show  Paths  Open
Memory files
  User memory - Saved in ~/.cc-rust/CLAUDE.md
> Project memory (new) - Saved in ./CLAUDE.md
  L ./docs/AGENTS.md - @-imported
  Open ~/.cc-rust/memory - auto-memory folder

Left/Right switch action tabs | Up/Down navigate | Enter select | Esc close
```

关键期望:

- Edit/Show/Paths 分别提交 `/memory edit`, `/memory show`, `/memory path`。
- Open tab 根据选中项提交 `/memory open project|global|team|auto`。
- `a`, `t`, `g`, `o` 快捷键可直接打开 auto/team/global/project。

## Sandbox

触发: `/sandbox`

Status tab:

```text
Sandbox
[Status]  Mode  Network
> Show sandbox status - sandbox=enabled; mode=workspace
Left/Right switch tabs | Up/Down navigate | Enter select | Esc close
```

Mode tab:

```text
Sandbox
 Status  [Mode]  Network
> Enable sandbox - session override: enabled=true
  Disable sandbox - session override: enabled=false
  Read-only mode - current mode=workspace
  Workspace mode - current mode=workspace
  Full mode - current mode=workspace
Left/Right switch tabs | Up/Down navigate | Enter select | Esc close
```

Network tab:

```text
Sandbox
 Status   Mode  [Network]
> Enable network - current network=disabled
  Disable network - current network=disabled
Left/Right switch tabs | Up/Down navigate | Enter select | Esc close
```

关键期望:

- Enter 提交 `/sandbox status`, `/sandbox on|off`, `/sandbox mode ...`, 或 `/sandbox network ...`。

## Skills

触发: `/skills`

```text
Skills (3)
filter: review
> code-review        enabled   Review diffs for regressions
  source: built-in
  security-review    enabled   Run security audit
  source: built-in
  writer             enabled   Documentation and migration notes
  source: built-in

Type to filter | Backspace edit filter | Enter details | r reload | d diagnostics | Esc close
```

关键期望:

- 普通字符输入更新 filter。
- Enter 提交 `/skills <name>`。
- filter 为空时 `r` 提交 `/skills reload`，`d` 提交 `/skills diagnostics`。

## Tasks

触发: `/tasks`

```text
Background tasks (3)
  cargo test         shell     running    12.4s [██████    ] running tests
> remote deploy      remote    failed       0ms [          ] connection lost
  review worker      agent     succeeded    0ms [          ] reported findings
Enter details | k/s stop | d delete tool | r refresh | Esc close
```

关键期望:

- Enter 提交 `/tasks show <id>`。
- `k` 或 `s` 对 tool task 提交 `/tasks stop <id>`；对 teammate task 提交 `/team kill <name>`。
- `d` 只删除 tool task，提交 `/tasks delete <id>`。

## Team Teams

触发: `/team` 或 `/teams`

Active team:

```text
Team: ui-port
> reviewer       running    mode=plan tasks=2
  role: Review implementation risks
  builder        running    mode=ask tasks=0 hidden
  role: Implement bounded changes
Enter status | k kill | s send | p spawn | l list | c create | Esc close
```

No active team:

```text
Team: none

No active teammates.

c create | l list | Esc close
```

关键期望:

- Enter 提交 `/team status`。
- `s` 填充 `/team send <name> `。
- `k` 提交 `/team kill <name>`。
- `p` 填充 `/team spawn `，`c` 填充 `/team create `。
