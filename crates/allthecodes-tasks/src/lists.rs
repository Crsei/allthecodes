use super::*;

#[derive(Debug)]
pub(crate) struct TaskListLock {
    path: PathBuf,
}

impl TaskListLock {
    pub(crate) fn acquire(dir: PathBuf) -> Result<Self> {
        fs::create_dir_all(&dir)
            .with_context(|| format!("failed to create task dir {}", dir.display()))?;
        let path = dir.join(TASK_LIST_LOCK_FILE);
        for attempt in 0..TASK_LIST_LOCK_RETRIES {
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
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                    // Some package-wide tests temporarily point ALLTHECODES_HOME at
                    // tempdirs from other tests. If that tempdir is torn down
                    // between create_dir_all() and lock creation, recreate the
                    // task-list directory and retry instead of failing with an
                    // unhelpful "path not found".
                    fs::create_dir_all(&dir).with_context(|| {
                        format!("failed to recreate task dir {}", dir.display())
                    })?;
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                Err(err) => {
                    return Err(err).with_context(|| {
                        format!("failed to create task-list lock {}", path.display())
                    });
                }
            }
        }

        anyhow::bail!("timed out acquiring task-list lock {}", path.display())
    }
}

impl Drop for TaskListLock {
    fn drop(&mut self) {
        if let Err(err) = fs::remove_file(&self.path) {
            if err.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(
                    path = %self.path.display(),
                    error = %err,
                    "failed to remove task-list lock"
                );
            }
        }
    }
}

// =============================================================================
// Global task store (lazy singleton)
// =============================================================================

#[cfg(test)]
static TEST_TASKS_ROOT: std::sync::LazyLock<PathBuf> = std::sync::LazyLock::new(|| {
    std::env::temp_dir().join(format!(
        "allthecodes-test-global-tasks-{}",
        uuid::Uuid::new_v4()
    ))
});

static GLOBAL_STORES: std::sync::LazyLock<Mutex<HashMap<String, TaskStore>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

fn task_lists_root() -> PathBuf {
    #[cfg(test)]
    {
        if let Ok(root) =
            std::env::var("ALLTHECODES_HOME").or_else(|_| std::env::var("CC_RUST_HOME"))
        {
            if !root.trim().is_empty() {
                return PathBuf::from(root).join("tasks");
            }
        }
        TEST_TASKS_ROOT.clone()
    }

    #[cfg(not(test))]
    {
        if let Ok(root) =
            std::env::var("ALLTHECODES_HOME").or_else(|_| std::env::var("CC_RUST_HOME"))
        {
            let root = root.trim();
            if !root.is_empty() {
                return PathBuf::from(root).join("tasks");
            }
        }

        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".allthecodes")
            .join("tasks")
    }
}

pub fn sanitize_task_list_id(input: &str) -> String {
    let sanitized: String = input
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();

    if sanitized.is_empty() {
        DEFAULT_TASK_LIST_ID.to_string()
    } else {
        sanitized
    }
}

pub fn task_list_dir(task_list_id: &str) -> PathBuf {
    task_lists_root().join(sanitize_task_list_id(task_list_id))
}

fn default_task_list_has_json_files(dir: &Path) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };

    entries.filter_map(std::result::Result::ok).any(|entry| {
        let path = entry.path();
        path.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("json")
    })
}

fn read_u64_file(path: &Path) -> u64 {
    fs::read_to_string(path)
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .unwrap_or(0)
}

