# Unified Settings Panel Snapshots

来源: 当前行为参考 [../commands/command-surfaces.md](../commands/command-surfaces.md)

这些 snapshot 描述目标 UI 结构。后续实现时应保持现有命令语义：无参数 slash command 打开 surface，带参数命令仍直接走命令处理器。

## Shared Shell

所有设置面板共享以下骨架：

```text
+ Surface title ---------------------------------------------------------+
| Context summary                                        status badges  |
|-----------------------------------------------------------------------|
| Sections              Detail panel                                    |
| > Section A           Section title                                  |
|   Section B           key/value rows, picker rows, or read-only info  |
|   Section C                                                           |
|                                                                       |
|-----------------------------------------------------------------------|
| Left/Right section | Up/Down navigate | Enter select | Esc close      |
+-----------------------------------------------------------------------+
```

关键期望:

- 左侧 section 名称稳定，不随内容过滤或行数变化。
- 右侧标题显示当前 section，下面显示主要数据。
- 当前值、来源、风险、执行命令都在行内明确呈现。
- footer 只出现一次，并固定在面板底部。

## Config Status

触发: `/config` 或 `/settings`

```text
+ Config ---------------------------------------------------------------+
| model=custom-model                         backend=native  source=user |
|-----------------------------------------------------------------------|
| Sections              Status                                          |
| > Status              Effective config                                |
|   Model               model                 custom-model      current |
|   Theme               backend               native            current |
|   Usage               effort                medium            10240 t |
|   Output              theme                 light             current |
|   Language            outputStyle           explanatory       user    |
|   Thinking            editorMode            normal            default |
|   Safety                                                              |
|   Config              Actions                                         |
|                       > Show effective config        /config show     |
|                         Show setting sources         /config sources  |
|-----------------------------------------------------------------------|
| Left/Right section | Up/Down navigate | Enter select | Esc close      |
+-----------------------------------------------------------------------+
```

关键期望:

- Enter on `Show effective config` submits `/config show`。
- Enter on `Show setting sources` submits `/config sources`。
- 当前值集中在 Status 面板，不再塞进 action description。

## Config Model Picker

```text
+ Config ---------------------------------------------------------------+
| model=custom-model                         effort=medium  10240 tokens |
|-----------------------------------------------------------------------|
| Sections              Model                                           |
|   Status              Filter  model                                   |
| > Model                                                               |
|   Theme               Available models                                |
|   Usage               > custom-model                                  |
|   Output                  id=custom-model  source=configured current  |
|   Language              SOTA                                          |
|   Thinking                id=claude-opus-4-20250514  source=configured|
|   Safety                                                              |
|   Config              Selection preview                               |
|                         command: /config set model custom-model       |
|-----------------------------------------------------------------------|
| Type filter | Up/Down navigate | Enter select | Left/Right section    |
+-----------------------------------------------------------------------+
```

关键期望:

- Model section 仍支持 Type filter。
- Enter on selected model submits `/config set model <id>`。
- 当前模型用 `current`，来源用 `source=configured|alias|custom`。

## Config Theme Picker

```text
+ Config ---------------------------------------------------------------+
| theme=light                                                   current |
|-----------------------------------------------------------------------|
| Sections              Theme                                           |
|   Status              Filter  theme                                   |
|   Model                                                               |
| > Theme               Themes                                          |
|   Usage                 Auto        match terminal when supported     |
|   Output                Dark        default ratatui palette           |
|   Language            > Light       light terminal palette  current   |
|   Thinking              Solarized   low-contrast terminal palette     |
|   Safety                Monokai     high-contrast editor palette      |
|   Config                Nord        cool low-saturation palette       |
|                                                                       |
|                       Selection preview                               |
|                         command: /config set theme light              |
|-----------------------------------------------------------------------|
| Type filter | Up/Down navigate | Enter select | Left/Right section    |
+-----------------------------------------------------------------------+
```

关键期望:

- Enter on selected theme submits `/config set theme <id>`。
- 不把 theme 列表渲染为普通 action list；它仍是 picker。

## Config Output

```text
+ Config ---------------------------------------------------------------+
| outputStyle=explanatory               editorMode=normal  progress=unset|
|-----------------------------------------------------------------------|
| Sections              Output                                          |
|   Status              Output style                                    |
|   Model               > Default        value=default                  |
|   Theme                 Explanatory    value=explanatory  current     |
|   Usage                 Learning       value=learning                 |
| > Output                Custom         fills /config set outputStyle  |
|   Language                                                            |
|   Thinking            Editor                                           |
|   Safety                Vim mode       value=vim                      |
|   Config                Normal mode    value=normal       current     |
|                                                                       |
|                       Terminal progress                               |
|                         Enable         value=true                     |
|                         Disable        value=false                    |
|-----------------------------------------------------------------------|
| Left/Right section | Up/Down navigate | Enter select | Esc close      |
+-----------------------------------------------------------------------+
```

