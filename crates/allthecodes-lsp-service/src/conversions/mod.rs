//! Conversions from `lsp-types` protocol types to internal tool-layer types.
//!
//! All LSP positions are **0-based**; our [`SourceLocation`] is **1-based**.
//! Every conversion adds 1 to line and character values.

use anyhow::{bail, Context, Result};
use serde_json::Value;

use crate::types::{CompletionItemInfo, HoverInfo, SourceLocation, SymbolInfo};

// ---------------------------------------------------------------------------
// URI helpers
// ---------------------------------------------------------------------------

/// Convert an `lsp_types::Uri` (file:// URI) to a file-system path string.
///
/// On Windows, strips the leading `/` from paths like `file:///C:/foo`.
pub fn uri_to_file_path(uri: &lsp_types::Uri) -> String {
    let s = uri.as_str();
    let path = if let Some(rest) = s.strip_prefix("file:///") {
        // Percent-decode is not performed here — LSP servers typically send
        // plain paths.  On Windows the URI looks like file:///C:/foo, so we
        // keep the drive letter intact.
        #[cfg(windows)]
        {
            rest.replace('/', "\\")
        }
        #[cfg(not(windows))]
        {
            format!("/{}", rest)
        }
    } else if let Some(rest) = s.strip_prefix("file://") {
        // file://host/path — rare, treat everything after `//` as path.
        rest.to_string()
    } else {
        // Fallback: return as-is.
        s.to_string()
    };
    path
}

/// Convert a file-system path string to an `lsp_types::Uri`.
///
/// Handles Windows backslashes by converting them to forward slashes.
pub fn file_path_to_uri(path: &str) -> Result<lsp_types::Uri> {
    let normalized = path.replace('\\', "/");
    // Ensure an absolute path starts with `/` (Unix) or a drive letter (Windows).
    let uri_string = if normalized.starts_with('/') {
        format!("file://{}", normalized)
    } else {
        // Windows absolute path like `C:/foo` → `file:///C:/foo`
        format!("file:///{}", normalized)
    };
    uri_string
        .parse::<lsp_types::Uri>()
        .map_err(|e| anyhow::anyhow!("failed to parse URI '{}': {}", uri_string, e))
}

// ---------------------------------------------------------------------------
// Location conversions
// ---------------------------------------------------------------------------

/// Convert an `lsp_types::Location` to a [`SourceLocation`] (0-based → 1-based).
pub fn location_to_source(loc: &lsp_types::Location) -> SourceLocation {
    SourceLocation {
        file_path: uri_to_file_path(&loc.uri),
        line: loc.range.start.line + 1,
        character: loc.range.start.character + 1,
        end_line: Some(loc.range.end.line + 1),
        end_character: Some(loc.range.end.character + 1),
    }
}

/// Convert an `lsp_types::LocationLink` to a [`SourceLocation`] (0-based → 1-based).
///
/// Uses `target_uri` and `target_selection_range` for the location.
pub fn location_link_to_source(link: &lsp_types::LocationLink) -> SourceLocation {
    SourceLocation {
        file_path: uri_to_file_path(&link.target_uri),
        line: link.target_selection_range.start.line + 1,
        character: link.target_selection_range.start.character + 1,
        end_line: Some(link.target_selection_range.end.line + 1),
        end_character: Some(link.target_selection_range.end.character + 1),
    }
}

/// Parse a JSON response that may be `null`, a single `Location`, a
/// `Location[]`, or a `LocationLink[]`.
///
/// This corresponds to the `GotoDefinitionResponse` union and similar
/// response types in the LSP specification.
pub fn parse_location_response(value: Value) -> Result<Vec<SourceLocation>> {
    if value.is_null() {
        return Ok(vec![]);
    }

    // Try GotoDefinitionResponse (Scalar | Array | Link)
    if let Ok(resp) = serde_json::from_value::<lsp_types::GotoDefinitionResponse>(value.clone()) {
        return Ok(match resp {
            lsp_types::GotoDefinitionResponse::Scalar(loc) => {
                vec![location_to_source(&loc)]
            }
            lsp_types::GotoDefinitionResponse::Array(locs) => {
                locs.iter().map(location_to_source).collect()
            }
            lsp_types::GotoDefinitionResponse::Link(links) => {
                links.iter().map(location_link_to_source).collect()
            }
        });
    }

    // Try single Location
    if let Ok(loc) = serde_json::from_value::<lsp_types::Location>(value.clone()) {
        return Ok(vec![location_to_source(&loc)]);
    }

    // Try Location[]
    if let Ok(locs) = serde_json::from_value::<Vec<lsp_types::Location>>(value.clone()) {
        return Ok(locs.iter().map(location_to_source).collect());
    }

    // Try LocationLink[]
    if let Ok(links) = serde_json::from_value::<Vec<lsp_types::LocationLink>>(value.clone()) {
        return Ok(links.iter().map(location_link_to_source).collect());
    }

    bail!("Unable to parse location response")
}

