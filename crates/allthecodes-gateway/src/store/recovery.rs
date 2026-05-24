use super::GatewayStore;
use crate::events::{RunEvent, RunEventKind};
use crate::run::{GatewayDiagnostic, GatewayError};
use crate::{RunId, RunMeta, RunStatus};
use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::fs::{self, File};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GatewayRecoveryReport {
    pub queued: usize,
    pub recoverable: usize,
    pub pending: usize,
    pub terminal: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionLock {
    pub session_key: String,
    pub owner_pid: u32,
    pub heartbeat_ms: u128,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionLockOutcome {
    Acquired(SessionLock),
    Recovered(SessionLock),
    Busy(SessionLock),
}

impl GatewayStore {
    pub fn list_runs(&self) -> Result<Vec<RunMeta>, GatewayError> {
        self.ensure_layout()?;
        let mut runs = Vec::new();
        for entry in fs::read_dir(&self.persistence.runs_dir).map_err(|error| {
            GatewayError::io(
                "store_read_failed",
                "The gateway could not read the runs directory.",
                "Check permissions for the allthecodes gateway runs directory.",
                &self.persistence.runs_dir,
                &error,
            )
        })? {
            let entry = entry.map_err(|error| {
                GatewayError::io(
                    "store_read_failed",
                    "The gateway could not read a run directory entry.",
                    "Check permissions for the allthecodes gateway runs directory.",
                    &self.persistence.runs_dir,
                    &error,
                )
            })?;
            if !entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
                continue;
            }
            let run_id = RunId::from_string(entry.file_name().to_string_lossy().to_string())?;
            runs.push(self.load_run(&run_id)?);
        }
        runs.sort_by_key(|meta| (meta.created_at_ms, meta.run_id.clone()));
        Ok(runs)
    }

    pub fn queued_runs(&self) -> Result<Vec<RunMeta>, GatewayError> {
        Ok(self
            .list_runs()?
            .into_iter()
            .filter(|meta| meta.status == RunStatus::Queued)
            .collect())
    }

    pub fn recover_on_startup(
        &self,
        pending_timeout: Duration,
    ) -> Result<GatewayRecoveryReport, GatewayError> {
        let mut report = GatewayRecoveryReport::default();
        let expires_at_ms = crate::run::now_millis() + pending_timeout.as_millis();
        for meta in self.list_runs()? {
            match meta.status {
                RunStatus::Queued => {
                    report.queued += 1;
                    self.append_recovery_event(
                        &meta.run_id,
                        "run_requeued_after_restart",
                        serde_json::json!({}),
                    )?;
                }
                RunStatus::Running => {
                    if self.has_terminal_status_event(&meta.run_id)? {
                        report.terminal += 1;
                    } else {
                        self.append_recovery_diagnostic(&meta.run_id)?;
                        self.update_status(&meta.run_id, RunStatus::Recoverable)?;
                        report.recoverable += 1;
                    }
                }
                RunStatus::WaitingApproval | RunStatus::WaitingUser => {
                    report.pending += 1;
                    self.append_recovery_event(
                        &meta.run_id,
                        "pending_response_retained_after_restart",
                        serde_json::json!({ "expiresAtMs": expires_at_ms }),
                    )?;
                }
                RunStatus::Recoverable => {
                    report.recoverable += 1;
                }
                RunStatus::Completed | RunStatus::Failed | RunStatus::Cancelled => {
                    report.terminal += 1;
                }
            }
        }
        Ok(report)
    }

    pub fn acquire_session_lock(
        &self,
        meta: &RunMeta,
        owner_pid: u32,
        stale_after: Duration,
    ) -> Result<SessionLockOutcome, GatewayError> {
        self.ensure_layout()?;
        let path = self.session_lock_path(&meta.session_key.to_string());
        let now = crate::run::now_millis();
        let next = SessionLock {
            session_key: meta.session_key.to_string(),
            owner_pid,
            heartbeat_ms: now,
        };

        if let Some(existing) = self.read_session_lock(&path)? {
            let stale = now.saturating_sub(existing.heartbeat_ms) > stale_after.as_millis();
            if existing.owner_pid != owner_pid && !stale {
                return Ok(SessionLockOutcome::Busy(existing));
            }
            self.write_session_lock(&path, &next)?;
            if existing.owner_pid != owner_pid && stale {
                self.append_event(&RunEvent::new(
                    meta.run_id.clone(),
                    self.next_sequence(&meta.run_id)?,
                    RunEventKind::SessionLockRecovered {
                        previous_owner_pid: existing.owner_pid,
                        new_owner_pid: owner_pid,
                    },
                ))?;
                return Ok(SessionLockOutcome::Recovered(next));
            }
            return Ok(SessionLockOutcome::Acquired(next));
        }

        self.write_session_lock(&path, &next)?;
        Ok(SessionLockOutcome::Acquired(next))
    }

    fn append_recovery_event(
        &self,
        run_id: &RunId,
        name: &str,
        payload: serde_json::Value,
    ) -> Result<(), GatewayError> {
        self.append_event(&RunEvent::new(
            run_id.clone(),
            self.next_sequence(run_id)?,
            RunEventKind::Custom {
                name: name.to_string(),
                payload,
            },
        ))
    }

    fn append_recovery_diagnostic(&self, run_id: &RunId) -> Result<(), GatewayError> {
        self.append_event(&RunEvent::new(
            run_id.clone(),
            self.next_sequence(run_id)?,
            RunEventKind::Diagnostic {
                diagnostic: GatewayDiagnostic::new(
                    "run_recovered_after_restart",
                    "The daemon restarted while the run was active.",
                    "Inspect durable events and retry or cancel the recoverable run.",
                )
                .with_context(format!("run_id={}", run_id)),
            },
        ))
    }

    fn has_terminal_status_event(&self, run_id: &RunId) -> Result<bool, GatewayError> {
        Ok(self.read_events(run_id)?.iter().any(|event| {
            matches!(
                event.kind,
                RunEventKind::StatusChanged { status } if status.is_terminal()
            )
        }))
    }

    fn session_lock_path(&self, session_key: &str) -> PathBuf {
        let digest = sha2::Sha256::digest(session_key.as_bytes());
        self.session_locks_dir()
            .join(format!("{}.json", hex::encode(digest)))
    }

    fn read_session_lock(&self, path: &PathBuf) -> Result<Option<SessionLock>, GatewayError> {
        if !path.exists() {
            return Ok(None);
        }
        let file = File::open(path).map_err(|error| {
            GatewayError::io(
                "session_lock_read_failed",
                "The gateway could not read the session lock.",
                "Check permissions for the gateway session-locks directory.",
                path,
                &error,
            )
        })?;
        serde_json::from_reader(file).map(Some).map_err(|error| {
            GatewayError::new(
                GatewayDiagnostic::new(
                    "session_lock_corrupt",
                    "The gateway session lock could not be decoded.",
                    "Inspect or remove the corrupt session lock.",
                )
                .with_context(format!("path={}, error={}", path.display(), error)),
            )
        })
    }

    fn write_session_lock(&self, path: &PathBuf, lock: &SessionLock) -> Result<(), GatewayError> {
        let tmp_path = path.with_extension(format!("{}.tmp", std::process::id()));
        let file = File::create(&tmp_path).map_err(|error| {
            GatewayError::io(
                "session_lock_write_failed",
                "The gateway could not write the session lock.",
                "Check permissions for the gateway session-locks directory.",
                &tmp_path,
                &error,
            )
        })?;
        serde_json::to_writer_pretty(file, lock).map_err(|error| {
            GatewayError::new(
                GatewayDiagnostic::new(
                    "session_lock_write_failed",
                    "The gateway could not encode the session lock.",
                    "Check the session lock payload.",
                )
                .with_context(format!(
                    "path={}, error={}",
                    tmp_path.display(),
                    error
                )),
            )
        })?;
        fs::rename(&tmp_path, path).map_err(|error| {
            GatewayError::io(
                "session_lock_write_failed",
                "The gateway could not publish the session lock.",
                "Check permissions for the gateway session-locks directory.",
                path,
                &error,
            )
        })
    }
}
