//! Shared metadata and pure helpers for execution tools.
//!
//! Runtime execution remains in the current owner until the shell/exec-policy
//! crates exist. This module intentionally exposes only stable tool names,
//! JSON schemas, prompts, and output shaping helpers.

pub mod bash;
pub mod powershell;
pub mod repl;
pub mod sleep;
pub mod sleep_tool;

pub use bash::truncate_output;
pub use sleep_tool::SleepTool;

pub const BASH_NAME: &str = bash::NAME;
pub const POWERSHELL_NAME: &str = powershell::NAME;
pub const REPL_NAME: &str = repl::NAME;
pub const SLEEP_NAME: &str = sleep::NAME;

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

    #[test]
    fn exec_schemas_are_object_schemas() {
        for schema in [
            bash::input_schema(),
            powershell::input_schema(),
            repl::input_schema(),
            sleep::input_schema(),
        ] {
            assert_eq!(schema.get("type").and_then(Value::as_str), Some("object"));
            assert!(schema.get("properties").is_some());
        }
    }
}
