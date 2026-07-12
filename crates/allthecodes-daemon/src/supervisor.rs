//! Supervisor/worker lifecycle management for daemon mode.
//!
//! Phase 2 keeps the existing KAIROS HTTP service in the supervisor process
//! while adding a real child-worker registry. Later phases can move query,
//! bridge, and scheduler responsibilities into richer worker kinds without
//! changing the process-state contract introduced here.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use chrono::{Local, Utc};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use allthecodes_config::features::{self, Feature};

use super::{
    bridge_worker::BridgeWorkerRuntime,
    gateway_bridge::{handle_worker_command, AssistantWorkerRuntime},
    process_state::{self, DaemonWorkerStatus},
};

pub const ASSISTANT_WORKER_ID: &str = "assistant-session-1";
pub const BRIDGE_WORKER_ID: &str = "bridge-sync-1";
pub const PROACTIVE_WORKER_ID: &str = "proactive-1";
pub const SCHEDULER_WORKER_ID: &str = "scheduler-1";
const REGISTRY_TICK_INTERVAL: Duration = Duration::from_secs(1);
const WORKER_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(1);
const WORKER_STALE_AFTER: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerKind {
    AssistantSession,
    BridgeSync,
    Proactive,
    Scheduler,
}

impl WorkerKind {
    pub fn parse(raw: &str) -> Result<Self> {
        match raw {
            "assistant-session" => Ok(Self::AssistantSession),
            "bridge-sync" => Ok(Self::BridgeSync),
            "proactive" => Ok(Self::Proactive),
            "scheduler" => Ok(Self::Scheduler),
            other => anyhow::bail!("unknown daemon worker kind: {other}"),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::AssistantSession => "assistant-session",
            Self::BridgeSync => "bridge-sync",
            Self::Proactive => "proactive",
            Self::Scheduler => "scheduler",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestartPolicy {
    pub max_restarts: u32,
    pub delay: Duration,
}

impl Default for RestartPolicy {
    fn default() -> Self {
        Self {
            max_restarts: 3,
            delay: Duration::from_millis(250),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerSpec {
    pub worker_id: String,
    pub kind: WorkerKind,
    pub cwd: PathBuf,
    pub env: Vec<(String, String)>,
    pub log_path: PathBuf,
    pub restart_policy: RestartPolicy,
    pub required: bool,
}

impl WorkerSpec {
    fn assistant_session(cwd: &Path) -> Self {
        Self {
            worker_id: ASSISTANT_WORKER_ID.to_string(),
            kind: WorkerKind::AssistantSession,
            cwd: cwd.to_path_buf(),
            env: Vec::new(),
            log_path: process_state::worker_log_path(ASSISTANT_WORKER_ID),
            restart_policy: RestartPolicy::default(),
            required: true,
        }
    }

    fn bridge_sync(cwd: &Path) -> Self {
        Self {
            worker_id: BRIDGE_WORKER_ID.to_string(),
            kind: WorkerKind::BridgeSync,
            cwd: cwd.to_path_buf(),
            env: Vec::new(),
            log_path: process_state::worker_log_path(BRIDGE_WORKER_ID),
            restart_policy: RestartPolicy::default(),
            required: false,
        }
    }

    fn proactive(cwd: &Path) -> Self {
        Self {
            worker_id: PROACTIVE_WORKER_ID.to_string(),
            kind: WorkerKind::Proactive,
            cwd: cwd.to_path_buf(),
            env: Vec::new(),
            log_path: process_state::worker_log_path(PROACTIVE_WORKER_ID),
            restart_policy: RestartPolicy::default(),
            required: false,
        }
    }

    fn scheduler(cwd: &Path) -> Self {
        Self {
            worker_id: SCHEDULER_WORKER_ID.to_string(),
            kind: WorkerKind::Scheduler,
            cwd: cwd.to_path_buf(),
            env: Vec::new(),
            log_path: process_state::worker_log_path(SCHEDULER_WORKER_ID),
            restart_policy: RestartPolicy::default(),
            required: false,
        }
    }
}

#[derive(Debug)]
struct ManagedWorker {
    spec: WorkerSpec,
    child: Child,
    restart_count: u32,
}

#[derive(Debug, Clone)]
struct WorkerHeartbeatObservation {
    persisted_heartbeat_at: Option<chrono::DateTime<Utc>>,
    observed_at: Instant,
}

impl WorkerHeartbeatObservation {
    fn new(observed_at: Instant, persisted_heartbeat_at: Option<chrono::DateTime<Utc>>) -> Self {
        Self {
            persisted_heartbeat_at,
            observed_at,
        }
    }

    fn observe_heartbeat(
        &mut self,
        persisted_heartbeat_at: Option<chrono::DateTime<Utc>>,
        observed_at: Instant,
    ) {
        if self.persisted_heartbeat_at != persisted_heartbeat_at {
            self.persisted_heartbeat_at = persisted_heartbeat_at;
            self.observed_at = observed_at;
        }
    }

    fn is_stale(&self, now: Instant, stale_after: Duration) -> bool {
        now.saturating_duration_since(self.observed_at) > stale_after
    }
}

#[derive(Debug)]
pub struct WorkerRegistry {
    specs: HashMap<String, WorkerSpec>,
    workers: HashMap<String, ManagedWorker>,
    heartbeat_observations: HashMap<String, WorkerHeartbeatObservation>,
}

impl WorkerRegistry {
    pub fn new(specs: Vec<WorkerSpec>) -> Self {
        let specs = specs
            .into_iter()
            .map(|spec| (spec.worker_id.clone(), spec))
            .collect();
        Self {
            specs,
            workers: HashMap::new(),
            heartbeat_observations: HashMap::new(),
        }
    }

    fn start_all(&mut self) -> Result<()> {
        let ids: Vec<String> = self.specs.keys().cloned().collect();
        for id in ids {
            if let Err(err) = self.start_worker(&id, 0) {
                if let Err(rollback_err) = self.terminate_all() {
                    warn!(error = %rollback_err, "failed to roll back partially started workers");
                }
                return Err(err);
            }
        }
        Ok(())
    }

    async fn poll(&mut self) -> Result<()> {
        let mut remove_ids = Vec::new();
        let mut restart_ids = Vec::new();
        let observed_at = Instant::now();

        for (worker_id, managed) in &mut self.workers {
            if let Some(status) = managed
                .child
                .try_wait()
                .with_context(|| format!("failed to poll worker {worker_id}"))?
            {
                let status_text = exit_status_text(status);
                process_state::write_worker_stopped(worker_id, Some(status_text.clone()))?;
                warn!(
                    worker_id,
                    status = %status_text,
                    "daemon worker exited"
                );
                if managed.restart_count < managed.spec.restart_policy.max_restarts {
                    restart_ids.push((worker_id.clone(), managed.restart_count + 1));
                }
                remove_ids.push(worker_id.clone());
                continue;
            }

            let heartbeat = self
                .heartbeat_observations
                .entry(worker_id.clone())
                .or_insert_with(|| WorkerHeartbeatObservation::new(observed_at, None));
            if worker_heartbeat_stale(worker_id, heartbeat, observed_at)? {
                process_state::write_worker_stale(worker_id, "heartbeat stale")?;
                if let Some(state) = process_state::read_worker_state(worker_id)? {
                    if let Some(pid) = state.pid {
                        let identity = process_state::process_matches_record(
                            pid,
                            state.process_start_key.as_deref(),
                        );
                        if identity.is_current_process_record() {
                            let _ = process_state::terminate_process_tree(
                                pid,
                                state.process_start_key.as_deref(),
                            );
                        } else {
                            warn!(
                                worker_id,
                                pid,
                                identity = %identity.as_diagnostic(),
                                "skipped stale worker termination because pid identity did not match"
                            );
                        }
                    }
                }
                warn!(worker_id, "daemon worker heartbeat is stale");
                if managed.restart_count < managed.spec.restart_policy.max_restarts {
                    restart_ids.push((worker_id.clone(), managed.restart_count + 1));
                }
                remove_ids.push(worker_id.clone());
            }
        }

        for worker_id in remove_ids {
            self.workers.remove(&worker_id);
            self.heartbeat_observations.remove(&worker_id);
        }

        for (worker_id, restart_count) in restart_ids {
            let delay = self
                .specs
                .get(&worker_id)
                .map(|spec| spec.restart_policy.delay)
                .unwrap_or_default();
            tokio::time::sleep(delay).await;
            self.start_worker(&worker_id, restart_count)?;
        }

        Ok(())
    }

    fn terminate_all(&mut self) -> Result<()> {
        let mut first_error: Option<anyhow::Error> = None;
        for (worker_id, managed) in &mut self.workers {
            let state = match process_state::read_worker_state(worker_id) {
                Ok(state) => state,
                Err(err) => {
                    if first_error.is_none() {
                        first_error = Some(err);
                    }
                    None
                }
            };
            if let Some(state) = state {
                if let Some(pid) = state.pid {
                    let identity = process_state::process_matches_record(
                        pid,
                        state.process_start_key.as_deref(),
                    );
                    if identity.is_current_process_record() {
                        if let Err(err) = process_state::terminate_process_tree(
                            pid,
                            state.process_start_key.as_deref(),
                        ) {
                            if first_error.is_none() {
                                first_error = Some(
                                    err.context(format!("failed to terminate worker {worker_id}")),
                                );
                            }
                        }
                    } else {
                        warn!(
                            worker_id,
                            pid,
                            identity = %identity.as_diagnostic(),
                            "skipped worker termination because pid identity did not match"
                        );
                    }
                }
            } else {
                let pid = managed.child.id();
                let start_key = process_state::process_start_key(pid);
                if let Err(err) = process_state::terminate_process_tree(pid, start_key.as_deref()) {
                    if first_error.is_none() {
                        first_error = Some(err);
                    }
                }
            }
            if let Err(err) = process_state::write_worker_stopped(worker_id, None) {
                if first_error.is_none() {
                    first_error = Some(err);
                }
            }
        }
        self.workers.clear();
        self.heartbeat_observations.clear();
        first_error.map_or(Ok(()), Err)
    }

    fn start_worker(&mut self, worker_id: &str, restart_count: u32) -> Result<()> {
        let spec = self
            .specs
            .get(worker_id)
            .with_context(|| format!("daemon worker spec not found: {worker_id}"))?
            .clone();
        let managed = spawn_worker(spec, restart_count)?;
        self.workers.insert(worker_id.to_string(), managed);
        self.heartbeat_observations.insert(
            worker_id.to_string(),
            WorkerHeartbeatObservation::new(Instant::now(), None),
        );
        Ok(())
    }
}

pub fn default_worker_specs(cwd: &Path) -> Vec<WorkerSpec> {
    let mut specs = vec![WorkerSpec::assistant_session(cwd)];
    if features::enabled(Feature::Kairos) {
        specs.push(WorkerSpec::bridge_sync(cwd));
        specs.push(WorkerSpec::scheduler(cwd));
    }
    if features::enabled(Feature::Proactive) {
        specs.push(WorkerSpec::proactive(cwd));
    }
    specs
}

pub struct SupervisorHandle {
    cancel: CancellationToken,
    task: tokio::task::JoinHandle<Result<()>>,
}

impl SupervisorHandle {
    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    pub async fn shutdown(mut self, grace: Duration) -> Result<()> {
        self.cancel.cancel();
        match tokio::time::timeout(grace, &mut self.task).await {
            Ok(result) => result.context("daemon supervisor task panicked")?,
            Err(_) => {
                self.task.abort();
                let _ = (&mut self.task).await;
                anyhow::bail!("daemon supervisor did not stop within {grace:?}")
            }
        }
    }
}

impl Drop for SupervisorHandle {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

pub fn start_supervisor(cwd: PathBuf, port: u16) -> Result<SupervisorHandle> {
    let mut registry = WorkerRegistry::new(default_worker_specs(&cwd));
    registry.start_all()?;
    if let Err(err) = process_state::write_supervisor_heartbeat(port, &cwd) {
        if let Err(rollback_err) = registry.terminate_all() {
            warn!(error = %rollback_err, "failed to roll back workers after supervisor state failure");
        }
        return Err(err);
    }
    let cancel = CancellationToken::new();
    let task_cancel = cancel.clone();
    let task =
        tokio::spawn(
            async move { run_supervisor_registry(registry, cwd, port, task_cancel).await },
        );
    Ok(SupervisorHandle { cancel, task })
}

async fn run_supervisor_registry(
    mut registry: WorkerRegistry,
    cwd: PathBuf,
    port: u16,
    cancel: CancellationToken,
) -> Result<()> {
    let mut tick = tokio::time::interval(REGISTRY_TICK_INTERVAL);
    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                info!("daemon supervisor cancellation requested");
            }
            _ = tick.tick() => {
                if !process_state::shutdown_requested() {
                    registry.poll().await?;
                    process_state::write_supervisor_heartbeat(port, &cwd)?;
                    continue;
                }
            }
        }
        info!("daemon supervisor shutdown requested");
        registry.terminate_all()?;
        process_state::write_supervisor_heartbeat(port, &cwd)?;
        return Ok(());
    }
}

pub async fn run_worker_mode(kind: &str, worker_id: &str, cwd: PathBuf) -> Result<()> {
    let kind = WorkerKind::parse(kind)?;
    match kind {
        WorkerKind::AssistantSession => run_assistant_worker_mode(kind, worker_id, cwd).await,
        WorkerKind::BridgeSync => run_bridge_worker_mode(kind, worker_id, cwd).await,
        WorkerKind::Proactive => run_proactive_worker_mode(kind, worker_id, cwd).await,
        WorkerKind::Scheduler => run_scheduler_worker_mode(kind, worker_id, cwd).await,
    }
}

async fn run_assistant_worker_mode(kind: WorkerKind, worker_id: &str, cwd: PathBuf) -> Result<()> {
    let runtime = AssistantWorkerRuntime::new(&cwd)?;
    ensure_worker_state(kind, worker_id, &cwd)?;

    let mut heartbeat = tokio::time::interval(WORKER_HEARTBEAT_INTERVAL);
    let mut command_poll = tokio::time::interval(Duration::from_millis(50));
    let mut submit: Option<tokio::task::JoinHandle<Result<bool>>> = None;
    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                if process_state::shutdown_requested() {
                    runtime.abort();
                    if let Some(handle) = submit.take() {
                        let _ = handle.await;
                    }
                    process_state::write_worker_stopped(worker_id, None)?;
                    return Ok(());
                }
                process_state::write_worker_heartbeat(worker_id)?;
            }
            _ = command_poll.tick() => {
                let command = if submit.is_some() {
                    super::protocol_store().claim_next_control_command(worker_id, kind.as_str())?
                } else {
                    super::protocol_store().claim_next_pending_command(worker_id, kind.as_str())?
                };
                let Some(command) = command else { continue };
                if command.kind == super::protocol::DaemonCommandKind::Submit {
                    let runtime = runtime.clone();
                    let worker_id = worker_id.to_string();
                    submit = Some(tokio::spawn(async move {
                        handle_worker_command(&worker_id, &runtime, command).await
                    }));
                } else if handle_worker_command(worker_id, &runtime, command).await? {
                    runtime.abort();
                    if let Some(handle) = submit.take() {
                        let _ = handle.await;
                    }
                    process_state::write_worker_stopped(worker_id, None)?;
                    return Ok(());
                }
            }
            result = async {
                match submit.as_mut() {
                    Some(handle) => handle.await,
                    None => std::future::pending().await,
                }
            }, if submit.is_some() => {
                result.context("assistant submit task panicked")??;
                submit = None;
            }
        }
    }
}

async fn run_bridge_worker_mode(kind: WorkerKind, worker_id: &str, cwd: PathBuf) -> Result<()> {
    let mut runtime = BridgeWorkerRuntime::new(&cwd)?;
    ensure_worker_state(kind, worker_id, &cwd)?;

    let mut tick = tokio::time::interval(WORKER_HEARTBEAT_INTERVAL);
    loop {
        tick.tick().await;
        if process_state::shutdown_requested() {
            process_state::write_worker_stopped(worker_id, None)?;
            return Ok(());
        }
        process_state::write_worker_heartbeat(worker_id)?;
        let outcome = runtime.poll_once()?;
        if outcome.processed > 0 {
            info!(
                worker_id,
                processed = outcome.processed,
                acknowledged = outcome.acknowledged,
                last_cursor = ?outcome.last_cursor,
                "bridge worker processed work items"
            );
        }
    }
}

async fn run_proactive_worker_mode(kind: WorkerKind, worker_id: &str, cwd: PathBuf) -> Result<()> {
    ensure_worker_state(kind, worker_id, &cwd)?;

    let mut heartbeat = tokio::time::interval(WORKER_HEARTBEAT_INTERVAL);
    let mut proactive_tick =
        tokio::time::interval(Duration::from_millis(super::tick::DEFAULT_TICK_INTERVAL_MS));
    proactive_tick.tick().await;

    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                if process_state::shutdown_requested() {
                    process_state::write_worker_stopped(worker_id, None)?;
                    return Ok(());
                }
                process_state::write_worker_heartbeat(worker_id)?;
            }
            _ = proactive_tick.tick() => {
                if process_state::shutdown_requested() {
                    process_state::write_worker_stopped(worker_id, None)?;
                    return Ok(());
                }
                match super::tick::enqueue_proactive_tick_once(
                    Local::now(),
                    daemon_terminal_focus_for_worker(),
                )? {
                    Some(command) => info!(
                        worker_id,
                        command_id = %command.command_id,
                        "proactive worker queued assistant command"
                    ),
                    None => {
                        tracing::debug!(worker_id, "proactive worker tick skipped");
                    }
                }
            }
        }
    }
}

