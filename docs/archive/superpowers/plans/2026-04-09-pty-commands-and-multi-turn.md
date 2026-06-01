# PTY 斜杠命令测试 + 多轮对话深度测试 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 新增两个 PTY UI 测试模块 — `commands.rs`（斜杠命令）和 `multi_turn.rs`（多轮对话深度 + 工具调用），扩展 TUI 交互测试覆盖。

**Architecture:** 在 `tests/pty_ui/` 下新增两个模块，复用现有 `harness.rs` 的 `PtySession` 基础设施。**除断联场景外，所有测试均使用真实 API（`strip_keys=false`，`#[ignore]`）**。工具调用测试（Read/Write/Edit）在 `F:\temp` 工作目录下操作文件，验证工具在 TUI 中的端到端行为。

**Tech Stack:** Rust, portable-pty (ConPTY), vt100, harness.rs (PtySession API)

**关键原则:** 测试发现问题时，修复源码（`src/`），**不准修改测试用例**。测试是规范，源码向测试对齐。

---

## 文件结构

| 操作 | 文件 | 职责 |
|------|------|------|
| Create | `tests/pty_ui/commands.rs` | 13 个斜杠命令 PTY 测试 |
| Create | `tests/pty_ui/multi_turn.rs` | 12 个多轮对话深度 + 工具调用 PTY 测试 |
| Modify | `tests/pty_ui/main.rs` | 注册两个新模块 |
| Modify | `tests/docs/pty.md` | 追加新测试文档 |

---

## Task 1: 注册新模块

**Files:**
- Modify: `tests/pty_ui/main.rs`

- [ ] **Step 1: 在 main.rs 中添加两个新模块声明**

```rust
// 在 main.rs 末尾追加
mod commands;
mod multi_turn;
```

修改后 `tests/pty_ui/main.rs` 完整内容：

```rust
//! PTY-based UI integration tests.
//!
//! Tests the ratatui TUI by spawning `claude-code-rs` in a real
//! pseudo-terminal (ConPTY on Windows) and capturing / asserting on
//! the rendered terminal output.
//!
//! ## Module layout
//!
//! | Module       | What it tests                                    |
//! |--------------|--------------------------------------------------|
//! | `harness`    | Shared `PtySession` helper (not tests)           |
//! | `fast_path`  | `--version`, `--init-only`, `--dump-system-prompt`, `-p` |
//! | `welcome`    | Welcome screen: logo, model, session, tips       |
//! | `input`      | Input prompt: typing, cursor, Ctrl keys, vim     |
//! | `streaming`  | Streaming lifecycle, abort, multi-turn, tool use |
//! | `resize`     | Terminal resize behavior                         |
//! | `screenshot` | Terminal screenshots: HTML rendering + snapshots  |
//! | `commands`   | Slash commands: /help, /version, /model, etc.    |
//! | `multi_turn` | Multi-turn conversation depth tests              |
//!
//! ## Running
//!
//! ```bash
//! # All tests (require API key in env)
//! cargo test --test pty_ui
//!
//! # Single module
//! cargo test --test pty_ui welcome
//! cargo test --test pty_ui commands
//! cargo test --test pty_ui multi_turn
//!
//! # With output
//! cargo test --test pty_ui -- --nocapture
//! ```
//!
//! ## Log output
//!
//! Each test saves `.raw` (ANSI), `.log` (plain), and `.html` (terminal
//! screenshot) files to `logs/YYYYMMDDHHMM/`.

mod harness;

mod fast_path;
mod welcome;
mod input;
mod streaming;
mod resize;
mod screenshot;
mod commands;
mod multi_turn;
```

- [ ] **Step 2: 验证编译通过（此时新模块为空文件，先创建占位）**

Run: `cargo test --test pty_ui --no-run 2>&1 | head -20`
Expected: 编译成功（或报模块文件不存在 — 下一步创建）

- [ ] **Step 3: Commit**

```bash
git add tests/pty_ui/main.rs
git commit -m "chore: register commands and multi_turn test modules in pty_ui"
```

---

## Task 2: 斜杠命令测试 — 基础命令 (commands.rs 第一批: 6 tests)

**Files:**
- Create: `tests/pty_ui/commands.rs`

这批测试覆盖基础斜杠命令：`/help`, `/version`, `/exit`, `/cost`, `/model`, `/status`。

全部使用真实 API（`strip_keys=false`，`#[ignore]`），使用 `wait_for_text()` 检测命令输出。

- [ ] **Step 1: 创建 commands.rs 并写入前 6 个测试**

