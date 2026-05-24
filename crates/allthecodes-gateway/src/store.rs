use crate::events::{RunEvent, RunEventKind};
use crate::run::{idempotency_hash, is_safe_id, GatewayDiagnostic, GatewayError};
use crate::{
    CreateRunOutcome, GatewayPersistence, RunId, RunMeta, RunRequest, RunStatus, SessionKeyPolicy,
};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::path::PathBuf;
use std::time::Duration;

const META_FILE: &str = "meta.json";
const EVENTS_FILE: &str = "events.ndjson";

mod recovery;
pub use recovery::{GatewayRecoveryReport, SessionLock, SessionLockOutcome};

#[derive(Debug, Clone)]
pub struct GatewayStore {
    pub(crate) persistence: GatewayPersistence,
    session_policy: SessionKeyPolicy,
}

impl GatewayStore {
    pub fn new(persistence: GatewayPersistence, session_policy: SessionKeyPolicy) -> Self {
        Self {
            persistence,
            session_policy,
        }
    }

    pub fn default_with_policy(session_policy: SessionKeyPolicy) -> Self {
        Self::new(GatewayPersistence::default(), session_policy)
    }

    pub fn create_run(&self, request: RunRequest) -> Result<CreateRunOutcome, GatewayError> {
        self.ensure_layout()?;

        if let Some(existing) = self.find_idempotent_run(&request)? {
            return Ok(CreateRunOutcome::Existing(existing));
        }

        let run_id = RunId::new();
        let meta = RunMeta::new(run_id.clone(), request.clone(), &self.session_policy);
        if let Some(existing) = self.reserve_idempotency_index(&request, &meta)? {
            return Ok(CreateRunOutcome::Existing(existing));
        }
        let run_dir = self.run_dir(&run_id);
        fs::create_dir_all(&run_dir).map_err(|error| {
            GatewayError::io(
                "store_create_failed",
                "The gateway could not create the run directory.",
                "Check permissions for the allthecodes gateway runs directory.",
                &run_dir,
                &error,
            )
        })?;
        self.write_meta(&meta)?;
        self.append_event(&RunEvent::new(run_id.clone(), 1, RunEventKind::Created))?;
        Ok(CreateRunOutcome::Created(meta))
    }

    pub fn load_run(&self, run_id: &RunId) -> Result<RunMeta, GatewayError> {
        let path = self.meta_path(run_id);
        let file = File::open(&path).map_err(|error| {
            GatewayError::io(
                "run_not_found",
                "The requested gateway run was not found.",
                "Verify the run id or inspect the gateway runs directory.",
                &path,
                &error,
            )
        })?;
        serde_json::from_reader(file).map_err(|error| {
            GatewayError::new(
                GatewayDiagnostic::new(
                    "corrupt_run_meta",
                    "The gateway run metadata could not be decoded.",
                    "Inspect or remove the corrupt run metadata file.",
                )
                .with_context(format!("path={}, error={}", path.display(), error)),
            )
        })
    }

    pub fn update_status(
        &self,
        run_id: &RunId,
        status: RunStatus,
    ) -> Result<RunMeta, GatewayError> {
        let mut meta = self.load_run(run_id)?;
        meta.set_status(status)?;
        self.write_meta(&meta)?;
        let sequence = self.next_sequence(run_id)?;
        self.append_event(&RunEvent::new(
            run_id.clone(),
            sequence,
            RunEventKind::StatusChanged { status },
        ))?;
        Ok(meta)
    }

    pub fn append_event(&self, event: &RunEvent) -> Result<(), GatewayError> {
        let path = self.events_path(&event.run_id);
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|error| {
                GatewayError::io(
                    "event_append_failed",
                    "The gateway could not append to the run event log.",
                    "Check permissions and filesystem state for events.ndjson.",
                    &path,
                    &error,
                )
            })?;

