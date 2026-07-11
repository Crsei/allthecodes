use super::*;
use crate::lists::TaskListLock;
use crate::repository::TaskRepository;

/// Shared task store backed by a durable repository and runtime handles.
#[derive(Debug, Clone)]
pub struct TaskStore {
    tasks: Arc<Mutex<HashMap<String, TaskEntry>>>,
    runtime_handles: Arc<Mutex<HashMap<String, TaskRuntimeHandle>>>,
    repository: Arc<TaskRepository>,
}

impl Default for TaskStore {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskStore {
    pub fn new() -> Self {
        Self::with_dir(task_list_dir(DEFAULT_TASK_LIST_ID))
    }

    pub fn with_dir(dir: impl Into<PathBuf>) -> Self {
        Self::with_dir_and_output_limit(dir, DEFAULT_OUTPUT_LIMIT_BYTES)
    }

    pub fn with_dir_and_output_limit(dir: impl Into<PathBuf>, output_limit_bytes: usize) -> Self {
        let repository = Arc::new(TaskRepository::new(dir.into(), output_limit_bytes));
        let tasks = match repository.load() {
            Ok(tasks) => tasks,
            Err(err) => {
                tracing::warn!(error = %err, "failed to load persisted tasks; starting with empty task store");
                HashMap::new()
            }
        };
        Self {
            tasks: Arc::new(Mutex::new(tasks)),
            runtime_handles: Arc::new(Mutex::new(HashMap::new())),
            repository,
        }
    }

    pub fn try_create(&self, subject: &str, description: &str) -> Result<TaskEntry> {
        self.try_create_with_options(subject, description, TaskCreateOptions::default())
    }

    #[cfg(test)]
    pub fn create(&self, subject: &str, description: &str) -> TaskEntry {
        self.try_create(subject, description)
            .expect("failed to create task")
    }

    #[cfg(test)]
    pub fn create_with_options(
        &self,
        subject: &str,
        description: &str,
        options: TaskCreateOptions,
    ) -> TaskEntry {
        self.try_create_with_options(subject, description, options)
            .expect("failed to create task")
    }

    pub fn try_create_with_options(
        &self,
        subject: &str,
        description: &str,
        options: TaskCreateOptions,
    ) -> Result<TaskEntry> {
        let _guard = self.acquire_task_list_lock("create task")?;
        let mut tasks = self.load_repository_tasks_strict()?;
        let now = chrono::Utc::now().timestamp();
        let id = match self.repository.reserve_next_task_id(&tasks) {
            Ok(id) => id,
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    "failed to reserve incremental task id; falling back to uuid"
                );
                fallback_task_id(&tasks)
            }
        };
        let mut entry = TaskEntry {
            id: id.clone(),
            kind: sanitize_kind(options.kind.as_deref().unwrap_or("tool")),
            subject: subject.to_string(),
            description: description.to_string(),
            status: TaskStatus::Pending,
            output: String::new(),
            output_summary: String::new(),
            output_bytes: 0,
            output_truncated: false,
            parent_id: options.parent_id.filter(|s| !s.trim().is_empty()),
            depends_on: normalize_dependencies(options.depends_on),
            owner: normalize_optional_string(options.owner),
            active_form: normalize_optional_string(options.active_form),
            metadata: options.metadata.filter(|value| !value.is_null()),
            tool_use_id: normalize_optional_string(options.tool_use_id),
            agent_id: normalize_optional_string(options.agent_id),
            supervisor_id: normalize_optional_string(options.supervisor_id),
            isolation: normalize_optional_string(options.isolation),
            worktree_path: normalize_optional_string(options.worktree_path),
            worktree_branch: normalize_optional_string(options.worktree_branch),
            remote_task_type: normalize_remote_task_type(options.remote_task_type),
            remote_session_id: normalize_optional_string(options.remote_session_id),
            remote_task_metadata: options
                .remote_task_metadata
                .filter(|value| !value.is_null()),
            poll_started_at: options.poll_started_at,
            cancel_requested_at: None,
            recovered_at: None,
            previous_status: None,
            runtime_activity: options.runtime_activity,
            created_at: now,
            updated_at: now,
        };
        refresh_output_metadata(&mut entry);