关键期望:

- Enter on fixed styles submits `/config set outputStyle default|explanatory|learning`。
- Enter on Custom fills `/config set outputStyle `。
- Editor mode actions submit `/config set editorMode vim|normal`。

## Config Thinking

```text
+ Config ---------------------------------------------------------------+
| thinking=auto                         effort=medium  fastMode=not set |
|-----------------------------------------------------------------------|
| Sections              Thinking                                        |
|   Status              Filter  effort                                  |
|   Model                                                               |
|   Theme               Effort budget                                   |
|   Usage                 Auto      model default              10240 t  |
|   Output                Low       shorter thinking budget     4096 t  |
|   Language            > Medium    balanced thinking budget   10240 t  |
| > Thinking              High      deeper thinking budget     24576 t  |
|   Safety                Max       largest fixed budget       32768 t  |
|   Config                                                              |
|                       Selection preview                               |
|                         command: /config set effortLevel medium       |
|-----------------------------------------------------------------------|
| Type filter | Up/Down navigate | Enter select | Left/Right section    |
+-----------------------------------------------------------------------+
```

关键期望:

- Enter on selected effort submits `/config set effortLevel <id>`。
- token budget 是 secondary column，不再混在 description 字符串里。

## Sandbox Config

触发: `/sandbox`

```text
+ Sandbox --------------------------------------------------------------+
| effective=enabled                 mode=workspace  network=disabled    |
|-----------------------------------------------------------------------|
| Sections              Config                                          |
| > Config              Effective policy                                |
|   Dependencies        sandbox.enabled        true        source=user  |
|   Overrides           sandbox.mode           workspace   source=user  |
|   Doctor              network.disabled       true        source=local |
|   Mode                allowedDomains         all domains allowed      |
|   Network                                                             |
|                       Actions                                         |
|                       > Show full status              /sandbox status |
|-----------------------------------------------------------------------|
| Left/Right section | Up/Down navigate | Enter select | Esc close      |
+-----------------------------------------------------------------------+
```

关键期望:

- Enter on `Show full status` submits `/sandbox status`。
- Config row 是只读状态汇总，不隐藏来源。

## Sandbox Dependencies

```text
+ Sandbox --------------------------------------------------------------+
| effective=enabled                 mode=workspace  dependency=available |
|-----------------------------------------------------------------------|
| Sections              Dependencies                                    |
|   Config              Runtime checks                                  |
| > Dependencies        OS-level sandbox       PASS available           |
|   Overrides           Fallback behavior      best-effort fallback     |
|   Doctor              Network proxy runtime  not configured           |
|   Mode                                                                |
|   Network             Actions                                         |
|                       > Require OS primitive     failIfUnavailable=true|
|                         Allow best-effort fallback failIfUnavailable=false|
|-----------------------------------------------------------------------|
| Left/Right section | Up/Down navigate | Enter select | Esc close      |
+-----------------------------------------------------------------------+
```

关键期望:

- Enter on `Require OS primitive` submits `/sandbox require`。
- Enter on `Allow best-effort fallback` submits `/sandbox optional`。
- Dependency rows are read-only; action rows are selectable.

## Sandbox Mode

```text
+ Sandbox --------------------------------------------------------------+
| effective=enabled                            current mode=workspace   |
|-----------------------------------------------------------------------|
| Sections              Mode                                            |
|   Config              Session override                                |
|   Dependencies        > Enable sandbox       value=true               |
|   Overrides             Disable sandbox      value=false  risk=high   |
|   Doctor                                                              |
| > Mode                Execution mode                                  |
|   Network               Read-only mode       value=read-only          |
|                         Workspace mode       value=workspace current  |
|                         Full mode            value=full risk=high     |
|-----------------------------------------------------------------------|
| Left/Right section | Up/Down navigate | Enter select | Esc close      |
+-----------------------------------------------------------------------+
```

关键期望:

- Enter submits `/sandbox on|off` or `/sandbox mode read-only|workspace|full`。
- `Disable sandbox` and `Full mode` are visually risky and carry `risk=high` in snapshot。

## Sandbox Network

```text
+ Sandbox --------------------------------------------------------------+
| network=disabled                         allowedDomains=all domains   |
|-----------------------------------------------------------------------|
| Sections              Network                                         |
|   Config              Network access                                  |
|   Dependencies        > Enable network       value=enabled risk=medium|
|   Overrides             Disable network      value=disabled current   |
|   Doctor                                                              |
|   Mode                Proxy runtime                                   |
| > Network               HTTP proxy          unset                     |
|                         SOCKS proxy         unset                     |
|-----------------------------------------------------------------------|
| Left/Right section | Up/Down navigate | Enter select | Esc close      |
+-----------------------------------------------------------------------+
```