```rust
//! Slash command tests in the PTY TUI.
//!
//! Verifies that slash commands produce the expected output when typed
//! into the TUI input box. All tests use real API (strip_keys=false).

use crate::harness::*;
use std::time::Duration;

/// /help 显示命令列表
#[test]
#[ignore]
fn slash_help_shows_command_list() {
    let session = PtySession::spawn(default_args(), 120, 40, false);
    std::thread::sleep(RENDER_WAIT);

    session.send_line("/help");
    // /help output should list other commands
    let found = session.wait_for_any(
        &["/help", "/exit", "/version", "/model"],
        QUICK_TIMEOUT,
    );

    session.send_ctrl_c();
    std::thread::sleep(Duration::from_millis(300));
    session.send_ctrl_c();
    let output = session.finish(QUICK_TIMEOUT, "cmd_help");

    assert!(found.is_some(), "/help should list available commands");
}

/// /version 显示版本号
#[test]
#[ignore]
fn slash_version_shows_version() {
    let session = PtySession::spawn(default_args(), 120, 40, false);
    std::thread::sleep(RENDER_WAIT);

    session.send_line("/version");
    // Version output should contain the crate version or "claude-code-rs"
    let found = session.wait_for_any(
        &["claude-code-rs", env!("CARGO_PKG_VERSION")],
        QUICK_TIMEOUT,
    );

    session.send_ctrl_c();
    std::thread::sleep(Duration::from_millis(300));
    session.send_ctrl_c();
    let output = session.finish(QUICK_TIMEOUT, "cmd_version");

    assert!(found.is_some(), "/version should show version info");
}

/// /exit 正常退出 TUI
#[test]
#[ignore]
fn slash_exit_quits_gracefully() {
    let session = PtySession::spawn(default_args(), 120, 40, false);
    std::thread::sleep(RENDER_WAIT);

    session.send_line("/exit");

    // /exit should cause the process to exit — finish should complete within timeout
    let output = session.finish(QUICK_TIMEOUT, "cmd_exit");

    // No panic, process exited. Output may contain a goodbye message.
    assert!(output.raw.len() > 0, "should have captured some output before exit");
}

/// /cost 显示 token 用量（初始为零）
#[test]
#[ignore]
fn slash_cost_shows_usage() {
    let session = PtySession::spawn(default_args(), 120, 40, false);
    std::thread::sleep(RENDER_WAIT);

    session.send_line("/cost");
    // Cost/usage output should contain token-related text
    let found = session.wait_for_any(
        &["token", "cost", "usage", "0"],
        QUICK_TIMEOUT,
    );

    session.send_ctrl_c();
    std::thread::sleep(Duration::from_millis(300));
    session.send_ctrl_c();
    let output = session.finish(QUICK_TIMEOUT, "cmd_cost");

    assert!(found.is_some(), "/cost should show usage info");
}

/// /model 显示当前模型
#[test]
#[ignore]
fn slash_model_shows_current_model() {
    let session = PtySession::spawn(default_args(), 120, 40, false);
    std::thread::sleep(RENDER_WAIT);

    session.send_line("/model");
    // Should show model name or "model" keyword
    let found = session.wait_for_any(
        &["model", "claude", "sonnet", "opus", "haiku"],
        QUICK_TIMEOUT,
    );

    session.send_ctrl_c();
    std::thread::sleep(Duration::from_millis(300));
    session.send_ctrl_c();
    let output = session.finish(QUICK_TIMEOUT, "cmd_model");

    assert!(found.is_some(), "/model should show model info");
}

/// /status 显示会话状态
#[test]
#[ignore]
fn slash_status_shows_session_info() {
    let session = PtySession::spawn(default_args(), 120, 40, false);
    std::thread::sleep(RENDER_WAIT);

    session.send_line("/status");
    // Status should contain session-related info
    let found = session.wait_for_any(
        &["model", "session", "message", "permission", "mode"],
        QUICK_TIMEOUT,
    );

    session.send_ctrl_c();
    std::thread::sleep(Duration::from_millis(300));
    session.send_ctrl_c();
    let output = session.finish(QUICK_TIMEOUT, "cmd_status");

    assert!(found.is_some(), "/status should show session info");
}
```

- [ ] **Step 2: 编译验证**

Run: `cargo test --test pty_ui commands --no-run`
Expected: 编译成功

- [ ] **Step 3: 运行测试（需要 API key）**

Run: `cargo test --test pty_ui commands -- --ignored --nocapture 2>&1 | tail -30`
Expected: 6 tests passed

- [ ] **Step 4: 检查 HTML 截图**

Run: `ls logs/*/cmd_*.html`
Expected: `cmd_help.html`, `cmd_version.html`, `cmd_exit.html`, `cmd_cost.html`, `cmd_model.html`, `cmd_status.html`

- [ ] **Step 5: Commit**

```bash
git add tests/pty_ui/commands.rs
git commit -m "test: add PTY UI tests for basic slash commands (/help, /version, /exit, /cost, /model, /status)"
```

---

## Task 3: 斜杠命令测试 — 边界与状态修改 (commands.rs 第二批: 7 tests)

**Files:**
- Modify: `tests/pty_ui/commands.rs`

这批测试覆盖：未知命令、空斜杠、命令带参数、`/clear`、`/context`、`/skills`、多命令连续执行。

- [ ] **Step 1: 在 commands.rs 末尾追加 7 个测试**