fn daemon_terminal_focus_for_worker() -> bool {
    process_state::read_terminal_focus_state()
        .ok()
        .flatten()
        .map(|state| state.focused)
        .unwrap_or(false)
}

async fn run_scheduler_worker_mode(kind: WorkerKind, worker_id: &str, cwd: PathBuf) -> Result<()> {
    ensure_worker_state(kind, worker_id, &cwd)?;

    let mut heartbeat = tokio::time::interval(WORKER_HEARTBEAT_INTERVAL);
    let mut scheduler_tick = tokio::time::interval(Duration::from_millis(
        super::scheduler_loop::SCHEDULER_TICK_INTERVAL_MS,
    ));
    scheduler_tick.tick().await;

    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                if process_state::shutdown_requested() {
                    process_state::write_worker_stopped(worker_id, None)?;
                    return Ok(());
                }
                process_state::write_worker_heartbeat(worker_id)?;
            }
            _ = scheduler_tick.tick() => {
                if process_state::shutdown_requested() {
                    process_state::write_worker_stopped(worker_id, None)?;
                    return Ok(());
                }
                if !super::automation_state::autonomous_worker_blocked() {
                    if let Err(err) = super::scheduler_loop::run_kairos_dream_tick_for_date(
                        Local::now().date_naive(),
                    ) {
                        warn!(worker_id, error = %err, "scheduler worker dream tick failed");
                    }
                }
                match super::scheduler_loop::enqueue_due_scheduled_task_once()? {
                    Some(command) => info!(
                        worker_id,
                        command_id = %command.command_id,
                        "scheduler worker queued assistant command"
                    ),
                    None => {
                        tracing::debug!(worker_id, "scheduler worker tick skipped");
                    }
                }
            }
        }
    }
}

