use super::config::RecordReplayConfig;
use super::types::{QueryEventRecord, RecordItem};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordClass {
    Canonical,
    Diagnostic,
    Ephemeral,
}

pub fn classify_record_item(item: &RecordItem, _config: &RecordReplayConfig) -> RecordClass {
    match item {
        RecordItem::QueryEvent(QueryEventRecord::RawStream { .. }) => RecordClass::Diagnostic,
        RecordItem::ToolProgress(_) => RecordClass::Diagnostic,
        _ => RecordClass::Canonical,
    }
}

pub fn should_persist(item: &RecordItem, config: &RecordReplayConfig) -> bool {
    if !config.enabled {
        return false;
    }

    match classify_record_item(item, config) {
        RecordClass::Canonical => true,
        RecordClass::Diagnostic => diagnostic_enabled(item, config),
        RecordClass::Ephemeral => false,
    }
}

pub fn may_drop_under_pressure(item: &RecordItem, config: &RecordReplayConfig) -> bool {
    !matches!(classify_record_item(item, config), RecordClass::Canonical)
}

fn diagnostic_enabled(item: &RecordItem, config: &RecordReplayConfig) -> bool {
    match item {
        RecordItem::QueryEvent(QueryEventRecord::RawStream { .. }) => config.include_raw_stream,
        RecordItem::ToolProgress(_) => config.include_tool_progress,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record_replay::types::{
        QueryEventRecord, ToolProgressRecord, TurnStartedRecord,
    };

    #[test]
    fn canonical_items_persist_by_default() {
        let config = RecordReplayConfig::default();
        let item = RecordItem::TurnStarted(TurnStartedRecord {
            user_message_uuid: Some("u1".into()),
            input_summary: None,
        });

        assert_eq!(classify_record_item(&item, &config), RecordClass::Canonical);
        assert!(should_persist(&item, &config));
        assert!(!may_drop_under_pressure(&item, &config));
    }

    #[test]
    fn diagnostic_items_require_matching_flag() {
        let item = RecordItem::ToolProgress(ToolProgressRecord {
            tool_use_id: "toolu_1".into(),
            data: serde_json::json!({ "step": "running" }),
        });
        let mut config = RecordReplayConfig::default();

        assert_eq!(
            classify_record_item(&item, &config),
            RecordClass::Diagnostic
        );
        assert!(!should_persist(&item, &config));
        assert!(may_drop_under_pressure(&item, &config));

        config.include_tool_progress = true;
        assert!(should_persist(&item, &config));
    }

    #[test]
    fn disabled_config_persists_nothing() {
        let config = RecordReplayConfig {
            enabled: false,
            ..RecordReplayConfig::default()
        };
        let item = RecordItem::TurnStarted(TurnStartedRecord::default());

        assert!(!should_persist(&item, &config));
    }

    #[test]
    fn diagnostic_returns_false_for_raw_stream_when_flag_off() {
        let config = RecordReplayConfig {
            enabled: true,
            include_raw_stream: false,
            ..RecordReplayConfig::default()
        };
        let item = RecordItem::QueryEvent(QueryEventRecord::RawStream {
            event: serde_json::json!({ "delta": "x" }),
        });

        assert_eq!(
            classify_record_item(&item, &config),
            RecordClass::Diagnostic
        );
        assert!(!should_persist(&item, &config));
    }
}
