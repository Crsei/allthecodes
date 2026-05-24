use serde_json::{json, Value};

pub const NAME: &str = "REPL";

pub const SUPPORTED_LANGUAGES: &[(&str, &str, &str)] = &[
    ("python", "python", ".py"),
    ("python3", "python3", ".py"),
    ("node", "node", ".js"),
    ("javascript", "node", ".js"),
    ("ruby", "ruby", ".rb"),
    ("perl", "perl", ".pl"),
    ("php", "php", ".php"),
    ("lua", "lua", ".lua"),
    ("bash", "bash", ".sh"),
    ("sh", "sh", ".sh"),
];

pub fn description(language: Option<&str>) -> String {
    let language = language.unwrap_or("code");
    format!("Execute a {language} code snippet.")
}

pub fn input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "language": {
                "type": "string",
                "description": "The programming language (python, python3, node, javascript, ruby, perl, php, lua, bash, sh)"
            },
            "code": {
                "type": "string",
                "description": "The code to execute"
            },
            "timeout": {
                "type": "number",
                "description": "Optional timeout in milliseconds (default 120000, max 600000)"
            }
        },
        "required": ["language", "code"]
    })
}

pub fn language_spec(language: &str) -> Option<(&'static str, &'static str)> {
    SUPPORTED_LANGUAGES
        .iter()
        .find(|(name, _, _)| *name == language)
        .map(|(_, interpreter, extension)| (*interpreter, *extension))
}

pub fn supported_language_names() -> Vec<&'static str> {
    SUPPORTED_LANGUAGES
        .iter()
        .map(|(name, _, _)| *name)
        .collect()
}

pub fn prompt() -> String {
    "Use the REPL tool to execute code snippets in supported languages.\n\n\
Supported languages: python, python3, node/javascript, ruby, perl, php, lua, bash, sh.\n\n\
The code is written to a temporary file and executed with the appropriate interpreter.\n\
stdout, stderr, and exit code are captured and returned.\n\
Default timeout is 120 seconds. Maximum is 600 seconds.\n\n\
Use this when you need to test a code snippet or compute something."
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_language_specs() {
        assert_eq!(language_spec("python"), Some(("python", ".py")));
        assert_eq!(language_spec("javascript"), Some(("node", ".js")));
        assert_eq!(language_spec("unknown"), None);
    }
}