fn ensure_worker_state(kind: WorkerKind, worker_id: &str, cwd: &Path) -> Result<()> {
    if process_state::read_worker_state(worker_id)?.is_some() {
        return Ok(());
    }
    let log_path = process_state::worker_log_path(worker_id);
    process_state::write_worker_running(
        worker_id,
        kind.as_str(),
        std::process::id(),
        cwd,
        &log_path,
        0,
        kind == WorkerKind::AssistantSession,
    )?;
    Ok(())
}

pub fn terminate_known_workers() -> Result<()> {
    for worker in process_state::read_worker_states()? {
        if matches!(
            worker.status,
            DaemonWorkerStatus::Running | DaemonWorkerStatus::Starting | DaemonWorkerStatus::Stale
        ) {
            if let Some(pid) = worker.pid {
                let identity =
                    process_state::process_matches_record(pid, worker.process_start_key.as_deref());
                if identity.is_current_process_record() {
                    process_state::terminate_process_tree(pid, worker.process_start_key.as_deref())
                        .with_context(|| {
                            format!("failed to terminate worker {}", worker.worker_id)
                        })?;
                } else {
                    warn!(
                        worker_id = %worker.worker_id,
                        pid,
                        identity = %identity.as_diagnostic(),
                        "skipped known worker termination because pid identity did not match"
                    );
                }
            }
        }
        process_state::write_worker_stopped(&worker.worker_id, None)?;
    }
    Ok(())
}

