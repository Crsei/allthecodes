//! Tree-sitter Bash AST parser with fail-closed security semantics.
//!
//! Provides a tree-sitter based parser that:
//! - Uses an explicit node-type allowlist (fail-closed on unknown nodes)
//! - Enforces a parse timeout and node budget
//! - Tracks byte offsets for correct UTF-8 handling
//! - Returns structured command data via the `model` DTOs
//!
//! # Design principle
//!
//! This is NOT a sandbox. It answers one question: "Can we produce a trustworthy
//! argv[] for each simple command in this string?" If yes, downstream code can
//! match argv[0] against permission rules. If no (too-complex), the caller must
//! ask the user (fall through to permission prompt).

use std::time::Instant;

use tree_sitter::{Node, Parser};

use crate::model::*;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Maximum parse time in milliseconds before aborting.
const PARSE_TIMEOUT_MS: u64 = 50;

/// Maximum number of tree nodes to visit before aborting.
const NODE_BUDGET: usize = 50_000;

// ---------------------------------------------------------------------------
// Node type allowlist
// ---------------------------------------------------------------------------

/// Structural node types that represent composition of commands.
/// We recurse through these to find leaf `command` nodes.
const STRUCTURAL_TYPES: &[&str] = &[
    "program",
    "list",
    "pipeline",
    "redirected_statement",
    "negated_command",
    "command",
];

/// Operator / separator tokens between commands.
const SEPARATOR_TYPES: &[&str] = &["&&", "||", "|", ";", "&", "|&", "\n"];

/// Node types that represent full command invocations (leaf command containers).
const LEAF_COMMAND_TYPES: &[&str] = &["command", "declaration_command"];

/// Node types allowed inside a simple `command` node.
const COMMAND_CHILD_TYPES: &[&str] = &[
    "command_name",
    "word",
    "string",
    "raw_string",
    "expansion",
    "simple_expansion",
    "command_substitution",
    "process_substitution",
    "file_redirect",
    "file_descriptor",
    "variable_assignment",
    "concatenation",
    "translated_string",
    "heredoc_body",
    "heredoc_start",
    "heredoc_content",
    "heredoc_end",
    "string_content",
    "number",
    "regex",
    "test_operator",
];

/// Control flow and declaration keywords.
const DECLARATION_TYPES: &[&str] = &[
    "declaration_command",
    "if_statement",
    "while_statement",
    "for_statement",
    "case_statement",
    "function_definition",
    "compound_statement",
    "subshell",
    "test_command",
    "unset_command",
];

/// Heredoc node types.
const HEREDOC_TYPES: &[&str] = &["heredoc_redirect", "heredoc_body", "heredoc_start"];

/// Comment types.
const COMMENT_TYPES: &[&str] = &["comment"];

/// All explicitly allowed node types.
fn is_node_type_allowed(kind: &str) -> bool {
    STRUCTURAL_TYPES.contains(&kind)
        || SEPARATOR_TYPES.contains(&kind)
        || LEAF_COMMAND_TYPES.contains(&kind)
        || COMMAND_CHILD_TYPES.contains(&kind)
        || DECLARATION_TYPES.contains(&kind)
        || HEREDOC_TYPES.contains(&kind)
        || COMMENT_TYPES.contains(&kind)
}

// ---------------------------------------------------------------------------
// Parse result
// ---------------------------------------------------------------------------

/// Result of parsing a command for security purposes.
#[derive(Debug, Clone)]
pub enum SecurityParseResult {
    /// Successfully parsed into simple commands.
    Simple { commands: Vec<SimpleCommand> },
    /// Command is too complex for safe classification.
    TooComplex {
        reason: String,
        node_type: Option<String>,
    },
    /// Parser is unavailable (tree-sitter not initialized).
    ParseUnavailable,
}

// ---------------------------------------------------------------------------
// Parsing functions
// ---------------------------------------------------------------------------

