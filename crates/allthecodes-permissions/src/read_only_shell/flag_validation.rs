use std::collections::HashMap;

use super::types::{ExternalCommandConfig, FlagArgType};

pub(crate) fn map_from_pairs(pairs: &[(&str, FlagArgType)]) -> HashMap<String, FlagArgType> {
    pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
}

fn validate_flag_arg(value: &str, arg_type: FlagArgType) -> bool {
    match arg_type {
        FlagArgType::None => false, // Should not be called
        FlagArgType::Number => value.chars().all(|c| c.is_ascii_digit()),
        FlagArgType::String => true,
        FlagArgType::Char => value.chars().count() == 1,
        FlagArgType::Brace => value == "{}",
    }
}

/// FLAG_PATTERN matches flag-like tokens.
/// A flag starts with `-` and has at least one more character.
pub(crate) fn is_flag(token: &str) -> bool {
    token.len() > 1 && token.starts_with('-')
}

/// Validate flags for a command against its config.
pub(crate) fn validate_flags(
    args: &[String],
    config: &ExternalCommandConfig,
    command_name: Option<&str>,
    raw_command: &str,
) -> bool {
    if let Some(check) = config.additional_check {
        if check(raw_command, args) {
            return false;
        }
    }

    let mut i = 0;

    while i < args.len() {
        let token = &args[i];

        if token.is_empty() {
            i += 1;
            continue;
        }

        // Handle `--` end-of-options
        if token == "--" {
            if config.respects_double_dash {
                break; // Everything after -- is positional args
            }
            i += 1;
            continue;
        }

        if is_flag(token) {
            // Handle --flag=value format
            let has_equals = token.contains('=');
            let parts: Vec<&str> = token.splitn(2, '=').collect();
            let flag = parts[0];
            let inline_value = parts.get(1).copied().unwrap_or("");

            // Get the flag arg type
            let flag_arg_type = config.safe_flags.get(flag).copied();

            match flag_arg_type {
                None => {
                    // Special case: git numeric shorthand (-<number>)
                    if command_name == Some("git")
                        && flag.len() > 1
                        && flag.chars().skip(1).all(|c| c.is_ascii_digit())
                    {
                        i += 1;
                        continue;
                    }
                    return false; // Unknown flag
                }
                Some(arg_type) => {
                    if arg_type == FlagArgType::None {
                        if has_equals {
                            return false; // Flag should not have a value
                        }
                        i += 1;
                    } else {
                        if has_equals {
                            // Use the inline value
                            if arg_type == FlagArgType::String
                                && inline_value.starts_with('-')
                                && !(command_name == Some("git")
                                    && flag == "--sort"
                                    && inline_value
                                        .chars()
                                        .nth(1)
                                        .map(|c| c.is_ascii_alphabetic())
                                        .unwrap_or(false))
                            {
                                return false;
                            }
                            if !validate_flag_arg(inline_value, arg_type) {
                                return false;
                            }
                            i += 1;
                        } else {
                            // Check if next token is the argument
                            if i + 1 < args.len() && !is_flag(&args[i + 1]) {
                                if arg_type == FlagArgType::String
                                    && args[i + 1].starts_with('-')
                                    && !(command_name == Some("git")
                                        && flag == "--sort"
                                        && args[i + 1]
                                            .chars()
                                            .nth(1)
                                            .map(|c| c.is_ascii_alphabetic())
                                            .unwrap_or(false))
                                {
                                    return false;
                                }
                                if !validate_flag_arg(&args[i + 1], arg_type) {
                                    return false;
                                }
                                i += 2;
                            } else {
                                return false; // Missing required argument
                            }
                        }
                    }
                }
            }
        } else {
            // Non-flag positional argument (revision specs, paths, patterns).
            // Keep scanning because later flag-looking tokens are still parsed by
            // the underlying command and may enable writes.
            i += 1;
        }
    }

    true
}

/// Try to match a command name + args against a known read-only command map.
///
/// Supports multi-level command keys like "git diff" or "gh pr view".
/// Returns the longest matching key and its config.
pub(crate) fn match_readonly_command<'a>(
    argv: &[String],
    map: &'a HashMap<String, ExternalCommandConfig>,
) -> Option<(&'a ExternalCommandConfig, Vec<String>)> {
    if argv.is_empty() {
        return None;
    }

    // Try longest match first (3-gram, 2-gram, 1-gram)
    let max_depth = argv.len().min(3);
    for depth in (1..=max_depth).rev() {
        let key = argv[0..depth].join(" ");
        if let Some(config) = map.get(&key) {
            let remaining: Vec<String> = argv[depth..].to_vec();
            return Some((config, remaining));
        }
    }

    // Try just the command basename
    let base = std::path::Path::new(&argv[0])
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or(&argv[0]);

    if let Some(config) = map.get(base) {
        let remaining: Vec<String> = argv[1..].to_vec();
        return Some((config, remaining));
    }

    None
}

/// Merge multiple HashMap sources
pub(crate) fn merge_maps(sources: &[HashMap<String, FlagArgType>]) -> HashMap<String, FlagArgType> {
    let mut result = HashMap::new();
    for source in sources {
        result.extend(source.iter().map(|(k, v)| (k.clone(), *v)));
    }
    result
}
