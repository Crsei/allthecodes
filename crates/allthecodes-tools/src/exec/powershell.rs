use serde_json::{json, Value};

pub const NAME: &str = "PowerShell";

pub fn description() -> &'static str {
    "Executes a PowerShell command and returns its output."
}

pub fn input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "command": {
                "type": "string",
                "description": "The PowerShell command to execute"
            },
            "timeout": {
                "type": "number",
                "description": "Optional timeout in milliseconds (default 120000, max 600000)"
            }
        },
        "required": ["command"]
    })
}

pub fn prompt() -> String {
    "Executes a PowerShell command and returns its output.\n\n\
Use this tool when you need to run PowerShell-specific commands or cmdlets.\n\
On Windows, uses powershell.exe; on other platforms, uses pwsh (PowerShell Core).\n\n\
- The command is passed via `-Command` so you can use full PowerShell syntax.\n\
- Default timeout is 120 seconds (120000 ms). Maximum is 600 seconds.\n\
- stdout and stderr are captured separately.\n\
- For simple shell commands, prefer the Bash tool instead."
        .to_string()
}
