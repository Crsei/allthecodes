#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

//! PTY TUI E2E tests: session and context commands.
//!
//! Implements test cases T01–T17 from:
//! docs/plan/command-e2e-test-plan-02-session-context.md
//!
//! Offline tests: T01–T13 (no API key required).
//! Online tests:  T14–T17 (require real API key, marked `#[ignore]`).

use crate::harness::API_TIMEOUT;
use crate::script::{TestCase, TestKey, TestRunner, TestStep};
use crate::tests::SCRIPTS_LOG_ROOT;
use std::time::Duration;

// ═══════════════════════════════════════════════════════════════════════
// Offline Tests (T01–T13)
// ═══════════════════════════════════════════════════════════════════════

// ─── T01: /session ───────────────────────────────────────────────────

/// T01: `/session` displays current session info (ID, title, messages, model).
#[test]
fn session_command_shows_info() {
    let case = TestCase::new("session_command_shows_info")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Command("session".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::AssertScreenContains("session".into()))
        .step(TestStep::Snapshot("session_info".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

// ─── T02: /session list ──────────────────────────────────────────────

/// T02: `/session list` lists workspace sessions without panic.
#[test]
fn session_list_command() {
    let case = TestCase::new("session_list_command")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::Command("session list".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

// ─── T03: /session list all ──────────────────────────────────────────

/// T03: `/session list all` lists all sessions across workspaces without panic.
#[test]
fn session_list_all() {
    let case = TestCase::new("session_list_all")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::Command("session list all".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

// ─── T04: /rename ────────────────────────────────────────────────────

/// T04: `/rename` sets a custom session title.
#[test]
fn rename_command_sets_title() {
    let case = TestCase::new("rename_command_sets_title")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::Command("rename My Test Session".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::AssertScreenContains("My Test Session".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

// ─── T05: /export ────────────────────────────────────────────────────

/// T05: `/export` exports the current session as Markdown to the default path.
#[test]
fn export_command_default() {
    let case = TestCase::new("export_command_default")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::Command("export".into()))
        .step(TestStep::Wait(Duration::from_secs(3)))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Snapshot("export_result".into()))
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

// ─── T06: /export list ───────────────────────────────────────────────

/// T06: `/export list` lists existing exports without panic.
#[test]
fn export_list() {
    let case = TestCase::new("export_list")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::Command("export list".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

// ─── T07: /audit-export ──────────────────────────────────────────────

/// T07: `/audit-export` exports an audit record without panic.
#[test]
fn audit_export_command() {
    let case = TestCase::new("audit_export_command")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::Command("audit-export".into()))
        .step(TestStep::Wait(Duration::from_secs(3)))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

// ─── T08: /audit (alias for /audit-export) ───────────────────────────

/// T08: `/audit` (alias for `/audit-export`) works without panic.
#[test]
fn audit_export_alias() {
    let case = TestCase::new("audit_export_alias")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::Command("audit".into()))
        .step(TestStep::Wait(Duration::from_secs(3)))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

// ─── T09: /session-export ────────────────────────────────────────────

/// T09: `/session-export` exports structured session JSON without panic.
#[test]
fn session_export_command() {
    let case = TestCase::new("session_export_command")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::Command("session-export".into()))
        .step(TestStep::Wait(Duration::from_secs(3)))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

// ─── T10: /sexport (alias for /session-export) ───────────────────────

/// T10: `/sexport` (alias for `/session-export`) works without panic.
#[test]
fn session_export_alias() {
    let case = TestCase::new("session_export_alias")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::Command("sexport".into()))
        .step(TestStep::Wait(Duration::from_secs(3)))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

// ─── T11: /add-dir (no args) ─────────────────────────────────────────

/// T11: `/add-dir` lists currently added working directories without panic.
#[test]
fn add_dir_lists_current() {
    let case = TestCase::new("add_dir_lists_current")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::Command("add-dir".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

// ─── T12: /add-dir <path> ────────────────────────────────────────────

/// T12: `/add-dir /tmp` adds a specific working directory.
#[test]
fn add_dir_with_path() {
    let case = TestCase::new("add_dir_with_path")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::Command("add-dir /tmp".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::AssertScreenContains("/tmp".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

// ─── T13: /init ──────────────────────────────────────────────────────

/// T13: `/init` initialises project configuration without panic.
#[test]
fn init_command() {
    let case = TestCase::new("init_command")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::Command("init".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

// ═══════════════════════════════════════════════════════════════════════
// Online Tests (T14–T17) — require real API key
// ═══════════════════════════════════════════════════════════════════════

// ─── T14: /resume ────────────────────────────────────────────────────

/// T14: `/resume` with no previous sessions shows a friendly message (online).
#[test]
#[ignore = "requires real API key"]
fn resume_no_sessions() {
    let case = TestCase::new("resume_no_sessions")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Command("resume".into()))
        .step(TestStep::Wait(Duration::from_secs(3)))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

// ─── T15: /branch ────────────────────────────────────────────────────

/// T15: `/branch` forks the current conversation into a new session (online).
#[test]
#[ignore = "requires real API key"]
fn branch_command_forks() {
    let case = TestCase::new("branch_command_forks")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Input("Remember the number 7749".into()))
        .step(TestStep::WaitForText("7749".into(), API_TIMEOUT))
        .step(TestStep::Command("branch test-fork".into()))
        .step(TestStep::Wait(Duration::from_secs(3)))
        .step(TestStep::Snapshot("after_branch".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

// ─── T16: /rewind ────────────────────────────────────────────────────

/// T16: `/rewind` rewinds the conversation to an earlier turn (online).
#[test]
#[ignore = "requires real API key"]
fn rewind_command() {
    let case = TestCase::new("rewind_command")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Input("First message".into()))
        .step(TestStep::WaitResponseDone(API_TIMEOUT))
        .step(TestStep::Input("Second message".into()))
        .step(TestStep::WaitResponseDone(API_TIMEOUT))
        .step(TestStep::Command("rewind".into()))
        .step(TestStep::Wait(Duration::from_secs(3)))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

// ─── T17: /insights ──────────────────────────────────────────────────

/// T17: `/insights` displays session insights and statistics (online).
#[test]
#[ignore = "requires real API key"]
fn insights_command() {
    let case = TestCase::new("insights_command")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Input("Hello".into()))
        .step(TestStep::WaitResponseDone(API_TIMEOUT))
        .step(TestStep::Command("insights".into()))
        .step(TestStep::Wait(Duration::from_secs(3)))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}