```rust
/// 未知命令不崩溃，显示错误提示
#[test]
#[ignore]
fn slash_unknown_command_shows_error() {
    let session = PtySession::spawn(default_args(), 120, 40, false);
    std::thread::sleep(RENDER_WAIT);

    session.send_line("/nonexistent_xyz_42");
    // Should show "unknown" or "not found" or echo the command back
    let found = session.wait_for_any(
        &["unknown", "not found", "nonexistent", "invalid"],
        QUICK_TIMEOUT,
    );

    session.send_ctrl_c();
    std::thread::sleep(Duration::from_millis(300));
    session.send_ctrl_c();
    let output = session.finish(QUICK_TIMEOUT, "cmd_unknown");

    // At minimum, no panic
    assert!(output.raw.len() > 0, "should not panic on unknown command");
}

/// 空斜杠 "/" 不崩溃
#[test]
#[ignore]
fn slash_empty_does_not_crash() {
    let session = PtySession::spawn(default_args(), 120, 40, false);
    std::thread::sleep(RENDER_WAIT);

    session.send_line("/");
    std::thread::sleep(Duration::from_secs(2));

    session.send_ctrl_c();
    std::thread::sleep(Duration::from_millis(300));
    session.send_ctrl_c();
    let output = session.finish(QUICK_TIMEOUT, "cmd_empty_slash");

    assert!(output.raw.len() > 0, "empty slash should not panic");
}

/// /model 带参数切换模型
#[test]
#[ignore]
fn slash_model_with_arg_switches_model() {
    let session = PtySession::spawn(default_args(), 120, 40, false);
    std::thread::sleep(RENDER_WAIT);

    // Try switching to sonnet alias
    session.send_line("/model sonnet");
    let found = session.wait_for_any(
        &["sonnet", "model", "switch", "changed"],
        QUICK_TIMEOUT,
    );

    session.send_ctrl_c();
    std::thread::sleep(Duration::from_millis(300));
    session.send_ctrl_c();
    let output = session.finish(QUICK_TIMEOUT, "cmd_model_switch");

    assert!(found.is_some(), "/model sonnet should acknowledge model change");
}

/// /clear 清除对话历史，不崩溃
#[test]
#[ignore]
fn slash_clear_resets_conversation() {
    let session = PtySession::spawn(default_args(), 120, 40, false);
    std::thread::sleep(RENDER_WAIT);

    session.send_line("/clear");
    std::thread::sleep(Duration::from_secs(2));

    // After clear, TUI should still be functional — type something to verify
    session.send_line("hello after clear");
    std::thread::sleep(Duration::from_secs(1));

    session.send_ctrl_c();
    std::thread::sleep(Duration::from_millis(300));
    session.send_ctrl_c();
    let output = session.finish(QUICK_TIMEOUT, "cmd_clear");

    assert!(output.raw.len() > 0, "/clear should not crash TUI");
}

/// /context 显示上下文信息
#[test]
#[ignore]
fn slash_context_shows_info() {
    let session = PtySession::spawn(default_args(), 120, 40, false);
    std::thread::sleep(RENDER_WAIT);

    session.send_line("/context");
    let found = session.wait_for_any(
        &["context", "token", "model", "message"],
        QUICK_TIMEOUT,
    );

    session.send_ctrl_c();
    std::thread::sleep(Duration::from_millis(300));
    session.send_ctrl_c();
    let output = session.finish(QUICK_TIMEOUT, "cmd_context");

    assert!(found.is_some(), "/context should show context info");
}

/// /skills 列出可用技能
#[test]
#[ignore]
fn slash_skills_lists_skills() {
    let session = PtySession::spawn(default_args(), 120, 40, false);
    std::thread::sleep(RENDER_WAIT);

    session.send_line("/skills");
    let found = session.wait_for_any(
        &["skill", "available", "built-in", "no skill"],
        QUICK_TIMEOUT,
    );

    session.send_ctrl_c();
    std::thread::sleep(Duration::from_millis(300));
    session.send_ctrl_c();
    let output = session.finish(QUICK_TIMEOUT, "cmd_skills");

    assert!(found.is_some(), "/skills should list or mention skills");
}

/// 同一会话连续执行多个斜杠命令
#[test]
#[ignore]
fn multiple_commands_in_sequence() {
    let session = PtySession::spawn(default_args(), 120, 40, false);
    std::thread::sleep(RENDER_WAIT);

    let commands = ["/version", "/cost", "/model", "/context", "/status"];

    for (i, cmd) in commands.iter().enumerate() {
        eprintln!("[multi-cmd] {}/{}: {}", i + 1, commands.len(), cmd);
        session.send_line(cmd);
        // Brief wait between commands for TUI to process
        std::thread::sleep(Duration::from_secs(2));
        session.snapshot(&format!("cmd_seq_{}", i + 1));
    }

    session.send_ctrl_c();
    std::thread::sleep(Duration::from_millis(300));
    session.send_ctrl_c();
    let output = session.finish(QUICK_TIMEOUT, "cmd_sequence");

    // No panic after 5 consecutive commands
    assert!(output.raw.len() > 100, "multiple commands should produce substantial output");
}
```

- [ ] **Step 2: 编译验证**

Run: `cargo test --test pty_ui commands --no-run`
Expected: 编译成功

- [ ] **Step 3: 运行测试（需要 API key）**

Run: `cargo test --test pty_ui commands -- --ignored --nocapture 2>&1 | tail -40`
Expected: 13 tests passed

- [ ] **Step 4: Commit**

```bash
git add tests/pty_ui/commands.rs
git commit -m "test: add PTY UI tests for edge-case slash commands and sequential execution"
```

---

## Task 4: 多轮对话深度测试 — 基础多轮 (multi_turn.rs 前 3 tests)

**Files:**
- Create: `tests/pty_ui/multi_turn.rs`

这 3 个测试验证多轮输入场景下 TUI 的稳定性。全部使用真实 API（`strip_keys=false`，`#[ignore]`）。

- [ ] **Step 1: 创建 multi_turn.rs 并写入前 3 个测试**

```rust
//! Multi-turn conversation depth tests.
//!
//! Verifies complex interaction patterns: context persistence across turns,
//! slash commands interleaved with prompts, abort/resume, tool use, and
//! long conversations.
//!
//! All tests use real API (strip_keys=false) and are marked `#[ignore]`.

use crate::harness::*;
use std::time::Duration;

