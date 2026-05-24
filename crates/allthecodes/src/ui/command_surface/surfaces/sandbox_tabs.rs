use crate::ui::form_navigation::{FormOption, FormTab};
use allthecodes_engine::types::app_state::AppState;
use allthecodes_sandbox::availability::{detect_availability, Availability};

pub(super) fn expand_status_tabs(
    tabs: &mut Vec<FormTab>,
    state: &AppState,
    configured_enabled: &str,
) {
    let details = details(state);
    if !tabs.is_empty() {
        tabs.remove(0);
    }
    let expanded = vec![
        config_tab(&details, configured_enabled),
        dependencies_tab(&details),
        overrides_tab(&details),
        doctor_tab(&details),
    ];
    tabs.splice(0..0, expanded);
}

fn config_tab(details: &SandboxDetails, configured_enabled: &str) -> FormTab {
    FormTab::new(
        "config",
        "Config",
        vec![
            FormOption::new("status", "Show full status").with_description(format!(
                "enabled={}; effective={}; mode={}",
                configured_enabled, details.enabled, details.mode
            )),
            FormOption::new("network-status", "Network policy")
                .with_description(format!(
                    "network={}; allowedDomains={}",
                    details.network, details.allowed_domains
                ))
                .disabled(),
            FormOption::new("config-source", "Config source")
                .with_description(format!("sandbox source={}", details.source))
                .disabled(),
        ],
    )
}

fn dependencies_tab(details: &SandboxDetails) -> FormTab {
    FormTab::new(
        "dependencies",
        "Dependencies",
        vec![
            FormOption::new("dependency-status", "OS-level sandbox")
                .with_description(details.dependency_status.clone())
                .disabled(),
            FormOption::new("fallback-status", "Fallback behavior")
                .with_description(details.fallback_description.clone())
                .disabled(),
            FormOption::new("require", "Require OS primitive")
                .with_description("session override: failIfUnavailable=true"),
            FormOption::new("optional", "Allow best-effort fallback")
                .with_description("session override: failIfUnavailable=false"),
            FormOption::new("proxy-runtime", "Network proxy runtime")
                .with_description(details.network_proxy_status.clone())
                .disabled(),
            FormOption::new("dependency-detail", "Dependency detail")
                .with_description(details.dependency_description.clone())
                .disabled(),
        ],
    )
}

fn overrides_tab(details: &SandboxDetails) -> FormTab {
    FormTab::new(
        "overrides",
        "Overrides",
        vec![
            FormOption::new("source", "Config source")
                .with_description(format!("sandbox source={}", details.source))
                .disabled(),
            FormOption::new("escape", "Unsandboxed escape")
                .with_description(format!("allowUnsandboxedCommands={}", details.escape))
                .disabled(),
            FormOption::new("fail-if-unavailable", "Fail if unavailable")
                .with_description(format!("failIfUnavailable={}", details.fail_if_unavailable))
                .disabled(),
            FormOption::new("managed-read", "Managed read paths")
                .with_description(format!(
                    "allowManagedReadPathsOnly={}",
                    details.managed_read_paths
                ))
                .disabled(),
            FormOption::new("managed-domains", "Managed domains")
                .with_description(format!(
                    "allowManagedDomainsOnly={}",
                    details.managed_domains
                ))
                .disabled(),
            FormOption::new("command-rules", "Command rules")
                .with_description(details.command_rules.clone())
                .disabled(),
            FormOption::new("filesystem-rules", "Filesystem rules")
                .with_description(details.filesystem_rules.clone())
                .disabled(),
        ],
    )
}

fn doctor_tab(details: &SandboxDetails) -> FormTab {
    FormTab::new(
        "doctor",
        "Doctor",
        vec![
            FormOption::new("doctor-status", "Current diagnosis")
                .with_description(details.doctor.clone())
                .disabled(),
            FormOption::new("status-from-doctor", "Open detailed doctor-style status")
                .with_description("runs /sandbox status"),
        ],
    )
}

struct SandboxDetails {
    source: String,
    enabled: &'static str,
    mode: String,
    network: &'static str,
    allowed_domains: String,
    dependency_status: String,
    dependency_description: String,
    network_proxy_status: String,
    fallback_description: String,
    doctor: String,
    fail_if_unavailable: &'static str,
    escape: &'static str,
    managed_read_paths: &'static str,
    managed_domains: &'static str,
    command_rules: String,
    filesystem_rules: String,
}