关键期望:

- Enter submits `/sandbox network on|off`。
- Network-on action carries explicit risk when current policy is disabled。

## Hooks User Scope

触发: `/hooks`

```text
+ Hooks ----------------------------------------------------------------+
| scope=user settings                         target=~/.cc-rust/settings |
|-----------------------------------------------------------------------|
| Scopes                Hook events                                     |
| > User settings       > PreToolUse       matchers=2  commands=2       |
|   Project settings      PostToolUse      matchers=1  commands=1       |
|                         Notification     matchers=0  commands=0       |
|                       Details                                         |
|                         event: PreToolUse                             |
|                         command: /hooks list PreToolUse               |
|                         open:    /hooks open user                     |
|-----------------------------------------------------------------------|
| Left/Right scope | Up/Down navigate | Enter list | o open scope | Esc |
+-----------------------------------------------------------------------+
```

关键期望:

- 面板本身只读。
- Enter submits `/hooks list <event>`。
- `o` submits `/hooks open user` in user scope。
- `u` submits `/hooks open user`; `p` submits `/hooks open project`。

## Hooks Project Scope

```text
+ Hooks ----------------------------------------------------------------+
| scope=project settings                       target=./.cc-rust/settings|
|-----------------------------------------------------------------------|
| Scopes                Hook events                                     |
|   User settings       > PreToolUse       matchers=0  commands=0       |
| > Project settings      PostToolUse      matchers=1  commands=1       |
|                         Notification     matchers=0  commands=0       |
|                         Stop             matchers=0  commands=0       |
|                       Details                                         |
|                         event: PreToolUse                             |
|                         command: /hooks list PreToolUse               |
|                         open:    /hooks open project                  |
|-----------------------------------------------------------------------|
| Left/Right scope | Up/Down navigate | Enter list | o open scope | Esc |
+-----------------------------------------------------------------------+
```

关键期望:

- Left/Right changes scope without changing selected event when possible。
- `o` submits `/hooks open project` in project scope。
- Empty hook events remain visible so users can discover valid event names。

## MCP Management

触发: `/mcp`

```text
+ MCP ------------------------------------------------------------------+
| servers=3                                  selected=docs  status=failed|
|-----------------------------------------------------------------------|
| Sections              Servers                                         |
| > Servers             Name          Kind     State       Tools        |
|   Edit                database      stdio    connected   1            |
|   Reconnect         > docs          remote   failed      0            |
|   Remove              agent-tools   agent    connecting  0            |
|   Add                                                                 |
|                       Details                                         |
|                         url: https://mcp.example.com                  |
|                         action: status                                |
|                         command: /mcp status                          |
|-----------------------------------------------------------------------|
| Left/Right action | Up/Down server | Enter apply | a add | Esc close  |
+-----------------------------------------------------------------------+
```

关键期望:

- Section/action selection still maps to current `/mcp status|edit|reconnect|remove` behavior.
- `a` fills `/mcp add `.
- Remove stays visible as `risk=medium` once the renderer supports risk styling.

## Login Management

触发: `/login`

```text
+ Login ----------------------------------------------------------------+
| profile=claude_code                      backend=native  codex=unknown |
|-----------------------------------------------------------------------|
| Sections              Login methods                                   |
| > Status              > Status       command=/login status            |
|   Claude Code key       Claude key   fills /login                     |
|   Claude.ai             Claude.ai    OAuth URL, then /login-code      |
|   Console               Console      OAuth URL, may create API key    |
|   Codex                 Codex        OpenAI Codex OAuth               |
|   Codex CLI             Codex CLI    import ~/.codex/auth.json        |
|                                                                       |
|                       Details                                         |
|                         selected: Status                              |
|                         shortcut: s                                   |
|-----------------------------------------------------------------------|
| Up/Down method | Enter select | s status | 1-5 select | Esc close     |
+-----------------------------------------------------------------------+
```

关键期望:

- Claude Code key fills `/login sk-ant-api03-`.
- OAuth rows submit `/login claude-ai`, `/login console`, or `/login codex-oauth` and then external URL output handles the next step.
- Codex CLI submits `/login codex-cli`.

## Permissions Management

触发: `/permissions` 或 `/perms`

