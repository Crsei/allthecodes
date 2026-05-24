//! LSP tool — code intelligence via Language Server Protocol.
//!
//! Corresponds to TypeScript: tools/LSPTool/LSPTool.ts
//!
//! Provides code navigation and live-editor operations through a unified interface:
//! - goToDefinition, goToImplementation
//! - findReferences
//! - hover
//! - documentSymbol, workspaceSymbol
//! - prepareCallHierarchy, incomingCalls, outgoingCalls
//! - completion, diagnostics
//!
//! The tool converts 1-based editor coordinates to 0-based LSP protocol
//! coordinates and formats results for the model.
//!
//! Depends on the `lsp-types` crate.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::{json, Value};

use allthecodes_tools::tool::*;
use allthecodes_types::message::AssistantMessage;

// ---------------------------------------------------------------------------
// LSP operations
// ---------------------------------------------------------------------------

/// Supported LSP operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LspOperation {
    GoToDefinition,
    GoToImplementation,
    FindReferences,
    Hover,
    DocumentSymbol,
    WorkspaceSymbol,
    PrepareCallHierarchy,
    IncomingCalls,
    OutgoingCalls,
    Completion,
    Diagnostics,
}

impl LspOperation {
    /// Parse an operation name from string.
    pub fn parse(s: &str) -> Option<Self> {
        s.parse().ok()
    }

    /// Get the LSP method name for this operation.
    pub fn method(&self) -> &'static str {
        match self {
            Self::GoToDefinition => "textDocument/definition",
            Self::GoToImplementation => "textDocument/implementation",
            Self::FindReferences => "textDocument/references",
            Self::Hover => "textDocument/hover",
            Self::DocumentSymbol => "textDocument/documentSymbol",
            Self::WorkspaceSymbol => "workspace/symbol",
            Self::PrepareCallHierarchy => "textDocument/prepareCallHierarchy",
            Self::IncomingCalls => "callHierarchy/incomingCalls",
            Self::OutgoingCalls => "callHierarchy/outgoingCalls",
            Self::Completion => "textDocument/completion",
            Self::Diagnostics => "textDocument/publishDiagnostics",
        }
    }

    /// Whether this operation requires file position (line + character).
    pub fn requires_position(&self) -> bool {
        matches!(
            self,
            Self::GoToDefinition
                | Self::GoToImplementation
                | Self::FindReferences
                | Self::Hover
                | Self::PrepareCallHierarchy
                | Self::IncomingCalls
                | Self::OutgoingCalls
                | Self::Completion
        )
    }

    /// All valid operation names.
    pub fn all_names() -> &'static [&'static str] {
        &[
            "goToDefinition",
            "goToImplementation",
            "findReferences",
            "hover",
            "documentSymbol",
            "workspaceSymbol",
            "prepareCallHierarchy",
            "incomingCalls",
            "outgoingCalls",
            "completion",
            "diagnostics",
        ]
    }
}

impl std::str::FromStr for LspOperation {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "goToDefinition" => Ok(Self::GoToDefinition),
            "goToImplementation" => Ok(Self::GoToImplementation),
            "findReferences" => Ok(Self::FindReferences),
            "hover" => Ok(Self::Hover),
            "documentSymbol" => Ok(Self::DocumentSymbol),
            "workspaceSymbol" => Ok(Self::WorkspaceSymbol),
            "prepareCallHierarchy" => Ok(Self::PrepareCallHierarchy),
            "incomingCalls" => Ok(Self::IncomingCalls),
            "outgoingCalls" => Ok(Self::OutgoingCalls),
            "completion" => Ok(Self::Completion),
            "diagnostics" => Ok(Self::Diagnostics),
            _ => Err(()),
        }
    }
}

// ---------------------------------------------------------------------------
// LSP location types
// ---------------------------------------------------------------------------
//
pub use crate::types::{CompletionItemInfo, HoverInfo, LspDiagnostic, SourceLocation, SymbolInfo};

// ---------------------------------------------------------------------------
// Result formatting
// ---------------------------------------------------------------------------

