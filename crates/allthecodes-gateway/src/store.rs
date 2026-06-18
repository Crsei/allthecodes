use crate::events::{RunEvent, RunEventKind};
use crate::run::{idempotency_hash, is_safe_id, GatewayDiagnostic, GatewayError};
use crate::{
    CreateRunOutcome, GatewayPersistence, RunId, RunMeta, RunRequest, RunStatus, SessionKeyPolicy,
};
use allthecodes_types::output::{
    EventSeq, OutputEvent, OutputLifecycleState, OutputReadBatch, OutputStream,
};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::path::PathBuf;
use std::time::Duration;

const META_FILE: &str = "meta.json";
const EVENTS_FILE: &str = "events.ndjson";
const OUTPUT_EVENTS_FILE: &str = "output.events.ndjson";
const TIMELINE_FILE: &str = "timeline.ndjson";

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
        self.append_timeline_event(&RunEvent::new(run_id.clone(), 1, RunEventKind::Created))?;
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
        self.append_timeline_event(&RunEvent::new(
            run_id.clone(),
            sequence,
            RunEventKind::StatusChanged { status },
        ))?;
        Ok(meta)
    }

    pub fn append_event(&self, event: &RunEvent) -> Result<(), GatewayError> {
        self.ensure_migrated(&event.run_id)?;
        match &event.kind {
            RunEventKind::AssistantDelta { text } => {
                self.append_output_chunk(&event.run_id, text, event.timestamp_ms)
            }
            _ => self.append_timeline_event(event),
        }
    }

    pub fn append_output_chunk(
        &self,
        run_id: &RunId,
        chunk: &str,
        timestamp_ms: u128,
    ) -> Result<(), GatewayError> {
        self.ensure_migrated(run_id)?;
        let events = self.read_output_events_raw(run_id)?;
        let seq = events
            .last()
            .map(|event| event.seq.saturating_add(1))
            .unwrap_or(1);
        let event = OutputEvent {
            seq,
            stream: OutputStream::Stdout,
            chunk: chunk.to_string(),
            timestamp_ms: u128_to_u64(timestamp_ms),
            process_or_run_id: run_id.to_string(),
        };
        self.append_output_event(run_id, &event)
    }

    pub fn read_output_events(
        &self,
        run_id: &RunId,
        after_seq: Option<EventSeq>,
        limit_bytes: usize,
    ) -> Result<OutputReadBatch, GatewayError> {
        self.ensure_migrated(run_id)?;
        let meta = self.load_run(run_id)?;
        let events = self.read_output_events_raw(run_id)?;
        Ok(output_read_batch_from_events(
            events,
            after_seq,
            limit_bytes.max(1),
            output_state_for_run(meta.status),
        ))
    }

    pub fn read_timeline_events(
        &self,
        run_id: &RunId,
        after_sequence: Option<u64>,
        limit: usize,
    ) -> Result<Vec<RunEvent>, GatewayError> {
        let mut events = self.read_events(run_id)?;
        if let Some(after_sequence) = after_sequence {
            events.retain(|event| event.sequence > after_sequence);
        }
        events.truncate(limit);
        Ok(events)
    }

    fn append_timeline_event(&self, event: &RunEvent) -> Result<(), GatewayError> {
        let path = self.timeline_path(&event.run_id);
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|error| {
                GatewayError::io(
                    "event_append_failed",
                    "The gateway could not append to the run timeline log.",
                    "Check permissions and filesystem state for timeline.ndjson.",
                    &path,
                    &error,
                )
            })?;

        serde_json::to_writer(&mut file, event).map_err(|error| {
            GatewayError::new(
                GatewayDiagnostic::new(
                    "event_encode_failed",
                    "The gateway could not encode the run timeline event.",
                    "Check the event payload for unsupported values.",
                )
                .with_context(format!("run_id={}, error={}", event.run_id, error)),
            )
        })?;
        file.write_all(b"\n").map_err(|error| {
            GatewayError::io(
                "event_append_failed",
                "The gateway could not finish writing to the run timeline log.",
                "Check permissions and available disk space for timeline.ndjson.",
                &path,
                &error,
            )
        })?;
        Ok(())
    }

    pub fn read_events(&self, run_id: &RunId) -> Result<Vec<RunEvent>, GatewayError> {
        self.ensure_migrated(run_id)?;
        self.read_timeline_events_raw(run_id)
    }

    pub fn run_dir(&self, run_id: &RunId) -> PathBuf {
        self.persistence.runs_dir.join(run_id.as_str())
    }

    pub fn events_path(&self, run_id: &RunId) -> PathBuf {
        self.run_dir(run_id).join(EVENTS_FILE)
    }

    pub fn output_events_path(&self, run_id: &RunId) -> PathBuf {
        self.run_dir(run_id).join(OUTPUT_EVENTS_FILE)
    }

    pub fn timeline_path(&self, run_id: &RunId) -> PathBuf {
        self.run_dir(run_id).join(TIMELINE_FILE)
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

    fn ensure_migrated(&self, run_id: &RunId) -> Result<(), GatewayError> {
        let legacy_path = self.events_path(run_id);
        let output_path = self.output_events_path(run_id);
        let timeline_path = self.timeline_path(run_id);
        if output_path.exists() || timeline_path.exists() || !legacy_path.exists() {
            return Ok(());
        }

        let legacy_events = self.read_legacy_events(run_id)?;
        let mut output_events = Vec::new();
        let mut timeline_events = Vec::new();
        for event in legacy_events {
            match event.kind {
                RunEventKind::AssistantDelta { text } => {
                    output_events.push(OutputEvent {
                        seq: output_events.len() as u64 + 1,
                        stream: OutputStream::Stdout,
                        chunk: text,
                        timestamp_ms: u128_to_u64(event.timestamp_ms),
                        process_or_run_id: run_id.to_string(),
                    });
                }
                kind => timeline_events.push(RunEvent {
                    run_id: event.run_id,
                    sequence: event.sequence,
                    timestamp_ms: event.timestamp_ms,
                    kind,
                }),
            }
        }
        self.write_output_events(run_id, &output_events)?;
        self.write_timeline_events(run_id, &timeline_events)?;
        Ok(())
    }

    fn read_legacy_events(&self, run_id: &RunId) -> Result<Vec<RunEvent>, GatewayError> {
        self.read_run_events_file(self.events_path(run_id), EVENTS_FILE)
    }

    fn read_timeline_events_raw(&self, run_id: &RunId) -> Result<Vec<RunEvent>, GatewayError> {
        self.read_run_events_file(self.timeline_path(run_id), TIMELINE_FILE)
    }

    fn read_run_events_file(
        &self,
        path: PathBuf,
        label: &'static str,
    ) -> Result<Vec<RunEvent>, GatewayError> {
        let file = match File::open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => {
                return Err(GatewayError::io(
                    "replay_unavailable",
                    "The gateway event replay log is unavailable.",
                    "Verify the run id and event log file.",
                    &path,
                    &error,
                ));
            }
        };

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
                            format!("Check filesystem permissions for {label}."),
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
                            format!("Inspect or repair the corrupt {label} line."),
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

    fn append_output_event(&self, run_id: &RunId, event: &OutputEvent) -> Result<(), GatewayError> {
        let path = self.output_events_path(run_id);
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|error| {
                GatewayError::io(
                    "output_append_failed",
                    "The gateway could not append to the run output log.",
                    "Check permissions and filesystem state for output.events.ndjson.",
                    &path,
                    &error,
                )
            })?;
        serde_json::to_writer(&mut file, event).map_err(|error| {
            GatewayError::new(
                GatewayDiagnostic::new(
                    "output_encode_failed",
                    "The gateway could not encode the run output event.",
                    "Check the output payload for unsupported values.",
                )
                .with_context(format!("run_id={}, error={}", run_id, error)),
            )
        })?;
        file.write_all(b"\n").map_err(|error| {
            GatewayError::io(
                "output_append_failed",
                "The gateway could not finish writing to the run output log.",
                "Check permissions and available disk space for output.events.ndjson.",
                &path,
                &error,
            )
        })
    }

    fn read_output_events_raw(&self, run_id: &RunId) -> Result<Vec<OutputEvent>, GatewayError> {
        let path = self.output_events_path(run_id);
        let file = match File::open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => {
                return Err(GatewayError::io(
                    "output_replay_unavailable",
                    "The gateway output replay log is unavailable.",
                    "Verify the run id and output event log file.",
                    &path,
                    &error,
                ));
            }
        };

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
                            "output_replay_unavailable",
                            "The gateway output replay log could not be read.",
                            "Check filesystem permissions for output.events.ndjson.",
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
                            "corrupt_output_log",
                            "The gateway output replay log contains an invalid event.",
                            "Inspect or repair the corrupt output.events.ndjson line.",
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

    fn write_output_events(
        &self,
        run_id: &RunId,
        events: &[OutputEvent],
    ) -> Result<(), GatewayError> {
        write_ndjson_file(
            &self.output_events_path(run_id),
            events,
            "output_import_failed",
        )
    }

    fn write_timeline_events(
        &self,
        run_id: &RunId,
        events: &[RunEvent],
    ) -> Result<(), GatewayError> {
        write_ndjson_file(
            &self.timeline_path(run_id),
            events,
            "timeline_import_failed",
        )
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

fn write_ndjson_file<T: Serialize>(
    path: &PathBuf,
    events: &[T],
    code: &'static str,
) -> Result<(), GatewayError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            GatewayError::io(
                code,
                "The gateway could not create the run log directory.",
                "Check permissions for the gateway runs directory.",
                parent,
                &error,
            )
        })?;
    }
    let tmp_path = path.with_extension("ndjson.tmp");
    let mut file = File::create(&tmp_path).map_err(|error| {
        GatewayError::io(
            code,
            "The gateway could not write a temporary run log.",
            "Check permissions and available disk space for the gateway run directory.",
            &tmp_path,
            &error,
        )
    })?;
    for event in events {
        serde_json::to_writer(&mut file, event).map_err(|error| {
            GatewayError::new(
                GatewayDiagnostic::new(
                    code,
                    "The gateway could not encode an imported run log event.",
                    "Inspect the legacy gateway event payload.",
                )
                .with_context(format!(
                    "path={}, error={}",
                    tmp_path.display(),
                    error
                )),
            )
        })?;
        file.write_all(b"\n").map_err(|error| {
            GatewayError::io(
                code,
                "The gateway could not finish writing an imported run log.",
                "Check permissions and available disk space for the gateway run directory.",
                &tmp_path,
                &error,
            )
        })?;
    }
    fs::rename(&tmp_path, path).map_err(|error| {
        GatewayError::io(
            code,
            "The gateway could not publish the imported run log.",
            "Check permissions for the gateway run directory.",
            path,
            &error,
        )
    })
}

