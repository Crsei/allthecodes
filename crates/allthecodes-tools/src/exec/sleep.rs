use serde_json::{json, Value};

pub const NAME: &str = "Sleep";
pub const MIN_DURATION_SECONDS: i64 = 1;
pub const MAX_DURATION_SECONDS: i64 = 3600;

pub fn description() -> &'static str {
    "Pause the proactive tick loop for a specified duration."
}

pub fn input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "duration_seconds": {
                "type": "integer",
                "minimum": MIN_DURATION_SECONDS,
                "maximum": MAX_DURATION_SECONDS,
                "description": "Number of seconds to pause the proactive tick loop (1-3600)"
            },
            "reason": {
                "type": "string",
                "description": "Optional reason for pausing (e.g. \"waiting for CI to finish\")"
            }
        },
        "required": ["duration_seconds"]
    })
}

pub fn validate_duration_seconds(duration: Option<i64>) -> Result<(), String> {
    match duration {
        None => Err("\"duration_seconds\" is required".to_string()),
        Some(d) if !(MIN_DURATION_SECONDS..=MAX_DURATION_SECONDS).contains(&d) => Err(format!(
            "\"duration_seconds\" must be between {MIN_DURATION_SECONDS} and {MAX_DURATION_SECONDS}, got {d}"
        )),
        Some(_) => Ok(()),
    }
}

pub fn prompt() -> String {
    "Use Sleep to pause the proactive tick loop for a specified number of seconds.\n\n\
         When you determine that no further action is needed for a period of time \
         (e.g. waiting for a CI build, a deployment, or an external event), call \
         Sleep with the estimated wait duration. The daemon will stop ticking \
         until the sleep period expires.\n\n\
         Parameters:\n\
         - duration_seconds (required): 1-3600 seconds.\n\
         - reason (optional): A brief explanation of why you are sleeping.\n\n\
         This tool does not block execution -- it signals intent to the tick loop."
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_duration_range() {
        assert!(validate_duration_seconds(Some(60)).is_ok());
        assert!(validate_duration_seconds(None).is_err());
        assert!(validate_duration_seconds(Some(0)).is_err());
        assert!(validate_duration_seconds(Some(7200)).is_err());
    }
}