fn format_locations(locations: &[SourceLocation]) -> String {
    if locations.is_empty() {
        return "No results found.".to_string();
    }

    let mut lines = Vec::new();
    // Group by file
    let mut by_file: std::collections::HashMap<&str, Vec<&SourceLocation>> =
        std::collections::HashMap::new();
    for loc in locations {
        by_file.entry(&loc.file_path).or_default().push(loc);
    }

    for (file, locs) in &by_file {
        lines.push(format!("**{}**", file));
        for loc in locs {
            lines.push(format!("  Line {}, Col {}", loc.line, loc.character));
        }
    }
    lines.join("\n")
}

fn format_symbols(symbols: &[SymbolInfo], indent: usize) -> String {
    let mut lines = Vec::new();
    let prefix = "  ".repeat(indent);
    for sym in symbols {
        lines.push(format!(
            "{}{} `{}` (line {})",
            prefix, sym.kind, sym.name, sym.location.line
        ));
        if !sym.children.is_empty() {
            lines.push(format_symbols(&sym.children, indent + 1));
        }
    }
    lines.join("\n")
}

fn format_hover(hover: &HoverInfo) -> String {
    if hover.contents.is_empty() {
        return "No hover information available.".to_string();
    }
    hover.contents.clone()
}

fn format_completions(items: &[CompletionItemInfo]) -> String {
    if items.is_empty() {
        return "No completion items available.".to_string();
    }

    items
        .iter()
        .take(50)
        .map(|item| {
            let kind = item.kind.as_deref().unwrap_or("Item");
            match item.detail.as_deref() {
                Some(detail) if !detail.is_empty() => {
                    format!("- {} `{}` - {}", kind, item.label, detail)
                }
                _ => format!("- {} `{}`", kind, item.label),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_diagnostics(diagnostics: &[LspDiagnostic]) -> String {
    if diagnostics.is_empty() {
        return "No diagnostics for this document.".to_string();
    }

    diagnostics
        .iter()
        .map(|diagnostic| {
            let source = diagnostic
                .source
                .as_deref()
                .filter(|source| !source.is_empty())
                .map(|source| format!(" ({source})"))
                .unwrap_or_default();
            format!(
                "- {}{} at line {}, col {}: {}",
                diagnostic.severity,
                source,
                diagnostic.range.start_line,
                diagnostic.range.start_character,
                diagnostic.message
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------------------------------------------------------------------------
// Tool implementation
// ---------------------------------------------------------------------------

/// Maximum file size for LSP operations (10 MB).
const MAX_LSP_FILE_SIZE: u64 = 10_000_000;

pub struct LspTool;

#[async_trait]
impl Tool for LspTool {
    fn name(&self) -> &str {
        "LSP"
    }

    async fn description(&self, _: &Value) -> String {
        "Code intelligence via Language Server Protocol.".to_string()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "operation": {
                    "type": "string",
                    "enum": LspOperation::all_names(),
                    "description": "The LSP operation to perform."
                },
                "filePath": {
                    "type": "string",
                    "description": "Absolute or relative path to the file."
                },
                "line": {
                    "type": "number",
                    "description": "Line number (1-based, as shown in editors)."
                },
                "character": {
                    "type": "number",
                    "description": "Column number (1-based, as shown in editors)."
                },
                "triggerCharacter": {
                    "type": "string",
                    "description": "Optional completion trigger character."
                }
            },
            "required": ["operation", "filePath"]
        })
    }

    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }

    fn is_read_only(&self, _: &Value) -> bool {
        true
    }

    fn is_enabled(&self) -> bool {
        true
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        let op_str = input
            .get("operation")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let Some(op) = LspOperation::parse(op_str) else {
            return ValidationResult::Error {
                message: format!(
                    "Invalid operation '{}'. Must be one of: {}",
                    op_str,
                    LspOperation::all_names().join(", ")
                ),
                error_code: 400,
            };
        };

        let file_path = input.get("filePath").and_then(|v| v.as_str()).unwrap_or("");
        if file_path.is_empty() {
            return ValidationResult::Error {
                message: "filePath is required".to_string(),
                error_code: 400,
            };
        }

        if op.requires_position() {
            let line = input.get("line").and_then(|v| v.as_u64());
            let character = input.get("character").and_then(|v| v.as_u64());
            if line.is_none() || character.is_none() {
                // documentSymbol doesn't need position
                if op != LspOperation::DocumentSymbol {
                    return ValidationResult::Error {
                        message: format!(
                            "Operation '{}' requires line and character parameters",
                            op_str
                        ),
                        error_code: 400,
                    };
                }
            }
        }

        ValidationResult::Ok
    }

    async fn call(
        &self,
        input: Value,
        ctx: &ToolUseContext,
        _parent: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let op_str = input
            .get("operation")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let op = LspOperation::parse(op_str).context("Invalid LSP operation")?;

        let file_path = input.get("filePath").and_then(|v| v.as_str()).unwrap_or("");

        // Convert 1-based editor coords to 0-based LSP coords
        let line = input
            .get("line")
            .and_then(|v| v.as_u64())
            .map(|l| l.saturating_sub(1) as u32)
            .unwrap_or(0);
        let character = input
            .get("character")
            .and_then(|v| v.as_u64())
            .map(|c| c.saturating_sub(1) as u32)
            .unwrap_or(0);
        let trigger_character = input
            .get("triggerCharacter")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        // Resolve file path
        let resolved = resolve_file_path(file_path, ctx);
        if !resolved.exists() {
            return Ok(ToolResult {
                data: json!({
                    "error": format!("File not found: {}", resolved.display()),
                    "operation": op_str,
                }),
                new_messages: vec![],
                ..Default::default()
            });
        }

        // Check file size
        if let Ok(meta) = std::fs::metadata(&resolved) {
            if meta.len() > MAX_LSP_FILE_SIZE {
                return Ok(ToolResult {
                    data: json!({
                        "error": format!(
                            "File too large ({} bytes, limit {} bytes)",
                            meta.len(),
                            MAX_LSP_FILE_SIZE
                        ),
                        "operation": op_str,
                    }),
                    new_messages: vec![],
                    ..Default::default()
                });
            }
        }

        // ---- LSP execution ----
        let result = execute_lsp_operation(op, &resolved, line, character, trigger_character).await;

        match result {
            Ok(output) => Ok(ToolResult {
                data: output,
                new_messages: vec![],
                ..Default::default()
            }),
            Err(e) => Ok(ToolResult {
                data: json!({
                    "error": format!("LSP operation failed: {}", e),
                    "operation": op_str,
                    "filePath": resolved.display().to_string(),
                }),
                new_messages: vec![],
                ..Default::default()
            }),
        }
    }

    async fn prompt(&self) -> String {
        r#"Provides code intelligence features via Language Server Protocol (LSP).

Supported operations:
- goToDefinition: Find where a symbol is defined
- goToImplementation: Find implementations of an interface/trait
- findReferences: Find all references to a symbol
- hover: Get type information and documentation
- documentSymbol: List all symbols in a file
- workspaceSymbol: Search for symbols across the workspace
- prepareCallHierarchy: Get the call hierarchy for a function
- incomingCalls: Find callers of a function
- outgoingCalls: Find functions called by a function
- completion: Get completion suggestions at a live editor position
- diagnostics: Read cached diagnostics for a file

All operations require filePath. Most require line and character (1-based).
Requires LSP servers to be configured for the file type."#
            .to_string()
    }

    fn user_facing_name(&self, input: Option<&Value>) -> String {
        if let Some(op) = input
            .and_then(|v| v.get("operation"))
            .and_then(|v| v.as_str())
        {
            return format!("LSP({})", op);
        }
        "LSP".to_string()
    }
}

// ---------------------------------------------------------------------------
// File path resolution
// ---------------------------------------------------------------------------

fn resolve_file_path(file_path: &str, ctx: &ToolUseContext) -> PathBuf {
    let p = Path::new(file_path);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        // Try to get CWD from app state
        let _state = (ctx.get_app_state)();
        // Fallback to current dir
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(file_path)
    }
}

// ---------------------------------------------------------------------------
// LSP execution
// ---------------------------------------------------------------------------

async fn execute_lsp_operation(
    op: LspOperation,
    file_path: &Path,
    line: u32,
    character: u32,
    trigger_character: Option<String>,
) -> Result<Value> {
    let uri = file_path_to_uri(file_path);

    match op {
        LspOperation::GoToDefinition => {
            let locations = crate::go_to_definition(&uri, line, character).await?;
            Ok(json!({
                "operation": "goToDefinition",
                "filePath": file_path.display().to_string(),
                "result": format_locations(&locations),
                "resultCount": locations.len(),
            }))
        }
        LspOperation::GoToImplementation => {
            let locations = crate::go_to_implementation(&uri, line, character).await?;
            Ok(json!({
                "operation": "goToImplementation",
                "filePath": file_path.display().to_string(),
                "result": format_locations(&locations),
                "resultCount": locations.len(),
            }))
        }
        LspOperation::FindReferences => {
            let locations = crate::find_references(&uri, line, character).await?;
            let file_count = locations
                .iter()
                .map(|l| &l.file_path)
                .collect::<std::collections::HashSet<_>>()
                .len();
            Ok(json!({
                "operation": "findReferences",
                "filePath": file_path.display().to_string(),
                "result": format_locations(&locations),
                "resultCount": locations.len(),
                "fileCount": file_count,
            }))
        }
        LspOperation::Hover => {
            let hover = crate::hover(&uri, line, character).await?;
            Ok(json!({
                "operation": "hover",
                "filePath": file_path.display().to_string(),
                "result": format_hover(&hover),
            }))
        }
        LspOperation::DocumentSymbol => {
            let symbols = crate::document_symbols(&uri).await?;
            Ok(json!({
                "operation": "documentSymbol",
                "filePath": file_path.display().to_string(),
                "result": format_symbols(&symbols, 0),
                "resultCount": symbols.len(),
            }))
        }
        LspOperation::WorkspaceSymbol => {
            // For workspace symbol, use the file_path as a query hint
            let query = file_path.to_string_lossy();
            let symbols = crate::workspace_symbols(&query).await?;
            Ok(json!({
                "operation": "workspaceSymbol",
                "result": format_symbols(&symbols, 0),
                "resultCount": symbols.len(),
            }))
        }
        LspOperation::PrepareCallHierarchy => {
            let items = crate::prepare_call_hierarchy(&uri, line, character).await?;
            Ok(json!({
                "operation": "prepareCallHierarchy",
                "filePath": file_path.display().to_string(),
                "result": format_symbols(&items, 0),
                "resultCount": items.len(),
            }))
        }
        LspOperation::IncomingCalls => {
            let items = crate::prepare_call_hierarchy(&uri, line, character).await?;
            if items.is_empty() {
                return Ok(json!({
                    "operation": "incomingCalls",
                    "result": "No call hierarchy items found at this position.",
                }));
            }
            let calls = crate::incoming_calls(&items[0]).await?;
            Ok(json!({
                "operation": "incomingCalls",
                "filePath": file_path.display().to_string(),
                "result": format_symbols(&calls, 0),
                "resultCount": calls.len(),
            }))
        }
        LspOperation::OutgoingCalls => {
            let items = crate::prepare_call_hierarchy(&uri, line, character).await?;
            if items.is_empty() {
                return Ok(json!({
                    "operation": "outgoingCalls",
                    "result": "No call hierarchy items found at this position.",
                }));
            }
            let calls = crate::outgoing_calls(&items[0]).await?;
            Ok(json!({
                "operation": "outgoingCalls",
                "filePath": file_path.display().to_string(),
                "result": format_symbols(&calls, 0),
                "resultCount": calls.len(),
            }))
        }
        LspOperation::Completion => {
            let items = crate::completion(&uri, line, character, trigger_character).await?;
            Ok(json!({
                "operation": "completion",
                "filePath": file_path.display().to_string(),
                "result": format_completions(&items),
                "resultCount": items.len(),
                "items": items,
            }))
        }
        LspOperation::Diagnostics => {
            let diagnostics = crate::diagnostics_snapshot(Some(&uri))
                .into_iter()
                .next()
                .map(|(_, diagnostics)| diagnostics)
                .unwrap_or_default();
            Ok(json!({
                "operation": "diagnostics",
                "filePath": file_path.display().to_string(),
                "result": format_diagnostics(&diagnostics),
                "resultCount": diagnostics.len(),
                "diagnostics": diagnostics,
            }))
        }
    }
}

fn file_path_to_uri(path: &Path) -> String {
    // file:///path/to/file
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
    format!(
        "file:///{}",
        abs.to_string_lossy()
            .replace('\\', "/")
            .trim_start_matches('/')
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lsp_operation_from_str() {
        assert_eq!(
            LspOperation::parse("goToDefinition"),
            Some(LspOperation::GoToDefinition)
        );
        assert_eq!(LspOperation::parse("hover"), Some(LspOperation::Hover));
        assert_eq!(
            LspOperation::parse("findReferences"),
            Some(LspOperation::FindReferences)
        );
        assert_eq!(LspOperation::parse("invalid"), None);
    }

    #[test]
    fn test_lsp_operation_method() {
        assert_eq!(
            LspOperation::GoToDefinition.method(),
            "textDocument/definition"
        );
        assert_eq!(LspOperation::WorkspaceSymbol.method(), "workspace/symbol");
        assert_eq!(LspOperation::Completion.method(), "textDocument/completion");
    }

    #[test]
    fn test_requires_position() {
        assert!(LspOperation::GoToDefinition.requires_position());
        assert!(LspOperation::Hover.requires_position());
        assert!(LspOperation::Completion.requires_position());
        assert!(!LspOperation::WorkspaceSymbol.requires_position());
        assert!(!LspOperation::DocumentSymbol.requires_position());
        assert!(!LspOperation::Diagnostics.requires_position());
    }

    #[test]
    fn test_all_names() {
        let names = LspOperation::all_names();
        assert_eq!(names.len(), 11);
        assert!(names.contains(&"goToDefinition"));
        assert!(names.contains(&"outgoingCalls"));
        assert!(names.contains(&"completion"));
        assert!(names.contains(&"diagnostics"));
    }

    #[test]
    fn test_format_locations_empty() {
        assert_eq!(format_locations(&[]), "No results found.");
    }

    #[test]
    fn test_format_locations() {
        let locs = vec![
            SourceLocation {
                file_path: "src/main.rs".into(),
                line: 10,
                character: 5,
                end_line: None,
                end_character: None,
            },
            SourceLocation {
                file_path: "src/main.rs".into(),
                line: 20,
                character: 1,
                end_line: None,
                end_character: None,
            },
        ];
        let text = format_locations(&locs);
        assert!(text.contains("src/main.rs"));
        assert!(text.contains("Line 10"));
        assert!(text.contains("Line 20"));
    }

    #[test]
    fn test_format_symbols() {
        let symbols = vec![SymbolInfo {
            name: "main".into(),
            kind: "function".into(),
            location: SourceLocation {
                file_path: "src/main.rs".into(),
                line: 1,
                character: 1,
                end_line: None,
                end_character: None,
            },
            children: vec![],
        }];
        let text = format_symbols(&symbols, 0);
        assert!(text.contains("function"));
        assert!(text.contains("`main`"));
    }

    #[test]
    fn test_format_hover_empty() {
        let hover = HoverInfo {
            contents: String::new(),
            range: None,
        };
        assert_eq!(format_hover(&hover), "No hover information available.");
    }

    #[tokio::test]
    async fn test_lsp_tool_basics() {
        let tool = LspTool;
        assert_eq!(tool.name(), "LSP");
        assert!(tool.is_read_only(&json!({})));
        assert!(tool.is_concurrency_safe(&json!({})));

        let schema = tool.input_json_schema();
        assert!(schema["properties"]["operation"].is_object());
        assert!(schema["properties"]["filePath"].is_object());
        assert!(schema["properties"]["line"].is_object());
    }

    #[test]
    fn test_user_facing_name() {
        let tool = LspTool;
        let input = json!({"operation": "hover", "filePath": "test.rs"});
        assert_eq!(tool.user_facing_name(Some(&input)), "LSP(hover)");
        assert_eq!(tool.user_facing_name(None), "LSP");
    }
}
