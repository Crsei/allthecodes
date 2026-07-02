use serde::{Deserialize, Serialize};

use super::config::RecordReplayConfig;
use super::types::RecordItem;

const REDACTED_VALUE: &str = "[redacted]";
const LARGE_STRING_CHARS: usize = 8 * 1024;
const LARGE_BLOB_CHARS: usize = 1024;

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

    let original = item.clone();
    let mut value = match serde_json::to_value(&item) {
        Ok(value) => value,
        Err(_) => {
            return RedactedRecordItem {
                item,
                summary: RedactionSummary::default(),
            };
        }
    };
    let mut summary = RedactionSummary::default();
    redact_json_value(&mut value, &mut summary);
    let changed = summary.secret_values_redacted > 0 || summary.large_values_elided > 0;
    let item = if changed {
        serde_json::from_value(value).unwrap_or(original)
    } else {
        item
    };

    RedactedRecordItem { item, summary }
}

fn redact_json_value(value: &mut serde_json::Value, summary: &mut RedactionSummary) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, value) in map.iter_mut() {
                if is_sensitive_key(key) {
                    if !value.is_null() {
                        *value = serde_json::Value::String(REDACTED_VALUE.to_string());
                        summary.secret_values_redacted += 1;
                    }
                } else {
                    redact_json_value(value, summary);
                }
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                redact_json_value(value, summary);
            }
        }
        serde_json::Value::String(text) => redact_string(text, summary),
        _ => {}
    }
}

fn redact_string(text: &mut String, summary: &mut RedactionSummary) {
    if looks_like_inline_secret(text) {
        *text = REDACTED_VALUE.to_string();
        summary.secret_values_redacted += 1;
    } else if text.chars().count() > LARGE_STRING_CHARS || looks_like_large_blob(text) {
        let len = text.chars().count();
        *text = format!("[elided {len} chars]");
        summary.large_values_elided += 1;
    }
}

fn is_sensitive_key(key: &str) -> bool {
    let normalized = key
        .chars()
        .filter(|ch| !matches!(ch, '_' | '-' | ' '))
        .collect::<String>()
        .to_ascii_lowercase();
    matches!(
        normalized.as_str(),
        "authorization"
            | "cookie"
            | "setcookie"
            | "apikey"
            | "token"
            | "authtoken"
            | "accesstoken"
            | "refreshtoken"
            | "secret"
            | "clientsecret"
            | "password"
            | "passwd"
            | "privatekey"
    ) || normalized.contains("apikey")
        || normalized.contains("accesstoken")
        || normalized.contains("refreshtoken")
        || normalized.contains("authtoken")
        || normalized.contains("clientsecret")
        || normalized.contains("privatekey")
}

fn looks_like_inline_secret(text: &str) -> bool {
    if text.len() < 24 {
        return false;
    }
    let lower = text.to_ascii_lowercase();
    lower.contains("authorization:") || lower.contains("bearer ")
}

fn looks_like_large_blob(text: &str) -> bool {
    if text.len() < LARGE_BLOB_CHARS || text.chars().any(char::is_whitespace) {
        return false;
    }
    let base64ish = text
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '/' | '=' | '-' | '_'))
        .count();
    base64ish * 100 / text.chars().count().max(1) >= 90
}

#[cfg(test)]
mod tests {
    use super::super::types::{PermissionRequestRecord, ToolProgressRecord};
    use super::*;

    #[test]
    fn redacts_secret_keys_inside_record_item() {
        let item = RecordItem::PermissionRequest(PermissionRequestRecord {
            request_id: "req-1".to_string(),
            tool_name: "WebFetch".to_string(),
            context: Some(serde_json::json!({
                "headers": {
                    "authorization": "Bearer sk-secret-token-value",
                    "x-api-key": "sk-test",
                },
                "url": "https://example.test",
            })),
        });

        let redacted = redact_record_item(item, &RecordReplayConfig::default());

        assert_eq!(redacted.summary.secret_values_redacted, 2);
        match redacted.item {
            RecordItem::PermissionRequest(record) => {
                let context = record.context.unwrap();
                assert_eq!(context["headers"]["authorization"], REDACTED_VALUE);
                assert_eq!(context["headers"]["x-api-key"], REDACTED_VALUE);
                assert_eq!(context["url"], "https://example.test");
            }
            other => panic!("unexpected item: {other:?}"),
        }
    }

    #[test]
    fn elides_large_string_values() {
        let blob = "a".repeat(LARGE_STRING_CHARS + 1);
        let item = RecordItem::ToolProgress(ToolProgressRecord {
            tool_use_id: "toolu-1".to_string(),
            data: serde_json::json!({ "payload": blob }),
        });

        let redacted = redact_record_item(item, &RecordReplayConfig::default());

        assert_eq!(redacted.summary.large_values_elided, 1);
        match redacted.item {
            RecordItem::ToolProgress(record) => {
                assert_eq!(
                    record.data["payload"],
                    format!("[elided {} chars]", LARGE_STRING_CHARS + 1)
                );
            }
            other => panic!("unexpected item: {other:?}"),
        }
    }
}
