use super::*;

pub fn task_to_json_from_store(task_store: &TaskStore, entry: &TaskEntry) -> Value {
    let blocked_dependencies = task_store.blocked_dependencies(entry);
    let blocked_tasks = task_store.blocked_tasks(entry);
    let blocked_by = entry.depends_on.clone();
    json!({
        "id": entry.id,
        "kind": entry.kind,
        "subject": entry.subject,
        "description": entry.description,
        "status": entry.status.as_str(),
        "created_at": entry.created_at,
        "updated_at": entry.updated_at,
        "parent_id": entry.parent_id,
        "depends_on": blocked_by.clone(),
        "blocked_by": blocked_by.clone(),
        "blockedBy": blocked_by,
        "blocks": blocked_tasks,
        "owner": entry.owner,
        "activeForm": entry.active_form,
        "metadata": entry.metadata,
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
        "blocked_dependencies": blocked_dependencies,
        "output_summary": entry.output_summary,
        "output_bytes": entry.output_bytes,
        "output_truncated": entry.output_truncated,
        "cancel_requested_at": entry.cancel_requested_at,
        "recovered_at": entry.recovered_at,
        "previous_status": entry.previous_status.map(|s| s.as_str()),
        "has_runtime_handle": task_store.has_runtime_handle(&entry.id),
    })
}
