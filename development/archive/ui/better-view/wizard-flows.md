# Wizard Flow Snapshots

来源: [../../COMMAND_UI_REFERENCE.md](../../COMMAND_UI_REFERENCE.md) §三、向导类

当前 Rust TUI 的多步骤流程多数是文本式流程。Better View 的目标是把这些流程呈现成可恢复、步骤明确的面板，同时不暗示当前运行时已经实现这些 wizard。

## Login OAuth Flow

```text
+ Login / Claude Code -------------------------------------------------+
| profile=claude_code method=Claude.ai        step=1/3  status=waiting |
|-----------------------------------------------------------------------|
| Steps                 OAuth details                                   |
| > Choose method       provider: Claude.ai                             |
|   Open URL            command: /login claude-ai                       |
|   Enter code          completion: /login-code <code>                  |
|                                                                       |
|                       Next action                                     |
|                         Start OAuth and print external URL            |
|-----------------------------------------------------------------------|
| Enter start | Back previous | Esc close                               |
+-----------------------------------------------------------------------+
```

关键期望:

- Starting OAuth submits `/login claude-ai`, `/login console`, or `/login codex-oauth`.
- The URL display belongs to external flow output.
- Code completion remains `/login-code <code>`.

## MCP Auth Flow

```text
+ MCP Auth -------------------------------------------------------------+
| server=docs                                  step=1/3  status=ready    |
|-----------------------------------------------------------------------|
| Steps                 Authorization                                  |
| > Start auth          command: /mcp auth start docs                   |
|   Open URL            redirect URI and state are printed externally   |
|   Complete            /mcp auth complete docs --code=<code>           |
|                                                                       |
|                       Safety                                          |
|                         state token must match start response         |
|-----------------------------------------------------------------------|
| Enter start | Back previous | Esc close                               |
+-----------------------------------------------------------------------+
```

关键期望:

- Auth start is text/external after Enter.
- Completion command is explicit and copyable in detail panel.

## Plan Workflow

```text
+ Plan workflow --------------------------------------------------------+
| mode=plan                                  state=draft  approval=none |
|-----------------------------------------------------------------------|
| Steps                 Plan lifecycle                                  |
| > Enter plan mode     command: /plan enter                            |
|   Open plan           command: /plan open                             |
|   Edit plan           command: /plan edit                             |
|   Approve             command: /plan approve                          |
|   Reject              command: /plan reject                           |
|                                                                       |
|                       Verification                                    |
|                         no file edits while planning                  |
|-----------------------------------------------------------------------|
| Up/Down step | Enter run | Esc close                                  |
+-----------------------------------------------------------------------+
```

关键期望:

- Enter/exit plan mode still use approval semantics from permission requests.
- `/plan open` and `/plan edit` are external editor flows.
- Approve/reject are explicit slash commands, not hidden buttons.

## Permissions Plan Mode

```text
+ Permission Plan Mode -------------------------------------------------+
| mode=default                                      target=plan          |
|-----------------------------------------------------------------------|
| Steps                 Effect                                          |
| > Enter plan mode     command: /permissions mode plan                 |
|   Review plan         tools require plan approval                     |
|   Exit/approve        command: /plan approve or /plan reject          |
|-----------------------------------------------------------------------|
| Enter apply | Esc close                                               |
+-----------------------------------------------------------------------+
```

关键期望:

- The panel explains the workflow state change before command submission.
- It must not bypass normal plan approval gates.

## Create Agent Wizard Target

尚未通过 `/agents` 接线，但组件存在。Better View 目标形态:

```text
+ Create agent ---------------------------------------------------------+
| step=3/10                                      location=project        |
|-----------------------------------------------------------------------|
| Steps                 Agent definition                                |
|   Method              generate or manual                              |
|   Type              > executor                                        |
| > Description         implement bounded feature work                  |
|   Prompt              system prompt preview                           |
|   Tools               inherited defaults                              |
|   Model               repo default                                    |
|   Location            project                                         |
|   Memory              none                                            |
|   Color               auto                                            |
|   Confirm                                                             |
|-----------------------------------------------------------------------|
| Next | Back | Enter edit | Esc cancel                                 |
+-----------------------------------------------------------------------+
```

关键期望:

- This is a future target only.
- `/agents` should not show create wizard until explicitly wired.