```text
+ Permissions ----------------------------------------------------------+
| mode=default                       rules=2  session_grants=1 source=mix|
|-----------------------------------------------------------------------|
| Sections              Permission policy                               |
| > Status              current mode          default                   |
|   Rules               always allow rules     1 project                |
|   Session grants      ask rules              0                        |
|   Modes               deny rules             1 local                  |
|   Reset                                                               |
|                       Actions                                         |
|                       > Show permission summary       /permissions    |
|                         Add allow rule              /permissions allow|
|                         Add deny rule                /permissions deny |
|-----------------------------------------------------------------------|
| Left/Right section | Up/Down navigate | Enter select | Esc close      |
+-----------------------------------------------------------------------+
```

关键期望:

- Ideal surface may be added even though the old reference says current Rust had no dedicated surface.
- Safety-changing modes like auto/bypass require explicit command text with `--confirm`.
- Reset is direct today; ideal UI should mark it `risk=high` before any future confirm dialog is introduced.

## Memory Management

触发: `/memory`

```text
+ Memory ---------------------------------------------------------------+
| project=./CLAUDE.md                         user=~/.cc-rust/CLAUDE.md |
|-----------------------------------------------------------------------|
| Actions               Memory files                                    |
| > Edit                User memory       global     ~/.cc-rust/CLAUDE.md|
|   Show              > Project memory    project    ./CLAUDE.md new    |
|   Paths               docs/AGENTS.md    import     @-imported         |
|   Open                Auto memory       auto       ~/.cc-rust/memory  |
|                                                                       |
|                       Details                                         |
|                         command: /memory edit                         |
|                         shortcuts: a auto, t team, g global, o project|
|-----------------------------------------------------------------------|
| Left/Right action | Up/Down file | Enter select | a/t/g/o open | Esc  |
+-----------------------------------------------------------------------+
```

关键期望:

- Edit/Show/Paths/Open remain action sections.
- Enter maps to `/memory edit`, `/memory show`, `/memory path`, or `/memory open <scope>`.
- Imported files remain read-only references in the list.

## Plugin Management

触发: `/plugin`

```text
+ Plugins --------------------------------------------------------------+
| installed=3                              enabled=2  active_in_session=2|
|-----------------------------------------------------------------------|
| Sections              Plugins                                         |
| > Status              Name        Enabled   Session   Source          |
|   Enable              github      true      active    user            |
|   Disable             teams       true      active    user            |
|   Uninstall         > browser     false     inactive  bundled         |
|   Reload                                                              |
|                       Details                                         |
|                         command: /plugin status browser               |
|                         reload hint: /reload-plugins                  |
|-----------------------------------------------------------------------|
| Left/Right action | Up/Down plugin | Enter apply | r reload | Esc     |
+-----------------------------------------------------------------------+
```

关键期望:

- This is a target surface; current `/plugin` is text-only.
- Disable and uninstall rows must carry direct-execute semantics unless a future confirm dialog is implemented.
- Reload submits `/reload-plugins`.

## Runtime Preferences

触发: `/model`、`/effort`、`/fast`、`/advisor`、`/experimental`、`/statusline`、`/notify`、`/voice`、`/ide`

```text
+ Runtime Preferences --------------------------------------------------+
| model=custom-model          effort=medium  fast=false  voice=disabled |
|-----------------------------------------------------------------------|
| Sections              Preference                                      |
| > Model               active model          custom-model              |
|   Effort              effort level          medium                    |
|   Fast mode           fastMode              false                     |
|   Advisor             advisor model         unset                     |
|   Experimental        enabled gates         0                         |
|   Status line         command               scripts/statusline.ps1    |
|   Notify              push notifications    unavailable              |
|   Voice               language              English                   |
|   IDE                 selected IDE          unset                     |
|                                                                       |
|                       Details                                         |
|                         preferred editor: use /config for model/effort|
|                         external bridge: use /ide reconnect           |
|-----------------------------------------------------------------------|
| Left/Right section | Up/Down navigate | Enter details | Esc close     |
+-----------------------------------------------------------------------+
```

关键期望:

- This consolidates text-only preference commands into a future management surface.
- `/model` and `/effort` should continue pointing users to `/config` pickers until implemented.
- `/statusline`, `/voice`, and `/ide` rows must show whether they update runtime-only state, persisted settings, or both.

## Implementation Notes

- First implementation pass should introduce a structured view model before changing individual surfaces.
- `ConfigSurface`, `SandboxSurface`, `HooksSurface`, `McpSurface`, `LoginSurface`, `MemorySurface`, and future text-command settings surfaces should render through the same panel primitive.
- Existing behavior tests should remain command-oriented; visual assertions should compare these snapshots only after the renderer is implemented.
- Do not migrate unrelated surfaces until the initial settings surfaces prove the shared layout handles pickers, read-only rows, action rows, and risk markers.
