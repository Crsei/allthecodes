use super::*;

pub const DEFAULT_TASK_OUTPUT_TIMEOUT_MS: u64 = 30_000;
pub const MAX_TASK_OUTPUT_TIMEOUT_MS: u64 = 600_000;
pub const MAX_TASK_OUTPUT_LIMIT_BYTES: usize = 1024 * 1024;

pub fn parse_task_output_timeout_ms(input: &Value) -> Result<u64> {
    let Some(timeout) = input.get("timeout") else {
        return Ok(DEFAULT_TASK_OUTPUT_TIMEOUT_MS);
    };
    let value = if let Some(value) = timeout.as_u64() {
        value
    } else if let Some(value) = timeout.as_f64() {
        if !value.is_finite() || value < 0.0 || value.fract() != 0.0 {
            anyhow::bail!("timeout must be an integer between 0 and {MAX_TASK_OUTPUT_TIMEOUT_MS}");
        }
        value as u64
    } else {
        anyhow::bail!("timeout must be an integer between 0 and {MAX_TASK_OUTPUT_TIMEOUT_MS}");
    };

    if value > MAX_TASK_OUTPUT_TIMEOUT_MS {
        anyhow::bail!("timeout must be between 0 and {MAX_TASK_OUTPUT_TIMEOUT_MS} milliseconds");
    }

    Ok(value)
}

pub fn parse_task_output_limit_bytes(input: &Value) -> Result<usize> {
    let Some(limit) = input.get("limit_bytes").or_else(|| input.get("limitBytes")) else {
        return Ok(MAX_TASK_OUTPUT_LIMIT_BYTES);
    };
    let value = if let Some(value) = limit.as_u64() {
        value
    } else if let Some(value) = limit.as_f64() {
        if !value.is_finite() || value <= 0.0 || value.fract() != 0.0 {
            anyhow::bail!(
                "limit_bytes must be an integer between 1 and {MAX_TASK_OUTPUT_LIMIT_BYTES}"
            );
        }
        value as u64
    } else {
        anyhow::bail!("limit_bytes must be an integer between 1 and {MAX_TASK_OUTPUT_LIMIT_BYTES}");
    };

    if value == 0 || value > MAX_TASK_OUTPUT_LIMIT_BYTES as u64 {
        anyhow::bail!("limit_bytes must be between 1 and {MAX_TASK_OUTPUT_LIMIT_BYTES}");
    }

    Ok(value as usize)
}

pub fn task_output_payload(
    entry: &TaskEntry,
    retrieval_status: TaskOutputRetrievalStatus,
) -> Value {
    task_output_payload_with_events(entry, retrieval_status, None)
}

pub fn task_output_payload_with_events(
    entry: &TaskEntry,
    retrieval_status: TaskOutputRetrievalStatus,
    output_events: Option<OutputReadBatch>,
) -> Value {
    let output = if entry.output.is_empty() {
        "(no output yet)".to_string()
    } else {
        entry.output.clone()
    };
    let mut task = json!({
        "task_id": entry.id,
        "task_type": entry.kind,
        "status": entry.status.as_str(),
        "description": entry.description,
        "output": output.clone(),
        "subject": entry.subject,
        "owner": entry.owner,
        "tool_use_id": entry.tool_use_id,
        "agent_id": entry.agent_id,
        "supervisor_id": entry.supervisor_id,
        "isolation": entry.isolation,
        "worktree_path": entry.worktree_path,
        "worktree_branch": entry.worktree_branch,
        "remote_task_type": entry.remote_task_type,
        "remote_session_id": entry.remote_session_id,
        "remote_task_metadata": entry.remote_task_metadata,
        "poll_started_at": entry.poll_started_at,
        "output_summary": entry.output_summary,
        "output_bytes": entry.output_bytes,
        "output_truncated": entry.output_truncated,
        "runtime_activity": entry.runtime_activity,
    });
    if let Some(events) = &output_events {
        task["output_events"] = json!(events.events);
        task["output_next_seq"] = json!(events.next_seq);
        task["output_first_available_seq"] = json!(events.first_available_seq);
        task["output_truncated_by_limit"] = json!(events.truncated);
        task["output_state"] = json!(events.state);
    }

    let mut payload = json!({
        "retrieval_status": retrieval_status.as_str(),
        "task": task,
        // Legacy flat fields remain for existing callers.
        "task_id": entry.id,
        "subject": entry.subject,
        "owner": entry.owner,
        "tool_use_id": entry.tool_use_id,
        "agent_id": entry.agent_id,
        "supervisor_id": entry.supervisor_id,
        "isolation": entry.isolation,
        "worktree_path": entry.worktree_path,
        "worktree_branch": entry.worktree_branch,
        "remote_task_type": entry.remote_task_type,
        "remote_session_id": entry.remote_session_id,
        "remote_task_metadata": entry.remote_task_metadata,
        "poll_started_at": entry.poll_started_at,
        "output": output,
        "output_summary": entry.output_summary,
        "output_bytes": entry.output_bytes,
        "output_truncated": entry.output_truncated,
    });
    if let Some(events) = output_events {
        payload["output_events"] = json!(events.events);
        payload["output_next_seq"] = json!(events.next_seq);
        payload["output_first_available_seq"] = json!(events.first_available_seq);
        payload["output_truncated_by_limit"] = json!(events.truncated);
        payload["output_state"] = json!(events.state);
    }
    payload
}

pub async fn wait_for_task_output(
    task_store: TaskStore,
    task_id: &str,
    timeout_ms: u64,
    mut abort_signal: tokio::sync::watch::Receiver<bool>,
) -> Result<TaskOutputWaitResult> {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);

    loop {
        let entry = task_store.get(task_id);
        if let Some(entry) = &entry {
            if !entry.status.is_active_for_output_wait() {
                return Ok(TaskOutputWaitResult::Ready(entry.clone()));
            }
        } else {
            return Ok(TaskOutputWaitResult::TimedOut(None));
        }

        let now = Instant::now();
        if timeout_ms == 0 || now >= deadline {
            return Ok(TaskOutputWaitResult::TimedOut(entry));
        }

        let remaining = deadline.saturating_duration_since(now);
        let delay = remaining.min(Duration::from_millis(TASK_OUTPUT_POLL_INTERVAL_MS));
        tokio::select! {
            changed = abort_signal.changed() => {
                if changed.is_ok() && *abort_signal.borrow() {
                    anyhow::bail!("TaskOutput wait aborted");
                }
                if changed.is_err() {
                    sleep(delay).await;
                }
            }
            _ = sleep(delay) => {}
        }
    }
}
