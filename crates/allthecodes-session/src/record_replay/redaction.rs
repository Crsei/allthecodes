use serde::{Deserialize, Serialize};

use super::config::RecordReplayConfig;
use super::types::RecordItem;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RedactionSummary {
    pub secret_values_redacted: usize,
    pub large_values_elided: usize,
}

#[derive(Debug, Clone)]
pub struct RedactedRecordItem {
    pub item: RecordItem,
    pub summary: RedactionSummary,
}

pub fn redact_record_item(item: RecordItem, config: &RecordReplayConfig) -> RedactedRecordItem {
    if !config.redaction_enabled {
        return RedactedRecordItem {
            item,
            summary: RedactionSummary::default(),
        };
    }

    RedactedRecordItem {
        item,
        summary: RedactionSummary::default(),
    }
}
