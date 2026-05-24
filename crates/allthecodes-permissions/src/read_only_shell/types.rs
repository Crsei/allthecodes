use std::collections::HashMap;

/// Describes what kind of argument a flag expects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlagArgType {
    None,   // No argument (--color, -n)
    Number, // Integer argument (--context=3)
    String, // Any string argument (--relative=path)
    Char,   // Single character delimiter
    Brace,  // Literal "{}" only
}

/// Configuration for a single external command (e.g., "git diff").
#[derive(Debug, Clone)]
pub struct ExternalCommandConfig {
    /// Map of flag -> expected argument type. Only these flags are allowed.
    pub safe_flags: HashMap<String, FlagArgType>,
    /// Optional callback for additional danger checks.
    /// Returns true if the command is dangerous.
    pub additional_check: Option<fn(&str, &[String]) -> bool>,
    /// Whether the tool respects POSIX `--` end-of-options.
    /// Default: true.
    pub respects_double_dash: bool,
}

impl ExternalCommandConfig {
    pub fn new(safe_flags: HashMap<String, FlagArgType>) -> Self {
        Self {
            safe_flags,
            additional_check: None,
            respects_double_dash: true,
        }
    }

    pub fn with_check(
        safe_flags: HashMap<String, FlagArgType>,
        check: fn(&str, &[String]) -> bool,
    ) -> Self {
        Self {
            safe_flags,
            additional_check: Some(check),
            respects_double_dash: true,
        }
    }

    pub fn without_double_dash(mut self) -> Self {
        self.respects_double_dash = false;
        self
    }
}
