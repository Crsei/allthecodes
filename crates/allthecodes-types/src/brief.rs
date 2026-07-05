use serde::{Deserialize, Serialize};

pub const BRIEF_TOOL_NAME: &str = "Brief";
pub const SEND_USER_MESSAGE_TOOL_NAME: &str = "SendUserMessage";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BriefMessageStatus {
    Normal,
    Proactive,
}

impl BriefMessageStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Proactive => "proactive",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BriefMessageLevel {
    Info,
    Warning,
    Error,
}

impl BriefMessageLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BriefMessagePayload {
    pub message: String,
    pub status: BriefMessageStatus,
    #[serde(default)]
    pub attachments: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<BriefMessageLevel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_tool_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<i64>,
}

pub fn is_brief_tool_name(tool_name: &str) -> bool {
    matches!(tool_name, BRIEF_TOOL_NAME | SEND_USER_MESSAGE_TOOL_NAME)
}

pub fn brief_payload_from_tool_result(
    tool_name: &str,
    tool_use_id: &str,
    session_id: &str,
    data: &serde_json::Value,
) -> Option<BriefMessagePayload> {
    match tool_name {
        BRIEF_TOOL_NAME => brief_tool_payload(tool_name, tool_use_id, session_id, data),
        SEND_USER_MESSAGE_TOOL_NAME => {
            send_user_message_payload(tool_name, tool_use_id, session_id, data)
        }
        _ => None,
    }
}

pub fn brief_payload_to_json(payload: &BriefMessagePayload) -> serde_json::Value {
    serde_json::to_value(payload).unwrap_or(serde_json::Value::Null)
}

fn brief_tool_payload(
    tool_name: &str,
    tool_use_id: &str,
    session_id: &str,
    data: &serde_json::Value,
) -> Option<BriefMessagePayload> {
    if data
        .get("is_brief_message")
        .and_then(serde_json::Value::as_bool)
        != Some(true)
    {
        return None;
    }

    let message = non_empty_message(data)?;
    let status = match data.get("status").and_then(serde_json::Value::as_str) {
        Some("proactive") => BriefMessageStatus::Proactive,
        Some("normal") | None => BriefMessageStatus::Normal,
        Some(_) => return None,
    };
    let attachments = data
        .get("attachments")
        .and_then(serde_json::Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();

    Some(base_payload(
        tool_name,
        tool_use_id,
        session_id,
        message,
        status,
        attachments,
        None,
    ))
}

fn send_user_message_payload(
    tool_name: &str,
    tool_use_id: &str,
    session_id: &str,
    data: &serde_json::Value,
) -> Option<BriefMessagePayload> {
    let message = non_empty_message(data)?;
    let level = match data.get("level").and_then(serde_json::Value::as_str) {
        Some("warning") => BriefMessageLevel::Warning,
        Some("error") => BriefMessageLevel::Error,
        Some("info") | None => BriefMessageLevel::Info,
        Some(_) => return None,
    };

    Some(base_payload(
        tool_name,
        tool_use_id,
        session_id,
        message,
        BriefMessageStatus::Normal,
        Vec::new(),
        Some(level),
    ))
}

fn non_empty_message(data: &serde_json::Value) -> Option<String> {
    let message = data.get("message")?.as_str()?.trim();
    (!message.is_empty()).then(|| message.to_string())
}

fn base_payload(
    tool_name: &str,
    tool_use_id: &str,
    session_id: &str,
    message: String,
    status: BriefMessageStatus,
    attachments: Vec<String>,
    level: Option<BriefMessageLevel>,
) -> BriefMessagePayload {
    BriefMessagePayload {
        message,
        status,
        attachments,
        level,
        source_tool_name: Some(tool_name.to_string()),
        tool_use_id: Some(tool_use_id.to_string()),
        session_id: Some(session_id.to_string()),
        timestamp: Some(chrono::Utc::now().timestamp_millis()),
    }
}
