/// Heuristic classification of a shell command.
///
/// Returns the semantic kind, subtype, and risk level for a given shell command
/// string. The classification is based on the command name and known flags.
///
/// This is used by `ToolClassifier` when the tool name is `Bash` or `PowerShell`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellClassification {
    pub kind: super::OperationKind,
    pub subtype: Option<super::OperationSubtype>,
    pub risk: super::OperationRisk,
    pub confidence: super::OperationConfidence,
    pub target: Option<String>,
}

/// Classify a shell command string into a semantic operation.
///
/// Extracts the first command word and applies pattern matching. Returns
/// `Execute` / `Shell` with `Medium` confidence when no specific pattern
/// matches.
pub fn shell_command_operation(command: &str) -> ShellClassification {
    let trimmed = command.trim();
    let words = split_shell_words(trimmed);
    let first_word = words
        .first()
        .map(|word| word.as_str())
        .unwrap_or("")
        .trim_start_matches(|c: char| c == '/' || c == '.' || c == '~')
        .to_string();

    let target = extract_first_path_arg(trimmed);

    // rm, unlink, rmdir → Delete
    if let Some(result) = match_delete(&first_word, trimmed) {
        return result;
    }

    // sed -i, perl -pi → Modify
    if let Some(result) = match_inline_edit(trimmed) {
        return result;
    }

    // mkdir, touch, cp → Create
    if let Some(result) = match_create(&first_word, trimmed) {
        return result;
    }

    // mv → Modify or Delete+Create
    if first_word == "mv" {
        return ShellClassification {
            kind: super::OperationKind::Modify,
            subtype: None,
            risk: super::OperationRisk::Medium,
            confidence: super::OperationConfidence::Medium,
            target,
        };
    }

    // Redirects >, >> → Create/Modify
    if trimmed.contains(">") {
        return ShellClassification {
            kind: super::OperationKind::Create,
            subtype: None,
            risk: super::OperationRisk::Low,
            confidence: super::OperationConfidence::Medium,
            target,
        };
    }

    // Build commands
    if let Some(result) = match_build(&first_word, trimmed) {
        return result;
    }

    // Test commands
    if let Some(result) = match_test(&first_word, trimmed) {
        return result;
    }

    // Format commands
    if let Some(result) = match_format(&first_word, trimmed) {
        return result;
    }

    // Install commands → Network + Install
    if let Some(result) = match_install(&first_word, trimmed) {
        return result;
    }

    // Network commands (curl, wget)
    if let Some(result) = match_network(&first_word, trimmed) {
        return result;
    }

    // Fallback: general execute
    ShellClassification {
        kind: super::OperationKind::Execute,
        subtype: Some(super::OperationSubtype::Shell),
        risk: super::OperationRisk::Low,
        confidence: super::OperationConfidence::Medium,
        target,
    }
}

fn match_delete(first_word: &str, _command: &str) -> Option<ShellClassification> {
    let target = extract_first_path_arg(_command);
    let is_destructive = _command.contains("-rf")
        || _command.contains("-r -f")
        || _command.contains("--recursive --force");

    match first_word {
        "rm" | "rmdir" | "unlink" => Some(ShellClassification {
            kind: super::OperationKind::Delete,
            subtype: None,
            risk: if is_destructive {
                super::OperationRisk::Destructive
            } else {
                super::OperationRisk::High
            },
            confidence: super::OperationConfidence::High,
            target,
        }),
        _ => None,
    }
}

fn match_inline_edit(command: &str) -> Option<ShellClassification> {
    let words = split_shell_words(command);
    let first_word = words.first().map(|word| word.as_str()).unwrap_or("");
    let has_inline = command.contains("-i")
        || command.contains("-pi")
        || command.contains("-i''")
        || command.contains("-i ''");

    if (first_word == "sed" && has_inline)
        || (first_word == "perl" && (command.contains("-pi") || command.contains("-i")))
    {
        let target = extract_inline_edit_target(command);
        return Some(ShellClassification {
            kind: super::OperationKind::Modify,
            subtype: None,
            risk: super::OperationRisk::Medium,
            confidence: super::OperationConfidence::Medium,
            target,
        });
    }
    None
}

fn match_create(first_word: &str, command: &str) -> Option<ShellClassification> {
    let target = extract_first_path_arg(command);
    let risk = if first_word == "cp" {
        super::OperationRisk::Low
    } else {
        super::OperationRisk::Safe
    };

    match first_word {
        "mkdir" | "touch" => Some(ShellClassification {
            kind: super::OperationKind::Create,
            subtype: None,
            risk,
            confidence: super::OperationConfidence::High,
            target,
        }),
        "cp" => {
            let dest = command.split_whitespace().last().map(str::to_string);
            Some(ShellClassification {
                kind: super::OperationKind::Create,
                subtype: None,
                risk,
                confidence: super::OperationConfidence::Medium,
                target: dest.or(target),
            })
        }
        _ => None,
    }
}