// ---------------------------------------------------------------------------
// Hover
// ---------------------------------------------------------------------------

/// Extract a plain string from `MarkedString`.
fn marked_string_to_text(ms: &lsp_types::MarkedString) -> String {
    match ms {
        lsp_types::MarkedString::String(s) => s.clone(),
        lsp_types::MarkedString::LanguageString(ls) => {
            format!("```{}\n{}\n```", ls.language, ls.value)
        }
    }
}

/// Convert an `lsp_types::Range` to an optional [`SourceLocation`] (no file path).
fn range_to_source_location(range: &lsp_types::Range) -> SourceLocation {
    SourceLocation {
        file_path: String::new(),
        line: range.start.line + 1,
        character: range.start.character + 1,
        end_line: Some(range.end.line + 1),
        end_character: Some(range.end.character + 1),
    }
}

/// Parse a hover response JSON value into [`HoverInfo`].
///
/// Handles `null`, `Hover { contents: MarkupContent | MarkedString | MarkedString[], range }`.
pub fn parse_hover_response(value: Value) -> Result<HoverInfo> {
    if value.is_null() {
        return Ok(HoverInfo {
            contents: String::new(),
            range: None,
        });
    }

    let hover: lsp_types::Hover =
        serde_json::from_value(value).context("Failed to parse Hover response")?;

    let contents = match &hover.contents {
        lsp_types::HoverContents::Scalar(ms) => marked_string_to_text(ms),
        lsp_types::HoverContents::Array(arr) => arr
            .iter()
            .map(marked_string_to_text)
            .collect::<Vec<_>>()
            .join("\n\n"),
        lsp_types::HoverContents::Markup(mc) => mc.value.clone(),
    };

    let range = hover.range.as_ref().map(range_to_source_location);

    Ok(HoverInfo { contents, range })
}

// ---------------------------------------------------------------------------
// Completion
// ---------------------------------------------------------------------------

fn completion_kind_str(kind: lsp_types::CompletionItemKind) -> &'static str {
    match kind {
        lsp_types::CompletionItemKind::TEXT => "Text",
        lsp_types::CompletionItemKind::METHOD => "Method",
        lsp_types::CompletionItemKind::FUNCTION => "Function",
        lsp_types::CompletionItemKind::CONSTRUCTOR => "Constructor",
        lsp_types::CompletionItemKind::FIELD => "Field",
        lsp_types::CompletionItemKind::VARIABLE => "Variable",
        lsp_types::CompletionItemKind::CLASS => "Class",
        lsp_types::CompletionItemKind::INTERFACE => "Interface",
        lsp_types::CompletionItemKind::MODULE => "Module",
        lsp_types::CompletionItemKind::PROPERTY => "Property",
        lsp_types::CompletionItemKind::UNIT => "Unit",
        lsp_types::CompletionItemKind::VALUE => "Value",
        lsp_types::CompletionItemKind::ENUM => "Enum",
        lsp_types::CompletionItemKind::KEYWORD => "Keyword",
        lsp_types::CompletionItemKind::SNIPPET => "Snippet",
        lsp_types::CompletionItemKind::COLOR => "Color",
        lsp_types::CompletionItemKind::FILE => "File",
        lsp_types::CompletionItemKind::REFERENCE => "Reference",
        lsp_types::CompletionItemKind::FOLDER => "Folder",
        lsp_types::CompletionItemKind::ENUM_MEMBER => "EnumMember",
        lsp_types::CompletionItemKind::CONSTANT => "Constant",
        lsp_types::CompletionItemKind::STRUCT => "Struct",
        lsp_types::CompletionItemKind::EVENT => "Event",
        lsp_types::CompletionItemKind::OPERATOR => "Operator",
        lsp_types::CompletionItemKind::TYPE_PARAMETER => "TypeParameter",
        _ => "Unknown",
    }
}

fn completion_documentation_to_text(doc: &lsp_types::Documentation) -> String {
    match doc {
        lsp_types::Documentation::String(s) => s.clone(),
        lsp_types::Documentation::MarkupContent(markup) => markup.value.clone(),
    }
}

fn completion_item_to_info(item: lsp_types::CompletionItem) -> CompletionItemInfo {
    CompletionItemInfo {
        label: item.label,
        kind: item.kind.map(|kind| completion_kind_str(kind).to_string()),
        detail: item.detail,
        documentation: item
            .documentation
            .as_ref()
            .map(completion_documentation_to_text),
        insert_text: item.insert_text,
        sort_text: item.sort_text,
        filter_text: item.filter_text,
    }
}

