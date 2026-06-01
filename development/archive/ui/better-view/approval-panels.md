# Approval Panel Snapshots

来源: [../../COMMAND_UI_REFERENCE.md](../../COMMAND_UI_REFERENCE.md) §五、非设置界面的确认/审批类

审批面板不是 settings surface，但应共享 Better View 的信息层级：上下文摘要、风险说明、可选决策、固定 footer。

## Tool Permission

```text
+ Permission required --------------------------------------------------+
| tool=bash                                      risk=shell command      |
|-----------------------------------------------------------------------|
| Request               cargo test                                      |
| Context               worker=executor                                 |
|                       cwd=F:/repo                                     |
|                                                                       |
| Decisions             > Allow once        allow                       |
|                         Deny              deny                        |
|                         Always allow      always allow  scope=project |
|-----------------------------------------------------------------------|
| Up/Down decision | Enter confirm | Esc deny                           |
+-----------------------------------------------------------------------+
```

关键期望:

- Esc denies.
- Always allow must show persistence scope.

## File Edit Permission

```text
+ File edit permission -------------------------------------------------+
| file=src/lib.rs                              operation=replace range   |
|-----------------------------------------------------------------------|
| Summary               Added 2 lines                                   |
| Diff                                                                  |
|   1  1   fn main() {                                                  |
|   2  2       new_call();                                              |
|      3 +     extra_call();                                            |
|   3  4   }                                                            |
|                                                                       |
| Decisions             > Allow edit        allow                       |
|                         Deny edit         deny                        |
|                         Always allow path always allow  scope=request |
|-----------------------------------------------------------------------|
| Up/Down decision | Enter confirm | Esc deny                           |
+-----------------------------------------------------------------------+
```

关键期望:

- Diff/patch preview is first-class content, not a description line.
- Always path scope must be explicit.

## Ask User Question

```text
+ Need input -----------------------------------------------------------+
| question=2/3                                      source=tool request  |
|-----------------------------------------------------------------------|
| Choices               Answer                                          |
|   [ ] Inspect logs                                                    |
| > [x] Ask operator                                                    |
|   [ ] Abort                                                           |
|                                                                       |
| Navigation            Back | Next | Submit                            |
|-----------------------------------------------------------------------|
| Up/Down choice | Space toggle | Enter next | Esc cancel               |
+-----------------------------------------------------------------------+
```

关键期望:

- Multiple-choice state must be visible.
- Submit remains distinct from Next when more questions remain.

## MCP Project Server Approval

Single server:

```text
+ New MCP server -------------------------------------------------------+
| server=playwright                         source=.mcp.json risk=code  |
|-----------------------------------------------------------------------|
| Decisions             Use this and future project MCP servers         |
|                       > Use this MCP server                           |
|                         Continue without this MCP server              |
|                                                                       |
| Commands              approve all: /mcp approve playwright --all-project|
|                       approve one: /mcp approve playwright            |
|                       reject:      /mcp reject playwright             |
|-----------------------------------------------------------------------|
| Up/Down decision | Enter submit | Esc reject                          |
+-----------------------------------------------------------------------+
```

Multiple servers:

```text
+ New MCP servers ------------------------------------------------------+
| count=3                                  source=.mcp.json risk=code    |
|-----------------------------------------------------------------------|
| Servers               Decision                                        |
|   [x] github          enable                                          |
| > [ ] playwright      disable                                         |
|   [x] sentry          enable                                          |
|                                                                       |
| Commands              enable:  /mcp approve github sentry             |
|                       disable: /mcp reject playwright                 |
|-----------------------------------------------------------------------|
| Up/Down server | Space toggle | Enter apply | Esc reject all          |
+-----------------------------------------------------------------------+
```

关键期望:

- Risk statement remains visible while choosing.
- Multi-server output shows derived approve/reject commands.

## Workspace Trust

```text
+ Workspace trust required --------------------------------------------+
| path=F:/AIclassmanager/cc/rust                 status=untrusted        |
|-----------------------------------------------------------------------|
| Decisions             > Trust this workspace                          |
|                         Exit                                          |
|                         Reject                                        |
|                                                                       |
| Effect                Trust allows project instructions and tools     |
|-----------------------------------------------------------------------|
| Up/Down decision | Enter confirm | Esc reject                         |
+-----------------------------------------------------------------------+
```

关键期望:

- Path must be exact.
- Reject/Exit behavior must be distinct if implementation distinguishes them.

## Direct Execute Status

适用: `/tasks stop <id>`、`/tasks delete <id>`、`/team kill|delete|leave`、`/logout`、`/clear`、`/mcp remove <name>`、`/plugin disable|uninstall <id>`、`/permissions reset`

```text
+ Direct execution -----------------------------------------------------+
| command=/tasks delete task-1                    confirmation=none      |
|-----------------------------------------------------------------------|
| Effect                delete task record                              |
| Status                executed after explicit slash command input     |
|                                                                       |
| Future UI note        candidate for confirmation dialog               |
|                       current snapshot must not draw a confirm modal   |
|-----------------------------------------------------------------------|
| Esc close                                                            |
+-----------------------------------------------------------------------+
```

关键期望:

- These commands must remain documented as direct execution until code adds confirmation.
- Future Better View designs may add confirm dialogs only with behavior changes.