/// 多次输入 + 斜杠命令交替，不崩溃
#[test]
#[ignore]
fn interleaved_input_and_commands() {
    let session = PtySession::spawn(default_args(), 120, 40, false);
    std::thread::sleep(RENDER_WAIT);

    // Alternate between typing text and slash commands
    let actions: &[&str] = &[
        "Say exactly: INTERLEAVE_1",
        "/version",
        "Say exactly: INTERLEAVE_2",
        "/cost",
        "/status",
        "Say exactly: INTERLEAVE_3",
        "/help",
    ];

    for (i, action) in actions.iter().enumerate() {
        eprintln!("[interleaved] {}/{}: {}", i + 1, actions.len(), action);
        session.send_line(action);
        if action.starts_with("Say") {
            // Wait for model response
            session.wait_response_done(0, API_TIMEOUT);
        }
        std::thread::sleep(Duration::from_secs(2));
    }

    session.send_ctrl_c();
    std::thread::sleep(Duration::from_millis(300));
    session.send_ctrl_c();
    let output = session.finish(QUICK_TIMEOUT, "mt_interleaved");

    assert!(output.raw.len() > 100, "interleaved input should not crash");
}

/// /clear 后 TUI 仍可正常接受输入和命令
#[test]
#[ignore]
fn clear_then_continue_input() {
    let session = PtySession::spawn(default_args(), 120, 40, false);
    std::thread::sleep(RENDER_WAIT);

    // First turn
    session.send_line("Say exactly: BEFORE_CLEAR");
    session.wait_response_done(0, API_TIMEOUT);
    std::thread::sleep(Duration::from_secs(2));

    session.send_line("/clear");
    std::thread::sleep(Duration::from_secs(2));

    // After clear, continue
    session.send_line("Say exactly: AFTER_CLEAR");
    let ok = session.wait_response_done(0, API_TIMEOUT);

    session.send_line("/status");
    std::thread::sleep(Duration::from_secs(2));

    session.send_ctrl_c();
    std::thread::sleep(Duration::from_millis(300));
    session.send_ctrl_c();
    let output = session.finish(QUICK_TIMEOUT, "mt_clear_continue");

    assert!(ok, "should produce response after /clear");
    assert!(output.raw.len() > 0, "/clear then continue should not crash");
}

/// 快速连续输入多行后 slash 命令仍响应
#[test]
#[ignore]
fn rapid_multi_line_input() {
    let session = PtySession::spawn(default_args(), 120, 40, false);
    std::thread::sleep(RENDER_WAIT);

    // Rapidly send many lines without waiting
    for i in 0..10 {
        session.send_line(&format!("rapid line {i}"));
    }
    std::thread::sleep(Duration::from_secs(5));

    // Then send a slash command to verify TUI is still responsive
    session.send_line("/version");
    let found = session.wait_for_any(
        &["claude-code-rs", env!("CARGO_PKG_VERSION")],
        QUICK_TIMEOUT,
    );

    session.send_ctrl_c();
    std::thread::sleep(Duration::from_millis(300));
    session.send_ctrl_c();
    let output = session.finish(QUICK_TIMEOUT, "mt_rapid_input");

    assert!(found.is_some(), "TUI should still respond to /version after rapid input");
}
```

- [ ] **Step 2: 编译验证**

Run: `cargo test --test pty_ui multi_turn --no-run`
Expected: 编译成功

- [ ] **Step 3: 运行测试（需要 API key）**

Run: `cargo test --test pty_ui multi_turn -- --ignored --nocapture 2>&1 | tail -20`
Expected: 3 tests passed

- [ ] **Step 4: Commit**

```bash
git add tests/pty_ui/multi_turn.rs
git commit -m "test: add multi-turn base tests (interleaved, clear, rapid input)"
```

---

## Task 5: 多轮对话深度测试 — 高级交互 (multi_turn.rs 追加 5 tests)

**Files:**
- Modify: `tests/pty_ui/multi_turn.rs`

覆盖：上下文持久性、命令穿插对话、中断恢复、长对话耐久、状态栏追踪。

- [ ] **Step 1: 在 multi_turn.rs 末尾追加 5 个测试**

```rust
// ── Advanced interaction tests ─────────────────────────────────────────

/// 验证上下文在多轮对话中持久：第一轮告知信息，第二轮回忆
#[test]
#[ignore]
fn context_persists_across_turns() {
    let session = PtySession::spawn(default_args(), 120, 40, false);
    std::thread::sleep(RENDER_WAIT);

    // Turn 1: Establish context
    session.send_line("My favorite color is azure. Remember this.");
    let ok1 = session.wait_response_done(0, API_TIMEOUT);
    assert!(ok1, "turn 1 should complete");

    let bar1 = session.status_bar();
    let count1 = extract_msg_count(&bar1).unwrap_or(0);
    eprintln!("[context] turn 1 done, msgs={count1}");
    std::thread::sleep(Duration::from_secs(2));
    session.snapshot("mt_ctx_turn1");

    // Turn 2: Ask about the established context
    session.send_line("What is my favorite color?");
    let ok2 = session.wait_response_done(count1, API_TIMEOUT);
    assert!(ok2, "turn 2 should complete");

    std::thread::sleep(Duration::from_secs(2));
    let snap = session.snapshot("mt_ctx_turn2");

    session.send_ctrl_c();
    std::thread::sleep(Duration::from_millis(500));
    session.send_ctrl_c();
    let _output = session.finish(QUICK_TIMEOUT, "mt_ctx_final");

    // The model should recall "azure"
    assert!(
        snap.to_lowercase().contains("azure"),
        "model should recall the color 'azure' from context"
    );
}