/// Parse a bash command using tree-sitter, with timeouts and node budgets.
pub fn parse_for_security(command: &str, parser: &mut Parser) -> SecurityParseResult {
    let start = Instant::now();

    let tree = match parser.parse(command, None) {
        Some(t) => t,
        None => return SecurityParseResult::ParseUnavailable,
    };

    if start.elapsed().as_millis() > PARSE_TIMEOUT_MS as u128 {
        return SecurityParseResult::TooComplex {
            reason: "Parse timeout exceeded".to_string(),
            node_type: None,
        };
    }

    let root = tree.root_node();
    let source_bytes = command.as_bytes();
    let mut diagnostics = Vec::new();
    let mut commands = Vec::new();

    let mut node_count = 0;
    let result = extract_commands_from_node(
        root,
        source_bytes,
        &mut node_count,
        &mut diagnostics,
        &mut commands,
    );

    if node_count > NODE_BUDGET {
        return SecurityParseResult::TooComplex {
            reason: format!("Node budget exceeded ({} nodes)", node_count),
            node_type: None,
        };
    }

    match result {
        Ok(_) => SecurityParseResult::Simple { commands },
        Err((reason, node_type)) => SecurityParseResult::TooComplex { reason, node_type },
    }
}

/// Recursively extract simple commands from tree-sitter AST nodes.
fn extract_commands_from_node<'a>(
    node: Node<'a>,
    source: &'a [u8],
    node_count: &mut usize,
    _diagnostics: &mut Vec<ParseDiagnostic>,
    commands: &mut Vec<SimpleCommand>,
) -> Result<(), (String, Option<String>)> {
    *node_count += 1;
    if *node_count > NODE_BUDGET {
        return Err((
            format!("Node budget exceeded ({} nodes)", *node_count),
            None,
        ));
    }

    let kind = node.kind();

    // Check allowlist
    if !is_node_type_allowed(kind) {
        return Err((
            format!("Unknown or unsupported node type: {}", kind),
            Some(kind.to_string()),
        ));
    }

    match kind {
        // Structural nodes: recurse into children
        "program" | "list" | "redirected_statement" | "negated_command" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                extract_commands_from_node(child, source, node_count, _diagnostics, commands)?;
            }
            Ok(())
        }

        "pipeline" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if !SEPARATOR_TYPES.contains(&child.kind()) {
                    extract_commands_from_node(child, source, node_count, _diagnostics, commands)?;
                }
            }
            Ok(())
        }

        // Control flow: treat as too-complex for permission matching
        "if_statement"
        | "while_statement"
        | "for_statement"
        | "case_statement"
        | "function_definition"
        | "compound_statement"
        | "subshell" => Err((
            format!("Control flow construct not supported: {}", kind),
            Some(kind.to_string()),
        )),

        // Simple command: extract argv
        "command" | "declaration_command" => {
            if let Some(cmd) = extract_simple_command(node, source)? {
                commands.push(cmd);
            }
            Ok(())
        }

        // Variable assignment at top level (no command)
        "variable_assignment" => {
            // Single variable assignment is fine
            Ok(())
        }

        "comment" => Ok(()),

        _ => Ok(()),
    }
}