        tasks.insert(id, entry.clone());
        self.persist_entry_strict(&entry)?;
        self.replace_tasks(tasks);
        Ok(entry)
    }

    pub fn try_upsert_with_id(
        &self,
        id: &str,
        subject: &str,
        description: &str,
        status: TaskStatus,
        output: &str,
        options: TaskCreateOptions,
    ) -> Result<TaskEntry> {
        let id = id.trim();
        if id.is_empty() {
            anyhow::bail!("task id must not be empty");
        }

        let _guard = self.acquire_task_list_lock("upsert task")?;
        let mut tasks = self.load_repository_tasks_strict()?;
        let now = chrono::Utc::now().timestamp();
        let status = normalize_new_status(status);
        let entry = if let Some(entry) = tasks.get_mut(id) {
            entry.kind = sanitize_kind(options.kind.as_deref().unwrap_or("tool"));
            entry.subject = subject.to_string();
            entry.description = description.to_string();
            entry.status = status;
            entry.output = output.to_string();
            entry.output_truncated = false;
            entry.parent_id = options.parent_id.filter(|s| !s.trim().is_empty());
            entry.depends_on = normalize_dependencies(options.depends_on);
            entry.owner = normalize_optional_string(options.owner);
            entry.active_form = normalize_optional_string(options.active_form);
            entry.metadata = options.metadata.filter(|value| !value.is_null());
            entry.tool_use_id = normalize_optional_string(options.tool_use_id);
            entry.agent_id = normalize_optional_string(options.agent_id);
            entry.supervisor_id = normalize_optional_string(options.supervisor_id);
            entry.isolation = normalize_optional_string(options.isolation);
            entry.worktree_path = normalize_optional_string(options.worktree_path);
            entry.worktree_branch = normalize_optional_string(options.worktree_branch);
            entry.remote_task_type = normalize_remote_task_type(options.remote_task_type);
            entry.remote_session_id = normalize_optional_string(options.remote_session_id);
            entry.remote_task_metadata = options
                .remote_task_metadata
                .filter(|value| !value.is_null());
            entry.poll_started_at = options.poll_started_at;
            if status == TaskStatus::Cancelled && entry.cancel_requested_at.is_none() {
                entry.cancel_requested_at = Some(now);
            }
            entry.updated_at = now;
            refresh_output_metadata(entry);
            entry.clone()
        } else {
            let mut entry = TaskEntry {
                id: id.to_string(),
                kind: sanitize_kind(options.kind.as_deref().unwrap_or("tool")),
                subject: subject.to_string(),
                description: description.to_string(),
                status,
                output: output.to_string(),
                output_summary: String::new(),
                output_bytes: 0,
                output_truncated: false,
                parent_id: options.parent_id.filter(|s| !s.trim().is_empty()),
                depends_on: normalize_dependencies(options.depends_on),
                owner: normalize_optional_string(options.owner),
                active_form: normalize_optional_string(options.active_form),
                metadata: options.metadata.filter(|value| !value.is_null()),
                tool_use_id: normalize_optional_string(options.tool_use_id),
                agent_id: normalize_optional_string(options.agent_id),
                supervisor_id: normalize_optional_string(options.supervisor_id),
                isolation: normalize_optional_string(options.isolation),
                worktree_path: normalize_optional_string(options.worktree_path),
                worktree_branch: normalize_optional_string(options.worktree_branch),
                remote_task_type: normalize_remote_task_type(options.remote_task_type),
                remote_session_id: normalize_optional_string(options.remote_session_id),
                remote_task_metadata: options
                    .remote_task_metadata
                    .filter(|value| !value.is_null()),
                poll_started_at: options.poll_started_at,
                cancel_requested_at: (status == TaskStatus::Cancelled).then_some(now),
                recovered_at: None,
                previous_status: None,
                runtime_activity: options.runtime_activity,
                created_at: now,
                updated_at: now,
            };
            refresh_output_metadata(&mut entry);
            tasks.insert(id.to_string(), entry.clone());
            entry
        };

        self.persist_entry_strict(&entry)?;
        self.replace_tasks(tasks);
        Ok(entry)
    }

    pub fn get(&self, id: &str) -> Option<TaskEntry> {
        self.refresh_from_repository();
        self.refresh_remote_review_timeout(id)
    }

    /// Atomically binds a pending delegation envelope to one child runtime.
    pub fn adopt_pending_agent_task(
        &self,
        id: &str,
        agent_id: &str,
        child_session_id: &str,
        worktree_path: Option<String>,
        worktree_branch: Option<String>,
    ) -> Result<TaskEntry> {
        let _guard = self.acquire_task_list_lock("adopt delegated task")?;
        let mut tasks = self.load_repository_tasks_strict()?;
        let Some(entry) = tasks.get_mut(id) else {
            anyhow::bail!("delegated task {id} was not found");
        };
        if entry.status != TaskStatus::Pending {
            anyhow::bail!(
                "delegated task {id} cannot be adopted from status {}",
                entry.status.as_str()
            );
        }
        let now_ms = chrono::Utc::now().timestamp_millis();
        entry.status = TaskStatus::InProgress;
        entry.agent_id = Some(agent_id.to_string());
        entry.supervisor_id = Some(agent_id.to_string());
        entry.remote_session_id = Some(child_session_id.to_string());
        entry.worktree_path = normalize_optional_string(worktree_path);
        entry.worktree_branch = normalize_optional_string(worktree_branch);
        entry.runtime_activity = Some(AgentRuntimeActivity {
            phase: AgentRuntimePhase::Running,
            last_heartbeat_at_ms: now_ms,
            last_progress_at_ms: now_ms,
            task_id: id.to_string(),
            agent_id: agent_id.to_string(),
            child_session_id: child_session_id.to_string(),
            partial_output_bytes: entry.output_bytes,
        });
        entry.updated_at = now_ms / 1000;
        let adopted = entry.clone();
        self.persist_entry_strict(&adopted)?;
        self.replace_tasks(tasks);
        Ok(adopted)
    }

    pub fn update_runtime_activity(
        &self,
        id: &str,
        activity: AgentRuntimeActivity,
    ) -> Result<Option<TaskEntry>> {
        let _guard = self.acquire_task_list_lock("update runtime activity")?;
        let mut tasks = self.load_repository_tasks_strict()?;
        let Some(entry) = tasks.get_mut(id) else {
            self.replace_tasks(tasks);
            return Ok(None);
        };
        entry.runtime_activity = Some(activity);
        entry.updated_at = chrono::Utc::now().timestamp();
        let updated = entry.clone();
        self.persist_entry_strict(&updated)?;
        self.replace_tasks(tasks);
        Ok(Some(updated))
    }

    #[cfg(test)]
    pub fn update_status(&self, id: &str, status: TaskStatus) -> Option<TaskEntry> {
        self.try_update_status(id, status).ok().flatten()
    }

    pub fn try_update_status(&self, id: &str, status: TaskStatus) -> Result<Option<TaskEntry>> {
        let _guard = self.acquire_task_list_lock("update task status")?;
        let mut tasks = self.load_repository_tasks_strict()?;
        if let Some(entry) = tasks.get_mut(id) {
            entry.status = normalize_new_status(status);
            entry.updated_at = chrono::Utc::now().timestamp();
            if entry.status == TaskStatus::Cancelled && entry.cancel_requested_at.is_none() {
                entry.cancel_requested_at = Some(entry.updated_at);
            }
            let cloned = entry.clone();
            self.persist_entry_strict(&cloned)?;
            self.replace_tasks(tasks);
            Ok(Some(cloned))
        } else {
            self.replace_tasks(tasks);
            Ok(None)
        }
    }

    pub fn try_update_fields(
        &self,
        id: &str,
        updates: TaskUpdateFields,
    ) -> Result<Option<TaskEntry>> {
        let _guard = self.acquire_task_list_lock("update task fields")?;
        let mut tasks = self.load_repository_tasks_strict()?;
        let now = chrono::Utc::now().timestamp();
        let mut changed_entries = Vec::new();
        let updated = {
            let Some(entry) = tasks.get_mut(id) else {
                self.replace_tasks(tasks);
                return Ok(None);
            };
            if let Some(subject) = updates.subject {
                entry.subject = subject;
            }
            if let Some(description) = updates.description {
                entry.description = description;
            }
            if let Some(active_form) = updates.active_form {
                entry.active_form = active_form;
            }
            if let Some(owner) = updates.owner {
                entry.owner = owner;
            }
            if let Some(metadata_patch) = updates.metadata_patch {
                entry.metadata = merge_metadata(entry.metadata.clone(), &metadata_patch);
            }
            if let Some(status) = updates.status {
                entry.status = normalize_new_status(status);
                if entry.status == TaskStatus::Cancelled && entry.cancel_requested_at.is_none() {
                    entry.cancel_requested_at = Some(now);
                }
            }
            for dependency_id in normalize_dependencies(updates.add_blocked_by) {
                if dependency_id != entry.id
                    && !entry.depends_on.iter().any(|id| id == &dependency_id)
                {
                    entry.depends_on.push(dependency_id);
                }
            }
            entry.depends_on = normalize_dependencies(std::mem::take(&mut entry.depends_on));
            entry.updated_at = now;
            entry.clone()
        };

        for blocked_task_id in normalize_dependencies(updates.add_blocks) {
            if blocked_task_id == id {
                continue;
            }
            if let Some(blocked_task) = tasks.get_mut(&blocked_task_id) {
                if !blocked_task.depends_on.iter().any(|dep_id| dep_id == id) {
                    blocked_task.depends_on.push(id.to_string());
                    blocked_task.depends_on =
                        normalize_dependencies(std::mem::take(&mut blocked_task.depends_on));
                    blocked_task.updated_at = now;
                    changed_entries.push(blocked_task.clone());
                }
            }
        }

        changed_entries.push(updated.clone());
        for entry in changed_entries {
            self.persist_entry_strict(&entry)?;
        }
        self.replace_tasks(tasks);
        Ok(Some(updated))
    }

    pub fn claim_task(
        &self,
        id: &str,
        owner: &str,
        check_agent_busy: bool,
    ) -> std::result::Result<TaskEntry, TaskClaimFailure> {
        let owner = owner.trim();
        if owner.is_empty() {
            return Err(TaskClaimFailure::new(TaskClaimFailureReason::OwnerRequired));
        }

        let _guard = self.acquire_task_list_lock("claim task").map_err(|err| {
            tracing::warn!(
                task_id = id,
                owner,
                error = %err,
                "failed to acquire task-list lock for claim"
            );
            TaskClaimFailure::new(TaskClaimFailureReason::LockUnavailable)
        })?;
        let mut tasks = self.load_repository_tasks_strict().map_err(|err| {
            tracing::warn!(
                task_id = id,
                owner,
                error = %err,
                "failed to refresh persisted tasks for claim"
            );
            TaskClaimFailure::new(TaskClaimFailureReason::LockUnavailable)
        })?;
        let Some(snapshot) = tasks.get(id).cloned() else {
            self.replace_tasks(tasks);
            return Err(TaskClaimFailure::new(TaskClaimFailureReason::TaskNotFound));
        };

        if snapshot.status.is_terminal() {
            self.replace_tasks(tasks);
            return Err(TaskClaimFailure::new(
                TaskClaimFailureReason::AlreadyResolved,
            ));
        }

        if let Some(existing_owner) = snapshot.owner.as_deref() {
            if existing_owner != owner {
                self.replace_tasks(tasks);
                return Err(
                    TaskClaimFailure::new(TaskClaimFailureReason::AlreadyClaimed)
                        .with_owner(existing_owner.to_string()),
                );
            }
        }

        let blocked_by = blocked_dependencies_with_tasks(&tasks, &snapshot);
        if !blocked_by.is_empty() {
            self.replace_tasks(tasks);
            return Err(
                TaskClaimFailure::new(TaskClaimFailureReason::Blocked).with_blocked_by(blocked_by)
            );
        }

        if check_agent_busy {
            if let Some(busy_task_id) = tasks
                .values()
                .find(|candidate| {
                    candidate.id != snapshot.id
                        && candidate.owner.as_deref() == Some(owner)
                        && !candidate.status.is_terminal()
                })
                .map(|candidate| candidate.id.clone())
            {
                self.replace_tasks(tasks);
                return Err(TaskClaimFailure::new(TaskClaimFailureReason::AgentBusy)
                    .with_busy_task_id(busy_task_id));
            }
        }

        let Some(entry) = tasks.get_mut(id) else {
            self.replace_tasks(tasks);
            return Err(TaskClaimFailure::new(TaskClaimFailureReason::TaskNotFound));
        };
        entry.owner = Some(owner.to_string());
        entry.status = TaskStatus::InProgress;
        entry.updated_at = chrono::Utc::now().timestamp();
        let cloned = entry.clone();
        self.persist_entry_strict(&cloned).map_err(|err| {
            tracing::warn!(
                task_id = id,
                owner,
                error = %err,
                "failed to persist claimed task"
            );
            TaskClaimFailure::new(TaskClaimFailureReason::LockUnavailable)
        })?;
        self.replace_tasks(tasks);
        Ok(cloned)
    }

    /// Append output text to a task's retained log.
    ///
    /// Output retention is bounded. When the configured byte cap is exceeded,
    /// the oldest bytes are dropped on a UTF-8 boundary and the task is marked
    /// as truncated.
    pub fn append_output(&self, id: &str, output: &str) -> Option<TaskEntry> {
        let _guard = match self.acquire_task_list_lock("append task output") {
            Ok(guard) => guard,
            Err(err) => {
                tracing::warn!(task_id = %id, error = %err, "failed to acquire task-list lock");
                return None;
            }
        };
        let mut tasks = self.load_repository_tasks();
        if let Some(entry) = tasks.get_mut(id) {
            if let Err(err) = self
                .repository
                .ensure_output_events_seeded(id, &entry.output)
            {
                tracing::warn!(
                    task_id = %id,
                    error = %err,
                    "failed to seed task output event log"
                );
            }
            let event_chunk = if !entry.output.is_empty() && !output.is_empty() {
                format!("\n{output}")
            } else {
                output.to_string()
            };
            if !entry.output.is_empty() && !output.is_empty() {
                entry.output.push('\n');
            }
            entry.output.push_str(output);
            let (trimmed, truncated_now) =
                trim_output_to_limit(&entry.output, self.repository.output_limit_bytes);
            entry.output = trimmed;
            entry.output_truncated |= truncated_now;
            entry.updated_at = chrono::Utc::now().timestamp();
            refresh_output_metadata(entry);
            let cloned = entry.clone();
            self.persist_entry(&cloned);
            if !event_chunk.is_empty() {
                if let Err(err) =
                    self.repository
                        .append_output_event(id, OutputStream::Stdout, &event_chunk)
                {
                    tracing::warn!(
                        task_id = %id,
                        error = %err,
                        "failed to append task output event"
                    );
                }
            }
            self.replace_tasks(tasks);
            Some(cloned)
        } else {
            None
        }
    }

    pub fn read_output_events(
        &self,
        id: &str,
        after_seq: Option<EventSeq>,
        limit_bytes: usize,
    ) -> Result<Option<OutputReadBatch>> {
        let _guard = self.acquire_task_list_lock("read task output events")?;
        let tasks = self.load_repository_tasks_strict()?;
        let Some(entry) = tasks.get(id) else {
            self.replace_tasks(tasks);
            return Ok(None);
        };
        self.repository
            .ensure_output_events_seeded(id, &entry.output)?;
        let events = self.repository.read_output_events(id)?;
        let state = output_state_for_task(entry.status);
        let batch = output_read_batch_from_events(events, after_seq, limit_bytes, state);
        self.replace_tasks(tasks);
        Ok(Some(batch))
    }

    pub fn list(&self) -> Vec<TaskEntry> {
        self.refresh_from_repository();
        self.refresh_remote_review_timeouts();
        let tasks = self.tasks.lock();
        let mut entries: Vec<TaskEntry> = tasks.values().cloned().collect();
        entries.sort_by_key(|e| (e.created_at, e.id.clone()));
        entries
    }

    pub fn try_delete(&self, id: &str) -> Result<Option<TaskEntry>> {
        let _guard = self.acquire_task_list_lock("delete task")?;
        let mut tasks = self.load_repository_tasks_strict()?;
        let removed = tasks.remove(id);
        if removed.is_some() {
            let mut changed = Vec::new();
            for entry in tasks.values_mut() {
                let before = entry.depends_on.len();
                entry.depends_on.retain(|dep_id| dep_id != id);
                if entry.depends_on.len() != before {
                    entry.updated_at = chrono::Utc::now().timestamp();
                    changed.push(entry.clone());
                }
            }
            self.runtime_handles.lock().remove(id);
            self.repository.delete(id)?;
            for entry in changed {
                self.persist_entry_strict(&entry)?;
            }
        }
        self.replace_tasks(tasks);
        Ok(removed)
    }

    #[cfg(test)]
    pub fn delete(&self, id: &str) -> Option<TaskEntry> {
        self.try_delete(id).ok().flatten()
    }

    pub fn unassign_teammate_tasks(
        &self,
        teammate_id: &str,
        teammate_name: &str,
    ) -> Vec<UnassignedTaskSummary> {
        self.try_unassign_teammate_tasks(teammate_id, teammate_name)
            .unwrap_or_default()
    }

    pub fn try_unassign_teammate_tasks(
        &self,
        teammate_id: &str,
        teammate_name: &str,
    ) -> Result<Vec<UnassignedTaskSummary>> {
        let _guard = self.acquire_task_list_lock("unassign teammate tasks")?;
        let mut tasks = self.load_repository_tasks_strict()?;
        let now = chrono::Utc::now().timestamp();
        let mut changed = Vec::new();
        for entry in tasks.values_mut() {
            let owner_matches = entry.owner.as_deref() == Some(teammate_id)
                || entry.owner.as_deref() == Some(teammate_name);
            if owner_matches && !entry.status.is_terminal() {
                entry.owner = None;
                entry.status = TaskStatus::Pending;
                entry.updated_at = now;
                changed.push(entry.clone());
            }
        }

        for entry in &changed {
            self.persist_entry_strict(entry)?;
        }
        self.replace_tasks(tasks);
        Ok(changed
            .into_iter()
            .map(|entry| UnassignedTaskSummary {
                id: entry.id,
                subject: entry.subject,
            })
            .collect())
    }

    #[cfg(test)]
    pub fn stop(&self, id: &str) -> Option<TaskEntry> {
        self.try_stop(id).ok().flatten()
    }

    pub fn try_stop(&self, id: &str) -> Result<Option<TaskEntry>> {
        if let Some(handle) = self.runtime_handles.lock().get(id).cloned() {
            handle.cancel();
        }

        let now = chrono::Utc::now().timestamp();
        let _guard = self.acquire_task_list_lock("stop task")?;
        let mut tasks = self.load_repository_tasks_strict()?;
        if let Some(entry) = tasks.get_mut(id) {
            entry.cancel_requested_at = Some(now);
            entry.status = TaskStatus::Cancelled;
            entry.updated_at = now;
            let cloned = entry.clone();
            self.persist_entry_strict(&cloned)?;
            self.replace_tasks(tasks);
            Ok(Some(cloned))
        } else {
            self.replace_tasks(tasks);
            Ok(None)
        }
    }

    pub fn register_runtime_handle(&self, id: &str, cancellation_token: CancellationToken) -> bool {
        if !self.tasks.lock().contains_key(id) {
            return false;
        }
        self.runtime_handles
            .lock()
            .insert(id.to_string(), TaskRuntimeHandle::new(cancellation_token));
        true
    }

    pub fn unregister_runtime_handle(&self, id: &str) -> Option<TaskRuntimeHandle> {
        self.runtime_handles.lock().remove(id)
    }

    pub fn has_runtime_handle(&self, id: &str) -> bool {
        self.runtime_handles.lock().contains_key(id)
    }

    pub fn get_by_agent_id(&self, agent_id: &str) -> Option<TaskEntry> {
        self.tasks
            .lock()
            .values()
            .find(|entry| entry.agent_id.as_deref() == Some(agent_id))
            .cloned()
    }

    pub fn blocked_dependencies(&self, entry: &TaskEntry) -> Vec<String> {
        let tasks = self.tasks.lock();
        blocked_dependencies_with_tasks(&tasks, entry)
    }

    pub fn blocked_tasks(&self, entry: &TaskEntry) -> Vec<String> {
        let tasks = self.tasks.lock();
        let mut ids: Vec<String> = tasks
            .values()
            .filter(|candidate| candidate.depends_on.iter().any(|id| id == &entry.id))
            .map(|candidate| candidate.id.clone())
            .collect();
        ids.sort();
        ids
    }

    pub(super) fn acquire_task_list_lock(&self, operation: &str) -> Result<TaskListLock> {
        if self.repository.uses_sqlite() {
            return Ok(TaskListLock::disabled());
        }

        TaskListLock::acquire(self.repository.dir.clone()).with_context(|| {
            format!(
                "failed to acquire task-list lock for {operation} in {}",
                self.repository.dir.display()
            )
        })
    }

    pub(super) fn load_repository_tasks(&self) -> HashMap<String, TaskEntry> {
        match self.repository.load_for_live_refresh() {
            Ok(tasks) => tasks,
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    "failed to refresh persisted tasks; keeping in-memory task state"
                );
                self.tasks.lock().clone()
            }
        }
    }

    pub(super) fn load_repository_tasks_strict(&self) -> Result<HashMap<String, TaskEntry>> {
        self.repository
            .load_for_live_refresh()
            .context("failed to refresh persisted tasks")
    }

    pub(super) fn refresh_from_repository(&self) {
        let tasks = self.load_repository_tasks();
        self.replace_tasks(tasks);
    }

    pub(super) fn replace_tasks(&self, tasks: HashMap<String, TaskEntry>) {
        *self.tasks.lock() = tasks;
    }

    pub(super) fn persist_entry(&self, entry: &TaskEntry) {
        if let Err(err) = self.repository.persist_entry(entry) {
            tracing::warn!(
                task_id = %entry.id,
                error = %err,
                "failed to persist task"
            );
        }
    }

    pub(super) fn persist_entry_strict(&self, entry: &TaskEntry) -> Result<()> {
        self.repository
            .persist_entry(entry)
            .with_context(|| format!("failed to persist task {}", entry.id))
    }

    pub(super) fn refresh_remote_review_timeouts(&self) {
        let ids: Vec<String> = self.tasks.lock().keys().cloned().collect();
        for id in ids {
            self.refresh_remote_review_timeout(&id);
        }
    }

    pub(super) fn refresh_remote_review_timeout(&self, id: &str) -> Option<TaskEntry> {
        let now = chrono::Utc::now();
        let now_ms = now.timestamp_millis();
        let mut tasks = self.tasks.lock();
        let entry = tasks.get_mut(id)?;

        if !remote_review_timed_out(entry, now_ms) {
            return Some(entry.clone());
        }

        if entry.previous_status.is_none() {
            entry.previous_status = Some(entry.status);
        }
        entry.status = TaskStatus::Failed;
        entry.updated_at = now.timestamp();
        if entry.output.trim().is_empty() {
            entry.output =
                "Remote review did not produce output (remote session exceeded 30 minutes)."
                    .to_string();
        } else if !entry.output.contains("remote session exceeded 30 minutes") {
            entry
                .output
                .push_str("\nRemote review timed out after 30 minutes.");
        }
        refresh_output_metadata(entry);
        let cloned = entry.clone();
        drop(tasks);
        self.persist_entry(&cloned);
        Some(cloned)
    }
}