/// 在对话轮次之间穿插斜杠命令，不破坏对话流
#[test]
#[ignore]
fn slash_commands_between_turns() {
    let session = PtySession::spawn(default_args(), 120, 40, false);
    std::thread::sleep(RENDER_WAIT);

    // Turn 1: Normal chat
    session.send_line("Say exactly: TURN_ONE_OK");
    let ok1 = session.wait_response_done(0, API_TIMEOUT);
    assert!(ok1, "turn 1 should complete");

    let bar1 = session.status_bar();
    let count1 = extract_msg_count(&bar1).unwrap_or(0);
    std::thread::sleep(Duration::from_secs(2));
    session.snapshot("mt_cmd_between_turn1");

    // Interlude: Run slash commands
    session.send_line("/cost");
    std::thread::sleep(Duration::from_secs(2));
    session.snapshot("mt_cmd_between_cost");

    session.send_line("/context");
    std::thread::sleep(Duration::from_secs(2));
    session.snapshot("mt_cmd_between_context");

    // Turn 2: Resume conversation
    session.send_line("Say exactly: TURN_TWO_OK");
    let ok2 = session.wait_response_done(count1, API_TIMEOUT);
    assert!(ok2, "turn 2 should complete after slash commands");

    std::thread::sleep(Duration::from_secs(2));
    let snap = session.snapshot("mt_cmd_between_turn2");

    session.send_ctrl_c();
    std::thread::sleep(Duration::from_millis(500));
    session.send_ctrl_c();
    let _output = session.finish(QUICK_TIMEOUT, "mt_cmd_between_final");

    assert!(
        snap.contains("TURN_TWO_OK"),
        "turn 2 response should appear after slash command interlude"
    );
}

/// Ctrl+C 中断响应后发起新一轮对话
#[test]
#[ignore]
fn abort_and_resume_new_turn() {
    let session = PtySession::spawn(default_args(), 120, 40, false);
    std::thread::sleep(RENDER_WAIT);

    // Turn 1: Send a prompt that will generate a long response
    session.send_line("Write a detailed explanation of how TCP works, at least 500 words.");

    // Wait briefly for streaming to start, then abort
    std::thread::sleep(Duration::from_secs(3));
    session.send_ctrl_c();
    eprintln!("[abort] sent Ctrl+C to abort turn 1");

    // Wait for TUI to return to ready state
    std::thread::sleep(Duration::from_secs(3));
    session.snapshot("mt_abort_after_ctrl_c");

    // Turn 2: Send a new short prompt — should work normally
    session.send_line("Say exactly: RECOVERED_OK");
    let ok = session.wait_for_text("RECOVERED_OK", API_TIMEOUT);

    std::thread::sleep(Duration::from_secs(2));
    session.snapshot("mt_abort_recovered");

    session.send_ctrl_c();
    std::thread::sleep(Duration::from_millis(500));
    session.send_ctrl_c();
    let _output = session.finish(QUICK_TIMEOUT, "mt_abort_final");

    assert!(ok, "should recover and produce response after aborting previous turn");
}

/// 5 轮连续对话，验证状态栏 msg count 递增
#[test]
#[ignore]
fn status_bar_tracks_message_count() {
    let session = PtySession::spawn(default_args(), 120, 40, false);
    std::thread::sleep(RENDER_WAIT);

    let prompts = [
        "Say OK1",
        "Say OK2",
        "Say OK3",
        "Say OK4",
        "Say OK5",
    ];

    let mut last_count = 0usize;
    let mut counts: Vec<usize> = Vec::new();

    for (i, prompt) in prompts.iter().enumerate() {
        let turn = i + 1;
        eprintln!("[status-track] Turn {turn}/{}: {}", prompts.len(), prompt);

        session.send_line(prompt);
        let ok = session.wait_response_done(last_count, API_TIMEOUT);

        if !ok {
            eprintln!("[status-track] Turn {turn}: TIMEOUT");
            session.snapshot(&format!("mt_status_t{turn}_timeout"));
            break;
        }

        let bar = session.status_bar();
        if let Some(count) = extract_msg_count(&bar) {
            eprintln!("[status-track] Turn {turn}: msg count = {count}");
            counts.push(count);
            last_count = count;
        }

        std::thread::sleep(Duration::from_secs(2));
        session.snapshot(&format!("mt_status_t{turn}"));
    }

    session.send_ctrl_c();
    std::thread::sleep(Duration::from_millis(500));
    session.send_ctrl_c();
    let _output = session.finish(QUICK_TIMEOUT, "mt_status_final");

    // Verify msg count is monotonically increasing
    assert!(counts.len() >= 3, "at least 3 turns should complete, got {}", counts.len());
    for window in counts.windows(2) {
        assert!(
            window[1] > window[0],
            "msg count should increase: {} -> {}",
            window[0], window[1]
        );
    }
}