fn spawn_worker(spec: WorkerSpec, restart_count: u32) -> Result<ManagedWorker> {
    let exe = std::env::current_exe().context("failed to resolve current executable")?;
    if let Some(parent) = spec.log_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let log_file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&spec.log_path)
        .with_context(|| format!("failed to open worker log {}", spec.log_path.display()))?;
    let log_file_err = log_file
        .try_clone()
        .with_context(|| format!("failed to clone worker log {}", spec.log_path.display()))?;

    let mut cmd = Command::new(exe);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    cmd.arg("--daemon-worker")
        .arg(spec.kind.as_str())
        .arg("--worker-id")
        .arg(&spec.worker_id)
        .current_dir(&spec.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log_file))
        .stderr(Stdio::from(log_file_err));
    for (key, value) in &spec.env {
        cmd.env(key, value);
    }

    let mut child = cmd
        .spawn()
        .with_context(|| format!("failed to spawn daemon worker {}", spec.worker_id))?;
    if let Err(err) = process_state::write_worker_running(
        &spec.worker_id,
        spec.kind.as_str(),
        child.id(),
        &spec.cwd,
        &spec.log_path,
        restart_count,
        spec.required,
    ) {
        let start_key = process_state::process_start_key(child.id());
        let _ = process_state::terminate_process_tree(child.id(), start_key.as_deref());
        let _ = child.wait();
        return Err(err)
            .with_context(|| format!("failed to persist daemon worker {} state", spec.worker_id));
    }
    info!(
        worker_id = %spec.worker_id,
        pid = child.id(),
        restart_count,
        "daemon worker started"
    );
    Ok(ManagedWorker {
        spec,
        child,
        restart_count,
    })
}