fn match_build(first_word: &str, command: &str) -> Option<ShellClassification> {
    let target = extract_subcommand_arg(command);
    let is_build = match first_word {
        "cargo" => command.contains(" build") || command.contains(" b "),
        "npm" => command.contains(" run build") || command.contains(" build"),
        "pnpm" => command.contains(" build"),
        "yarn" => command.contains(" build"),
        "make" | "cmake" => true,
        "go" => command.contains(" build"),
        "rustc" => true,
        _ => false,
    };

    if is_build {
        Some(ShellClassification {
            kind: super::OperationKind::Execute,
            subtype: Some(super::OperationSubtype::Build),
            risk: super::OperationRisk::Medium,
            confidence: super::OperationConfidence::High,
            target,
        })
    } else {
        None
    }
}

fn match_test(first_word: &str, command: &str) -> Option<ShellClassification> {
    let target = extract_subcommand_arg(command);
    let is_test = match first_word {
        "cargo" => command.contains(" test") || command.contains(" t "),
        "npm" => command.contains(" test") || command.contains(" run test"),
        "pnpm" => command.contains(" test"),
        "yarn" => command.contains(" test"),
        "pytest" | "vitest" | "jest" | "mocha" => true,
        "go" => command.contains(" test"),
        _ => false,
    };

    if is_test {
        Some(ShellClassification {
            kind: super::OperationKind::Execute,
            subtype: Some(super::OperationSubtype::Test),
            risk: super::OperationRisk::Medium,
            confidence: super::OperationConfidence::High,
            target,
        })
    } else {
        None
    }
}

fn match_format(first_word: &str, command: &str) -> Option<ShellClassification> {
    let target = extract_subcommand_arg(command);
    let is_format = match first_word {
        "cargo" => command.contains(" fmt"),
        "rustfmt" => true,
        "prettier" => true,
        "eslint" => command.contains(" --fix"),
        "black" | "ruff" => true,
        "gofmt" | "goimports" => true,
        _ => false,
    };

    if is_format {
        Some(ShellClassification {
            kind: super::OperationKind::Execute,
            subtype: Some(super::OperationSubtype::Format),
            risk: super::OperationRisk::Low,
            confidence: super::OperationConfidence::High,
            target,
        })
    } else {
        None
    }
}

fn match_install(first_word: &str, command: &str) -> Option<ShellClassification> {
    let target = extract_package_arg(command);
    let is_install = match first_word {
        "npm" => {
            command.contains(" install") || command.contains(" i ") || command.contains(" add")
        }
        "pnpm" => command.contains(" install") || command.contains(" add"),
        "yarn" => command.contains(" add") || command.contains(" install"),
        "cargo" => command.contains(" add") || command.contains(" install"),
        "pip" | "pip3" => command.contains(" install"),
        "brew" => command.contains(" install"),
        "apt" | "apt-get" => command.contains(" install"),
        "go" => command.contains(" get") || command.contains(" install"),
        _ => false,
    };

    if is_install {
        Some(ShellClassification {
            kind: super::OperationKind::Network,
            subtype: Some(super::OperationSubtype::Install),
            risk: super::OperationRisk::High,
            confidence: super::OperationConfidence::High,
            target,
        })
    } else {
        None
    }
}

fn match_network(first_word: &str, _command: &str) -> Option<ShellClassification> {
    let target = extract_first_path_arg(_command);
    match first_word {
        "curl" | "wget" => Some(ShellClassification {
            kind: super::OperationKind::Network,
            subtype: None,
            risk: super::OperationRisk::High,
            confidence: super::OperationConfidence::High,
            target,
        }),
        "gh" | "hub" => Some(ShellClassification {
            kind: super::OperationKind::Network,
            subtype: None,
            risk: super::OperationRisk::Medium,
            confidence: super::OperationConfidence::Medium,
            target,
        }),
        _ => None,
    }
}

fn split_shell_words(command: &str) -> Vec<String> {
    shell_words::split(command)
        .unwrap_or_else(|_| command.split_whitespace().map(str::to_string).collect())
}

/// Extract the first path-like argument from a command string.
fn extract_first_path_arg(command: &str) -> Option<String> {
    split_shell_words(command)
        .into_iter()
        .skip(1)
        .find(|arg| !arg.starts_with('-') && !arg.starts_with("//"))
}

/// Extract the edited file target from commands like `sed -i 's///' file`.
fn extract_inline_edit_target(command: &str) -> Option<String> {
    split_shell_words(command)
        .into_iter()
        .skip(1)
        .filter(|arg| !arg.starts_with('-'))
        .last()
}

