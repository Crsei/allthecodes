# Selector Surface Snapshots

来源: [../../COMMAND_UI_REFERENCE.md](../../COMMAND_UI_REFERENCE.md) §二、选择器类

这些 snapshot 描述选择器类入口的目标形态。它们和设置面板共享“左侧导航、右侧详情、固定 footer”的结构，但以列表选择和预览为主。

## Command Palette

触发: 输入 `/`

```text
+ Commands -------------------------------------------------------------+
| query=/                                   matches=64  mode=insert      |
|-----------------------------------------------------------------------|
| Commands              Command details                                 |
| > /init               Initialize project config and CLAUDE.md         |
|   /add-dir            usage: /init                                    |
|   /advisor            example: /init                                  |
|   /agents                                                             |
|   /audit-export       Behavior                                        |
|                         Enter inserts: /init                          |
|                         Ctrl+E opens edit target when available        |
|-----------------------------------------------------------------------|
| Type filter | Up/Down navigate | Enter insert | Ctrl+E target | Esc   |
+-----------------------------------------------------------------------+
```

关键期望:

- Enter inserts `/<command> `, not execute argument-taking commands.
- Filter text remains inside palette state.
- Commands with edit targets expose target preview in the detail panel.

## Edit Target Picker

触发: command palette 中按 `Ctrl+E`

```text
+ Edit target ----------------------------------------------------------+
| command=/hooks                                      targets=3          |
|-----------------------------------------------------------------------|
| Targets               Target details                                  |
| > user settings       path=~/.cc-rust/settings.json                   |
|   project settings    path=./.cc-rust/settings.json                   |
|   local settings      path=./.cc-rust/settings.local.json             |
|                                                                       |
|                       Insert preview                                  |
|                         /hooks user                                   |
|-----------------------------------------------------------------------|
| Up/Down target | Enter insert target | Esc cancel                     |
+-----------------------------------------------------------------------+
```

关键期望:

- The picker inserts a target argument only; it does not open the editor directly.
- Target names must remain compatible with normal slash command arguments.

## Agents

触发: `/agents`

```text
+ Agents ---------------------------------------------------------------+
| source=all                                      agents=3               |
|-----------------------------------------------------------------------|
| Sources               Agents                                          |
| > All               > general-purpose   built-in  General task exec   |
|   Built-in agents     debugger          built-in  Root-cause analysis |
|   User agents         code-reviewer     built-in  Review regressions  |
|   Project agents                                                      |
|                       Details                                         |
|                         command: /agents show general-purpose         |
|-----------------------------------------------------------------------|
| Left/Right source | Up/Down agent | Enter show | Esc close            |
+-----------------------------------------------------------------------+
```

关键期望:

- Source tab changes filter and resets selection.
- Enter submits `/agents show <agent>`.
- Create-agent wizard remains hidden until explicitly wired.

## Diff

触发: `/diff`

```text
+ Uncommitted changes --------------------------------------------------+
| source=worktree                                  files=1  +3 -1        |
|-----------------------------------------------------------------------|
| Sources               Files                                           |
| > Worktree          > src/main.rs                         +3 -1       |
|   Staged                                                              |
|                                                                       |
|                       Preview                                         |
|                         @@ src/main.rs                                |
|                         - old_call();                                 |
|                         + new_call();                                 |
|-----------------------------------------------------------------------|
| Left/Right source | Up/Down file | Enter detail | Esc close           |
+-----------------------------------------------------------------------+
```

Detail view:

```text
+ Uncommitted changes / src/main.rs ------------------------------------+
| source=worktree                                      +3 -1             |
|-----------------------------------------------------------------------|
| Diff                                                                  |
| @@ src/main.rs                                                        |
|   fn main() {                                                         |
| -     old_call();                                                     |
| +     new_call();                                                     |
| +     extra_call();                                                   |
|   }                                                                   |
|-----------------------------------------------------------------------|
| b back | Up/Down scroll | Esc close                                   |
+-----------------------------------------------------------------------+
```

关键期望:

- Enter switches from list to detail.
- `b` returns to list.
- Source selection is only available in list mode.

## Skills

