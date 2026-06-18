# Approval And Direct-Execution Snapshots

来源: [COMMAND_UI_REFERENCE.md](../../COMMAND_UI_REFERENCE.md)

这些 snapshot 覆盖非设置界面的确认、审批，以及当前明确没有额外确认弹窗的状态变更命令。

## Tool Permission Dialogs

通用工具权限:

```text
Permission required
tool: bash
request: cargo test
worker: executor
risk: shell command
details:
  - cwd: F:/repo
options:
> Allow [allow] - run this tool once (this request)
  Deny [deny] - block this tool call (this request)
  Always allow [always allow] - save a reusable allow rule (project)

Enter confirms | Esc denies
```

Shell/PowerShell 权限:

```text
PowerShell command permission
tool: powershell
request: Get-ChildItem
details:
  - shell: powershell
command: Get-ChildItem
risk: shell command
options:
> Allow [allow] - run shell command once (this request)
  Deny [deny] - do not run command (this request)
  Always allow exact command [always allow] - save exact command rule (project)
```

文件写入/编辑权限:

```text
File edit permission
tool: edit
request: src/lib.rs
details:
  - operation: replace range
file: src/lib.rs
summary: Added 2 lines
   1    1   fn main() {
   2    2       new_call();
        3 +     extra_call();
   3    4   }
options:
> Allow edit [allow] - permit edit to src/lib.rs (this request)
  Deny edit [deny] - leave file unchanged (this request)
  Always allow path [always allow] - save rule for src/lib.rs (this request)
```

Web fetch 权限:

```text
Web fetch permission
tool: web_fetch
request: https://example.com
risk: network access
details:
  - method: GET
options:
  Allow [allow] - run this tool once (this request)
  Deny [deny] - block this tool call (this request)
> Always allow [always allow] - save a reusable allow rule (project)
```

Computer-use 权限:

```text
Computer use approval
tool: computer
request: click
risk: interactive desktop action
details:
  - target: browser window
  - screenshot: true
options:
> Allow [allow] - run this tool once (this request)
  Deny [deny] - block this tool call (this request)
  Always allow [always allow] - save a reusable allow rule (project)
```

AskUserQuestion:

```text
Ask user question
Need input
  [ ] Inspect logs
> [x] Ask operator
  [ ] Abort
Question 2/3 | Back | Next | Submit
```

## Plan Approval

进入 plan mode:

```text
Enter plan mode
reason: Need design first
No file edits will be made while planning
```

退出 plan mode:

```text
Exit plan mode
plan: implement helpers
verification:
  - cargo test
```

Slash command 审批:

```text
/plan approve
state: approved
decision: current plan may proceed
```

```text
/plan reject
state: rejected
decision: current plan must be revised
```

Team plan approval:

```text
Team plan approval
teammate: builder
request: approve implementation plan

> Approve
  Reject
  Ask for changes
```

## MCP Project Server Approval

单 server:

```text
New MCP server found in .mcp.json: playwright / Filter...
MCP servers may execute code or access system resources. All tool calls require approval.

  Use this and all future MCP servers in this project
    enable current server and trust future project MCP entries -> /mcp approve playwright --all-project
> Use this MCP server
    enable only this project MCP server -> /mcp approve playwright
  Continue without using this MCP server
    record this project MCP server as disabled -> /mcp reject playwright

Enter submit | Up/Down navigate | Esc reject
```

多 server:

```text
3 new MCP servers found in .mcp.json
Select any you wish to enable.
MCP servers may execute code or access system resources. All tool calls require approval.

  [x] github
> [ ] playwright
  [x] sentry

enable: /mcp approve github sentry
disable: /mcp reject playwright
Enter apply | Space toggle | Esc reject all
```

## LSP Recommendation

触发: LSP plugin recommendation event

```text
LSP plugin recommendation
language: Rust
plugin: rust-analyzer
reason: Cargo.toml detected

> Yes, install rust-analyzer
  No, not now
  Never for Rust
  Disable recommendations

Enter submit | Up/Down navigate | Esc no
```

关键期望:

- Yes 填充 `/plugin install rust-analyzer `。
- 当前 `/plugin` handler 是否支持 install 由命令实现决定，不在此处暗示已完整支持。

## Workspace Trust Gate

启动时 workspace trust prompt:

```text
Workspace trust required
path: F:/AIclassmanager/cc/rust

Do you trust this workspace?

> Trust this workspace
  Exit
  Reject

Enter confirm | Esc reject
```

## Direct Execute No Confirm

以下命令会改变状态，但当前没有额外确认 dialog。Snapshot 要表现为“显式输入后直接执行”，不要画 confirm modal。

### Tasks Stop Delete

触发: `/tasks stop <id>`

```text
/tasks stop task-1
action: stop background task
confirmation_dialog: none
status: stop requested
```

触发: `/tasks delete <id>`

```text
/tasks delete task-1
action: delete task record
confirmation_dialog: none
status: deleted
```

### Team Kill Delete Leave

触发: `/team kill <name>`

```text
/team kill builder
action: stop teammate
confirmation_dialog: none
status: kill requested
```

触发: `/team delete <name>`

```text
/team delete builder
action: delete teammate metadata
confirmation_dialog: none
status: deleted
```

触发: `/team leave`

```text
/team leave
action: leave active team
confirmation_dialog: none
status: left team
```

### Logout Clear

触发: `/logout`

```text
/logout
action: clear auth/onboarding derived state
confirmation_dialog: none
status: logged out
```

触发: `/clear`

```text
/clear
action: clear current conversation view/state
confirmation_dialog: none
status: cleared
```

### MCP Remove

触发: `/mcp remove <name>`

```text
/mcp remove docs
action: remove MCP server entry
confirmation_dialog: none
status: removed
```

### Plugin Disable Uninstall

触发: `/plugin disable <id>`

```text
/plugin disable browser
action: disable plugin
confirmation_dialog: none
status: disabled
note: /reload-plugins may be needed for current session
```

触发: `/plugin uninstall <id>`

```text
/plugin uninstall browser
action: uninstall plugin
confirmation_dialog: none
status: uninstalled
note: /reload-plugins may be needed for current session
```

### Permissions Reset

触发: `/permissions reset`

```text
/permissions reset
action: reset permission rules
confirmation_dialog: none
status: reset complete
```