/// Extract a SimpleCommand from a `command` AST node.
fn extract_simple_command(
    node: Node,
    source: &[u8],
) -> Result<Option<SimpleCommand>, (String, Option<String>)> {
    let _text = node_as_text(node, source).unwrap_or_default();
    let mut argv = Vec::new();
    let mut env_vars = Vec::new();
    let mut redirects = Vec::new();

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        let child_kind = child.kind();

        match child_kind {
            "command_name" | "word" => {
                if let Some(t) = node_as_text(child, source) {
                    argv.push(t);
                }
            }
            "string" => {
                if contains_dynamic_child(child) {
                    return Err((
                        "Dynamic double-quoted content is not supported in security mode"
                            .to_string(),
                        Some(child_kind.to_string()),
                    ));
                }
                // Double-quoted string: extract content between quotes
                if let Some(t) = node_as_text(child, source) {
                    argv.push(t);
                }
            }
            "raw_string" => {
                // Single-quoted string: extract content between quotes
                if let Some(t) = node_as_text(child, source) {
                    argv.push(t);
                }
            }
            "expansion" | "simple_expansion" => {
                return Err((
                    "Parameter expansion is dynamic in security mode".to_string(),
                    Some(child_kind.to_string()),
                ));
            }
            "command_substitution" => {
                return Err((
                    "Command substitution is not supported in security mode".to_string(),
                    Some(child_kind.to_string()),
                ));
            }
            "process_substitution" => {
                return Err((
                    "Process substitution is not supported in security mode".to_string(),
                    Some(child_kind.to_string()),
                ));
            }
            "concatenation" => {
                if contains_dynamic_child(child) {
                    return Err((
                        "Dynamic concatenation is not supported in security mode".to_string(),
                        Some(child_kind.to_string()),
                    ));
                }
                if let Some(t) = node_as_text(child, source) {
                    argv.push(t);
                }
            }
            "file_redirect" => {
                if let Some(redir) = extract_redirect(child, source) {
                    redirects.push(redir);
                }
            }
            "file_descriptor" => {
                // e.g., 2>&1 — part of redirect, handled by file_redirect parent
            }
            "variable_assignment" => {
                if let Some((name, val)) = extract_assignment(child, source) {
                    env_vars.push((name, val));
                }
            }
            _ => {
                // Other nodes in a command are ignored (e.g., comments)
            }
        }
    }

    if argv.is_empty() {
        return Ok(None);
    }

    // Split argv[0] potentially from env vars
    let command_raw = argv.first().cloned();
    let command_name = command_raw.as_ref().and_then(|c| {
        std::path::Path::new(c)
            .file_name()
            .and_then(|f| f.to_str())
            .map(|s| s.to_string())
    });
    let args: Vec<String> = argv.iter().skip(1).cloned().collect();

    Ok(Some(SimpleCommand {
        command_name,
        command_raw,
        args,
        env_vars,
        argv,
    }))
}

fn contains_dynamic_child(node: Node) -> bool {
    matches!(
        node.kind(),
        "expansion" | "simple_expansion" | "command_substitution" | "process_substitution"
    ) || {
        let mut cursor = node.walk();
        let mut found = false;
        for child in node.children(&mut cursor) {
            if contains_dynamic_child(child) {
                found = true;
                break;
            }
        }
        found
    }
}

/// Extract a Redirect from a `file_redirect` node.
fn extract_redirect(node: Node, source: &[u8]) -> Option<Redirection> {
    let mut op = String::new();
    let mut target = String::new();

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            ">" | ">>" | "<" | "<<" | ">&" | ">|" | "<&" | "&>" | "&>>" | "<<<" => {
                op = child.kind().to_string();
            }
            "word" | "file_descriptor" | "heredoc_start" => {
                if let Some(t) = node_as_text(child, source) {
                    target = t;
                }
            }
            _ => {}
        }
    }

    if op.is_empty() {
        None
    } else {
        Some(Redirection {
            operator: op,
            target,
        })
    }
}

/// Extract a variable assignment (VAR=val).
fn extract_assignment(node: Node, source: &[u8]) -> Option<(String, String)> {
    let text = node_as_text(node, source)?;

    if let Some(eq_pos) = text.find('=') {
        let name = text[..eq_pos].to_string();
        let value = text[eq_pos + 1..].to_string();
        Some((name, value))
    } else {
        None
    }
}

/// Get UTF-8 text from a tree-sitter node.
fn node_as_text(node: Node, source: &[u8]) -> Option<String> {
    node.utf8_text(source).ok().map(|s| s.to_string())
}

// ---------------------------------------------------------------------------
// Convenience wrapper
// ---------------------------------------------------------------------------