fn output_state_for_run(status: RunStatus) -> OutputLifecycleState {
    match status {
        RunStatus::Queued => OutputLifecycleState::Starting,
        RunStatus::Running | RunStatus::WaitingApproval | RunStatus::WaitingUser => {
            OutputLifecycleState::Running
        }
        RunStatus::Recoverable => OutputLifecycleState::Running,
        RunStatus::Completed | RunStatus::Cancelled => OutputLifecycleState::Exited,
        RunStatus::Failed => OutputLifecycleState::Failed,
    }
}

fn output_read_batch_from_events(
    events: Vec<OutputEvent>,
    after_seq: Option<EventSeq>,
    limit_bytes: usize,
    state: OutputLifecycleState,
) -> OutputReadBatch {
    let first_available_seq = events.first().map(|event| event.seq).unwrap_or(1);
    let latest_seq = events.last().map(|event| event.seq).unwrap_or(0);
    let requested_after_seq = after_seq.unwrap_or(first_available_seq.saturating_sub(1));
    let mut truncated = requested_after_seq.saturating_add(1) < first_available_seq;
    let mut bytes = 0usize;
    let mut retained = Vec::new();

    for event in events
        .iter()
        .filter(|event| event.seq > requested_after_seq)
    {
        let chunk_len = event.chunk.len();
        if !retained.is_empty() && bytes.saturating_add(chunk_len) > limit_bytes {
            truncated = true;
            break;
        }
        bytes = bytes.saturating_add(chunk_len);
        retained.push(event.clone());
        if bytes >= limit_bytes {
            truncated = events.iter().any(|candidate| candidate.seq > event.seq);
            break;
        }
    }

    let next_seq = retained
        .last()
        .map(|event| event.seq.saturating_add(1))
        .unwrap_or_else(|| latest_seq.saturating_add(1))
        .max(requested_after_seq.saturating_add(1));

    OutputReadBatch {
        events: retained,
        next_seq,
        truncated,
        first_available_seq,
        state,
    }
}

