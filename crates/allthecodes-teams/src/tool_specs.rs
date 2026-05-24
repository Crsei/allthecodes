//! Tool-facing schemas and input helpers for Agent Teams.
//!
//! Runtime execution still belongs to the binary until the rest of the team
//! runtime is moved, but the conversation-facing contract lives with
//! `cc-teams` so the root tool wrappers stay thin.

use serde::Deserialize;
use serde_json::{json, Value};

pub const SEND_MESSAGE_TOOL_NAME: &str = "SendMessage";
pub const TEAM_SPAWN_TOOL_NAME: &str = "TeamSpawn";
pub const TEAM_LEAD_NAME: &str = "team-lead";

#[derive(Debug, Deserialize)]
pub struct SendMessageInput {
    /// Recipient: teammate name, "*" for broadcast.
    pub to: String,
    /// The message text or structured content.
    pub message: String,
    /// Optional 5-10 word summary.
    #[serde(default)]
    pub summary: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TeamSpawnInput {
    /// Unique teammate name (used as mailbox name + agent id).
    pub name: String,
    /// Initial prompt passed to the teammate's QueryEngine.
    pub prompt: String,
    /// Optional model override (defaults to parent model).
    #[serde(default)]
    pub model: Option<String>,
    /// Optional UI color.
    #[serde(default)]
    pub color: Option<String>,
    /// Optional team name.
    #[serde(default)]
    pub team: Option<String>,
    /// Optional description for an implicitly-created team.
    #[serde(default)]
    pub description: Option<String>,
    /// Optional backend. allthecodes supports only in-process.
    #[serde(default)]
    pub backend: Option<String>,
    /// Optional permission mode for the teammate.
    #[serde(default)]
    pub mode: Option<String>,
    /// Optional agent definition used for system prompt/tool policy.
    #[serde(default)]
    pub agent_type: Option<String>,
}

pub fn send_message_description() -> String {
    "Send a message to a teammate or broadcast to all teammates.".to_string()
}

pub fn send_message_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "to": {
                "type": "string",
                "description": "Recipient teammate name, or \"*\" for broadcast"
            },
            "message": {
                "type": "string",
                "description": "Message text or structured JSON message"
            },
            "summary": {
                "type": "string",
                "description": "Brief 5-10 word summary of the message"
            }
        },
        "required": ["to", "message"]
    })
}

pub fn validate_send_message(input: &Value) -> Result<(), &'static str> {
    let to = input.get("to").and_then(|v| v.as_str()).unwrap_or("");
    if to.trim().is_empty() {
        return Err("'to' field is required");
    }
    let msg = input.get("message").and_then(|v| v.as_str()).unwrap_or("");
    if msg.trim().is_empty() {
        return Err("'message' field is required");
    }
    Ok(())
}

pub fn backfill_send_message_observable_input(input: &mut serde_json::Map<String, Value>) {
    if input.contains_key("type") {
        return;
    }

    let Some(to) = input.get("to").and_then(Value::as_str).map(str::to_string) else {
        return;
    };
    let Some(message) = input.get("message").cloned() else {
        return;
    };

    match message {
        Value::String(text) if to == "*" => {
            input.insert("type".to_string(), Value::String("broadcast".to_string()));
            input.insert("content".to_string(), Value::String(text));
        }
        Value::String(text) => {
            input.insert("type".to_string(), Value::String("message".to_string()));
            input.insert("recipient".to_string(), Value::String(to));
            input.insert("content".to_string(), Value::String(text));
        }
        Value::Object(message) => {
            if let Some(value) = message.get("type") {
                input.insert("type".to_string(), value.clone());
            }
            input.insert("recipient".to_string(), Value::String(to));
            if let Some(value) = message.get("request_id") {
                input.insert("request_id".to_string(), value.clone());
            }
            if let Some(value) = message.get("approve") {
                input.insert("approve".to_string(), value.clone());
            }
            if let Some(value) = message.get("reason").or_else(|| message.get("feedback")) {
                input.insert("content".to_string(), value.clone());
            }
        }
        _ => {}
    }
}

pub fn send_message_prompt() -> String {
    "Send a message to a teammate or broadcast to all teammates. \
     Use the 'to' field with a teammate name or '*' for broadcast. \
     Include a brief summary for quick context."
        .to_string()
}

