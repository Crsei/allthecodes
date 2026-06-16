use super::*;
#[cfg(all(feature = "sqlite-storage", not(feature = "json-storage")))]
use crate::sqlite::SqliteTaskRepository;

#[derive(Debug)]
pub(super) struct TaskRepository {
    pub(super) dir: PathBuf,
    pub(super) output_limit_bytes: usize,
    id_reservation_lock: Mutex<()>,
    #[cfg(all(feature = "sqlite-storage", not(feature = "json-storage")))]
    sqlite: Option<SqliteTaskRepository>,
}

#[derive(Debug, Deserialize)]
struct LegacyTaskRecord {
    id: String,
    subject: String,
    description: String,
    status: String,
    #[serde(default)]
    output: String,
    created_at: i64,
    updated_at: i64,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum TaskFileOnDisk {
    Versioned(Box<PersistedTaskFile>),
    Legacy(LegacyTaskRecord),
}

impl TaskRepository {
    pub(super) fn new(dir: PathBuf, output_limit_bytes: usize) -> Self {
        #[cfg(all(feature = "sqlite-storage", not(feature = "json-storage")))]
        let sqlite = if task_sqlite_disabled_by_env() {
            None
        } else {
            Some(SqliteTaskRepository::new(
                dir.clone(),
                output_limit_bytes.max(1),
            ))
        };

        Self {
            dir,
            output_limit_bytes: output_limit_bytes.max(1),
            id_reservation_lock: Mutex::new(()),
            #[cfg(all(feature = "sqlite-storage", not(feature = "json-storage")))]
            sqlite,
        }
    }

    pub(super) fn uses_sqlite(&self) -> bool {
        #[cfg(all(feature = "sqlite-storage", not(feature = "json-storage")))]
        {
            self.sqlite.is_some()
        }

        #[cfg(any(not(feature = "sqlite-storage"), feature = "json-storage"))]
        {
            false
        }
    }

    pub(super) fn load(&self) -> Result<HashMap<String, TaskEntry>> {
        self.load_with_startup_recovery(true)
    }

    pub(super) fn load_for_live_refresh(&self) -> Result<HashMap<String, TaskEntry>> {
        self.load_with_startup_recovery(false)
    }

    pub(super) fn load_with_startup_recovery(
        &self,
        recover_on_startup: bool,
    ) -> Result<HashMap<String, TaskEntry>> {
        #[cfg(all(feature = "sqlite-storage", not(feature = "json-storage")))]
        if let Some(sqlite) = &self.sqlite {
            match self.load_from_sqlite(sqlite, recover_on_startup) {
                Ok(tasks) => return Ok(tasks),
                Err(err) => {
                    tracing::warn!(
                        error = %err,
                        "failed to load tasks from sqlite; falling back to JSON task files"
                    );
                }
            }
        }

        let mut tasks = HashMap::new();
        if !self.dir.exists() {
            return Ok(tasks);
        }

        for entry in fs::read_dir(&self.dir)
            .with_context(|| format!("failed to read task dir {}", self.dir.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }

            match self.load_entry(&path, recover_on_startup) {
                Ok(Some(task)) => {
                    tasks.insert(task.id.clone(), task);
                }
                Ok(None) => {}
                Err(err) => {
                    tracing::warn!(
                        path = %path.display(),
                        error = %err,
                        "failed to load persisted task record"
                    );
                }
            }
        }
        Ok(tasks)
    }

    #[cfg(all(feature = "sqlite-storage", not(feature = "json-storage")))]
    fn load_from_sqlite(
        &self,
        sqlite: &SqliteTaskRepository,
        recover_on_startup: bool,
    ) -> Result<HashMap<String, TaskEntry>> {
        let mut tasks = HashMap::new();
        let records = sqlite.load_records()?;
        for record in records {
            let mut task = self.record_to_entry(record)?;
            let now = chrono::Utc::now();
            let was_recovered = recover_on_startup
                && recover_task_after_restart(&mut task, now.timestamp(), now.timestamp_millis());
            refresh_output_metadata(&mut task);

            if was_recovered {
                self.persist_entry(&task)?;
            }

            tasks.insert(task.id.clone(), task);
        }
        Ok(tasks)
    }

