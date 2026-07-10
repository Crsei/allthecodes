#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

//! PTY TUI E2E tests for `/sandbox` and `/permissions` commands.
//!
//! Covers all offline subcommands and modes for both sandbox and permission
//! management (plan document T01-T14).
//!
//! All tests use `permission_mode("bypass")` so that mode changes are effective
//! without requiring a dismissable confirmation dialog.  No API calls are needed
//! because every test only runs local slash commands that modify in-memory state
//! and return text output.

use crate::script::{TestCase, TestRunner, TestStep};
use crate::tests::SCRIPTS_LOG_ROOT;
use std::time::Duration;

// ═══════════════════════════════════════════════════════════════════════════════
// T01 – T09:  /sandbox
// ═══════════════════════════════════════════════════════════════════════════════

/// T01: `/sandbox` (no arguments) displays the current sandbox status.
#[test]
fn sandbox_shows_status() {
    let case = TestCase::new("sandbox_shows_status")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Snapshot("initial".into()))
        .step(TestStep::Command("sandbox".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Snapshot("sandbox_status".into()))
        .step(TestStep::AssertNoPanic);

    TestRunner::new().run(&case).assert_no_errors();
}

/// T02: `/sandbox status` subcommand shows sandbox status info.
#[test]
fn sandbox_status_subcommand() {
    let case = TestCase::new("sandbox_status_subcommand")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Snapshot("initial".into()))
        .step(TestStep::Command("sandbox status".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Snapshot("status_sub".into()))
        .step(TestStep::AssertNoPanic);

    TestRunner::new().run(&case).assert_no_errors();
}