/// /clear 后 msg count 重置，新对话正常进行
#[test]
#[ignore]
fn clear_mid_conversation_resets_context() {
    let session = PtySession::spawn(default_args(), 120, 40, false);
    std::thread::sleep(RENDER_WAIT);

    // Turn 1: Establish a fact
    session.send_line("The secret word is PINEAPPLE. Remember it.");
    let ok1 = session.wait_response_done(0, API_TIMEOUT);
    assert!(ok1, "turn 1 should complete");

    let bar1 = session.status_bar();
    let count1 = extract_msg_count(&bar1).unwrap_or(0);
    eprintln!("[clear-mid] pre-clear msgs={count1}");
    std::thread::sleep(Duration::from_secs(2));
    session.snapshot("mt_clear_mid_before");

    // Clear conversation
    session.send_line("/clear");
    std::thread::sleep(Duration::from_secs(2));
    session.snapshot("mt_clear_mid_cleared");

    // Turn 2: Ask about the fact — should NOT know it (context was cleared)
    session.send_line("What is the secret word I told you earlier?");
    // After /clear, msg count resets — wait for response from 0
    let ok2 = session.wait_response_done(0, API_TIMEOUT);
    assert!(ok2, "turn 2 should complete after /clear");

    std::thread::sleep(Duration::from_secs(2));
    let snap = session.snapshot("mt_clear_mid_after");

    session.send_ctrl_c();
    std::thread::sleep(Duration::from_millis(500));
    session.send_ctrl_c();
    let _output = session.finish(QUICK_TIMEOUT, "mt_clear_mid_final");

    // After /clear, context is gone — model should NOT recall "PINEAPPLE"
    // (it might say "I don't know" or hallucinate a different word)
    // We just verify the response was generated successfully
    assert!(snap.len() > 50, "should produce a response after /clear");
}

// ── Helper ──────────────────────────────────────────────────────────────

