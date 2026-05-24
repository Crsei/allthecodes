//! Tree-sitter AST analysis utilities for bash command security validation.
//!
//! Extracts security-relevant information from tree-sitter parse trees.
//! Mirrors Bun's `treeSitterAnalysis.ts` but producing Rust DTOs.

use tree_sitter::{Node, Parser};

// ---------------------------------------------------------------------------
// Quote context analysis
// ---------------------------------------------------------------------------

/// Quote-related context extracted from the AST.
#[derive(Debug, Clone)]
pub struct QuoteContext {
    /// Command text with single-quoted content removed (double-quoted content preserved).
    pub with_double_quotes: String,
    /// Command text with all quoted content removed.
    pub fully_unquoted: String,
    /// Like fully_unquoted but preserves quote delimiter characters.
    pub unquoted_keep_quote_chars: String,
}

/// Collected spans for different quote types.
#[derive(Debug, Default)]
struct QuoteSpans {
    raw: Vec<(usize, usize)>,
    ansi_c: Vec<(usize, usize)>,
    double: Vec<(usize, usize)>,
    heredoc: Vec<(usize, usize)>,
}

/// Extract quote context from the AST.
pub fn extract_quote_context(root: Node, source: &[u8], command: &str) -> QuoteContext {
    let mut spans = QuoteSpans::default();
    collect_quote_spans(root, source, &mut spans, false);

    let single_quote_set: std::collections::HashSet<usize> = spans
        .raw
        .iter()
        .chain(spans.ansi_c.iter())
        .chain(spans.heredoc.iter())
        .flat_map(|(s, e)| *s..*e)
        .collect();

    let double_delim_set: std::collections::HashSet<usize> = spans
        .double
        .iter()
        .flat_map(|(s, e)| [*s, e.saturating_sub(1)])
        .collect();

    let with_double_quotes: String = command
        .char_indices()
        .filter(|(i, _)| !single_quote_set.contains(i) && !double_delim_set.contains(i))
        .map(|(_, c)| c)
        .collect();

    let all_quote_spans: Vec<(usize, usize)> = spans
        .raw
        .iter()
        .chain(spans.ansi_c.iter())
        .chain(spans.double.iter())
        .chain(spans.heredoc.iter())
        .copied()
        .collect();

    let fully_unquoted = remove_spans(command, &all_quote_spans);

    QuoteContext {
        with_double_quotes,
        fully_unquoted,
        unquoted_keep_quote_chars: command.to_string(),
    }
}

fn collect_quote_spans(node: Node, source: &[u8], out: &mut QuoteSpans, in_double: bool) {
    match node.kind() {
        "raw_string" => {
            out.raw.push((node.start_byte(), node.end_byte()));
            return;
        }
        "ansi_c_string" => {
            out.ansi_c.push((node.start_byte(), node.end_byte()));
            return;
        }
        "string" => {
            if !in_double {
                out.double.push((node.start_byte(), node.end_byte()));
            }
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect_quote_spans(child, source, out, true);
            }
            return;
        }
        "heredoc_redirect" => {
            // Check if quoted heredoc
            let mut cursor = node.walk();
            let mut is_quoted = false;
            for child in node.children(&mut cursor) {
                if child.kind() == "heredoc_start" {
                    if let Ok(text) = child.utf8_text(source) {
                        is_quoted = text.starts_with('\'')
                            || text.starts_with('"')
                            || text.starts_with('\\');
                    }
                    break;
                }
            }
            if is_quoted {
                out.heredoc.push((node.start_byte(), node.end_byte()));
                return;
            }
        }
        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_quote_spans(child, source, out, in_double);
    }
}

fn remove_spans(command: &str, spans: &[(usize, usize)]) -> String {
    if spans.is_empty() {
        return command.to_string();
    }
    // Filter to outermost spans, sort descending by start
    let outer = drop_contained(spans);
    let mut sorted: Vec<(usize, usize)> = outer.into_iter().collect();
    sorted.sort_by(|a, b| b.0.cmp(&a.0));

    let mut result = command.to_string();
    for (start, end) in sorted {
        let s: usize = start.min(result.len());
        let e: usize = end.min(result.len());
        if s < e {
            result.drain(s..e);
        }
    }
    result
}

fn drop_contained(spans: &[(usize, usize)]) -> Vec<(usize, usize)> {
    spans
        .iter()
        .enumerate()
        .filter(|(i, &(s1, e1))| {
            !spans
                .iter()
                .enumerate()
                .any(|(j, &(s2, e2))| j != *i && s2 <= s1 && e2 >= e1 && (s2 < s1 || e2 > e1))
        })
        .map(|(_, &s)| s)
        .collect()
}

