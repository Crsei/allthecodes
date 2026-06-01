# External Flow Snapshots

来源: [../../COMMAND_UI_REFERENCE.md](../../COMMAND_UI_REFERENCE.md) §四、外部页面、二维码或 App

这些流程会离开当前 TUI，打开 URL、外部编辑器或 IDE/App bridge。Better View 目标是把“离开 TUI 后要做什么”渲染成清晰的外部步骤面板。

## OAuth URL Output

适用: `/login claude-ai`、`/login console`、`/login codex-oauth`、`/mcp auth start <name>`

```text
+ External authorization ----------------------------------------------+
| provider=OpenAI Codex                         status=waiting_for_code |
|-----------------------------------------------------------------------|
| Steps                 Details                                         |
| > Open URL            https://example.auth/authorize?...              |
|   Complete in browser Grant access to cc-rust                         |
|   Paste code          /login-code <code>                              |
|                                                                       |
|                       Notes                                           |
|                         TUI does not embed a browser                  |
|                         Keep state token until completion             |
|-----------------------------------------------------------------------|
| Copy URL | Enter continue | Esc close                                 |
+-----------------------------------------------------------------------+
```

关键期望:

- Do not imply embedded browser support.
- Completion command must be visible.
- Provider-specific notes may appear in the detail panel.

## External Editor Open

适用: `/hooks open <layer>`、`/keybindings`、`/keybindings open`、`/plan open`、`/plan edit`

```text
+ External editor ------------------------------------------------------+
| editor=code --wait                            status=launch requested |
|-----------------------------------------------------------------------|
| Target                File                                            |
| > Project settings    ./.cc-rust/settings.json                       |
|                                                                       |
|                       Behavior                                        |
|                         command: /hooks open project                  |
|                         fallback: print path when editor unavailable  |
|-----------------------------------------------------------------------|
| Enter open | Esc close                                                |
+-----------------------------------------------------------------------+
```

关键期望:

- File creation, if any, is stated before launch.
- No editor configured means show path, not failure.

## Chrome And IDE Bridge

适用: `/chrome help`、`/chrome reconnect`、`/ide reconnect`

```text
+ External bridge ------------------------------------------------------+
| bridge=Chrome native host                    status=needs_reconnect   |
|-----------------------------------------------------------------------|
| Actions               Detail                                          |
| > Show help           Chrome Web Store and permissions URLs           |
|   Reconnect           reinstall or refresh native host                |
|                                                                       |
|                       Command preview                                 |
|                         /chrome reconnect                             |
|-----------------------------------------------------------------------|
| Up/Down action | Enter run | Esc close                                |
+-----------------------------------------------------------------------+
```

关键期望:

- These commands may print URLs or trigger bridge detection; they do not directly open the browser unless command behavior changes.
- IDE reconnect should name the selected IDE when available.

## No QR Code Session Output

适用: `/session`

```text
+ Sessions -------------------------------------------------------------+
| remote_qr=unsupported                         mode=text list          |
|-----------------------------------------------------------------------|
| Sessions              Detail                                          |
| > current session     Rust TUI shows text session information         |
|                                                                       |
|                       Compatibility note                              |
|                         TypeScript remote session QR is not rendered  |
|                         by current Rust slash command behavior        |
|-----------------------------------------------------------------------|
| Up/Down session | Enter inspect | Esc close                           |
+-----------------------------------------------------------------------+
```

关键期望:

- Do not add QR snapshots for Rust `/session`.
- If future remote QR support is introduced, it needs a separate explicit design update.