pub fn send_message_user_facing_name(input: Option<&Value>) -> String {
    if let Some(to) = input.and_then(|v| v.get("to")).and_then(|v| v.as_str()) {
        format!("SendMessage(to: {})", to)
    } else {
        SEND_MESSAGE_TOOL_NAME.to_string()
    }
}

pub fn team_spawn_description() -> String {
    "Spawn a new in-process teammate agent that runs in parallel and can be messaged via SendMessage."
        .to_string()
}

pub fn team_spawn_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "name": {
                "type": "string",
                "description": "Unique teammate name (letters, numbers, - and _ only)."
            },
            "prompt": {
                "type": "string",
                "description": "Initial instruction for the teammate - what role it plays, what task to start on."
            },
            "model": {
                "type": "string",
                "description": "Optional model id. Defaults to the parent agent's model."
            },
            "color": {
                "type": "string",
                "description": "Optional UI color tag (red, blue, green, yellow, purple, orange, pink, cyan)."
            },
            "team": {
                "type": "string",
                "description": "Optional team name. Defaults to the active team, or creates one tied to the current session."
            },
            "description": {
                "type": "string",
                "description": "Optional description used only when an implicit team is created."
            },
            "backend": {
                "type": "string",
                "enum": ["in-process"],
                "description": "Execution backend. allthecodes intentionally supports only in-process Agent Teams."
            },
            "mode": {
                "type": "string",
                "enum": ["default", "auto", "bypass", "plan", "acceptEdits", "dontAsk"],
                "description": "Optional permission mode for the teammate. Use \"plan\" to require plan approval before edits."
            },
            "agent_type": {
                "type": "string",
                "description": "Optional built-in or custom agent type for the teammate. Coordinator mode defaults to worker."
            }
        },
        "required": ["name", "prompt"]
    })
}

pub fn validate_team_spawn<F>(input: &Value, backend_supported: F) -> Result<(), String>
where
    F: Fn(&str) -> Result<(), String>,
{
    let name = input.get("name").and_then(|v| v.as_str()).unwrap_or("");
    if name.trim().is_empty() {
        return Err("'name' is required".to_string());
    }
    if name == TEAM_LEAD_NAME {
        return Err(format!(
            "'{}' is reserved for the team lead",
            TEAM_LEAD_NAME
        ));
    }
    let prompt = input.get("prompt").and_then(|v| v.as_str()).unwrap_or("");
    if prompt.trim().is_empty() {
        return Err("'prompt' is required".to_string());
    }
    if let Some(raw_backend) = input.get("backend").and_then(|v| v.as_str()) {
        backend_supported(raw_backend)?;
    }
    Ok(())
}

pub fn team_spawn_plan_mode_required(mode: Option<&str>) -> bool {
    matches!(
        mode.map(|value| value.trim().to_ascii_lowercase()),
        Some(value) if value == "plan" || value == "readonly" || value == "read-only"
    )
}

pub fn resolve_team_spawn_agent_type(
    explicit: Option<&str>,
    default_agent_type: &str,
) -> Option<String> {
    explicit
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| Some(default_agent_type.to_string()))
}

pub fn team_spawn_prompt() -> String {
    "Spawn a new teammate to work in parallel. Use SendMessage to \
     communicate with it after spawn. If no team exists, a session-scoped \
     team is created automatically and you become the team lead."
        .to_string()
}

pub fn team_spawn_user_facing_name(input: Option<&Value>) -> String {
    if let Some(name) = input.and_then(|v| v.get("name")).and_then(|v| v.as_str()) {
        format!("TeamSpawn({})", name)
    } else {
        TEAM_SPAWN_TOOL_NAME.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn send_message_schema_has_required_fields() {
        let schema = send_message_schema();
        assert!(schema["properties"]["to"].is_object());
        assert!(schema["properties"]["message"].is_object());
    }

    #[test]
    fn validate_team_spawn_rejects_reserved_lead_name() {
        let input = json!({"name": TEAM_LEAD_NAME, "prompt": "work"});
        let err = validate_team_spawn(&input, |_| Ok(())).unwrap_err();
        assert!(err.contains("reserved"));
    }

    #[test]
    fn plan_mode_accepts_read_only_alias() {
        assert!(team_spawn_plan_mode_required(Some("read-only")));
        assert!(!team_spawn_plan_mode_required(Some("acceptEdits")));
    }
}