fn worker_heartbeat_stale(
    worker_id: &str,
    observation: &mut WorkerHeartbeatObservation,
    observed_at: Instant,
) -> Result<bool> {
    let Some(state) = process_state::read_worker_state(worker_id)? else {
        return Ok(false);
    };
    if state.status != DaemonWorkerStatus::Running {
        return Ok(false);
    }
    observation.observe_heartbeat(state.last_heartbeat_at, observed_at);
    Ok(observation.is_stale(observed_at, WORKER_STALE_AFTER))
}

fn exit_status_text(status: ExitStatus) -> String {
    status
        .code()
        .map(|code| format!("exit code {code}"))
        .unwrap_or_else(|| "terminated by signal".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_config::features::{self, FeatureFlags};
    use serial_test::serial;
    use std::time::Instant;

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &Path) -> Self {
            let previous = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(previous) = &self.previous {
                std::env::set_var(self.key, previous);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    struct FeatureOverrideGuard {
        previous: Option<FeatureFlags>,
    }

    impl FeatureOverrideGuard {
        fn set(flags: FeatureFlags) -> Self {
            let previous = features::runtime_override();
            features::set_runtime_override(flags);
            Self { previous }
        }
    }

    impl Drop for FeatureOverrideGuard {
        fn drop(&mut self) {
            match self.previous.take() {
                Some(flags) => features::set_runtime_override(flags),
                None => features::clear_runtime_override(),
            }
        }
    }

    #[test]
    fn heartbeat_staleness_uses_local_monotonic_elapsed_time() {
        let observed_at = Instant::now();
        let persisted_heartbeat = Utc::now() - chrono::Duration::hours(1);
        let observation = WorkerHeartbeatObservation::new(observed_at, Some(persisted_heartbeat));

        assert!(!observation.is_stale(
            observed_at + WORKER_STALE_AFTER - Duration::from_millis(1),
            WORKER_STALE_AFTER,
        ));
        assert!(observation.is_stale(
            observed_at + WORKER_STALE_AFTER + Duration::from_millis(1),
            WORKER_STALE_AFTER,
        ));
    }

    #[test]
    fn continued_heartbeats_keep_long_submit_live_beyond_thirty_seconds() {
        let started_at = Instant::now();
        let wall_clock_start = Utc::now();
        let mut observation = WorkerHeartbeatObservation::new(started_at, Some(wall_clock_start));

        for elapsed_seconds in [9_u64, 18, 27, 36] {
            let now = started_at + Duration::from_secs(elapsed_seconds);
            let persisted = wall_clock_start
                + chrono::Duration::seconds(i64::try_from(elapsed_seconds).unwrap());
            observation.observe_heartbeat(Some(persisted), now);
            assert!(
                !observation.is_stale(now, WORKER_STALE_AFTER),
                "active submit was marked stale after {elapsed_seconds} seconds"
            );
        }

        assert!(observation.is_stale(started_at + Duration::from_secs(47), WORKER_STALE_AFTER,));
    }

    #[test]
    #[serial]
    fn default_worker_spec_uses_daemon_paths() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
        let _features = FeatureOverrideGuard::set(FeatureFlags::all_disabled());
        let specs = default_worker_specs(temp.path());

        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].worker_id, ASSISTANT_WORKER_ID);
        assert_eq!(specs[0].kind, WorkerKind::AssistantSession);
        assert!(specs[0].log_path.starts_with(temp.path()));
        assert!(specs[0].required);
    }

    #[test]
    #[serial]
    fn worker_state_includes_process_record_metadata() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
        let log_path = process_state::worker_log_path("worker-process-record");
        process_state::write_worker_running(
            "worker-process-record",
            WorkerKind::AssistantSession.as_str(),
            std::process::id(),
            temp.path(),
            &log_path,
            0,
            true,
        )
        .expect("worker state");

        let state = process_state::read_worker_state("worker-process-record")
            .expect("read worker")
            .expect("worker exists");
        assert_eq!(
            state.command_kind.as_deref(),
            Some("daemon-worker:assistant-session")
        );
        assert_eq!(
            state.binary_version.as_deref(),
            Some(env!("CARGO_PKG_VERSION"))
        );
        assert!(state.binary_path.is_some());
        assert_eq!(state.log_path, log_path);
        assert!(state.process_start_key.is_some());
    }

    #[test]
    #[serial]
    fn proactive_worker_terminal_focus_defaults_unfocused_without_frontend() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
        process_state::write_started(19836, temp.path()).expect("supervisor state");
        let log_path = process_state::worker_log_path(PROACTIVE_WORKER_ID);
        process_state::write_worker_running(
            PROACTIVE_WORKER_ID,
            WorkerKind::Proactive.as_str(),
            std::process::id(),
            temp.path(),
            &log_path,
            0,
            false,
        )
        .expect("worker state");

        assert!(!daemon_terminal_focus_for_worker());
    }

    #[test]
    #[serial]
    fn proactive_worker_uses_persisted_frontend_focus_for_tick_payload() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
        let _features = FeatureOverrideGuard::set(FeatureFlags {
            proactive: true,
            ..FeatureFlags::all_disabled()
        });
        process_state::write_terminal_focus_state(true).expect("terminal focus state");

        let command = crate::tick::enqueue_proactive_tick_once(
            Local::now(),
            daemon_terminal_focus_for_worker(),
        )
        .unwrap()
        .expect("focused proactive tick should enqueue");

        assert_eq!(command.payload["terminal_focus"], true);
        assert_eq!(command.payload["proactive"]["terminal_focus"], true);
    }

    #[test]
    fn worker_kind_accepts_bridge_sync() {
        assert_eq!(
            WorkerKind::parse("assistant-session").unwrap(),
            WorkerKind::AssistantSession
        );
        assert_eq!(
            WorkerKind::parse("bridge-sync").unwrap(),
            WorkerKind::BridgeSync
        );
        assert!(WorkerKind::parse("proactive").is_ok());
        assert!(WorkerKind::parse("scheduler").is_ok());
        assert!(WorkerKind::parse("unknown").is_err());
    }

    #[test]
    #[serial]
    fn default_worker_specs_include_bridge_sync_when_kairos_enabled() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
        let _features = FeatureOverrideGuard::set(FeatureFlags {
            kairos: true,
            proactive: true,
            ..FeatureFlags::all_disabled()
        });

        let specs = default_worker_specs(temp.path());

        assert!(specs
            .iter()
            .any(|spec| spec.kind == WorkerKind::AssistantSession));
        let bridge = specs
            .iter()
            .find(|spec| spec.kind == WorkerKind::BridgeSync)
            .expect("bridge-sync spec");
        assert_eq!(bridge.worker_id, "bridge-sync-1");
        assert!(bridge.log_path.starts_with(temp.path()));
    }

    #[test]
    #[serial]
    fn default_worker_specs_include_proactive_and_scheduler_when_kairos_enabled() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
        let _features = FeatureOverrideGuard::set(FeatureFlags {
            kairos: true,
            proactive: true,
            ..FeatureFlags::all_disabled()
        });

        let specs = default_worker_specs(temp.path());
        let kinds = specs
            .iter()
            .map(|spec| spec.kind.as_str())
            .collect::<Vec<_>>();

        assert!(kinds.contains(&"assistant-session"));
        assert!(kinds.contains(&"bridge-sync"));
        assert!(kinds.contains(&"proactive"));
        assert!(kinds.contains(&"scheduler"));
        assert!(specs
            .iter()
            .filter(|spec| !spec.required)
            .all(|spec| spec.log_path.starts_with(temp.path())));
    }

    #[test]
    #[serial]
    fn default_worker_specs_include_proactive_without_kairos() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
        let _features = FeatureOverrideGuard::set(FeatureFlags {
            proactive: true,
            ..FeatureFlags::all_disabled()
        });

        let specs = default_worker_specs(temp.path());
        let kinds = specs
            .iter()
            .map(|spec| spec.kind.as_str())
            .collect::<Vec<_>>();

        assert!(kinds.contains(&"assistant-session"));
        assert!(kinds.contains(&"proactive"));
        assert!(!kinds.contains(&"bridge-sync"));
        assert!(!kinds.contains(&"scheduler"));
    }
}