/// Parse a `textDocument/completion` response.
///
/// Handles `null`, `CompletionItem[]`, and `CompletionList`.
pub fn parse_completion_response(value: Value) -> Result<Vec<CompletionItemInfo>> {
    if value.is_null() {
        return Ok(vec![]);
    }

    let response: lsp_types::CompletionResponse =
        serde_json::from_value(value).context("Failed to parse completion response")?;

    Ok(match response {
        lsp_types::CompletionResponse::Array(items) => {
            items.into_iter().map(completion_item_to_info).collect()
        }
        lsp_types::CompletionResponse::List(list) => list
            .items
            .into_iter()
            .map(completion_item_to_info)
            .collect(),
    })
}

// ---------------------------------------------------------------------------
// Symbols
// ---------------------------------------------------------------------------

/// Convert an `lsp_types::SymbolKind` to a human-readable string.
///
/// Covers all 26 standard symbol kinds defined in LSP 3.17.
pub fn symbol_kind_str(kind: lsp_types::SymbolKind) -> &'static str {
    match kind {
        lsp_types::SymbolKind::FILE => "File",
        lsp_types::SymbolKind::MODULE => "Module",
        lsp_types::SymbolKind::NAMESPACE => "Namespace",
        lsp_types::SymbolKind::PACKAGE => "Package",
        lsp_types::SymbolKind::CLASS => "Class",
        lsp_types::SymbolKind::METHOD => "Method",
        lsp_types::SymbolKind::PROPERTY => "Property",
        lsp_types::SymbolKind::FIELD => "Field",
        lsp_types::SymbolKind::CONSTRUCTOR => "Constructor",
        lsp_types::SymbolKind::ENUM => "Enum",
        lsp_types::SymbolKind::INTERFACE => "Interface",
        lsp_types::SymbolKind::FUNCTION => "Function",
        lsp_types::SymbolKind::VARIABLE => "Variable",
        lsp_types::SymbolKind::CONSTANT => "Constant",
        lsp_types::SymbolKind::STRING => "String",
        lsp_types::SymbolKind::NUMBER => "Number",
        lsp_types::SymbolKind::BOOLEAN => "Boolean",
        lsp_types::SymbolKind::ARRAY => "Array",
        lsp_types::SymbolKind::OBJECT => "Object",
        lsp_types::SymbolKind::KEY => "Key",
        lsp_types::SymbolKind::NULL => "Null",
        lsp_types::SymbolKind::ENUM_MEMBER => "EnumMember",
        lsp_types::SymbolKind::STRUCT => "Struct",
        lsp_types::SymbolKind::EVENT => "Event",
        lsp_types::SymbolKind::OPERATOR => "Operator",
        lsp_types::SymbolKind::TYPE_PARAMETER => "TypeParameter",
        _ => "Unknown",
    }
}

/// Recursively convert an `lsp_types::DocumentSymbol` (hierarchical) into [`SymbolInfo`].
fn document_symbol_to_info(sym: &lsp_types::DocumentSymbol, file_path: &str) -> SymbolInfo {
    let children = sym
        .children
        .as_ref()
        .map(|kids| {
            kids.iter()
                .map(|c| document_symbol_to_info(c, file_path))
                .collect()
        })
        .unwrap_or_default();

    SymbolInfo {
        name: sym.name.clone(),
        kind: symbol_kind_str(sym.kind).to_string(),
        location: SourceLocation {
            file_path: file_path.to_string(),
            line: sym.selection_range.start.line + 1,
            character: sym.selection_range.start.character + 1,
            end_line: Some(sym.selection_range.end.line + 1),
            end_character: Some(sym.selection_range.end.character + 1),
        },
        children,
    }
}

/// Convert an `lsp_types::SymbolInformation` (flat) into [`SymbolInfo`].
fn symbol_information_to_info(sym: &lsp_types::SymbolInformation) -> SymbolInfo {
    SymbolInfo {
        name: sym.name.clone(),
        kind: symbol_kind_str(sym.kind).to_string(),
        location: location_to_source(&sym.location),
        children: vec![],
    }
}