/// T03: `/sandbox on` and `/sandbox off` toggle — both succeed without panic.
#[test]
fn sandbox_toggle() {
    let case = TestCase::new("sandbox_toggle")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Snapshot("initial".into()))
        .step(TestStep::Command("sandbox on".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Command("sandbox off".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Snapshot("after_off".into()))
        .step(TestStep::AssertNoPanic);

    TestRunner::new().run(&case).assert_no_errors();
}

/// T04: `/sandbox mode workspace` sets the sandbox mode to workspace.
#[test]
fn sandbox_mode_workspace() {
    let case = TestCase::new("sandbox_mode_workspace")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Snapshot("initial".into()))
        .step(TestStep::Command("sandbox mode workspace".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Snapshot("mode_workspace".into()))
        .step(TestStep::AssertNoPanic);

    TestRunner::new().run(&case).assert_no_errors();
}

/// T05: `/sandbox mode read-only` sets the sandbox mode to read-only.
#[test]
fn sandbox_mode_readonly() {
    let case = TestCase::new("sandbox_mode_readonly")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Snapshot("initial".into()))
        .step(TestStep::Command("sandbox mode read-only".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Snapshot("mode_readonly".into()))
        .step(TestStep::AssertNoPanic);

    TestRunner::new().run(&case).assert_no_errors();
}

/// T06: `/sandbox mode full` disables the OS-level sandbox.
#[test]
fn sandbox_mode_full() {
    let case = TestCase::new("sandbox_mode_full")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Snapshot("initial".into()))
        .step(TestStep::Command("sandbox mode full".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Snapshot("mode_full".into()))
        .step(TestStep::AssertNoPanic);

    TestRunner::new().run(&case).assert_no_errors();
}

/// T07: `/sandbox require` enables fail-closed mode (require OS-level primitive).
#[test]
fn sandbox_require() {
    let case = TestCase::new("sandbox_require")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Snapshot("initial".into()))
        .step(TestStep::Command("sandbox require".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Snapshot("require".into()))
        .step(TestStep::AssertNoPanic);

    TestRunner::new().run(&case).assert_no_errors();
}

/// T08: `/sandbox network off` and `/sandbox network on` toggle network policy.
#[test]
fn sandbox_network_toggle() {
    let case = TestCase::new("sandbox_network_toggle")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Snapshot("initial".into()))
        .step(TestStep::Command("sandbox network off".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Command("sandbox network on".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Snapshot("network_on".into()))
        .step(TestStep::AssertNoPanic);

    TestRunner::new().run(&case).assert_no_errors();
}

/// T09: `/sandbox no-network` disables all network access.
#[test]
fn sandbox_no_network() {
    let case = TestCase::new("sandbox_no_network")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Snapshot("initial".into()))
        .step(TestStep::Command("sandbox no-network".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Snapshot("no_network".into()))
        .step(TestStep::AssertNoPanic);

    TestRunner::new().run(&case).assert_no_errors();
}

// ═══════════════════════════════════════════════════════════════════════════════
// T10:  /permissions mode <mode>
// ═══════════════════════════════════════════════════════════════════════════════

/// T10a: `/permissions mode default` sets mode to default.
#[test]
fn permissions_mode_default() {
    let case = TestCase::new("permissions_mode_default")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Snapshot("initial".into()))
        .step(TestStep::Command("permissions mode default".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::AssertScreenContains("default".into()))
        .step(TestStep::AssertNoPanic);

    TestRunner::new().run(&case).assert_no_errors();
}

/// T10b: `/permissions mode auto` shows classifier hint (needs --confirm).
#[test]
fn permissions_mode_auto() {
    let case = TestCase::new("permissions_mode_auto")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Snapshot("initial".into()))
        .step(TestStep::Command("permissions mode auto".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::AssertScreenContains("Auto".into()))
        .step(TestStep::AssertNoPanic);

    TestRunner::new().run(&case).assert_no_errors();
}

/// T10c: `/permissions mode plan` enters plan (read-only) mode.
#[test]
fn permissions_mode_plan() {
    let case = TestCase::new("permissions_mode_plan")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Snapshot("initial".into()))
        .step(TestStep::Command("permissions mode plan".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::AssertScreenContains("plan".into()))
        .step(TestStep::AssertNoPanic);

    TestRunner::new().run(&case).assert_no_errors();
}

/// T10d: `/permissions mode acceptEdits` sets mode to acceptEdits.
#[test]
fn permissions_mode_accept_edits() {
    let case = TestCase::new("permissions_mode_acceptEdits")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Snapshot("initial".into()))
        .step(TestStep::Command("permissions mode acceptEdits".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::AssertNoPanic);

    TestRunner::new().run(&case).assert_no_errors();
}

/// T10e: `/permissions mode dontAsk` sets mode to dontAsk.
#[test]
fn permissions_mode_dont_ask() {
    let case = TestCase::new("permissions_mode_dontAsk")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Snapshot("initial".into()))
        .step(TestStep::Command("permissions mode dontAsk".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::AssertNoPanic);

    TestRunner::new().run(&case).assert_no_errors();
}

// ═══════════════════════════════════════════════════════════════════════════════
// T11:  /permissions allow / ask / deny <rule> --session
// ═══════════════════════════════════════════════════════════════════════════════

/// T11a: `/permissions allow Bash --session` adds a session-level allow rule.
#[test]
fn permissions_allow_rule() {
    let case = TestCase::new("permissions_allow_rule")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Snapshot("initial".into()))
        .step(TestStep::Command("permissions allow Bash --session".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Snapshot("allow_rule".into()))
        .step(TestStep::AssertNoPanic);

    TestRunner::new().run(&case).assert_no_errors();
}

/// T11b: `/permissions ask Edit --session` — session scope rejects ask rules.
#[test]
fn permissions_ask_rule() {
    let case = TestCase::new("permissions_ask_rule")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Snapshot("initial".into()))
        .step(TestStep::Command("permissions ask Edit --session".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Snapshot("ask_rule".into()))
        .step(TestStep::AssertNoPanic);

    TestRunner::new().run(&case).assert_no_errors();
}

/// T11c: `/permissions deny Write --session` — session scope rejects deny rules.
#[test]
fn permissions_deny_rule() {
    let case = TestCase::new("permissions_deny_rule")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Snapshot("initial".into()))
        .step(TestStep::Command("permissions deny Write --session".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Snapshot("deny_rule".into()))
        .step(TestStep::AssertNoPanic);

    TestRunner::new().run(&case).assert_no_errors();
}

// ═══════════════════════════════════════════════════════════════════════════════
// T12:  /permissions session-grant + clear-session-grants
// ═══════════════════════════════════════════════════════════════════════════════

/// T12: `/permissions session-grant Bash` then `/permissions clear-session-grants`.
#[test]
fn permissions_session_grant() {
    let case = TestCase::new("permissions_session_grant")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Snapshot("initial".into()))
        .step(TestStep::Command("permissions session-grant Bash".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Command("permissions clear-session-grants".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Snapshot("grants_cleared".into()))
        .step(TestStep::AssertNoPanic);

    TestRunner::new().run(&case).assert_no_errors();
}

// ═══════════════════════════════════════════════════════════════════════════════
// T13:  /permissions reset
// ═══════════════════════════════════════════════════════════════════════════════

/// T13: `/permissions reset` resets in-memory permission rules.
#[test]
fn permissions_reset() {
    let case = TestCase::new("permissions_reset")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Snapshot("initial".into()))
        .step(TestStep::Command("permissions reset".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Snapshot("reset".into()))
        .step(TestStep::AssertNoPanic);

    TestRunner::new().run(&case).assert_no_errors();
}

// ═══════════════════════════════════════════════════════════════════════════════
// T14:  /permissions alias /perms
// ═══════════════════════════════════════════════════════════════════════════════

/// T14: `/perms` is an alias for `/permissions`.
#[test]
fn permissions_alias_perms() {
    let case = TestCase::new("permissions_alias_perms")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Snapshot("initial".into()))
        .step(TestStep::Command("perms".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Snapshot("perms_alias".into()))
        .step(TestStep::AssertNoPanic);

    TestRunner::new().run(&case).assert_no_errors();
}
