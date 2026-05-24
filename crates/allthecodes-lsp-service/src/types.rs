//! Shared LSP result types used by both the LSP service (this crate) and the
//! `Lsp` tool wrapper in `lsp_service::tool`.
//!
//! Previously these lived in `tools::lsp`, which produced a
//! `lsp_service -> tools` edge that blocked Phase 6 crate extraction. Moving
//! them here reverses the direction: `lsp_service::tool` now imports from
//! `lsp_service::types`, which is the natural direction given the tool is a
//! thin wrapper over the service.

use serde::{Deserialize, Serialize};

/// Ranged document edit expressed with 1-based line/character offsets.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentChange {
    pub range: SourceRange,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range_length: Option<u32>,
    pub text: String,
}

/// Character range within a source document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceRange {
    pub start_line: u32,
    pub start_character: u32,
    pub end_line: u32,
    pub end_character: u32,
}

/// Completion item projection returned by the LSP tool adapter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionItemInfo {
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub documentation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub insert_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter_text: Option<String>,
}

/// A location in a source file (simplified LSP Location).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceLocation {
    pub file_path: String,
    pub line: u32,      // 1-based
    pub character: u32, // 1-based
    pub end_line: Option<u32>,
    pub end_character: Option<u32>,
}

/// A symbol in a document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolInfo {
    pub name: String,
    pub kind: String,
    pub location: SourceLocation,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<SymbolInfo>,
}

/// Hover information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HoverInfo {
    pub contents: String,
    pub range: Option<SourceLocation>,
}

/// Snapshot of one live document tracked by an LSP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentSyncState {
    pub uri: String,
    pub language_id: String,
    pub version: i32,
}

/// A single diagnostic reported by an LSP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspDiagnostic {
    pub range: DiagnosticRange,
    pub severity: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

/// Diagnostic range expressed as 1-based line/character offsets.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticRange {
    pub start_line: u32,
    pub start_character: u32,
    pub end_line: u32,
    pub end_character: u32,
}

/// Status information for an LSP server instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspServerInfo {
    pub language_id: String,
    pub state: String,
    pub extensions: Vec<String>,
    pub open_files_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Events emitted by the standalone LSP service.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LspEvent {
    ServerStateChanged {
        language_id: String,
        state: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    DocumentSynced {
        uri: String,
        language_id: String,
        version: i32,
        change_kind: String,
    },
    DiagnosticsPublished {
        uri: String,
        diagnostics: Vec<LspDiagnostic>,
    },
    CompletionResults {
        request_id: String,
        uri: String,
        items: Vec<CompletionItemInfo>,
    },
    CommandError {
        request_id: Option<String>,
        message: String,
    },
}
