#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use crate::script::{TestCase, TestKey, TestRunner, TestStep};
use crate::tests::SCRIPTS_LOG_ROOT;
use std::time::Duration;

// ── KAIROS / Feature-Gated Commands ──────────────────────────────
//
// All tests are offline (no API key required). Feature-gated commands
// should never crash even when the feature gate is closed. Each test
// uses permission_mode "bypass".

/// T01: /sleep without feature gate FEATURE_PROACTIVE=1.
///
/// Should display a gate-notice or usage info, never panic.
#[test]
fn sleep_no_gate() {
    let case = TestCase::new("sleep_no_gate")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Snapshot("initial".into()))
        .step(TestStep::Command("sleep".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T02: /sleep with invalid argument (0, below MIN_SLEEP_SECS=1).
///
/// Should reject the value gracefully, never panic.
#[test]
fn sleep_invalid_arg() {
    let case = TestCase::new("sleep_invalid_arg")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Snapshot("initial".into()))
        .step(TestStep::Command("sleep 0".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T03: /sleep with valid argument (60 seconds).
///
/// Should accept the value or show gate-notice, never panic.
#[test]
fn sleep_valid_arg() {
    let case = TestCase::new("sleep_valid_arg")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Snapshot("initial".into()))
        .step(TestStep::Command("sleep 60".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T04: /assistant without feature gate FEATURE_KAIROS=1.
///
/// Should show gate-notice or status panel, never panic.
#[test]
fn assistant_no_gate() {
    let case = TestCase::new("assistant_no_gate")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Snapshot("initial".into()))
        .step(TestStep::Command("assistant".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T05: /assistant alias /kairos.
///
/// The alias should work identically to /assistant, never panic.
#[test]
fn assistant_alias() {
    let case = TestCase::new("assistant_alias")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Snapshot("initial".into()))
        .step(TestStep::Command("kairos".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T06: /daemon status (default subcommand).
///
/// Should show daemon status or gate-notice, never panic.
#[test]
fn daemon_status() {
    let case = TestCase::new("daemon_status")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Snapshot("initial".into()))
        .step(TestStep::Command("daemon".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T07: /daemon stop.
///
/// Should show stop-requested message or gate-notice, never panic.
#[test]
fn daemon_stop() {
    let case = TestCase::new("daemon_stop")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Snapshot("initial".into()))
        .step(TestStep::Command("daemon stop".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T08: /notify status (default subcommand).
///
/// Should show notification status or gate-notice, never panic.
#[test]
fn notify_status() {
    let case = TestCase::new("notify_status")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Snapshot("initial".into()))
        .step(TestStep::Command("notify".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T09: /notify test.
///
/// Should send a test notification or show gate-notice, never panic.
#[test]
fn notify_test() {
    let case = TestCase::new("notify_test")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Snapshot("initial".into()))
        .step(TestStep::Command("notify test".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T10: /notify on/off toggle.
///
/// Toggle notifications on then off. Both operations should never panic.
#[test]
fn notify_toggle() {
    let case = TestCase::new("notify_toggle")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Snapshot("initial".into()))
        .step(TestStep::Command("notify on".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Command("notify off".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T11: /remote command (no feature gate).
///
/// Should show remote gateway status or help, never panic.
#[test]
fn remote_command() {
    let case = TestCase::new("remote_command")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Snapshot("initial".into()))
        .step(TestStep::Command("remote".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T12: /channels without feature gate FEATURE_KAIROS_CHANNELS=1.
///
/// Should show gate-notice or channel list, never panic.
#[test]
fn channels_no_gate() {
    let case = TestCase::new("channels_no_gate")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Snapshot("initial".into()))
        .step(TestStep::Command("channels".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T13: /channels list.
///
/// Should list channels or show gate-notice, never panic.
#[test]
fn channels_list() {
    let case = TestCase::new("channels_list")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Snapshot("initial".into()))
        .step(TestStep::Command("channels list".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T14: /dream without feature gate FEATURE_KAIROS=1.
///
/// Should show gate-notice or dream status, never panic.
#[test]
fn dream_no_gate() {
    let case = TestCase::new("dream_no_gate")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Snapshot("initial".into()))
        .step(TestStep::Command("dream".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T15: /dream with --days flag.
///
/// Should accept --days 14 or show gate-notice, never panic.
#[test]
fn dream_with_days() {
    let case = TestCase::new("dream_with_days")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Snapshot("initial".into()))
        .step(TestStep::Command("dream --days 14".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T16: /dream help.
///
/// Should show dream help text or gate-notice, never panic.
#[test]
fn dream_help() {
    let case = TestCase::new("dream_help")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Snapshot("initial".into()))
        .step(TestStep::Command("dream help".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T17: /dream alias /logs.
///
/// The /logs alias should work identically to /dream, never panic.
#[test]
fn dream_alias_logs() {
    let case = TestCase::new("dream_alias_logs")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Snapshot("initial".into()))
        .step(TestStep::Command("logs".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T18: All KAIROS commands in batch — verify none crash.
///
/// Executes /sleep, /assistant, /daemon, /notify, /remote, /channels,
/// and /dream sequentially with AssertNoPanic after each.
/// Even without feature gates, all commands must degrade gracefully.
#[test]
fn kairos_batch_no_crash() {
    let case = TestCase::new("kairos_batch_no_crash")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Snapshot("initial".into()))
        // /sleep
        .step(TestStep::Command("sleep".into()))
        .step(TestStep::AssertNoPanic)
        // /assistant
        .step(TestStep::Command("assistant".into()))
        .step(TestStep::AssertNoPanic)
        // /daemon
        .step(TestStep::Command("daemon".into()))
        .step(TestStep::AssertNoPanic)
        // /notify
        .step(TestStep::Command("notify".into()))
        .step(TestStep::AssertNoPanic)
        // /remote
        .step(TestStep::Command("remote".into()))
        .step(TestStep::AssertNoPanic)
        // /channels
        .step(TestStep::Command("channels".into()))
        .step(TestStep::AssertNoPanic)
        // /dream
        .step(TestStep::Command("dream".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}