#[cfg(all(test, feature = "sqlite-storage", not(feature = "json-storage")))]
mod tests {
    use super::*;

    #[test]
    #[serial_test::serial]
    fn delegated_adoption_is_atomic_and_pending_only() {
        let dir = tempfile::tempdir().unwrap();
        let _storage = EnvVarGuard::set("ALLTHECODES_TASK_STORAGE", "json");
        let store = TaskStore::with_dir(dir.path());
        let task = store.try_create("delegate", "prompt").unwrap();

        let adopted = store
            .adopt_pending_agent_task(
                &task.id,
                "child-session",
                "child-session",
                Some("/tmp/worktree".to_string()),
                Some("agent-worktree-child".to_string()),
            )
            .unwrap();
        assert_eq!(adopted.status, TaskStatus::InProgress);
        assert_eq!(adopted.agent_id.as_deref(), Some("child-session"));
        assert_eq!(
            adopted
                .runtime_activity
                .as_ref()
                .map(|activity| activity.phase),
            Some(AgentRuntimePhase::Running)
        );
        assert!(store
            .adopt_pending_agent_task(&task.id, "other", "other", None, None)
            .is_err());

        let cancelled = store.try_create("cancelled", "prompt").unwrap();
        store
            .try_update_status(&cancelled.id, TaskStatus::Cancelled)
            .unwrap();
        assert!(store
            .adopt_pending_agent_task(&cancelled.id, "other", "other", None, None)
            .is_err());
    }
    use std::sync::{Arc, Barrier};
    use std::thread;