fn u128_to_u64(value: u128) -> u64 {
    value.min(u64::MAX as u128) as u64
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RemoteSource, RemoteTransport, RunPolicy};
    use serde_json::json;

    fn temp_store(temp: &tempfile::TempDir) -> GatewayStore {
        let gateway_dir = temp.path().join("gateway");
        GatewayStore::new(
            GatewayPersistence {
                runs_dir: gateway_dir.join("runs"),
                adapters_dir: gateway_dir.join("adapters"),
                webhooks_dir: gateway_dir.join("webhooks"),
                gateway_dir,
            },
            SessionKeyPolicy::default(),
        )
    }

    fn request() -> RunRequest {
        RunRequest {
            prompt: "hello".to_string(),
            source: RemoteSource::new(
                RemoteTransport::Local,
                "local",
                "/workspace",
                "client",
                "user",
                "thread",
            ),
            policy: RunPolicy::default(),
            idempotency_key: None,
        }
    }

    fn write_legacy_events(store: &GatewayStore, run_id: &RunId, events: &[RunEvent]) {
        let path = store.events_path(run_id);
        let mut file = File::create(path).unwrap();
        for event in events {
            serde_json::to_writer(&mut file, event).unwrap();
            file.write_all(b"\n").unwrap();
        }
    }

    #[test]
    fn created_run_writes_timeline_event() {
        let temp = tempfile::tempdir().unwrap();
        let store = temp_store(&temp);
        let run_id = store.create_run(request()).unwrap().meta().run_id.clone();

        let timeline = store.read_events(&run_id).unwrap();
        assert!(matches!(timeline[0].kind, RunEventKind::Created));
        let output = store.read_output_events(&run_id, None, 1024).unwrap();
        assert!(output.events.is_empty());
        assert_eq!(output.state, OutputLifecycleState::Starting);
    }

    #[test]
    fn assistant_delta_appends_output_not_timeline() {
        let temp = tempfile::tempdir().unwrap();
        let store = temp_store(&temp);
        let run_id = store.create_run(request()).unwrap().meta().run_id.clone();

        store
            .append_event(&RunEvent::new(
                run_id.clone(),
                2,
                RunEventKind::AssistantDelta {
                    text: "partial".to_string(),
                },
            ))
            .unwrap();

        let output = store.read_output_events(&run_id, None, 1024).unwrap();
        assert_eq!(output.events.len(), 1);
        assert_eq!(output.events[0].seq, 1);
        assert_eq!(output.events[0].chunk, "partial");
        let timeline = store.read_events(&run_id).unwrap();
        assert!(timeline
            .iter()
            .all(|event| !matches!(event.kind, RunEventKind::AssistantDelta { .. })));
    }

    #[test]
    fn legacy_events_are_lazily_split_into_output_and_timeline() {
        let temp = tempfile::tempdir().unwrap();
        let store = temp_store(&temp);
        let run_id = store.create_run(request()).unwrap().meta().run_id.clone();
        fs::remove_file(store.timeline_path(&run_id)).unwrap();
        write_legacy_events(
            &store,
            &run_id,
            &[
                RunEvent::new(run_id.clone(), 1, RunEventKind::Created),
                RunEvent::new(
                    run_id.clone(),
                    2,
                    RunEventKind::AssistantDelta {
                        text: "legacy text".to_string(),
                    },
                ),
                RunEvent::new(
                    run_id.clone(),
                    3,
                    RunEventKind::Custom {
                        name: "diagnostic".to_string(),
                        payload: json!({"ok": true}),
                    },
                ),
            ],
        );

        let output = store.read_output_events(&run_id, None, 1024).unwrap();
        assert_eq!(output.events.len(), 1);
        assert_eq!(output.events[0].chunk, "legacy text");
        let timeline = store.read_events(&run_id).unwrap();
        assert_eq!(timeline.len(), 2);
        assert!(matches!(timeline[0].kind, RunEventKind::Created));
        assert!(matches!(timeline[1].kind, RunEventKind::Custom { .. }));
        assert!(store.events_path(&run_id).exists());
        assert!(store.output_events_path(&run_id).exists());
        assert!(store.timeline_path(&run_id).exists());
    }

    #[test]
    fn output_read_honors_limit_bytes() {
        let temp = tempfile::tempdir().unwrap();
        let store = temp_store(&temp);
        let run_id = store.create_run(request()).unwrap().meta().run_id.clone();
        store.append_output_chunk(&run_id, "abc", 10).unwrap();
        store.append_output_chunk(&run_id, "def", 11).unwrap();

        let output = store.read_output_events(&run_id, Some(0), 3).unwrap();
        assert_eq!(output.events.len(), 1);
        assert_eq!(output.events[0].chunk, "abc");
        assert_eq!(output.next_seq, 2);
        assert!(output.truncated);
    }
}