    pub(super) fn reserve_next_task_id(
        &self,
        tasks: &HashMap<String, TaskEntry>,
    ) -> Result<String> {
        #[cfg(all(feature = "sqlite-storage", not(feature = "json-storage")))]
        if let Some(sqlite) = &self.sqlite {
            match sqlite.reserve_next_task_id() {
                Ok(id) => return Ok(id),
                Err(err) => {
                    tracing::warn!(
                        error = %err,
                        "failed to reserve sqlite task id; falling back to JSON high watermark"
                    );
                }
            }
        }

        let _process_guard = self.id_reservation_lock.lock();
        fs::create_dir_all(&self.dir)
            .with_context(|| format!("failed to create task dir {}", self.dir.display()))?;

        let _guard = HighWatermarkLock::acquire(self.dir.join(TASK_HIGHWATERMARK_LOCK_FILE))?;
        let highest_seen = self
            .read_highwatermark()?
            .max(highest_numeric_task_id(tasks));
        let next_id = highest_seen.saturating_add(1);
        write_text_atomic(
            &self.dir.join(TASK_HIGHWATERMARK_FILE),
            &format!("{next_id}\n"),
        )?;
        Ok(next_id.to_string())
    }

    pub(super) fn read_highwatermark(&self) -> Result<u64> {
        let path = self.dir.join(TASK_HIGHWATERMARK_FILE);
        match fs::read_to_string(&path) {
            Ok(raw) => Ok(raw.trim().parse::<u64>().unwrap_or(0)),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(0),
            Err(err) => Err(err)
                .with_context(|| format!("failed to read task high watermark {}", path.display())),
        }
    }

    pub(super) fn load_entry(
        &self,
        path: &Path,
        recover_on_startup: bool,
    ) -> Result<Option<TaskEntry>> {
        let raw = fs::read_to_string(path)
            .with_context(|| format!("failed to read task file {}", path.display()))?;
        let on_disk: TaskFileOnDisk = serde_json::from_str(&raw)
            .with_context(|| format!("failed to parse task file {}", path.display()))?;

        let (record, needs_schema_rewrite) = match on_disk {
            TaskFileOnDisk::Versioned(file) => {
                (file.task, file.schema_version != TASK_SCHEMA_VERSION)
            }
            TaskFileOnDisk::Legacy(legacy) => {
                let output_file = Some(output_file_name(&legacy.id));
                (
                    PersistedTaskRecord {
                        id: legacy.id,
                        kind: default_task_kind(),
                        subject: legacy.subject,
                        description: legacy.description,
                        status: legacy.status,
                        output_file,
                        output_summary: String::new(),
                        output_bytes: 0,
                        output_truncated: false,
                        parent_id: None,
                        depends_on: Vec::new(),
                        owner: None,
                        active_form: None,
                        metadata: None,
                        tool_use_id: None,
                        agent_id: None,
                        supervisor_id: None,
                        isolation: None,
                        worktree_path: None,
                        worktree_branch: None,
                        remote_task_type: None,
                        remote_session_id: None,
                        remote_task_metadata: None,
                        poll_started_at: None,
                        cancel_requested_at: None,
                        recovered_at: None,
                        previous_status: None,
                        created_at: legacy.created_at,
                        updated_at: legacy.updated_at,
                        legacy_inline_output: Some(legacy.output),
                    },
                    true,
                )
            }
        };

        let mut task = self.record_to_entry(record)?;
        let now = chrono::Utc::now();
        let was_recovered = recover_on_startup
            && recover_task_after_restart(&mut task, now.timestamp(), now.timestamp_millis());
        refresh_output_metadata(&mut task);

        if needs_schema_rewrite || was_recovered {
            self.persist_entry(&task)?;
        }

        Ok(Some(task))
    }

    pub(super) fn record_to_entry(&self, record: PersistedTaskRecord) -> Result<TaskEntry> {
        let status = TaskStatus::from_str(&record.status).unwrap_or(TaskStatus::Interrupted);
        let previous_status = record
            .previous_status
            .as_deref()
            .and_then(TaskStatus::from_str);
        let output = match record.legacy_inline_output {
            Some(output) => output,
            None => {
                let output_path = self.dir.join(
                    record
                        .output_file
                        .unwrap_or_else(|| output_file_name(&record.id)),
                );
                fs::read_to_string(&output_path).unwrap_or_default()
            }
        };
        let (output, output_truncated_now) = trim_output_to_limit(&output, self.output_limit_bytes);

        Ok(TaskEntry {
            id: record.id,
            kind: sanitize_kind(&record.kind),
            subject: record.subject,
            description: record.description,
            status,
            output,
            output_summary: record.output_summary,
            output_bytes: record.output_bytes,
            output_truncated: record.output_truncated || output_truncated_now,
            parent_id: record.parent_id.filter(|s| !s.trim().is_empty()),
            depends_on: normalize_dependencies(record.depends_on),
            owner: normalize_optional_string(record.owner),
            active_form: normalize_optional_string(record.active_form),
            metadata: record.metadata.filter(|value| !value.is_null()),
            tool_use_id: normalize_optional_string(record.tool_use_id),
            agent_id: normalize_optional_string(record.agent_id),
            supervisor_id: normalize_optional_string(record.supervisor_id),
            isolation: normalize_optional_string(record.isolation),
            worktree_path: normalize_optional_string(record.worktree_path),
            worktree_branch: normalize_optional_string(record.worktree_branch),
            remote_task_type: normalize_remote_task_type(record.remote_task_type),
            remote_session_id: normalize_optional_string(record.remote_session_id),
            remote_task_metadata: record.remote_task_metadata.filter(|value| !value.is_null()),
            poll_started_at: record.poll_started_at,
            cancel_requested_at: record.cancel_requested_at,
            recovered_at: record.recovered_at,
            previous_status,
            created_at: record.created_at,
            updated_at: record.updated_at,
        })
    }