    static SQLITE_ENV_LOCK: std::sync::LazyLock<std::sync::Mutex<()>> =
        std::sync::LazyLock::new(|| std::sync::Mutex::new(()));

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvVarGuard {
        fn unset(key: &'static str) -> Self {
            let previous = std::env::var(key).ok();
            std::env::remove_var(key);
            Self { key, previous }
        }

        fn set(key: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
            let previous = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            if let Some(previous) = &self.previous {
                std::env::set_var(self.key, previous);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    #[test]
    fn sqlite_store_persists_task_rows_and_file_backed_output() {
        let _lock = SQLITE_ENV_LOCK.lock().expect("sqlite env lock");
        let _storage_guard = EnvVarGuard::unset("ALLTHECODES_TASK_STORAGE");
        let _path_guard = EnvVarGuard::unset("ALLTHECODES_TASK_SQLITE_PATH");
        let temp = tempfile::tempdir().expect("tempdir");
        let dir = temp.path().join("tasks").join("phase-one");
        let store = TaskStore::with_dir_and_output_limit(&dir, 1024);

        let created = store
            .try_create_with_options(
                "SQLite phase",
                "Persist task metadata in sqlite",
                TaskCreateOptions {
                    depends_on: vec!["missing-dependency".to_string()],
                    owner: Some("agent-1".to_string()),
                    metadata: Some(json!({"source": "test"})),
                    ..TaskCreateOptions::default()
                },
            )
            .expect("create task");
        let updated = store
            .append_output(&created.id, "large text stays in an output file")
            .expect("append output");

        assert_eq!(updated.output_summary, "large text stays in an output file");
        assert!(dir.join(output_file_name(&created.id)).exists());
        assert!(!dir
            .join(format!("{}.json", safe_file_stem(&created.id)))
            .exists());
        assert!(temp.path().join("state").join("state_5.sqlite").exists());

        let reloaded = TaskStore::with_dir_and_output_limit(&dir, 1024);
        let loaded = reloaded.get(&created.id).expect("load task from sqlite");
        assert_eq!(loaded.subject, "SQLite phase");
        assert_eq!(loaded.depends_on, vec!["missing-dependency"]);
        assert_eq!(loaded.owner.as_deref(), Some("agent-1"));
        assert_eq!(loaded.output, "large text stays in an output file");
        assert_eq!(
            loaded
                .metadata
                .as_ref()
                .and_then(|value| value.get("source")),
            Some(&json!("test"))
        );
    }

    #[test]
    fn sqlite_store_persists_core_task_mutations() {
        let _lock = SQLITE_ENV_LOCK.lock().expect("sqlite env lock");
        let _storage_guard = EnvVarGuard::unset("ALLTHECODES_TASK_STORAGE");
        let _path_guard = EnvVarGuard::unset("ALLTHECODES_TASK_SQLITE_PATH");
        let temp = tempfile::tempdir().expect("tempdir");
        let dir = temp.path().join("tasks").join("mutations");
        let store = TaskStore::with_dir_and_output_limit(&dir, 1024);

        let dependency = store.create("Dependency", "finish first");
        let blocked = store.create_with_options(
            "Blocked",
            "waits for dependency",
            TaskCreateOptions {
                depends_on: vec![dependency.id.clone()],
                metadata: Some(json!({"priority": "normal"})),
                ..TaskCreateOptions::default()
            },
        );
        let other_blocked = store.create("Other blocked", "updated through add_blocks");

        assert_eq!(
            store
                .claim_task(&blocked.id, "agent-a", false)
                .expect_err("dependency should block claim")
                .reason,
            TaskClaimFailureReason::Blocked
        );

        let dependency = store
            .update_status(&dependency.id, TaskStatus::Completed)
            .expect("complete dependency");
        assert_eq!(dependency.status, TaskStatus::Completed);

        let claimed = store
            .claim_task(&blocked.id, "agent-a", false)
            .expect("claim unblocked task");
        assert_eq!(claimed.owner.as_deref(), Some("agent-a"));
        assert_eq!(claimed.status, TaskStatus::InProgress);

        let updated = store
            .try_update_fields(
                &blocked.id,
                TaskUpdateFields {
                    subject: Some("Blocked updated".to_string()),
                    description: Some("new description".to_string()),
                    active_form: Some(Some("working".to_string())),
                    owner: Some(Some("agent-b".to_string())),
                    metadata_patch: Some(json!({"priority": "high", "stale": null})),
                    add_blocks: vec![other_blocked.id.clone()],
                    status: Some(TaskStatus::Failed),
                    ..TaskUpdateFields::default()
                },
            )
            .expect("update fields")
            .expect("updated task");
        assert_eq!(updated.subject, "Blocked updated");
        assert_eq!(updated.description, "new description");
        assert_eq!(updated.active_form.as_deref(), Some("working"));
        assert_eq!(updated.owner.as_deref(), Some("agent-b"));
        assert_eq!(updated.status, TaskStatus::Failed);
        assert_eq!(
            updated
                .metadata
                .as_ref()
                .and_then(|value| value.get("priority")),
            Some(&json!("high"))
        );
        assert!(updated
            .metadata
            .as_ref()
            .and_then(|value| value.get("stale"))
            .is_none());

        let reloaded = TaskStore::with_dir_and_output_limit(&dir, 1024);
        let loaded_blocked = reloaded.get(&blocked.id).expect("reload updated task");
        assert_eq!(loaded_blocked.subject, "Blocked updated");
        assert_eq!(loaded_blocked.owner.as_deref(), Some("agent-b"));
        assert_eq!(loaded_blocked.status, TaskStatus::Failed);
        assert_eq!(
            reloaded
                .get(&other_blocked.id)
                .expect("reload dependent task")
                .depends_on,
            vec![blocked.id.clone()]
        );

        let removed = reloaded.delete(&blocked.id).expect("delete task");
        assert_eq!(removed.id, blocked.id);

        let after_delete = TaskStore::with_dir_and_output_limit(&dir, 1024);
        assert!(after_delete.get(&blocked.id).is_none());
        assert!(after_delete
            .get(&other_blocked.id)
            .expect("dependent task remains")
            .depends_on
            .is_empty());
    }

    #[test]
    fn task_claims_are_serialized_across_concurrent_attempts() {
        let _lock = SQLITE_ENV_LOCK.lock().expect("sqlite env lock");
        let _storage_guard = EnvVarGuard::set("ALLTHECODES_TASK_STORAGE", "json");
        let _path_guard = EnvVarGuard::unset("ALLTHECODES_TASK_SQLITE_PATH");
        let temp = tempfile::tempdir().expect("tempdir");
        let dir = temp.path().join("tasks").join("claim-race");
        let store_a = TaskStore::with_dir_and_output_limit(&dir, 1024);
        let store_b = TaskStore::with_dir_and_output_limit(&dir, 1024);
        let task = store_a.create("Race target", "concurrent claim regression");
        let barrier = Arc::new(Barrier::new(3));

        let mut handles = Vec::new();
        let task_id = task.id.clone();
        for (store, owner) in [
            (store_a, "agent-a".to_string()),
            (store_b, "agent-b".to_string()),
        ] {
            let barrier = Arc::clone(&barrier);
            let task_id = task_id.clone();
            handles.push(thread::spawn(move || {
                barrier.wait();
                store.claim_task(&task_id, &owner, false)
            }));
        }

        barrier.wait();

        let mut winners = Vec::new();
        let mut failures = Vec::new();
        for handle in handles {
            match handle.join().expect("thread join") {
                Ok(entry) => winners.push(entry),
                Err(failure) => failures.push(failure),
            }
        }

        assert_eq!(winners.len(), 1, "exactly one claim should succeed");
        assert_eq!(failures.len(), 1, "exactly one claim should fail");
        assert_eq!(failures[0].reason, TaskClaimFailureReason::AlreadyClaimed);

        let winner = &winners[0];
        let final_entry = TaskRepository::new(dir.clone(), 1024)
            .load_for_live_refresh()
            .expect("reload final task map")
            .get(&task.id)
            .cloned()
            .expect("reload final task");
        assert_eq!(final_entry.owner.as_deref(), winner.owner.as_deref());
        assert_eq!(final_entry.status, TaskStatus::InProgress);
        assert!(matches!(
            final_entry.owner.as_deref(),
            Some("agent-a") | Some("agent-b")
        ));
    }

    #[test]
    fn task_output_events_support_incremental_replay_and_legacy_seed() {
        let _lock = SQLITE_ENV_LOCK.lock().expect("sqlite env lock");
        let _storage_guard = EnvVarGuard::set("ALLTHECODES_TASK_STORAGE", "json");
        let temp = tempfile::tempdir().expect("tempdir");
        let dir = temp.path().join("tasks").join("output-events");
        let store = TaskStore::with_dir_and_output_limit(&dir, 1024);
        let created = store.create("Output events", "record task output events");

        store
            .append_output(&created.id, "first")
            .expect("append first output");
        store
            .append_output(&created.id, "second")
            .expect("append second output");

        let batch = store
            .read_output_events(&created.id, Some(0), 1024)
            .expect("read output events")
            .expect("task output events");
        assert_eq!(batch.first_available_seq, 1);
        assert_eq!(batch.next_seq, 3);
        assert!(!batch.truncated);
        assert_eq!(batch.events.len(), 2);
        assert_eq!(batch.events[0].seq, 1);
        assert_eq!(batch.events[0].chunk, "first");
        assert_eq!(batch.events[1].seq, 2);
        assert_eq!(batch.events[1].chunk, "\nsecond");

        let incremental = store
            .read_output_events(&created.id, Some(1), 1024)
            .expect("read incremental events")
            .expect("task output events");
        assert_eq!(incremental.events.len(), 1);
        assert_eq!(incremental.events[0].chunk, "\nsecond");
    }

    #[test]
    fn task_output_event_retention_is_bounded() {
        let _lock = SQLITE_ENV_LOCK.lock().expect("sqlite env lock");
        let _storage_guard = EnvVarGuard::set("ALLTHECODES_TASK_STORAGE", "json");
        let temp = tempfile::tempdir().expect("tempdir");
        let dir = temp.path().join("tasks").join("bounded-output-events");
        let store = TaskStore::with_dir_and_output_limit(&dir, 1024);
        let created = store.create("Bounded output", "large event chunks are compacted");

        let large = "x".repeat((TASK_OUTPUT_EVENT_LIMIT_BYTES / 2) + 16);
        store
            .append_output(&created.id, &large)
            .expect("append first large output");
        store
            .append_output(&created.id, &large)
            .expect("append second large output");

        let batch = store
            .read_output_events(&created.id, Some(0), TASK_OUTPUT_EVENT_LIMIT_BYTES)
            .expect("read output events")
            .expect("task output events");
        assert!(batch.truncated);
        assert_eq!(batch.first_available_seq, 2);
        assert_eq!(batch.events.len(), 1);
        assert_eq!(batch.events[0].seq, 2);
    }

    #[test]
    fn sqlite_store_unassigns_teammate_tasks() {
        let _lock = SQLITE_ENV_LOCK.lock().expect("sqlite env lock");
        let _storage_guard = EnvVarGuard::unset("ALLTHECODES_TASK_STORAGE");
        let _path_guard = EnvVarGuard::unset("ALLTHECODES_TASK_SQLITE_PATH");
        let temp = tempfile::tempdir().expect("tempdir");
        let dir = temp.path().join("tasks").join("unassign");
        let store = TaskStore::with_dir_and_output_limit(&dir, 1024);

        let active = store.create_with_options(
            "Active teammate task",
            "should be unassigned",
            TaskCreateOptions {
                owner: Some("teammate-1".to_string()),
                ..TaskCreateOptions::default()
            },
        );
        store
            .update_status(&active.id, TaskStatus::InProgress)
            .expect("mark active");
        let completed = store.create_with_options(
            "Completed teammate task",
            "should remain assigned",
            TaskCreateOptions {
                owner: Some("teammate-1".to_string()),
                ..TaskCreateOptions::default()
            },
        );
        store
            .update_status(&completed.id, TaskStatus::Completed)
            .expect("mark completed");

        let summaries = store.unassign_teammate_tasks("teammate-1", "Teammate One");
        assert_eq!(
            summaries,
            vec![UnassignedTaskSummary {
                id: active.id.clone(),
                subject: "Active teammate task".to_string(),
            }]
        );

        let active_now = store.get(&active.id).expect("load active");
        assert_eq!(active_now.owner, None);
        assert_eq!(active_now.status, TaskStatus::Pending);

        let reloaded = TaskStore::with_dir_and_output_limit(&dir, 1024);
        let active = reloaded.get(&active.id).expect("reload active");
        assert_eq!(active.owner, None);
        assert_eq!(active.status, TaskStatus::Interrupted);
        let completed = reloaded.get(&completed.id).expect("reload completed");
        assert_eq!(completed.owner.as_deref(), Some("teammate-1"));
        assert_eq!(completed.status, TaskStatus::Completed);
    }

    #[test]
    fn sqlite_store_falls_back_to_json_when_sqlite_is_unavailable() {
        let _lock = SQLITE_ENV_LOCK.lock().expect("sqlite env lock");
        let _storage_guard = EnvVarGuard::unset("ALLTHECODES_TASK_STORAGE");
        let temp = tempfile::tempdir().expect("tempdir");
        let dir = temp.path().join("tasks").join("fallback");
        let blocked_db_path = temp.path().join("state").join("state_5.sqlite");
        fs::create_dir_all(&blocked_db_path).expect("create directory at sqlite path");
        let _path_guard = EnvVarGuard::set("ALLTHECODES_TASK_SQLITE_PATH", &blocked_db_path);

        let store = TaskStore::with_dir_and_output_limit(&dir, 1024);
        let created = store.create("JSON fallback", "sqlite cannot open");
        let updated = store
            .append_output(&created.id, "fallback output")
            .expect("append output through fallback");

        assert_eq!(updated.output_summary, "fallback output");
        assert!(dir.join(output_file_name(&created.id)).exists());
        assert!(dir
            .join(format!("{}.json", safe_file_stem(&created.id)))
            .exists());
        assert!(blocked_db_path.is_dir());

        let reloaded = TaskStore::with_dir_and_output_limit(&dir, 1024);
        let loaded = reloaded.get(&created.id).expect("load json fallback task");
        assert_eq!(loaded.subject, "JSON fallback");
        assert_eq!(loaded.output, "fallback output");
    }
}
