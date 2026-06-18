use crate::protocol;
use allthecodes_gateway::{
    GatewayStore, RunEvent, RunEventKind, RunId, RunStatus, SessionKeyPolicy,
};
use anyhow::{Context, Result};
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) fn append_gateway_sdk_event(
    command: &protocol::DaemonCommand,
    event_type: &str,
    data: Value,
) -> Result<()> {
    if event_type == "stream_delta" {
        if let Some(text) = stream_delta_text(&data) {
            return append_gateway_output(command, &text);
        }
    }
    append_gateway_event(
        command,
        RunEventKind::Custom {
            name: event_type.to_string(),
            payload: data,
        },
    )
}

pub(super) fn append_gateway_event(
    command: &protocol::DaemonCommand,
    kind: RunEventKind,
) -> Result<()> {
    let Some(run_id) = gateway_run_id(command)? else {
        return Ok(());
    };
    let store = GatewayStore::default_with_policy(SessionKeyPolicy::default());
    let sequence = store
        .read_events(&run_id)
        .map(|events| events.len() as u64 + 1)
        .unwrap_or(1);
    store
        .append_event(&RunEvent::new(run_id, sequence, kind))
        .context("append gateway run event")
}

pub(super) fn append_gateway_output(command: &protocol::DaemonCommand, chunk: &str) -> Result<()> {
    let Some(run_id) = gateway_run_id(command)? else {
        return Ok(());
    };
    GatewayStore::default_with_policy(SessionKeyPolicy::default())
        .append_output_chunk(&run_id, chunk, now_millis())
        .context("append gateway run output")
}

pub(super) fn update_gateway_status(
    command: &protocol::DaemonCommand,
    status: RunStatus,
) -> Result<()> {
    let Some(run_id) = gateway_run_id(command)? else {
        return Ok(());
    };
    GatewayStore::default_with_policy(SessionKeyPolicy::default())
        .update_status(&run_id, status)
        .context("update gateway run status")?;
    Ok(())
}

fn stream_delta_text(data: &Value) -> Option<String> {
    let delta = data.get("event")?.get("delta")?;
    for key in ["text", "thinking", "partial_json"] {
        if let Some(value) = delta.get(key).and_then(Value::as_str) {
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

fn gateway_run_id(command: &protocol::DaemonCommand) -> Result<Option<RunId>> {
    let Some(run_id) = command
        .payload
        .get("gateway")
        .and_then(|gateway| gateway.get("runId"))
        .and_then(Value::as_str)
    else {
        return Ok(None);
    };
    RunId::from_string(run_id)
        .map(Some)
        .map_err(|error| anyhow::anyhow!(error))
}