        serde_json::to_writer(&mut file, event).map_err(|error| {
            GatewayError::new(
                GatewayDiagnostic::new(
                    "event_encode_failed",
                    "The gateway could not encode the run event.",
                    "Check the event payload for unsupported values.",
                )
                .with_context(format!("run_id={}, error={}", event.run_id, error)),
            )
        })?;
        file.write_all(b"\n").map_err(|error| {
            GatewayError::io(
                "event_append_failed",
                "The gateway could not finish writing to the run event log.",
                "Check permissions and available disk space for events.ndjson.",
                &path,
                &error,
            )
        })?;
        Ok(())
    }

    pub fn read_events(&self, run_id: &RunId) -> Result<Vec<RunEvent>, GatewayError> {
        let path = self.events_path(run_id);
        let file = File::open(&path).map_err(|error| {
            GatewayError::io(
                "replay_unavailable",
                "The gateway event replay log is unavailable.",
                "Verify the run id and event log file.",
                &path,
                &error,
            )
        })?;

        BufReader::new(file)
            .lines()
            .enumerate()
            .filter_map(|(idx, line)| match line {
                Ok(value) if value.trim().is_empty() => None,
                Ok(value) => Some((idx + 1, value)),
                Err(error) => Some((idx + 1, format!("__io_error__:{error}"))),
            })
            .map(|(line_no, line)| {
                if let Some(error) = line.strip_prefix("__io_error__:") {
                    return Err(GatewayError::new(
                        GatewayDiagnostic::new(
                            "replay_unavailable",
                            "The gateway event replay log could not be read.",
                            "Check filesystem permissions for events.ndjson.",
                        )
                        .with_context(format!(
                            "path={}, line={}, error={}",
                            path.display(),
                            line_no,
                            error
                        )),
                    ));
                }
                serde_json::from_str(&line).map_err(|error| {
                    GatewayError::new(
                        GatewayDiagnostic::new(
                            "corrupt_event_log",
                            "The gateway event replay log contains an invalid event.",
                            "Inspect or repair the corrupt events.ndjson line.",
                        )
                        .with_context(format!(
                            "path={}, line={}, error={}",
                            path.display(),
                            line_no,
                            error
                        )),
                    )
                })
            })
            .collect()
    }

    pub fn run_dir(&self, run_id: &RunId) -> PathBuf {
        self.persistence.runs_dir.join(run_id.as_str())
    }

    pub fn events_path(&self, run_id: &RunId) -> PathBuf {
        self.run_dir(run_id).join(EVENTS_FILE)
    }

    pub(crate) fn ensure_layout(&self) -> Result<(), GatewayError> {
        self.persistence.validate_layout().map_err(|error| {
            GatewayError::new(
                GatewayDiagnostic::new(
                    "store_path_escape",
                    "The gateway persistence paths are outside the gateway root.",
                    "Keep gateway runs, adapters, and webhooks under the allthecodes gateway directory.",
                )
                .with_context(error),
            )
        })?;

        for path in [
            &self.persistence.gateway_dir,
            &self.persistence.runs_dir,
            &self.persistence.adapters_dir,
            &self.persistence.webhooks_dir,
            &self.idempotency_dir(),
            &self.session_locks_dir(),
        ] {
            fs::create_dir_all(path).map_err(|error| {
                GatewayError::io(
                    "store_create_failed",
                    "The gateway could not create its persistence directory.",
                    "Check permissions for the allthecodes gateway directory.",
                    path,
                    &error,
                )
            })?;
        }
        Ok(())
    }

    fn write_meta(&self, meta: &RunMeta) -> Result<(), GatewayError> {
        let path = self.meta_path(&meta.run_id);
        let tmp_path = path.with_extension("json.tmp");
        let file = File::create(&tmp_path).map_err(|error| {
            GatewayError::io(
                "store_write_failed",
                "The gateway could not write run metadata.",
                "Check permissions and available disk space for meta.json.",
                &tmp_path,
                &error,
            )
        })?;
        serde_json::to_writer_pretty(file, meta).map_err(|error| {
            GatewayError::new(
                GatewayDiagnostic::new(
                    "store_encode_failed",
                    "The gateway could not encode run metadata.",
                    "Check the run metadata payload.",
                )
                .with_context(format!(
                    "path={}, error={}",
                    tmp_path.display(),
                    error
                )),
            )
        })?;
        fs::rename(&tmp_path, &path).map_err(|error| {
            GatewayError::io(
                "store_write_failed",
                "The gateway could not publish run metadata.",
                "Check permissions for the run directory.",
                &path,
                &error,
            )
        })
    }

    fn meta_path(&self, run_id: &RunId) -> PathBuf {
        self.run_dir(run_id).join(META_FILE)
    }

    pub(crate) fn next_sequence(&self, run_id: &RunId) -> Result<u64, GatewayError> {
        Ok(self.read_events(run_id)?.len() as u64 + 1)
    }

    fn idempotency_dir(&self) -> PathBuf {
        self.persistence.gateway_dir.join("idempotency")
    }

    pub(crate) fn session_locks_dir(&self) -> PathBuf {
        self.persistence.gateway_dir.join("session-locks")
    }

    fn idempotency_path(&self, request: &RunRequest) -> Option<PathBuf> {
        request.idempotency_key.as_ref().map(|key| {
            self.idempotency_dir()
                .join(format!("{}.json", idempotency_hash(&request.source, key)))
        })
    }

    fn find_idempotent_run(&self, request: &RunRequest) -> Result<Option<RunMeta>, GatewayError> {
        let Some(path) = self.idempotency_path(request) else {
            return Ok(None);
        };
        if !path.exists() {
            return Ok(None);
        }

        let file = File::open(&path).map_err(|error| {
            GatewayError::io(
                "idempotency_lookup_failed",
                "The gateway could not read the idempotency index.",
                "Check permissions for the gateway idempotency directory.",
                &path,
                &error,
            )
        })?;
        let record: IdempotencyRecord = serde_json::from_reader(file).map_err(|error| {
            GatewayError::new(
                GatewayDiagnostic::new(
                    "idempotency_lookup_failed",
                    "The gateway idempotency index is corrupt.",
                    "Inspect or remove the corrupt idempotency index entry.",
                )
                .with_context(format!("path={}, error={}", path.display(), error)),
            )
        })?;

        if !is_safe_id(record.run_id.as_str()) {
            return Err(GatewayError::new(GatewayDiagnostic::new(
                "idempotency_lookup_failed",
                "The gateway idempotency index points to an invalid run id.",
                "Inspect or remove the corrupt idempotency index entry.",
            )));
        }
        self.load_reserved_run(&record.run_id).map(Some)
    }

    fn reserve_idempotency_index(
        &self,
        request: &RunRequest,
        meta: &RunMeta,
    ) -> Result<Option<RunMeta>, GatewayError> {
        let Some(path) = self.idempotency_path(request) else {
            return Ok(None);
        };

        let record = IdempotencyRecord {
            run_id: meta.run_id.clone(),
        };
        let tmp_path = path.with_extension(format!("{}.tmp", meta.run_id.as_str()));
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp_path)
            .map_err(|error| {
                GatewayError::io(
                    "idempotency_write_failed",
                    "The gateway could not write a temporary idempotency index.",
                    "Check permissions for the gateway idempotency directory.",
                    &tmp_path,
                    &error,
                )
            })?;
        serde_json::to_writer_pretty(file, &record).map_err(|error| {
            GatewayError::new(
                GatewayDiagnostic::new(
                    "idempotency_write_failed",
                    "The gateway could not encode the idempotency index.",
                    "Check the idempotency payload.",
                )
                .with_context(format!(
                    "path={}, error={}",
                    tmp_path.display(),
                    error
                )),
            )
        })?;

        match fs::hard_link(&tmp_path, &path) {
            Ok(()) => {
                let _ = fs::remove_file(&tmp_path);
                Ok(None)
            }
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                let _ = fs::remove_file(&tmp_path);
                self.find_idempotent_run(request)
            }
            Err(error) => {
                let _ = fs::remove_file(&tmp_path);
                Err(GatewayError::io(
                    "idempotency_write_failed",
                    "The gateway could not reserve the idempotency index.",
                    "Check permissions for the gateway idempotency directory.",
                    &path,
                    &error,
                ))
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IdempotencyRecord {
    run_id: RunId,
}

impl GatewayStore {
    fn load_reserved_run(&self, run_id: &RunId) -> Result<RunMeta, GatewayError> {
        let mut last_error = None;
        for _ in 0..20 {
            match self.load_run(run_id) {
                Ok(meta) => return Ok(meta),
                Err(error) if error.diagnostic().code == "run_not_found" => {
                    last_error = Some(error);
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => return Err(error),
            }
        }

        Err(last_error.unwrap_or_else(|| {
            GatewayError::new(GatewayDiagnostic::new(
                "idempotency_lookup_failed",
                "The gateway idempotency index could not be resolved.",
                "Retry the request or inspect the gateway idempotency directory.",
            ))
        }))
    }
}