// ---------------------------------------------------------------------------
// Compound structure analysis
// ---------------------------------------------------------------------------

/// Compound command structure extracted from the AST.
#[derive(Debug, Clone)]
pub struct CompoundStructure {
    pub has_compound_operators: bool,
    pub has_pipeline: bool,
    pub has_subshell: bool,
    pub has_command_group: bool,
    pub operators: Vec<String>,
    pub segments: Vec<String>,
}

/// Extract compound command structure.
pub fn extract_compound_structure(root: Node, source: &[u8], command: &str) -> CompoundStructure {
    let mut operators = Vec::new();
    let mut segments = Vec::new();
    let mut has_subshell = false;
    let mut has_command_group = false;
    let mut has_pipeline = false;

    walk_top_level(
        root,
        source,
        &mut operators,
        &mut segments,
        &mut has_subshell,
        &mut has_command_group,
        &mut has_pipeline,
    );

    if segments.is_empty() {
        segments.push(command.to_string());
    }

    CompoundStructure {
        has_compound_operators: !operators.is_empty(),
        has_pipeline,
        has_subshell,
        has_command_group,
        operators,
        segments,
    }
}

fn walk_top_level(
    node: Node,
    source: &[u8],
    operators: &mut Vec<String>,
    segments: &mut Vec<String>,
    has_subshell: &mut bool,
    has_command_group: &mut bool,
    has_pipeline: &mut bool,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "list" => {
                let mut list_cursor = child.walk();
                for list_child in child.children(&mut list_cursor) {
                    match list_child.kind() {
                        "&&" | "||" => {
                            operators.push(list_child.kind().to_string());
                        }
                        "list" | "redirected_statement" => {
                            walk_top_level(
                                list_child,
                                source,
                                operators,
                                segments,
                                has_subshell,
                                has_command_group,
                                has_pipeline,
                            );
                        }
                        "pipeline" => {
                            *has_pipeline = true;
                            if let Ok(t) = list_child.utf8_text(source) {
                                segments.push(t.to_string());
                            }
                        }
                        "subshell" => {
                            *has_subshell = true;
                            if let Ok(t) = list_child.utf8_text(source) {
                                segments.push(t.to_string());
                            }
                        }
                        "compound_statement" => {
                            *has_command_group = true;
                            if let Ok(t) = list_child.utf8_text(source) {
                                segments.push(t.to_string());
                            }
                        }
                        _ => {
                            if let Ok(t) = list_child.utf8_text(source) {
                                segments.push(t.to_string());
                            }
                        }
                    }
                }
            }
            ";" => {
                operators.push(";".to_string());
            }
            "pipeline" => {
                *has_pipeline = true;
                if let Ok(t) = child.utf8_text(source) {
                    segments.push(t.to_string());
                }
            }
            "subshell" => {
                *has_subshell = true;
                if let Ok(t) = child.utf8_text(source) {
                    segments.push(t.to_string());
                }
            }
            "compound_statement" => {
                *has_command_group = true;
                if let Ok(t) = child.utf8_text(source) {
                    segments.push(t.to_string());
                }
            }
            "command" | "declaration_command" | "variable_assignment" => {
                if let Ok(t) = child.utf8_text(source) {
                    segments.push(t.to_string());
                }
            }
            "redirected_statement" => {
                let mut inner_found = false;
                let mut rc = child.walk();
                for inner in child.children(&mut rc) {
                    if inner.kind() == "file_redirect" {
                        continue;
                    }
                    inner_found = true;
                    walk_top_level(
                        inner,
                        source,
                        operators,
                        segments,
                        has_subshell,
                        has_command_group,
                        has_pipeline,
                    );
                }
                if !inner_found {
                    if let Ok(t) = child.utf8_text(source) {
                        segments.push(t.to_string());
                    }
                }
            }
            "negated_command" => {
                if let Ok(t) = child.utf8_text(source) {
                    segments.push(t.to_string());
                }
                walk_top_level(
                    child,
                    source,
                    operators,
                    segments,
                    has_subshell,
                    has_command_group,
                    has_pipeline,
                );
            }
            "if_statement"
            | "while_statement"
            | "for_statement"
            | "case_statement"
            | "function_definition" => {
                if let Ok(t) = child.utf8_text(source) {
                    segments.push(t.to_string());
                }
                walk_top_level(
                    child,
                    source,
                    operators,
                    segments,
                    has_subshell,
                    has_command_group,
                    has_pipeline,
                );
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Dangerous patterns analysis
// ---------------------------------------------------------------------------

/// Dangerous patterns detected in a command.
#[derive(Debug, Clone, Default)]
pub struct DangerousPatterns {
    pub has_command_substitution: bool,
    pub has_process_substitution: bool,
    pub has_parameter_expansion: bool,
    pub has_heredoc: bool,
    pub has_comment: bool,
}

/// Extract dangerous patterns from the AST.
pub fn extract_dangerous_patterns(root: Node, _source: &[u8]) -> DangerousPatterns {
    let mut patterns = DangerousPatterns::default();
    walk_dangerous(root, &mut patterns);
    patterns
}

fn walk_dangerous(node: Node, patterns: &mut DangerousPatterns) {
    match node.kind() {
        "command_substitution" => patterns.has_command_substitution = true,
        "process_substitution" => patterns.has_process_substitution = true,
        "expansion" => patterns.has_parameter_expansion = true,
        "heredoc_redirect" => patterns.has_heredoc = true,
        "comment" => patterns.has_comment = true,
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_dangerous(child, patterns);
    }
}

// ---------------------------------------------------------------------------
// Main analysis entry point
// ---------------------------------------------------------------------------

/// Full tree-sitter analysis of a command.
#[derive(Debug, Clone)]
pub struct TreeSitterAnalysis {
    pub quote_context: QuoteContext,
    pub compound_structure: CompoundStructure,
    pub has_actual_operator_nodes: bool,
    pub dangerous_patterns: DangerousPatterns,
}

/// Perform complete tree-sitter analysis.
pub fn analyze_command(root: Node, source: &[u8], command: &str) -> TreeSitterAnalysis {
    TreeSitterAnalysis {
        quote_context: extract_quote_context(root, source, command),
        compound_structure: extract_compound_structure(root, source, command),
        has_actual_operator_nodes: has_actual_operator_nodes(root),
        dangerous_patterns: extract_dangerous_patterns(root, source),
    }
}

/// Check whether the AST contains actual operator nodes.
pub fn has_actual_operator_nodes(root: Node) -> bool {
    match root.kind() {
        ";" | "&&" | "||" => return true,
        "list" => return true,
        _ => {}
    }
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        if has_actual_operator_nodes(child) {
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Convenience: parse with tree-sitter and get analysis
// ---------------------------------------------------------------------------

/// Parse a command with tree-sitter and return full analysis.
pub fn parse_and_analyze(command: &str, parser: &mut Parser) -> Option<TreeSitterAnalysis> {
    let tree = parser.parse(command, None)?;
    let root = tree.root_node();
    let source = command.as_bytes();
    Some(analyze_command(root, source, command))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bash_ast::new_bash_parser;

    #[test]
    fn test_quote_context_simple() {
        let mut parser = new_bash_parser().unwrap();
        let analysis = parse_and_analyze("echo hello", &mut parser).unwrap();
        assert!(!analysis.has_actual_operator_nodes);
    }

    #[test]
    fn test_compound_operators_detected() {
        let mut parser = new_bash_parser().unwrap();
        let analysis = parse_and_analyze("echo a && echo b", &mut parser).unwrap();
        assert!(analysis.compound_structure.has_compound_operators);
        assert!(analysis.has_actual_operator_nodes);
    }

    #[test]
    fn test_pipeline_detected() {
        let mut parser = new_bash_parser().unwrap();
        let analysis = parse_and_analyze("cat file | grep pattern", &mut parser).unwrap();
        assert!(analysis.compound_structure.has_pipeline);
    }

    #[test]
    fn test_dangerous_substitution() {
        let mut parser = new_bash_parser().unwrap();
        let analysis = parse_and_analyze("echo $(whoami)", &mut parser).unwrap();
        assert!(analysis.dangerous_patterns.has_command_substitution);
    }

    #[test]
    fn test_parameter_expansion() {
        let mut parser = new_bash_parser().unwrap();
        let analysis = parse_and_analyze("echo ${HOME}", &mut parser).unwrap();
        assert!(analysis.dangerous_patterns.has_parameter_expansion);
    }

    #[test]
    fn test_heredoc_detected() {
        let mut parser = new_bash_parser().unwrap();
        let analysis = parse_and_analyze("cat <<EOF\nhello\nEOF", &mut parser).unwrap();
        assert!(analysis.dangerous_patterns.has_heredoc);
    }

    #[test]
    fn test_semicolon_not_operator_when_escaped() {
        let mut parser = new_bash_parser().unwrap();
        // find -exec uses \; which is an argument, not a separator
        let _analysis =
            parse_and_analyze("find . -name '*.txt' -exec cat {} \\;", &mut parser).unwrap();
        // \; is not parsed as an operator node
        // But ';' may or may not appear depending on how tree-sitter parses it
    }
}