    pub(super) fn persist_entry(&self, entry: &TaskEntry) -> Result<()> {
        fs::create_dir_all(&self.dir)
            .with_context(|| format!("failed to create task dir {}", self.dir.display()))?;

        let mut to_write = entry.clone();
        let (trimmed, truncated_now) =
            trim_output_to_limit(&to_write.output, self.output_limit_bytes);
        to_write.output = trimmed;
        to_write.output_truncated |= truncated_now;
        refresh_output_metadata(&mut to_write);

        let output_path = self.dir.join(output_file_name(&to_write.id));
        write_text_atomic(&output_path, &to_write.output)?;

        #[cfg(all(feature = "sqlite-storage", not(feature = "json-storage")))]
        if let Some(sqlite) = &self.sqlite {
            match sqlite.persist_record(&persisted_record_from_entry(&to_write)) {
                Ok(()) => return Ok(()),
                Err(err) => {
                    tracing::warn!(
                        task_id = %to_write.id,
                        error = %err,
                        "failed to persist task to sqlite; falling back to JSON task file"
                    );
                }
            }
        }

        let file = PersistedTaskFile {
            schema_version: TASK_SCHEMA_VERSION,
            task: persisted_record_from_entry(&to_write),
        };
        let json = serde_json::to_string_pretty(&file)?;
        write_text_atomic(&self.task_json_path(&to_write.id), &json)?;
        Ok(())
    }

    pub(super) fn delete(&self, id: &str) -> Result<()> {
        #[cfg(all(feature = "sqlite-storage", not(feature = "json-storage")))]
        if let Some(sqlite) = &self.sqlite {
            match sqlite.delete(id) {
                Ok(()) => {
                    let output_path = self.dir.join(output_file_name(id));
                    remove_if_exists(&output_path)?;
                    return Ok(());
                }
                Err(err) => {
                    tracing::warn!(
                        task_id = %id,
                        error = %err,
                        "failed to delete task from sqlite; falling back to JSON task file delete"
                    );
                }
            }
        }

        let json_path = self.task_json_path(id);
        let output_path = self.dir.join(output_file_name(id));
        let output_events_path = self.dir.join(output_events_file_name(id));
        remove_if_exists(&json_path)?;
        remove_if_exists(&output_path)?;
        remove_if_exists(&output_events_path)?;
        Ok(())
    }

    pub(super) fn task_json_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{}.json", safe_file_stem(id)))
    }

    pub(super) fn ensure_output_events_seeded(&self, id: &str, output: &str) -> Result<()> {
        if output.is_empty() {
            return Ok(());
        }
        let path = self.dir.join(output_events_file_name(id));
        if path.exists() {
            return Ok(());
        }
        let event = OutputEvent {
            seq: 1,
            stream: OutputStream::Stdout,
            chunk: output.to_string(),
            timestamp_ms: chrono::Utc::now().timestamp_millis().max(0) as u64,
            process_or_run_id: id.to_string(),
        };
        self.write_output_events(id, &[event])
    }

    pub(super) fn append_output_event(
        &self,
        id: &str,
        stream: OutputStream,
        chunk: &str,
    ) -> Result<OutputEvent> {
        fs::create_dir_all(&self.dir)
            .with_context(|| format!("failed to create task dir {}", self.dir.display()))?;

        let mut events = self.read_output_events(id)?;
        let seq = events
            .last()
            .map(|event| event.seq.saturating_add(1))
            .unwrap_or(1);
        let event = OutputEvent {
            seq,
            stream,
            chunk: chunk.to_string(),
            timestamp_ms: chrono::Utc::now().timestamp_millis().max(0) as u64,
            process_or_run_id: id.to_string(),
        };
        events.push(event.clone());
        compact_output_events(&mut events, TASK_OUTPUT_EVENT_LIMIT_BYTES);
        self.write_output_events(id, &events)?;
        Ok(event)
    }