fn migrate_legacy_flat_default_task_list(root: &Path, default_dir: &Path) {
    if !root.exists() || default_task_list_has_json_files(default_dir) {
        return;
    }

    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(err) => {
            tracing::warn!(
                path = %root.display(),
                error = %err,
                "failed to scan legacy flat task directory"
            );
            return;
        }
    };

    let mut legacy_files = Vec::new();
    let mut legacy_highwatermark = None;
    for entry in entries.filter_map(std::result::Result::ok) {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if file_name == TASK_HIGHWATERMARK_FILE {
            legacy_highwatermark = Some(path);
        } else if file_name.ends_with(".json") || file_name.ends_with(".output.log") {
            legacy_files.push(path);
        }
    }

    if legacy_files.is_empty() && legacy_highwatermark.is_none() {
        return;
    }

    if let Err(err) = fs::create_dir_all(default_dir) {
        tracing::warn!(
            path = %default_dir.display(),
            error = %err,
            "failed to create default task-list directory for legacy migration"
        );
        return;
    }

    let mut copied = 0usize;
    for source in legacy_files {
        let Some(file_name) = source.file_name() else {
            continue;
        };
        let destination = default_dir.join(file_name);
        if destination.exists() {
            continue;
        }
        match fs::copy(&source, &destination) {
            Ok(_) => copied += 1,
            Err(err) => tracing::warn!(
                source = %source.display(),
                destination = %destination.display(),
                error = %err,
                "failed to copy legacy flat task file"
            ),
        }
    }

    if let Some(source) = legacy_highwatermark {
        let destination = default_dir.join(TASK_HIGHWATERMARK_FILE);
        let source_value = read_u64_file(&source);
        let destination_value = read_u64_file(&destination);
        if source_value > destination_value {
            if let Err(err) = write_text_atomic(&destination, &format!("{source_value}\n")) {
                tracing::warn!(
                    source = %source.display(),
                    destination = %destination.display(),
                    error = %err,
                    "failed to migrate legacy task high watermark"
                );
            }
        }
    }

    if copied > 0 {
        tracing::info!(
            legacy_dir = %root.display(),
            default_task_list_dir = %default_dir.display(),
            copied,
            "copied legacy flat tasks into default task list"
        );
    }
}

fn env_task_list_id(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[derive(Debug, Clone, Default)]
pub struct TaskListScope {
    pub explicit_task_list_id: Option<String>,
    pub scoped_team_name: Option<String>,
    pub app_team_name: Option<String>,
    pub session_id: Option<String>,
}

pub fn task_list_id_from_parts(scope: TaskListScope) -> String {
    if let Some(id) = env_task_list_id(ALLTHECODES_TASK_LIST_ID_ENV)
        .or_else(|| env_task_list_id(CC_RUST_TASK_LIST_ID_ENV))
        .or_else(|| env_task_list_id(CLAUDE_CODE_TASK_LIST_ID_ENV))
    {
        return id;
    }

    if let Some(id) = scope
        .explicit_task_list_id
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        return id;
    }

    if let Some(team_name) = scope
        .scoped_team_name
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        return team_name;
    }

    if let Some(team_name) = scope
        .app_team_name
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| env_task_list_id(ALLTHECODES_TEAM_NAME_ENV))
        .or_else(|| env_task_list_id(CLAUDE_CODE_TEAM_NAME_ENV))
    {
        return team_name;
    }

    scope
        .session_id
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_TASK_LIST_ID.to_string())
}

pub fn store_for_task_list_id(task_list_id: &str) -> TaskStore {
    let key = sanitize_task_list_id(task_list_id);
    let root = task_lists_root();
    let dir = root.join(&key);
    let registry_key = dir.to_string_lossy().to_string();
    let mut stores = GLOBAL_STORES.lock();
    stores
        .entry(registry_key)
        .or_insert_with(|| {
            if key == DEFAULT_TASK_LIST_ID {
                migrate_legacy_flat_default_task_list(&root, &dir);
            }
            TaskStore::with_dir(dir)
        })
        .clone()
}

pub fn global_store() -> TaskStore {
    store_for_task_list_id(DEFAULT_TASK_LIST_ID)
}

pub fn unassign_teammate_tasks(
    task_list_id: &str,
    teammate_id: &str,
    teammate_name: &str,
    reason: TeammateTaskExitReason,
) -> UnassignTeammateTasksResult {
    let task_store = store_for_task_list_id(task_list_id);
    let unassigned_tasks = task_store.unassign_teammate_tasks(teammate_id, teammate_name);
    let action = match reason {
        TeammateTaskExitReason::Terminated => "was terminated",
        TeammateTaskExitReason::Shutdown => "has shut down",
    };
    let mut notification_message = format!("{teammate_name} {action}.");
    if !unassigned_tasks.is_empty() {
        let task_list = unassigned_tasks
            .iter()
            .map(|task| format!("#{} \"{}\"", task.id, task.subject))
            .collect::<Vec<_>>()
            .join(", ");
        notification_message.push_str(&format!(
            " {} task(s) were unassigned: {}. Use TaskList to check availability and TaskUpdate with owner to reassign them to idle teammates.",
            unassigned_tasks.len(),
            task_list
        ));
    }
    UnassignTeammateTasksResult {
        unassigned_tasks,
        notification_message,
    }
}
