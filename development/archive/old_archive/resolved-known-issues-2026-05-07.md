# 2026-05-07 已关闭问题归档

本文件记录从 [../KNOWN_ISSUES.md](../KNOWN_ISSUES.md) 移出的已修复、已失效或已被更精确问题替代的历史问题。

## 已修复或已关闭

| 历史编号 | 标题 | 关闭口径 |
| --- | --- | --- |
| #2 | Composer lacked a frame and busy indicator could ghost in the footer | 已修复，历史细节从活跃 issue 入口移出。 |
| #3 | Message area had no visible scroll affordance and was hard to navigate | 已修复，消息滚动、键盘滚动和 responsive wrapping 已落地。 |
| #6 | Background agent + worktree isolation 未组合 | 由 `engine/agent/supervisor.rs` 路径收口，不再作为 open caveat；若回归再新开。 |
| #7 | Background agent 子引擎无 permission_callback | 同上，不再作为 open caveat。 |
| #8 | Background agent 无取消机制 | 同上，不再作为 open caveat。 |
| #9 | Tool-call display and shortcut discoverability were too weak | 已修复，tool activity timeline 与快捷键 registry 已落地。 |
| #10 | Busy prompt could not queue follow-up steering messages | 已修复，frontend FIFO queue 与 composer preview 已落地。 |
| #11 | Prompt-mode tool groups hid the actual commands and felt too opaque | 已修复，prompt summary 与 transcript full detail 已分层。 |
| #12 | Composer stopped accepting pasted text | 已修复，paste event 与 multi-character insertion 已覆盖。 |
| #13 | IME multi-character input only kept the last committed character | 已修复，live refs insertion path 已修正。 |
| #14 | Conversation rows lacked visual separation and file references blended into code | 已修复，message row 和 markdown token 样式已调整。 |
| #15 | Headless AskUserQuestion left an orphaned tool call | 已修复，pending-question bridge 已接入。 |
| #16 | AskUserQuestion tool activity rendered raw JSON | 已修复，question summary 与 callout render 已补。 |
| #18 | Rust TUI subsystem setting modules had no selectable surfaces | 已修复，`/agents`、`/hooks`、`/mcp`、`/memory` 和 LSP recommendation overlay 已接入。 |
| #19 | Rust TUI terminal scrolling did not receive wheel events | 已修复，wheel/key scroll dispatch 与 PTY regression 已补。 |
| #20 | Rust TUI mouse capture blocked native terminal text selection | 已修复，mouse capture 默认关闭并提供 opt-in。 |
| BUILD-001 | `plugins/tools.rs` 的 `permission_ctx.mode` 部分移动导致 `claude-code-rs` 无法编译 | 已关闭。当前实现对 `permission_ctx.mode` 按引用匹配并 clone 出目标 `PermissionMode`，避免部分移动；`cargo build -p claude-code-rs` 和 `cargo build -p claude-code-rs --release` 均通过。 |

## 已替换为更精确的开放项

| 历史编号 | 旧问题 | 现开放项 |
| --- | --- | --- |
| #1 | Terminal resize does not reflow content | Rust TUI 已关闭；TS/OpenTUI resize/fullscreen artifact 保留为 `UI-001`。 |
| #4 | Welcome screen Tips text truncated in narrow terminals | Rust welcome 已基本收口；若仍复现，按 UI 截图证据新开问题。 |
| #5 | ASCII art logo renders as fragmented blocks | Rust welcome 已移除旧 ASCII logo 路径；若仍复现，按 UI 截图证据新开问题。 |
| #17 | Maximizing then restoring terminal leaves white rows | 合并到 `UI-001`。 |
| #21 | Rust TUI latest shell output auto-expand | 保留为 `UI-002`。 |
| #22 | Rust TUI Ctrl+R session-only history | 保留为 `UI-003`。 |
| Browser MCP rendering path | Real-server path not exercised | 保留为 `UI-004`。 |

## 2026-05-14 验证关闭

| 历史编号 | 标题 | 关闭口径 |
| --- | --- | --- |
| TEST-001 | `tests/e2e_terminal/phase6.rs` 未纳入版本控制 | `phase6.rs` 当前已在 git tracked 文件集中，且 `cargo test --workspace` 覆盖 `e2e_terminal::phase6` 通过。 |
| CONTEXT-003 | Phase 10 归档记录显示 `cargo test -p claude-code-rs` 门禁未 green | 2026-05-14 crate-migration 验证中 `cargo test --workspace` 通过；默认 live PTY/API 用例保持 `#[ignore]`。 |
| TEST-002 | ratatui UI parity closeout 后 package-wide test run not green | 2026-05-14 crate-migration 验证中 `cargo fmt --all --check`、`cargo check --workspace --all-targets`、`cargo test --workspace` 和 `cargo build --workspace --release` 均通过。 |

## 2026-05-17 验证关闭

| 历史编号 | 标题 | 关闭口径 |
| --- | --- | --- |
| CLIPPY-001 | `cargo clippy -p claude-code-rs --all-targets -- -D warnings` 被跨模块 lint 阻塞 | 已清理 `gateway`、`cc-ipc-protocol`、IPC transport/adapters、`cc-tools`、`cc-engine` 与 Rust TUI 主 crate 的 clippy 阻塞项；同命令通过。机器缺少 `npm` 导致 web-ui build script 仍打印跳过 warning，按仓库说明不计入 Rust clippy 阻塞。 |
| WORKTREE-001 | `WorktreeRemove` repo root / symlink / Windows junction 边界不完整 | `WorktreeSession` 记录创建时解析出的 `git_root`，ExitWorktree hook payload 与 git cleanup 使用该 root；worktree path 校验改为词法 root + resolved-boundary 校验，并覆盖 Unix symlink 逃逸与 Windows junction 逃逸测试路径。 |

## 2026-05-07 编译过程记录

- `cargo build -p claude-code-rs`：通过；未出现 compiler error 或 warning。
- `cargo test -p claude-code-rs plugins::tools`：通过；3 个 plugin tool 权限/stdio runtime 测试通过。
- `cargo test -p claude-code-rs model_mapping`：通过；11 个 model mapping 测试通过，确认原先被 `plugins/tools.rs` 编译错误挡住的目标测试现在可以进入并完成。
- `cargo build -p claude-code-rs --release`：通过；未出现 compiler error 或 warning。
- `cargo build --release`：通过；顶层 release 构建未出现 compiler error 或 warning。
- `cargo clippy -p claude-code-rs --all-targets -- -D warnings`：仍失败，已登记为 `CLIPPY-001`。本轮已清理低风险机械项：`cc-config` 的 `map_or(true, ...)`、`cc-sandbox` 的 needless lifetime、`cc-mcp` 的 `io::ErrorKind::Other` / `as_bytes().len()`、`cc-engine` 的 derivable `Default`、`cc-session` 的 explicit counter loop。
- 当前 clippy 剩余问题主要集中在 `claude-code-rs` 包内：`too_many_arguments`、`large_enum_variant`、`field_reassign_with_default`、`items_after_test_module`、`manual_strip`、`manual_is_multiple_of`、`needless_borrow` 等，需单独计划清理或明确 allow 口径。
- `cargo fmt --all --check`：仍失败于仓库范围既有格式差异；本轮触碰的 Rust 文件已用 `rustfmt --edition 2021 --check` 单独确认通过。
- 编译过程中的非失败现象：并行执行 cargo 命令时出现 package cache / build directory file lock 等待提示；等待后自动继续，没有形成构建阻塞。