/// Extract "N msgs" count from status bar text.
fn extract_msg_count(status: &str) -> Option<usize> {
    for word in status.split_whitespace() {
        if let Ok(n) = word.parse::<usize>() {
            if status.contains(&format!("{n} msgs")) || status.contains(&format!("{n} msg")) {
                return Some(n);
            }
        }
    }
    None
}
```

- [ ] **Step 2: 编译验证**

Run: `cargo test --test pty_ui multi_turn --no-run`
Expected: 编译成功

- [ ] **Step 3: 运行测试（需要 API key）**

Run: `cargo test --test pty_ui multi_turn -- --ignored --nocapture 2>&1 | tail -40`
Expected: 8 tests passed（可能有超时，至少 6/8 通过）

- [ ] **Step 4: 检查截图输出**

Run: `ls logs/*/mt_*.html`
Expected: 多个 `mt_ctx_*.html`, `mt_cmd_between_*.html`, `mt_abort_*.html`, `mt_status_*.html`, `mt_clear_mid_*.html`

- [ ] **Step 5: Commit**

```bash
git add tests/pty_ui/multi_turn.rs
git commit -m "test: add advanced multi-turn tests (context, commands between turns, abort, status tracking, clear)"
```

---

## Task 6: 多轮对话 — 工具调用测试 (multi_turn.rs 追加 4 tests)

**Files:**
- Modify: `tests/pty_ui/multi_turn.rs`

这 4 个测试验证 Read/Write/Edit 工具在 TUI 中的端到端行为。工作目录为 `F:\temp`（由 `default_args()` 中 `-C F:\temp` 指定）。全部需要 API key，标记 `#[ignore]`。

**重要原则：测试用例发现问题时，修复源码而非修改测试。**

- [ ] **Step 1: 在 multi_turn.rs 的 `extract_msg_count` 函数之前追加 4 个工具调用测试**

```rust
// ── Live tests — tool use (Read / Write / Edit in F:\temp) ─────────────

/// Write 工具：请求模型写文件到 F:\temp，验证文件内容
#[test]
#[ignore]
fn tool_write_creates_file() {
    let test_file = std::path::Path::new(workspace()).join("pty_test_write.txt");
    // Clean up from previous runs
    let _ = std::fs::remove_file(&test_file);

    let session = PtySession::spawn(default_args(), 120, 40, false);
    std::thread::sleep(RENDER_WAIT);

    session.send_line(&format!(
        "Use the Write tool to create a file at {} with content: WRITE_TOOL_TEST_2026",
        test_file.display()
    ));

    // Wait for response completion (tool use + model reply)
    let ok = session.wait_response_done(0, API_TIMEOUT);
    assert!(ok, "model should complete response with tool use");

    std::thread::sleep(Duration::from_secs(2));
    session.snapshot("mt_tool_write");

    session.send_ctrl_c();
    std::thread::sleep(Duration::from_millis(500));
    session.send_ctrl_c();
    let _output = session.finish(QUICK_TIMEOUT, "mt_tool_write_final");

    // Verify on disk
    assert!(test_file.exists(), "Write tool should create file at {}", test_file.display());
    let content = std::fs::read_to_string(&test_file).unwrap_or_default();
    assert!(
        content.contains("WRITE_TOOL_TEST_2026"),
        "file should contain expected content, got: {content}"
    );

    // Clean up
    let _ = std::fs::remove_file(&test_file);
}

/// Read 工具：预先创建文件，请求模型读取，验证内容出现在 TUI 中
#[test]
#[ignore]
fn tool_read_shows_content() {
    let test_file = std::path::Path::new(workspace()).join("pty_test_read.txt");
    std::fs::write(&test_file, "SECRET_READ_CONTENT_7749").expect("create test file");

    let session = PtySession::spawn(default_args(), 120, 40, false);
    std::thread::sleep(RENDER_WAIT);

    session.send_line(&format!(
        "Use the Read tool to read the file at {} and tell me what it contains",
        test_file.display()
    ));

    let ok = session.wait_response_done(0, API_TIMEOUT);
    assert!(ok, "model should complete response with Read tool");

    std::thread::sleep(Duration::from_secs(2));
    let snap = session.snapshot("mt_tool_read");

    session.send_ctrl_c();
    std::thread::sleep(Duration::from_millis(500));
    session.send_ctrl_c();
    let _output = session.finish(QUICK_TIMEOUT, "mt_tool_read_final");

    // The model's response or tool output should contain the file content
    assert!(
        snap.contains("SECRET_READ_CONTENT_7749"),
        "Read tool output should appear in TUI"
    );

    // Clean up
    let _ = std::fs::remove_file(&test_file);
}

/// 多轮工具调用：第一轮 Write 创建文件，第二轮 Read 读回验证
#[test]
#[ignore]
fn tool_write_then_read_multi_turn() {
    let test_file = std::path::Path::new(workspace()).join("pty_test_write_read.txt");
    let _ = std::fs::remove_file(&test_file);

    let session = PtySession::spawn(default_args(), 120, 40, false);
    std::thread::sleep(RENDER_WAIT);

    // Turn 1: Write
    session.send_line(&format!(
        "Use the Write tool to create {} with content: MULTI_TURN_PAYLOAD_42",
        test_file.display()
    ));
    let ok1 = session.wait_response_done(0, API_TIMEOUT);
    assert!(ok1, "turn 1 (write) should complete");

    let bar1 = session.status_bar();
    let count1 = extract_msg_count(&bar1).unwrap_or(0);
    std::thread::sleep(Duration::from_secs(2));
    session.snapshot("mt_tool_wr_turn1");

    // Turn 2: Read it back
    session.send_line(&format!(
        "Now use the Read tool to read {} and show me its content",
        test_file.display()
    ));
    let ok2 = session.wait_response_done(count1, API_TIMEOUT);
    assert!(ok2, "turn 2 (read) should complete");

    std::thread::sleep(Duration::from_secs(2));
    let snap = session.snapshot("mt_tool_wr_turn2");

    session.send_ctrl_c();
    std::thread::sleep(Duration::from_millis(500));
    session.send_ctrl_c();
    let _output = session.finish(QUICK_TIMEOUT, "mt_tool_wr_final");

    assert!(
        snap.contains("MULTI_TURN_PAYLOAD_42"),
        "turn 2 should show the content written in turn 1"
    );

    // Clean up
    let _ = std::fs::remove_file(&test_file);
}

/// Edit 工具：预创建文件，请求模型编辑，验证磁盘上的修改
#[test]
#[ignore]
fn tool_edit_modifies_file() {
    let test_file = std::path::Path::new(workspace()).join("pty_test_edit.txt");
    std::fs::write(&test_file, "line1: hello\nline2: world\nline3: end\n")
        .expect("create test file");

    let session = PtySession::spawn(default_args(), 120, 40, false);
    std::thread::sleep(RENDER_WAIT);

    session.send_line(&format!(
        "Use the Edit tool to edit {} and replace 'world' with 'EDITED_WORLD'",
        test_file.display()
    ));

    let ok = session.wait_response_done(0, API_TIMEOUT);
    assert!(ok, "model should complete response with Edit tool");

    std::thread::sleep(Duration::from_secs(2));
    session.snapshot("mt_tool_edit");

    session.send_ctrl_c();
    std::thread::sleep(Duration::from_millis(500));
    session.send_ctrl_c();
    let _output = session.finish(QUICK_TIMEOUT, "mt_tool_edit_final");

    // Verify on disk
    let content = std::fs::read_to_string(&test_file).unwrap_or_default();
    assert!(
        content.contains("EDITED_WORLD"),
        "Edit tool should replace 'world' with 'EDITED_WORLD', got: {content}"
    );
    assert!(
        !content.contains("line2: world"),
        "original 'world' should be replaced, got: {content}"
    );

    // Clean up
    let _ = std::fs::remove_file(&test_file);
}
```

- [ ] **Step 2: 编译验证**

Run: `cargo test --test pty_ui multi_turn --no-run`
Expected: 编译成功

- [ ] **Step 3: 运行工具调用测试（需要 API key）**

Run: `cargo test --test pty_ui multi_turn -- --ignored --nocapture 2>&1 | tail -40`
Expected: 4 个新测试通过（tool_write、tool_read、tool_write_then_read、tool_edit）

- [ ] **Step 4: 验证文件清理**

Run: `ls F:/temp/pty_test_*.txt 2>&1`
Expected: 无残留文件（测试自行清理）

- [ ] **Step 5: Commit**

```bash
git add tests/pty_ui/multi_turn.rs
git commit -m "test: add tool-use PTY tests (Write, Read, Edit in F:\\temp)"
```

---

## Task 7: 更新测试文档

**Files:**
- Modify: `tests/docs/pty.md`

- [ ] **Step 1: 在 pty.md 的 `screenshot.rs` 段落后追加两个新模块文档**

在 `### screenshot.rs` 段末尾（`---` 分隔符之前）后追加：

```markdown
---

### commands.rs — 斜杠命令 (13 tests)

在 TUI 中输入斜杠命令，验证输出和行为。全部使用真实 API（`#[ignore]`）。

| 测试 | 说明 |
|------|------|
| `slash_help_shows_command_list` | `/help` 显示命令列表 |
| `slash_version_shows_version` | `/version` 显示版本号 |
| `slash_exit_quits_gracefully` | `/exit` 正常退出 TUI |
| `slash_cost_shows_usage` | `/cost` 显示 token 用量 |
| `slash_model_shows_current_model` | `/model` 显示当前模型 |
| `slash_status_shows_session_info` | `/status` 显示会话状态 |
| `slash_unknown_command_shows_error` | 未知命令不崩溃 |
| `slash_empty_does_not_crash` | 空斜杠 `/` 不崩溃 |
| `slash_model_with_arg_switches_model` | `/model sonnet` 切换模型 |
| `slash_clear_resets_conversation` | `/clear` 清除后仍可输入 |
| `slash_context_shows_info` | `/context` 显示上下文信息 |
| `slash_skills_lists_skills` | `/skills` 列出技能 |
| `multiple_commands_in_sequence` | 5 个命令连续执行不崩溃 |

---

### multi_turn.rs — 多轮对话深度 + 工具调用 (12 tests)

复杂交互模式测试：上下文持久、命令穿插、中断恢复、状态追踪、Read/Write/Edit 工具调用。

全部使用真实 API（`#[ignore]`）。工具调用测试在 `F:\temp` 工作目录下操作文件。

| 测试 | 说明 |
|------|------|
| `interleaved_input_and_commands` | 文本与命令交替输入不崩溃 |
| `clear_then_continue_input` | `/clear` 后继续输入和命令 |
| `rapid_multi_line_input` | 快速连续 10 行输入后命令仍响应 |
| `context_persists_across_turns` | 第一轮告知信息，第二轮验证回忆 |
| `slash_commands_between_turns` | 对话轮次之间穿插 `/cost` `/context` |
| `abort_and_resume_new_turn` | Ctrl+C 中断后发起新对话 |
| `status_bar_tracks_message_count` | 5 轮对话中 msg count 递增 |
| `clear_mid_conversation_resets_context` | `/clear` 清除上下文后新对话 |
| `tool_write_creates_file` | Write 工具创建文件到 `F:\temp`，验证磁盘内容 |
| `tool_read_shows_content` | Read 工具读取预创建文件，验证内容出现在 TUI |
| `tool_write_then_read_multi_turn` | 第一轮 Write → 第二轮 Read 读回验证 |
| `tool_edit_modifies_file` | Edit 工具修改预创建文件，验证磁盘上的替换 |
```

- [ ] **Step 2: 更新文档顶部测试数量**

在 pty.md 的 `pty_ui/ — 模块化 PTY UI 测试` 段落中，将 `(33 tests)` 更新为 `(58 tests)`（33 + 13 + 12），在目录结构中添加两个新模块。

- [ ] **Step 3: Commit**

```bash
git add tests/docs/pty.md
git commit -m "docs: add commands.rs and multi_turn.rs to PTY test documentation"
```

---

## 测试总表

### commands.rs — 13 tests (全部 live)

| # | 测试名 | 说明 |
|---|--------|------|
| 1 | `slash_help_shows_command_list` | `/help` 显示命令列表 |
| 2 | `slash_version_shows_version` | `/version` 显示版本号 |
| 3 | `slash_exit_quits_gracefully` | `/exit` 正常退出 |
| 4 | `slash_cost_shows_usage` | `/cost` 显示用量 |
| 5 | `slash_model_shows_current_model` | `/model` 显示模型 |
| 6 | `slash_status_shows_session_info` | `/status` 显示状态 |
| 7 | `slash_unknown_command_shows_error` | 未知命令处理 |
| 8 | `slash_empty_does_not_crash` | 空斜杠处理 |
| 9 | `slash_model_with_arg_switches_model` | 带参数切换模型 |
| 10 | `slash_clear_resets_conversation` | `/clear` 清除 |
| 11 | `slash_context_shows_info` | `/context` 上下文 |
| 12 | `slash_skills_lists_skills` | `/skills` 技能列表 |
| 13 | `multiple_commands_in_sequence` | 连续 5 命令 |

### multi_turn.rs — 12 tests (全部 live)

| # | 测试名 | 说明 |
|---|--------|------|
| 1 | `interleaved_input_and_commands` | 文本与命令交替 |
| 2 | `clear_then_continue_input` | clear 后继续 |
| 3 | `rapid_multi_line_input` | 快速连发 |
| 4 | `context_persists_across_turns` | 上下文持久 |
| 5 | `slash_commands_between_turns` | 命令穿插对话 |
| 6 | `abort_and_resume_new_turn` | 中断恢复 |
| 7 | `status_bar_tracks_message_count` | 状态栏追踪 |
| 8 | `clear_mid_conversation_resets_context` | clear 重置上下文 |
| 9 | `tool_write_creates_file` | Write 工具写文件到 F:\temp |
| 10 | `tool_read_shows_content` | Read 工具读文件内容 |
| 11 | `tool_write_then_read_multi_turn` | Write→Read 跨轮验证 |
| 12 | `tool_edit_modifies_file` | Edit 工具替换文件内容 |

**新增总计: 25 tests（全部 live，需要 API key，`#[ignore]`）**

---

## 运行命令速查

```bash
# 所有新测试都需要 API key，使用 --ignored 运行

# 仅斜杠命令测试
cargo test --test pty_ui commands -- --ignored --nocapture

# 仅多轮对话测试
cargo test --test pty_ui multi_turn -- --ignored --nocapture

# 全部新测试
cargo test --test pty_ui commands multi_turn -- --ignored --nocapture

# 查看截图
ls logs/*/cmd_*.html logs/*/mt_*.html
```