触发: `/skills`

```text
+ Skills ---------------------------------------------------------------+
| filter=review                                  visible=3  total=48     |
|-----------------------------------------------------------------------|
| Skills                Skill details                                   |
| > code-review         enabled=true  source=bundled                    |
|   security-review     Review diffs for regressions                    |
|   writer                                                              |
|                       Commands                                        |
|                         Enter: /skills code-review                    |
|                         r: /skills reload                             |
|                         d: /skills diagnostics                        |
|-----------------------------------------------------------------------|
| Type filter | Backspace edit | Up/Down skill | Enter details | Esc    |
+-----------------------------------------------------------------------+
```

关键期望:

- Normal characters update filter.
- `r` and `d` are command shortcuts only when filter is empty.
- Disabled skills remain visible with disabled reason where available.

## Tasks

触发: `/tasks`

```text
+ Background tasks -----------------------------------------------------+
| tasks=3                                      running=1  failed=1       |
|-----------------------------------------------------------------------|
| Tasks                 Task details                                    |
|   cargo test          shell    running     12.4s  running tests       |
| > remote deploy       remote   failed       0ms   connection lost     |
|   review worker       agent    succeeded    0ms   reported findings  |
|                                                                       |
|                       Commands                                        |
|                         Enter: /tasks show task-2                     |
|                         s/k:   /tasks stop task-2                     |
|                         d:     /tasks delete task-2                   |
|-----------------------------------------------------------------------|
| Up/Down task | Enter details | s/k stop | d delete | r refresh | Esc  |
+-----------------------------------------------------------------------+
```

关键期望:

- Team-backed task stop maps to `/team kill <name>`.
- Delete is available only for tool-backed tasks.
- Direct-execute commands are clearly visible and should not imply an existing confirm dialog.

## Team

触发: `/team` 或 `/teams`

```text
+ Team -----------------------------------------------------------------+
| team=ui-port                                  active=true  members=2   |
|-----------------------------------------------------------------------|
| Teammates             Teammate details                                |
| > builder             status=running  mode=ask  tasks=1               |
|   reviewer            status=idle     mode=ask  tasks=0               |
|                                                                       |
|                       Commands                                        |
|                         Enter: /team status                           |
|                         s:     /team send builder                     |
|                         k:     /team kill builder                     |
|                         p:     /team spawn                            |
|-----------------------------------------------------------------------|
| Up/Down member | Enter status | s send | k kill | p spawn | Esc       |
+-----------------------------------------------------------------------+
```

关键期望:

- Inactive team state should collapse to create/list actions only.
- Kill is direct-execute today and must be marked as such in future detail rows.

## LSP Recommendation

触发: LSP plugin recommendation event

```text
+ LSP plugin recommendation -------------------------------------------+
| language=Rust                             reason=Cargo.toml detected  |
|-----------------------------------------------------------------------|
| Choices               Plugin                                          |
| > Yes, install        rust-analyzer                                   |
|   No, not now         install prompt: /plugin install rust-analyzer   |
|   Never for Rust                                                      |
|   Disable recommendations                                             |
|-----------------------------------------------------------------------|
| Up/Down choice | Enter submit | Esc no                                |
+-----------------------------------------------------------------------+
```

关键期望:

- Yes fills `/plugin install rust-analyzer `.
- The snapshot must not imply that `/plugin install` is fully implemented unless command support exists.

## History Search

触发: `Ctrl+R`

```text
+ History search -------------------------------------------------------+
| filter=cargo                                      matches=3            |
|-----------------------------------------------------------------------|
| History               Preview                                         |
| > cargo test -p claude-code-rs --test e2e_terminal                    |
|   cargo clippy --workspace --all-targets -- -D warnings               |
|   cargo build --release                                               |
|                                                                       |
|                       Fill preview                                    |
|                         prompt: cargo test -p claude-code-rs ...      |
|-----------------------------------------------------------------------|
| Type filter | Up/Down history | Enter fill prompt | Esc close         |
+-----------------------------------------------------------------------+
```

关键期望:

- Enter fills the prompt; it does not auto-submit.
- This is not a slash command surface but belongs in selector coverage.