/// Parse a `textDocument/documentSymbol` response.
///
/// Handles both `DocumentSymbol[]` (hierarchical, with children) and
/// `SymbolInformation[]` (flat, no children).
pub fn parse_document_symbols_response(value: Value) -> Result<Vec<SymbolInfo>> {
    if value.is_null() {
        return Ok(vec![]);
    }

    let resp: lsp_types::DocumentSymbolResponse =
        serde_json::from_value(value).context("Failed to parse document symbols response")?;

    Ok(match resp {
        lsp_types::DocumentSymbolResponse::Nested(symbols) => {
            // DocumentSymbol doesn't carry a URI — use empty string as placeholder.
            // Callers can patch this with the known document URI.
            symbols
                .iter()
                .map(|s| document_symbol_to_info(s, ""))
                .collect()
        }
        lsp_types::DocumentSymbolResponse::Flat(infos) => {
            infos.iter().map(symbol_information_to_info).collect()
        }
    })
}

/// Parse a `workspace/symbol` response.
///
/// Handles `SymbolInformation[]` and `WorkspaceSymbol[]` (3.17+).
pub fn parse_workspace_symbols_response(value: Value) -> Result<Vec<SymbolInfo>> {
    if value.is_null() {
        return Ok(vec![]);
    }

    // Try WorkspaceSymbolResponse first (handles both variants).
    if let Ok(resp) = serde_json::from_value::<lsp_types::WorkspaceSymbolResponse>(value.clone()) {
        return Ok(match resp {
            lsp_types::WorkspaceSymbolResponse::Flat(infos) => {
                infos.iter().map(symbol_information_to_info).collect()
            }
            lsp_types::WorkspaceSymbolResponse::Nested(ws_symbols) => ws_symbols
                .iter()
                .map(|ws| {
                    let file_path = match &ws.location {
                        lsp_types::OneOf::Left(loc) => uri_to_file_path(&loc.uri),
                        lsp_types::OneOf::Right(wl) => uri_to_file_path(&wl.uri),
                    };
                    let location = match &ws.location {
                        lsp_types::OneOf::Left(loc) => location_to_source(loc),
                        lsp_types::OneOf::Right(_wl) => SourceLocation {
                            file_path,
                            line: 1,
                            character: 1,
                            end_line: None,
                            end_character: None,
                        },
                    };
                    SymbolInfo {
                        name: ws.name.clone(),
                        kind: symbol_kind_str(ws.kind).to_string(),
                        location,
                        children: vec![],
                    }
                })
                .collect(),
        });
    }

    bail!("Failed to parse workspace symbols response")
}

// ---------------------------------------------------------------------------
// Call hierarchy
// ---------------------------------------------------------------------------

/// Convert a `CallHierarchyItem` into a [`SymbolInfo`].
pub fn call_hierarchy_item_to_info(item: &lsp_types::CallHierarchyItem) -> SymbolInfo {
    SymbolInfo {
        name: item.name.clone(),
        kind: symbol_kind_str(item.kind).to_string(),
        location: SourceLocation {
            file_path: uri_to_file_path(&item.uri),
            line: item.selection_range.start.line + 1,
            character: item.selection_range.start.character + 1,
            end_line: Some(item.selection_range.end.line + 1),
            end_character: Some(item.selection_range.end.character + 1),
        },
        children: vec![],
    }
}

/// Parse a `textDocument/prepareCallHierarchy` response into a list of [`SymbolInfo`].
pub fn parse_call_hierarchy_items(value: Value) -> Result<Vec<SymbolInfo>> {
    if value.is_null() {
        return Ok(vec![]);
    }

    let items: Vec<lsp_types::CallHierarchyItem> =
        serde_json::from_value(value).context("Failed to parse CallHierarchyItem[]")?;

    Ok(items.iter().map(call_hierarchy_item_to_info).collect())
}

/// Parse a `callHierarchy/incomingCalls` response.
///
/// Extracts the `.from` field of each `CallHierarchyIncomingCall`.
pub fn parse_incoming_calls(value: Value) -> Result<Vec<SymbolInfo>> {
    if value.is_null() {
        return Ok(vec![]);
    }

    let calls: Vec<lsp_types::CallHierarchyIncomingCall> =
        serde_json::from_value(value).context("Failed to parse CallHierarchyIncomingCall[]")?;

    Ok(calls
        .iter()
        .map(|c| call_hierarchy_item_to_info(&c.from))
        .collect())
}

/// Parse a `callHierarchy/outgoingCalls` response.
///
/// Extracts the `.to` field of each `CallHierarchyOutgoingCall`.
pub fn parse_outgoing_calls(value: Value) -> Result<Vec<SymbolInfo>> {
    if value.is_null() {
        return Ok(vec![]);
    }

    let calls: Vec<lsp_types::CallHierarchyOutgoingCall> =
        serde_json::from_value(value).context("Failed to parse CallHierarchyOutgoingCall[]")?;

    Ok(calls
        .iter()
        .map(|c| call_hierarchy_item_to_info(&c.to))
        .collect())
}

#[cfg(test)]
mod tests;
