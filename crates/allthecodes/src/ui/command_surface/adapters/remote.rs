use allthecodes_gateway::GatewayConfig;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RemoteSurfaceSnapshot {
    pub(crate) daemon: RemoteDaemonRow,
    pub(crate) adapters: Vec<RemoteAdapterRow>,
    pub(crate) runs: Vec<RemoteRunRow>,
    pub(crate) security: Vec<RemoteSecurityRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RemoteDaemonRow {
    pub(crate) state: String,
    pub(crate) detail: String,
    pub(crate) action: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RemoteAdapterRow {
    pub(crate) provider: String,
    pub(crate) state: String,
    pub(crate) action: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RemoteRunRow {
    pub(crate) run_id: String,
    pub(crate) status: String,
    pub(crate) source: String,
    pub(crate) updated_at_ms: u128,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RemoteSecurityRow {
    pub(crate) label: String,
    pub(crate) value: String,
}

pub(crate) fn remote_surface_initial_snapshot() -> RemoteSurfaceSnapshot {
    RemoteSurfaceSnapshot {
        daemon: RemoteDaemonRow {
            state: "unknown".to_string(),
            detail: "use /remote status to refresh daemon state".to_string(),
            action: "/remote status".to_string(),
        },
        adapters: adapter_rows(),
        runs: Vec::new(),
        security: security_rows(),
    }
}

fn adapter_rows() -> Vec<RemoteAdapterRow> {
    ["telegram", "lark"]
        .into_iter()
        .map(|provider| RemoteAdapterRow {
            provider: provider.to_string(),
            state: "status via /remote adapters".to_string(),
            action: format!("/remote connect {provider}"),
        })
        .collect()
}

fn security_rows() -> Vec<RemoteSecurityRow> {
    let config = GatewayConfig::default();
    vec![
        RemoteSecurityRow {
            label: "auth".to_string(),
            value: "local daemon token required; raw value hidden".to_string(),
        },
        RemoteSecurityRow {
            label: "gateway dir".to_string(),
            value: config.persistence.gateway_dir.display().to_string(),
        },
        RemoteSecurityRow {
            label: "runs dir".to_string(),
            value: config.persistence.runs_dir.display().to_string(),
        },
        RemoteSecurityRow {
            label: "adapter dir".to_string(),
            value: config.persistence.adapters_dir.display().to_string(),
        },
    ]
}

pub(crate) fn truncate_middle(value: &str, max_len: usize) -> String {
    if value.len() <= max_len {
        return value.to_string();
    }
    if max_len <= 3 {
        return "...".to_string();
    }
    let head = (max_len - 3) / 2;
    let tail = max_len - 3 - head;
    format!("{}...{}", &value[..head], &value[value.len() - tail..])
}
