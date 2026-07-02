use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use anyhow::{bail, Context, Result};

use super::types::{RecordItem, RecordLine, RECORD_SCHEMA_VERSION};

#[derive(Debug, Clone)]
pub struct ReplayReader {
    pub strict_schema: bool,
    pub strict_session_id: bool,
}

impl Default for ReplayReader {
    fn default() -> Self {
        Self {
            strict_schema: true,
            strict_session_id: false,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ReplayReadResult {
    pub lines: Vec<RecordLine>,
    pub warnings: Vec<ReplayReadWarning>,
}

#[derive(Debug, Clone)]
pub struct ReplayReadWarning {
    pub line_number: usize,
    pub message: String,
    pub raw_line: Option<String>,
}

impl ReplayReader {
    pub fn read_path(&self, path: &Path) -> Result<ReplayReadResult> {
        let file = File::open(path)
            .with_context(|| format!("failed to open rollout log {}", path.display()))?;
        let reader = BufReader::new(file);
        let mut result = ReplayReadResult::default();
        let mut canonical_session_id: Option<String> = None;

        for (index, line) in reader.lines().enumerate() {
            let line_number = index + 1;
            let line =
                line.with_context(|| format!("failed to read rollout line {line_number}"))?;
            if line.trim().is_empty() {
                continue;
            }

            let parsed: RecordLine = match serde_json::from_str(&line) {
                Ok(parsed) => parsed,
                Err(error) => {
                    result.warnings.push(ReplayReadWarning {
                        line_number,
                        message: format!("failed to parse rollout JSON line: {error}"),
                        raw_line: Some(line),
                    });
                    continue;
                }
            };

            if parsed.schema_version > RECORD_SCHEMA_VERSION {
                let message = format!(
                    "unsupported rollout schema version {}",
                    parsed.schema_version
                );
                if self.strict_schema {
                    bail!("{message} at line {line_number}");
                }
                result.warnings.push(ReplayReadWarning {
                    line_number,
                    message,
                    raw_line: None,
                });
                continue;
            }

            if matches!(&parsed.item, RecordItem::SessionMeta(_)) && canonical_session_id.is_none()
            {
                canonical_session_id = Some(parsed.session_id.clone());
            }

            if let Some(canonical) = &canonical_session_id {
                if parsed.session_id != *canonical {
                    let message = format!(
                        "session id {} does not match canonical session id {}",
                        parsed.session_id, canonical
                    );
                    if self.strict_session_id {
                        bail!("{message} at line {line_number}");
                    }
                    result.warnings.push(ReplayReadWarning {
                        line_number,
                        message,
                        raw_line: None,
                    });
                }
            }

            result.lines.push(parsed);
        }

        Ok(result)
    }
}

pub fn read_rollout_file(path: &Path) -> Result<ReplayReadResult> {
    ReplayReader::default().read_path(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record_replay::fixtures::sample_record_lines;
    use crate::record_replay::types::SessionMetaRecord;

    #[test]
    fn reader_loads_valid_jsonl() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("rollout.jsonl");
        let content = sample_record_lines("session-1")
            .into_iter()
            .map(|line| serde_json::to_string(&line).unwrap())
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(&path, format!("{content}\n")).unwrap();

        let result = read_rollout_file(&path).unwrap();

        assert_eq!(result.lines.len(), 3);
        assert!(result.warnings.is_empty());
        assert_eq!(result.lines[0].seq, 0);
    }

    #[test]
    fn bad_lines_are_warnings_not_panics() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("rollout.jsonl");
        let mut lines = sample_record_lines("session-1")
            .into_iter()
            .map(|line| serde_json::to_string(&line).unwrap())
            .collect::<Vec<_>>();
        lines.insert(1, "{".into());
        std::fs::write(&path, format!("{}\n", lines.join("\n"))).unwrap();

        let result = read_rollout_file(&path).unwrap();

        assert_eq!(result.lines.len(), 3);
        assert_eq!(result.warnings.len(), 1);
        assert_eq!(result.warnings[0].line_number, 2);
    }

    #[test]
    fn unsupported_schema_errors_in_strict_mode() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("rollout.jsonl");
        let mut line = sample_record_lines("session-1").remove(0);
        line.schema_version = RECORD_SCHEMA_VERSION + 1;
        std::fs::write(
            &path,
            format!("{}\n", serde_json::to_string(&line).unwrap()),
        )
        .unwrap();

        let error = ReplayReader {
            strict_schema: true,
            strict_session_id: false,
        }
        .read_path(&path)
        .unwrap_err();

        assert!(error.to_string().contains("unsupported rollout schema"));
    }

    #[test]
    fn session_id_inconsistency_generates_warning() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("rollout.jsonl");
        let meta = RecordLine {
            schema_version: RECORD_SCHEMA_VERSION,
            seq: 0,
            timestamp: chrono::Utc::now(),
            session_id: "session-A".into(),
            turn_id: None,
            item: RecordItem::SessionMeta(SessionMetaRecord {
                created_at: chrono::Utc::now(),
                cwd: "/repo".into(),
                workspace_key: None,
                workspace_root: None,
                workspace_name: None,
                model: None,
                config_summary: None,
                parent_session_id: None,
                branch_from_seq: None,
                migrated_from: None,
            }),
        };
        let mismatched = RecordLine {
            schema_version: RECORD_SCHEMA_VERSION,
            seq: 1,
            timestamp: chrono::Utc::now(),
            session_id: "session-B".into(),
            turn_id: None,
            item: RecordItem::TurnStarted(crate::record_replay::types::TurnStartedRecord::default()),
        };

        let lines = vec![
            serde_json::to_string(&meta).unwrap(),
            serde_json::to_string(&mismatched).unwrap(),
        ];
        std::fs::write(&path, format!("{}\n", lines.join("\n"))).unwrap();

        let result = read_rollout_file(&path).unwrap();

        assert_eq!(result.lines.len(), 2);
        assert_eq!(result.warnings.len(), 1);
        assert!(result.warnings[0].message.contains("session id"));
        assert!(result.warnings[0].message.contains("session-B"));
        assert!(result.warnings[0].message.contains("session-A"));
    }
}