fn details(state: &AppState) -> SandboxDetails {
    let sandbox = &state.settings.sandbox;
    let availability = detect_availability();
    let dependency_available = availability.is_available();

    SandboxDetails {
        source: state
            .settings
            .sources
            .get("sandbox")
            .map(|source| source.as_str().to_string())
            .unwrap_or_else(|| "default (no sandbox config source recorded)".to_string()),
        enabled: setting_bool(sandbox.enabled, "unset -> default false"),
        mode: sandbox
            .mode
            .clone()
            .unwrap_or_else(|| "unset -> workspace when enabled, full when disabled".to_string()),
        network: sandbox
            .network
            .disabled
            .map(|disabled| if disabled { "disabled" } else { "enabled" })
            .unwrap_or("unset -> default enabled"),
        allowed_domains: list_status(&sandbox.network.allowed_domains, "all domains allowed"),
        dependency_status: dependency_status(&availability),
        dependency_description: availability.describe(),
        network_proxy_status: network_proxy_status(
            sandbox.network.http_proxy_port,
            sandbox.network.socks_proxy_port,
        ),
        fallback_description: fallback_description(
            sandbox.fail_if_unavailable,
            dependency_available,
        ),
        doctor: doctor_summary(state, dependency_available),
        fail_if_unavailable: setting_bool(sandbox.fail_if_unavailable, "unset -> default false"),
        escape: setting_bool(sandbox.allow_unsandboxed_commands, "unset -> default true"),
        managed_read_paths: setting_bool(
            sandbox.allow_managed_read_paths_only,
            "unset -> merged read paths allowed",
        ),
        managed_domains: setting_bool(
            sandbox.allow_managed_domains_only,
            "unset -> merged domains allowed",
        ),
        command_rules: format!(
            "allowedCommands={}, excludedCommands={}",
            sandbox.allowed_commands.len(),
            sandbox.excluded_commands.len()
        ),
        filesystem_rules: format!(
            "allowRead={}, denyRead={}, allowWrite={}, denyWrite={}",
            sandbox.filesystem.allow_read.len(),
            sandbox.filesystem.deny_read.len(),
            sandbox.filesystem.allow_write.len(),
            sandbox.filesystem.deny_write.len()
        ),
    }
}

fn network_proxy_status(http_proxy_port: Option<u16>, socks_proxy_port: Option<u16>) -> String {
    match (http_proxy_port, socks_proxy_port) {
        (None, None) => "not configured; direct network policy checks only".to_string(),
        (http, socks) => format!(
            "UNSUPPORTED proxy runtime: httpProxyPort={}, socksProxyPort={}",
            optional_port(http),
            optional_port(socks)
        ),
    }
}

fn optional_port(port: Option<u16>) -> String {
    port.map(|value| value.to_string())
        .unwrap_or_else(|| "unset".to_string())
}

fn dependency_status(availability: &Availability) -> String {
    match availability {
        Availability::Available(mechanism) => {
            format!("PASS available via {}", mechanism.as_str())
        }
        Availability::Unavailable { platform, reason }
            if reason.contains("not implemented") || reason.contains("no sandbox back-end") =>
        {
            format!("WARNING unsupported on {platform}; OS primitive unavailable")
        }
        Availability::Unavailable { platform, .. } => {
            format!("WARNING unavailable on {platform}; OS primitive unavailable")
        }
    }
}

fn fallback_description(fail_if_unavailable: Option<bool>, dependency_available: bool) -> String {
    if dependency_available {
        "PASS OS-level dependency available; no fallback needed".to_string()
    } else if fail_if_unavailable.unwrap_or(false) {
        "WARNING unavailable dependency fails closed; sandboxed commands will fail".to_string()
    } else {
        "WARNING unavailable dependency falls back to Rust-level checks".to_string()
    }
}

fn setting_bool(value: Option<bool>, default: &'static str) -> &'static str {
    match value {
        Some(true) => "true",
        Some(false) => "false",
        None => default,
    }
}

fn list_status(values: &[String], empty: &str) -> String {
    if values.is_empty() {
        empty.to_string()
    } else {
        format!("{} configured", values.len())
    }
}

fn doctor_summary(state: &AppState, dependency_available: bool) -> String {
    let sandbox = &state.settings.sandbox;
    let mut findings = Vec::new();
    if sandbox.enabled.unwrap_or(false) && !dependency_available {
        findings.push("WARNING dependency unavailable; run /sandbox status for full reason");
    }
    if sandbox.network.disabled.unwrap_or(false) && sandbox.network.allowed_domains.is_empty() {
        findings.push("WARNING network disabled; allowedDomains unavailable");
    }
    if sandbox.mode.as_deref() == Some("full") && sandbox.enabled.unwrap_or(false) {
        findings.push("WARNING mode=full disables OS sandbox despite enabled=true");
    }

    if findings.is_empty() {
        "PASS status data available; no sandbox doctor warnings".to_string()
    } else {
        findings.join("; ")
    }
}
