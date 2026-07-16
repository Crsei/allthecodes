#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use crate::script::{TestCase, TestKey, TestRunner, TestStep};
use crate::tests::SCRIPTS_LOG_ROOT;
use std::time::Duration;

/// T01: `/mcp` no args displays help / config example.
///
/// All tests in this file are offline (no API key needed).
#[test]
fn mcp_help_no_args() {
    let case = TestCase::new("mcp_help_no_args")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Command("mcp".into()))
        .step(TestStep::WaitForScreenText(
            "list".into(),
            Duration::from_secs(3),
        ))
        .step(TestStep::Snapshot("mcp_help".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T02: `/mcp list` lists discovered MCP servers or shows "none".
#[test]
fn mcp_list() {
    let case = TestCase::new("mcp_list")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Command("mcp list".into()))
        .step(TestStep::Snapshot("mcp_list".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T03: `/mcp ls` (alias for list) must be available without panic.
#[test]
fn mcp_ls_alias() {
    let case = TestCase::new("mcp_ls_alias")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Command("mcp ls".into()))
        .step(TestStep::Snapshot("mcp_ls_alias".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T04: `/mcp status` shows live connection status.
#[test]
fn mcp_status() {
    let case = TestCase::new("mcp_status")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Command("mcp status".into()))
        .step(TestStep::Snapshot("mcp_status".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T05: `/mcp add` with streamable-http transport.
#[test]
fn mcp_add_streamable_http() {
    let case = TestCase::new("mcp_add_streamable_http")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Command(
            "mcp add test-server --transport=streamable-http --url=https://example.com/mcp".into(),
        ))
        .step(TestStep::Snapshot("mcp_add_streamable_http".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T06: `/mcp remove` removes a server or shows "not found".
#[test]
fn mcp_remove() {
    let case = TestCase::new("mcp_remove")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Command("mcp remove test-server".into()))
        .step(TestStep::Snapshot("mcp_remove".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T07: `/mcp connect` and `/mcp disconnect` server management.
#[test]
fn mcp_connect_disconnect() {
    let case = TestCase::new("mcp_connect_disconnect")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Command("mcp connect test-server".into()))
        .step(TestStep::Snapshot("mcp_connect".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Command("mcp disconnect test-server".into()))
        .step(TestStep::Snapshot("mcp_disconnect".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T08: `/mcp auth status` shows OAuth credentials status.
#[test]
fn mcp_auth_status() {
    let case = TestCase::new("mcp_auth_status")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Command("mcp auth status test-server".into()))
        .step(TestStep::Snapshot("mcp_auth_status".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T09: `/mcp auth clear` clears stored credentials or shows "not found".
#[test]
fn mcp_auth_clear() {
    let case = TestCase::new("mcp_auth_clear")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Command("mcp auth clear test-server".into()))
        .step(TestStep::Snapshot("mcp_auth_clear".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T10: `/plugin` no args displays plugin help.
#[test]
fn plugin_help() {
    let case = TestCase::new("plugin_help")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Command("plugin".into()))
        .step(TestStep::Snapshot("plugin_help".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T11: `/plugin list` lists plugins or "none".
#[test]
fn plugin_list() {
    let case = TestCase::new("plugin_list")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Command("plugin list".into()))
        .step(TestStep::Snapshot("plugin_list".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T12: `/plugin status` shows plugin status summary.
#[test]
fn plugin_status() {
    let case = TestCase::new("plugin_status")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Command("plugin status".into()))
        .step(TestStep::Snapshot("plugin_status".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T13a: `/plugin enable` missing ID shows error message.
#[test]
fn plugin_enable_missing_id() {
    let case = TestCase::new("plugin_enable_missing_id")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Command("plugin enable".into()))
        .step(TestStep::WaitForScreenText(
            "Usage".into(),
            Duration::from_secs(3),
        ))
        .step(TestStep::Snapshot("plugin_enable_missing_id".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T13b: `/plugin disable` missing ID shows error message.
#[test]
fn plugin_disable_missing_id() {
    let case = TestCase::new("plugin_disable_missing_id")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Command("plugin disable".into()))
        .step(TestStep::WaitForScreenText(
            "Usage".into(),
            Duration::from_secs(3),
        ))
        .step(TestStep::Snapshot("plugin_disable_missing_id".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T14: `/reload-plugins` hot-reloads the plugin registry.
#[test]
fn reload_plugins() {
    let case = TestCase::new("reload_plugins")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Command("reload-plugins".into()))
        .step(TestStep::Snapshot("reload_plugins".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T15: `/ide` shows IDE detection status.
#[test]
fn ide_command() {
    let case = TestCase::new("ide_command")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Command("ide".into()))
        .step(TestStep::Snapshot("ide_command".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T16: `/lsp` shows LSP server status.
#[test]
fn lsp_command() {
    let case = TestCase::new("lsp_command")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Command("lsp".into()))
        .step(TestStep::Snapshot("lsp_command".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T17: `/chrome` shows Chrome integration status.
#[test]
fn chrome_command() {
    let case = TestCase::new("chrome_command")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Command("chrome".into()))
        .step(TestStep::Snapshot("chrome_command".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T18: Batch all MCP/plugin/IDE commands sequentially to verify no crash.
///
/// Executes: /mcp list, /mcp status, /plugin list, /plugin status,
/// /reload-plugins, /ide, /lsp, /chrome — each followed by AssertNoPanic.
#[test]
fn mcp_plugin_batch_no_crash() {
    let case = TestCase::new("mcp_plugin_batch_no_crash")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        // /mcp list
        .step(TestStep::Command("mcp list".into()))
        .step(TestStep::AssertNoPanic)
        // /mcp status
        .step(TestStep::Command("mcp status".into()))
        .step(TestStep::AssertNoPanic)
        // /plugin list
        .step(TestStep::Command("plugin list".into()))
        .step(TestStep::AssertNoPanic)
        // /plugin status
        .step(TestStep::Command("plugin status".into()))
        .step(TestStep::AssertNoPanic)
        // /reload-plugins
        .step(TestStep::Command("reload-plugins".into()))
        .step(TestStep::AssertNoPanic)
        // /ide
        .step(TestStep::Command("ide".into()))
        .step(TestStep::AssertNoPanic)
        // /lsp
        .step(TestStep::Command("lsp".into()))
        .step(TestStep::AssertNoPanic)
        // /chrome
        .step(TestStep::Command("chrome".into()))
        .step(TestStep::AssertNoPanic)
        // final snapshot
        .step(TestStep::Snapshot("mcp_plugin_batch".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}