/// Extract the subcommand argument (e.g., `cargo build -p foo` → `foo`).
fn extract_subcommand_arg(command: &str) -> Option<String> {
    let mut prev = "";
    let words = split_shell_words(command);
    for word in &words {
        if prev == "-p" || prev == "--package" || prev == "--project" {
            return Some(word.clone());
        }
        prev = word;
    }
    extract_first_path_arg(command)
}

/// Extract the package argument after `install`/`add`.
fn extract_package_arg(command: &str) -> Option<String> {
    let words = split_shell_words(command);
    for (i, word) in words.iter().enumerate() {
        if (word == "install" || word == "add" || word == "get" || word == "i")
            && i + 1 < words.len()
        {
            let next = &words[i + 1];
            if !next.starts_with('-') {
                return Some(next.clone());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::OperationKind;

    #[test]
    fn test_delete_rm() {
        let result = shell_command_operation("rm /tmp/foo");
        assert_eq!(result.kind, OperationKind::Delete);
        assert_eq!(result.risk, crate::OperationRisk::High);
        assert_eq!(result.target.as_deref(), Some("/tmp/foo"));
    }

    #[test]
    fn test_delete_rm_rf() {
        let result = shell_command_operation("rm -rf /tmp/build/");
        assert_eq!(result.kind, OperationKind::Delete);
        assert_eq!(result.risk, crate::OperationRisk::Destructive);
    }

    #[test]
    fn test_mkdir() {
        let result = shell_command_operation("mkdir -p src/components");
        assert_eq!(result.kind, OperationKind::Create);
    }

    #[test]
    fn test_cargo_build() {
        let result = shell_command_operation("cargo build --release");
        assert_eq!(result.kind, OperationKind::Execute);
        assert_eq!(result.subtype, Some(crate::OperationSubtype::Build));
    }

    #[test]
    fn test_cargo_test() {
        let result = shell_command_operation("cargo test -p allthecodes-ui");
        assert_eq!(result.kind, OperationKind::Execute);
        assert_eq!(result.subtype, Some(crate::OperationSubtype::Test));
        assert_eq!(result.target.as_deref(), Some("allthecodes-ui"));
    }

    #[test]
    fn test_cargo_fmt() {
        let result = shell_command_operation("cargo fmt");
        assert_eq!(result.kind, OperationKind::Execute);
        assert_eq!(result.subtype, Some(crate::OperationSubtype::Format));
    }

    #[test]
    fn test_npm_install() {
        let result = shell_command_operation("npm install express");
        assert_eq!(result.kind, OperationKind::Network);
        assert_eq!(result.subtype, Some(crate::OperationSubtype::Install));
    }

    #[test]
    fn test_curl() {
        let result = shell_command_operation("curl -s https://example.com/data");
        assert_eq!(result.kind, OperationKind::Network);
        assert_eq!(result.risk, crate::OperationRisk::High);
    }

    #[test]
    fn test_generic_command() {
        let result = shell_command_operation("ls -la");
        assert_eq!(result.kind, OperationKind::Execute);
        assert_eq!(result.subtype, Some(crate::OperationSubtype::Shell));
        assert_eq!(result.confidence, crate::OperationConfidence::Medium);
    }

    #[test]
    fn test_sed_inline_edit() {
        let result = shell_command_operation(r#"sed -i 's/foo/bar/g' src/main.rs"#);
        assert_eq!(result.kind, OperationKind::Modify);
        assert_eq!(result.target.as_deref(), Some("src/main.rs"));
    }

    #[test]
    fn test_pytest() {
        let result = shell_command_operation("pytest tests/ -x -v");
        assert_eq!(result.kind, OperationKind::Execute);
        assert_eq!(result.subtype, Some(crate::OperationSubtype::Test));
    }

    #[test]
    fn test_prettier() {
        let result = shell_command_operation("prettier --write src/");
        assert_eq!(result.kind, OperationKind::Execute);
        assert_eq!(result.subtype, Some(crate::OperationSubtype::Format));
    }

    #[test]
    fn test_cargo_add() {
        let result = shell_command_operation("cargo add serde --features derive");
        assert_eq!(result.kind, OperationKind::Network);
        assert_eq!(result.subtype, Some(crate::OperationSubtype::Install));
    }

    #[test]
    fn test_redirect_modify() {
        let result = shell_command_operation("echo 'config' >> /etc/app.conf");
        assert_eq!(result.kind, OperationKind::Create);
    }

    #[test]
    fn test_rmdir() {
        let result = shell_command_operation("rmdir /tmp/empty");
        assert_eq!(result.kind, OperationKind::Delete);
    }

    #[test]
    fn test_empty_command() {
        let result = shell_command_operation("");
        assert_eq!(result.kind, OperationKind::Execute);
        assert_eq!(result.subtype, Some(crate::OperationSubtype::Shell));
    }
}