    pub(super) fn read_output_events(&self, id: &str) -> Result<Vec<OutputEvent>> {
        let path = self.dir.join(output_events_file_name(id));
        let raw = match fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => {
                return Err(err).with_context(|| {
                    format!("failed to read task output events {}", path.display())
                });
            }
        };

        raw.lines()
            .enumerate()
            .filter(|(_, line)| !line.trim().is_empty())
            .map(|(idx, line)| {
                serde_json::from_str::<OutputEvent>(line).with_context(|| {
                    format!(
                        "failed to parse task output event {} line {}",
                        path.display(),
                        idx + 1
                    )
                })
            })
            .collect()
    }

    fn write_output_events(&self, id: &str, events: &[OutputEvent]) -> Result<()> {
        if let Some(parent) = self.dir.join(output_events_file_name(id)).parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }

        let mut text = String::new();
        for event in events {
            text.push_str(&serde_json::to_string(event)?);
            text.push('\n');
        }
        write_text_atomic(&self.dir.join(output_events_file_name(id)), &text)
    }
}

#[derive(Debug)]
struct HighWatermarkLock {
    path: PathBuf,
}

impl HighWatermarkLock {
    fn acquire(path: PathBuf) -> Result<Self> {
        for attempt in 0..TASK_HIGHWATERMARK_LOCK_RETRIES {
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(_) => return Ok(Self { path }),
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                    let backoff_ms = (5_u64 << attempt.min(8)).min(250);
                    std::thread::sleep(std::time::Duration::from_millis(backoff_ms));
                }
                Err(err) => {
                    return Err(err).with_context(|| {
                        format!("failed to create high watermark lock {}", path.display())
                    });
                }
            }
        }

        anyhow::bail!("timed out acquiring high watermark lock {}", path.display())
    }
}

impl Drop for HighWatermarkLock {
    fn drop(&mut self) {
        if let Err(err) = fs::remove_file(&self.path) {
            if err.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(
                    path = %self.path.display(),
                    error = %err,
                    "failed to remove high watermark lock"
                );
            }
        }
    }
}

fn persisted_record_from_entry(entry: &TaskEntry) -> PersistedTaskRecord {
    PersistedTaskRecord {
        id: entry.id.clone(),
        kind: entry.kind.clone(),
        subject: entry.subject.clone(),
        description: entry.description.clone(),
        status: entry.status.as_str().to_string(),
        output_file: Some(output_file_name(&entry.id)),
        output_summary: entry.output_summary.clone(),
        output_bytes: entry.output_bytes,
        output_truncated: entry.output_truncated,
        parent_id: entry.parent_id.clone(),
        depends_on: entry.depends_on.clone(),
        owner: entry.owner.clone(),
        active_form: entry.active_form.clone(),
        metadata: entry.metadata.clone(),
        tool_use_id: entry.tool_use_id.clone(),
        agent_id: entry.agent_id.clone(),
        supervisor_id: entry.supervisor_id.clone(),
        isolation: entry.isolation.clone(),
        worktree_path: entry.worktree_path.clone(),
        worktree_branch: entry.worktree_branch.clone(),
        remote_task_type: entry.remote_task_type.clone(),
        remote_session_id: entry.remote_session_id.clone(),
        remote_task_metadata: entry.remote_task_metadata.clone(),
        poll_started_at: entry.poll_started_at,
        cancel_requested_at: entry.cancel_requested_at,
        recovered_at: entry.recovered_at,
        previous_status: entry.previous_status.map(|s| s.as_str().to_string()),
        created_at: entry.created_at,
        updated_at: entry.updated_at,
        legacy_inline_output: None,
    }
}

fn compact_output_events(events: &mut Vec<OutputEvent>, max_bytes: usize) {
    if max_bytes == 0 {
        events.clear();
        return;
    }

    let mut total: usize = events.iter().map(|event| event.chunk.len()).sum();
    while total > max_bytes {
        if events.is_empty() {
            break;
        }
        let removed = events.remove(0);
        total = total.saturating_sub(removed.chunk.len());
    }
}

#[cfg(all(feature = "sqlite-storage", not(feature = "json-storage")))]
fn task_sqlite_disabled_by_env() -> bool {
    std::env::var("ALLTHECODES_TASK_STORAGE")
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "json" | "file" | "files"
            )
        })
        .unwrap_or(false)
}