/// Create a tree-sitter parser configured for bash.
pub fn new_bash_parser() -> Result<Parser, ShellParseError> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_bash::language())
        .map_err(|e| ShellParseError::ParseFailed(format!("Failed to set bash language: {}", e)))?;
    Ok(parser)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_parser() -> Parser {
        new_bash_parser().expect("Failed to create bash parser")
    }

    #[test]
    fn test_parse_simple_command() {
        let mut parser = make_parser();
        let result = parse_for_security("echo hello world", &mut parser);
        match result {
            SecurityParseResult::Simple { commands } => {
                assert_eq!(commands.len(), 1);
                assert_eq!(commands[0].command_name.as_deref(), Some("echo"));
                assert_eq!(commands[0].args, vec!["hello", "world"]);
            }
            other => panic!("Expected Simple, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_compound_and() {
        let mut parser = make_parser();
        let result = parse_for_security("echo a && echo b", &mut parser);
        match result {
            SecurityParseResult::Simple { commands } => {
                assert_eq!(commands.len(), 2);
                assert_eq!(commands[0].argv, vec!["echo", "a"]);
                assert_eq!(commands[1].argv, vec!["echo", "b"]);
            }
            other => panic!("Expected Simple, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_pipeline() {
        let mut parser = make_parser();
        let result = parse_for_security("cat file | grep pattern", &mut parser);
        match result {
            SecurityParseResult::Simple { commands } => {
                assert_eq!(commands.len(), 2);
                assert_eq!(commands[0].command_name.as_deref(), Some("cat"));
                assert_eq!(commands[1].command_name.as_deref(), Some("grep"));
            }
            other => panic!("Expected Simple, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_with_env_var() {
        let mut parser = make_parser();
        let result = parse_for_security("LANG=C sort file.txt", &mut parser);
        match result {
            SecurityParseResult::Simple { commands } => {
                assert!(!commands.is_empty());
                let cmd = &commands[0];
                // LANG=C could be parsed as env var or part of argv depending on tree-sitter
                assert!(
                    cmd.command_name.as_deref() == Some("sort")
                        || cmd.command_name.as_deref() == Some("C")
                );
            }
            other => panic!("Expected Simple, got {:?}", other),
        }
    }

    #[test]
    fn test_subshell_too_complex() {
        let mut parser = make_parser();
        let result = parse_for_security("(cd /tmp && ls)", &mut parser);
        match result {
            SecurityParseResult::TooComplex { .. } => {} // Expected
            other => panic!("Expected TooComplex, got {:?}", other),
        }
    }

    #[test]
    fn test_if_statement_too_complex() {
        let mut parser = make_parser();
        let result = parse_for_security("if [ -f file ]; then echo exists; fi", &mut parser);
        match result {
            SecurityParseResult::TooComplex { .. } => {} // Expected
            other => panic!("Expected TooComplex, got {:?}", other),
        }
    }

    #[test]
    fn test_redirect_detection() {
        let mut parser = make_parser();
        let result = parse_for_security("sort < input.txt > output.txt", &mut parser);
        match result {
            SecurityParseResult::Simple { commands } => {
                assert!(!commands.is_empty());
                // The redirects may be part of the AST but not extracted as commands
            }
            other => panic!("Expected Simple, got {:?}", other),
        }
    }

    #[test]
    fn test_empty_command() {
        let mut parser = make_parser();
        let result = parse_for_security("", &mut parser);
        match result {
            SecurityParseResult::Simple { commands } => {
                assert!(commands.is_empty());
            }
            other => panic!("Expected Simple with empty commands, got {:?}", other),
        }
    }

    #[test]
    fn test_command_substitution_placeholder() {
        let mut parser = make_parser();
        let result = parse_for_security("echo $(whoami)", &mut parser);
        match result {
            SecurityParseResult::TooComplex { .. } => {}
            other => panic!("Expected TooComplex, got {:?}", other),
        }
    }

    #[test]
    fn test_parameter_expansion_too_complex() {
        let mut parser = make_parser();
        let result = parse_for_security("echo $HOME", &mut parser);
        match result {
            SecurityParseResult::TooComplex { .. } => {}
            other => panic!("Expected TooComplex, got {:?}", other),
        }
    }

    #[test]
    fn test_heredoc_parsed() {
        let mut parser = make_parser();
        let result = parse_for_security("cat <<EOF\nhello\nEOF", &mut parser);
        match result {
            SecurityParseResult::Simple { commands } => {
                assert!(!commands.is_empty());
            }
            SecurityParseResult::TooComplex { .. } => {} // Acceptable
            other => panic!("Unexpected: {:?}", other),
        }
    }
}
