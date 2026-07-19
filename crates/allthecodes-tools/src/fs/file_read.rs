use std::path::{Path, PathBuf};
use anyhow::Result;
use async_trait::async_trait;
use base64::Engine as _;
use serde_json::{json, Value};

use crate::tool::{
    FileStateReceipt, InterruptBehavior, Tool, ToolProgress, ToolResult, ToolUseContext,
    ValidationResult,
};
use allthecodes_types::message::AssistantMessage;

use super::hashline;

/// FileReadTool — Read files from the filesystem
///
/// Corresponds to TypeScript: tools/FileReadTool
pub struct FileReadTool;

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReadTarget {
    original_path: String,
    read_path: PathBuf,
    resolved_path: String,
    symlink_resolved: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DecodedText {
    content: String,
    encoding: &'static str,
    lossy: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FormattedRead {
    output: String,
    total_lines: usize,
    start_line: usize,
    end_line: usize,
    next_offset: Option<usize>,
    line_limited: bool,
}

impl FileReadTool {
    const DEFAULT_TEXT_LINE_LIMIT: usize = 2000;

    pub fn new() -> Self {
        FileReadTool
    }

    fn parse_input(input: &Value) -> (String, Option<usize>, Option<usize>, Option<String>) {
        let file_path = input
            .get("file_path")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let offset = input
            .get("offset")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize);
        let limit = input
            .get("limit")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize);
        let pages = input
            .get("pages")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        (file_path, offset, limit, pages)
    }

    fn text_read_receipt(
        ctx: &ToolUseContext,
        target: &ReadTarget,
        content: &str,
    ) -> FileStateReceipt {
        FileStateReceipt::from_content(
            &ctx.cwd,
            &target.original_path,
            &target.read_path,
            content.as_bytes(),
        )
    }

    /// Detect if content is likely binary by checking for null bytes
    fn is_binary(content: &[u8]) -> bool {
        let check_len = content.len().min(8192);
        content[..check_len].contains(&0)
    }

    async fn resolve_read_target(file_path: &str) -> std::io::Result<ReadTarget> {
        let path = Path::new(file_path);
        let symlink_metadata = tokio::fs::symlink_metadata(path).await?;
        let symlink_resolved = symlink_metadata.file_type().is_symlink();
        let read_path = tokio::fs::canonicalize(path).await?;
        let resolved_path = read_path.to_string_lossy().to_string();

        Ok(ReadTarget {
            original_path: file_path.to_string(),
            read_path,
            resolved_path,
            symlink_resolved,
        })
    }

    fn attach_read_target(result: &mut ToolResult, target: &ReadTarget) {
        if let Some(obj) = result.data.as_object_mut() {
            obj.insert("file_path".to_string(), json!(target.original_path));
            obj.insert("resolved_path".to_string(), json!(target.resolved_path));
            obj.insert(
                "symlink_resolved".to_string(),
                json!(target.symlink_resolved),
            );
        }
    }

    fn target_matches(target: &ReadTarget, predicate: fn(&Path) -> bool) -> bool {
        predicate(Path::new(&target.original_path)) || predicate(&target.read_path)
    }

    /// Check if the file is an image based on extension
    fn is_image_file(path: &Path) -> bool {
        match path.extension().and_then(|e| e.to_str()) {
            Some(ext) => matches!(
                ext.to_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "svg"
            ),
            None => false,
        }
    }

    /// Check if the file is a PDF based on extension
    fn is_pdf_file(path: &Path) -> bool {
        match path.extension().and_then(|e| e.to_str()) {
            Some(ext) => ext.eq_ignore_ascii_case("pdf"),
            None => false,
        }
    }

    /// Check if the file is a Jupyter notebook based on extension
    fn is_notebook_file(path: &Path) -> bool {
        match path.extension().and_then(|e| e.to_str()) {
            Some(ext) => ext.eq_ignore_ascii_case("ipynb"),
            None => false,
        }
    }

    /// Get the MIME media type for an image extension
    fn image_media_type(ext: &str) -> &'static str {
        match ext.to_lowercase().as_str() {
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "bmp" => "image/bmp",
            "webp" => "image/webp",
            "svg" => "image/svg+xml",
            _ => "application/octet-stream",
        }
    }

    /// Read an image file and return the appropriate result
    async fn read_image(file_path: &str) -> Result<ToolResult> {
        let path = Path::new(file_path);
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        // SVG is text-based, return content directly
        if ext == "svg" {
            let bytes = tokio::fs::read(file_path).await?;
            let decoded = Self::decode_text_bytes(&bytes)?
                .ok_or_else(|| anyhow::anyhow!("SVG file appears to be binary"))?;
            return Ok(ToolResult {
                data: json!({
                    "type": "image",
                    "media_type": "image/svg+xml",
                    "content": decoded.content,
                    "encoding": decoded.encoding,
                    "encoding_lossy": decoded.lossy,
                    "file_path": file_path,
                }),
                new_messages: vec![],
                ..Default::default()
            });
        }

        // Binary image: read bytes and base64-encode
        let bytes = tokio::fs::read(file_path).await?;
        let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
        let media_type = Self::image_media_type(&ext);

        Ok(ToolResult {
            data: json!({
                "type": "image",
                "media_type": media_type,
                "base64": encoded,
                "file_path": file_path,
            }),
            new_messages: vec![],
            ..Default::default()
        })
    }

    fn decode_text_bytes(bytes: &[u8]) -> Result<Option<DecodedText>> {
        if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
            let content = std::str::from_utf8(&bytes[3..])?.to_string();
            return Ok(Some(DecodedText {
                content,
                encoding: "utf-8-bom",
                lossy: false,
            }));
        }

        if bytes.starts_with(&[0xFF, 0xFE]) {
            let content = Self::decode_utf16_bytes(&bytes[2..], true)?;
            return Ok(Some(DecodedText {
                content,
                encoding: "utf-16le-bom",
                lossy: false,
            }));
        }

        if bytes.starts_with(&[0xFE, 0xFF]) {
            let content = Self::decode_utf16_bytes(&bytes[2..], false)?;
            return Ok(Some(DecodedText {
                content,
                encoding: "utf-16be-bom",
                lossy: false,
            }));
        }

        if let Some(little_endian) = Self::detect_utf16_without_bom(bytes) {
            let content = Self::decode_utf16_bytes(bytes, little_endian)?;
            return Ok(Some(DecodedText {
                content,
                encoding: if little_endian {
                    "utf-16le"
                } else {
                    "utf-16be"
                },
                lossy: false,
            }));
        }

        if Self::is_binary(bytes) {
            return Ok(None);
        }

        match std::str::from_utf8(bytes) {
            Ok(content) => Ok(Some(DecodedText {
                content: content.to_string(),
                encoding: "utf-8",
                lossy: false,
            })),
            Err(_) => Ok(Some(DecodedText {
                content: String::from_utf8_lossy(bytes).to_string(),
                encoding: "utf-8-lossy",
                lossy: true,
            })),
        }
    }

    fn detect_utf16_without_bom(bytes: &[u8]) -> Option<bool> {
        let check_len = bytes.len().min(8192) & !1;
        if check_len < 8 {
            return None;
        }

        let sample = &bytes[..check_len];
        let pairs = check_len / 2;
        let even_nul = sample.iter().step_by(2).filter(|&&b| b == 0).count();
        let odd_nul = sample
            .iter()
            .skip(1)
            .step_by(2)
            .filter(|&&b| b == 0)
            .count();

        let even_ratio = even_nul as f32 / pairs as f32;
        let odd_ratio = odd_nul as f32 / pairs as f32;

        if odd_ratio > 0.30 && even_ratio < 0.05 {
            Some(true)
        } else if even_ratio > 0.30 && odd_ratio < 0.05 {
            Some(false)
        } else {
            None
        }
    }

    fn decode_utf16_bytes(bytes: &[u8], little_endian: bool) -> Result<String> {
        if !bytes.len().is_multiple_of(2) {
            return Err(anyhow::anyhow!("UTF-16 text has an odd number of bytes"));
        }

        let units: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|chunk| {
                if little_endian {
                    u16::from_le_bytes([chunk[0], chunk[1]])
                } else {
                    u16::from_be_bytes([chunk[0], chunk[1]])
                }
            })
            .collect();

        String::from_utf16(&units).map_err(|e| anyhow::anyhow!("Invalid UTF-16 text: {}", e))
    }

    /// Parse a pages parameter like "1-5", "3", "10-20" into (first_page, last_page)
    fn parse_pages(pages: &str) -> Result<(u32, u32)> {
        let pages = pages.trim();
        if let Some((first, last)) = pages.split_once('-') {
            let first: u32 = first
                .trim()
                .parse()
                .map_err(|_| anyhow::anyhow!("Invalid page number: {}", first.trim()))?;
            let last: u32 = last
                .trim()
                .parse()
                .map_err(|_| anyhow::anyhow!("Invalid page number: {}", last.trim()))?;
            if first > last {
                return Err(anyhow::anyhow!(
                    "First page ({}) must be <= last page ({})",
                    first,
                    last
                ));
            }
            Ok((first, last))
        } else {
            let page: u32 = pages
                .parse()
                .map_err(|_| anyhow::anyhow!("Invalid page number: {}", pages))?;
            Ok((page, page))
        }
    }

    /// Read a PDF file using pdftotext subprocess
    async fn read_pdf(file_path: &str, pages: Option<String>) -> Result<ToolResult> {
        let mut cmd = tokio::process::Command::new("pdftotext");

        if let Some(ref pages_str) = pages {
            let (first, last) = Self::parse_pages(pages_str)?;
            cmd.arg("-f").arg(first.to_string());
            cmd.arg("-l").arg(last.to_string());
        }

        // pdftotext <input> - sends output to stdout
        cmd.arg(file_path);
        cmd.arg("-");

        let output = match cmd.output().await {
            Ok(o) => o,
            Err(e) => {
                if e.kind() == std::io::ErrorKind::NotFound {
                    return Ok(ToolResult {
                        data: json!({
                            "error": "PDF reading requires pdftotext. Install poppler-utils or provide a text version."
                        }),
                        new_messages: vec![],
                        ..Default::default()
                    });
                }
                return Err(anyhow::anyhow!("Failed to run pdftotext: {}", e));
            }
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Ok(ToolResult {
                data: json!({ "error": format!("pdftotext failed: {}", stderr) }),
                new_messages: vec![],
                ..Default::default()
            });
        }

        let text = String::from_utf8_lossy(&output.stdout).to_string();
        if text.is_empty() {
            return Ok(ToolResult {
                data: json!({
                    "output": "(PDF contains no extractable text)",
                    "file_path": file_path,
                }),
                new_messages: vec![],
                ..Default::default()
            });
        }

        // Format with line numbers
        let (formatted, total_lines) = Self::format_with_line_numbers(&text, 0, None);

        Ok(ToolResult {
            data: json!({
                "output": formatted,
                "total_lines": total_lines,
                "file_path": file_path,
            }),
            new_messages: vec![],
            ..Default::default()
        })
    }

    /// Read a Jupyter notebook (.ipynb) and extract cells with outputs
    async fn read_notebook(
        file_path: &str,
        read_state: Option<(&ToolUseContext, &ReadTarget)>,
    ) -> Result<ToolResult> {
        let content = tokio::fs::read_to_string(file_path).await?;
        let file_state_receipts = read_state
            .map(|(ctx, target)| Self::text_read_receipt(ctx, target, &content))
            .into_iter()
            .collect();
        let notebook: Value = serde_json::from_str(&content)
            .map_err(|e| anyhow::anyhow!("Failed to parse notebook JSON: {}", e))?;

        let cells = notebook
            .get("cells")
            .and_then(|c| c.as_array())
            .ok_or_else(|| anyhow::anyhow!("Invalid notebook format: missing 'cells' array"))?;

        let mut result = String::new();

        for (i, cell) in cells.iter().enumerate() {
            let cell_type = cell
                .get("cell_type")
                .and_then(|t| t.as_str())
                .unwrap_or("unknown");
            let editable_id = cell
                .get("id")
                .and_then(|id| id.as_str())
                .map(|id| format!("id: {}; index: cell-{}", id, i))
                .unwrap_or_else(|| format!("index: cell-{}", i));

            // Extract source content
            let source = Self::extract_notebook_text(cell.get("source"));

            result.push_str(&format!(
                "# Cell [{}] (type: {}) ({})\n",
                i + 1,
                cell_type,
                editable_id
            ));
            result.push_str(&source);
            if !source.ends_with('\n') {
                result.push('\n');
            }

            // Extract outputs if present
            if let Some(outputs) = cell.get("outputs").and_then(|o| o.as_array()) {
                if !outputs.is_empty() {
                    result.push_str("# Output:\n");
                    for output in outputs {
                        let output_text = Self::extract_notebook_output(output);
                        if !output_text.is_empty() {
                            result.push_str(&output_text);
                            if !output_text.ends_with('\n') {
                                result.push('\n');
                            }
                        }
                    }
                }
            }

            result.push('\n');
        }

        // Format with line numbers
        let (formatted, total_lines) = Self::format_with_line_numbers(&result, 0, None);

        Ok(ToolResult {
            data: json!({
                "output": formatted,
                "total_lines": total_lines,
                "file_path": file_path,
            }),
            new_messages: vec![],
            file_state_receipts,
            ..Default::default()
        })
    }

    /// Extract text from a notebook source or text field.
    /// Notebook fields can be a string or an array of strings.
    fn extract_notebook_text(value: Option<&Value>) -> String {
        match value {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Array(arr)) => arr
                .iter()
                .filter_map(|v| v.as_str())
                .collect::<Vec<_>>()
                .join(""),
            _ => String::new(),
        }
    }

    /// Extract text from a single notebook output object
    fn extract_notebook_output(output: &Value) -> String {
        let output_type = output
            .get("output_type")
            .and_then(|t| t.as_str())
            .unwrap_or("");

        match output_type {
            "stream" => Self::extract_notebook_text(output.get("text")),
            "execute_result" | "display_data" => {
                // Try text/plain from data
                if let Some(data) = output.get("data") {
                    if let Some(text) = data.get("text/plain") {
                        return Self::extract_notebook_text(Some(text));
                    }
                }
                String::new()
            }
            "error" => {
                let ename = output
                    .get("ename")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Error");
                let evalue = output.get("evalue").and_then(|v| v.as_str()).unwrap_or("");
                format!("{}: {}", ename, evalue)
            }
            _ => String::new(),
        }
    }

    /// Format file content with line numbers (like cat -n)
    fn format_with_line_numbers(
        content: &str,
        offset: usize,
        limit: Option<usize>,
    ) -> (String, usize) {
        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();

        // offset is 1-based line number (0 means start from beginning)
        let start = if offset > 0 { offset - 1 } else { 0 };
        let end = match limit {
            Some(limit) => start.saturating_add(limit).min(total_lines),
            None => total_lines,
        };

        if start >= total_lines {
            return (String::new(), total_lines);
        }

        let mut result = String::new();
        for (i, line) in lines[start..end].iter().enumerate() {
            let line_num = start + i + 1;
            result.push_str(&format!("{}\t{}\n", line_num, line));
        }

        (result, total_lines)
    }

    fn format_text_window(content: &str, offset: usize, limit: Option<usize>) -> FormattedRead {
        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();
        let start = if offset > 0 { offset - 1 } else { 0 };

        if start >= total_lines {
            return FormattedRead {
                output: String::new(),
                total_lines,
                start_line: offset,
                end_line: 0,
                next_offset: None,
                line_limited: false,
            };
        }

        let window_limit = limit.unwrap_or(Self::DEFAULT_TEXT_LINE_LIMIT);
        let end = start.saturating_add(window_limit).min(total_lines);
        let mut output = String::new();

        for (i, line) in lines[start..end].iter().enumerate() {
            let line_num = start + i + 1;
            output.push_str(&format!("{}\t{}\n", line_num, line));
        }

        let next_offset = if end < total_lines {
            Some(end + 1)
        } else {
            None
        };

        FormattedRead {
            output,
            total_lines,
            start_line: start + 1,
            end_line: end,
            next_offset,
            line_limited: next_offset.is_some(),
        }
    }

    fn format_hashline_text_window(
        content: &str,
        offset: usize,
        limit: Option<usize>,
    ) -> FormattedRead {
        let (output, total_lines, start_line, end_line, next_offset, line_limited) =
            hashline::format_window_with_hashlines(
                content,
                offset,
                limit,
                Self::DEFAULT_TEXT_LINE_LIMIT,
            );
        FormattedRead {
            output,
            total_lines,
            start_line,
            end_line,
            next_offset,
            line_limited,
        }
    }

    async fn read_text(
        target: &ReadTarget,
        offset: Option<usize>,
        limit: Option<usize>,
        max_chars: usize,
        ctx: Option<&ToolUseContext>,
    ) -> Result<ToolResult> {
        let bytes = tokio::fs::read(&target.read_path).await?;
        let decoded = match Self::decode_text_bytes(&bytes)? {
            Some(decoded) => decoded,
            None => {
                return Ok(ToolResult {
                    data: json!({
                        "error": "File appears to be binary. Cannot display binary file contents.",
                        "file_path": target.original_path,
                        "resolved_path": target.resolved_path,
                        "symlink_resolved": target.symlink_resolved,
                    }),
                    new_messages: vec![],
                    ..Default::default()
                });
            }
        };
        let file_state_receipt =
            ctx.map(|ctx| Self::text_read_receipt(ctx, target, &decoded.content));

        let effective_offset = offset.unwrap_or(0);
        let hashline_mode = ctx
            .map(|ctx| {
                (ctx.get_app_state)()
                    .settings
                    .hashline_mode
                    .unwrap_or(false)
            })
            .unwrap_or(false);
        let formatted = if hashline_mode {
            Self::format_hashline_text_window(&decoded.content, effective_offset, limit)
        } else {
            Self::format_text_window(&decoded.content, effective_offset, limit)
        };
        if formatted.output.is_empty() && formatted.total_lines > 0 {
            return Ok(ToolResult {
                data: json!({
                    "output": format!("File has {} lines, but offset {} is beyond the end.", formatted.total_lines, effective_offset),
                    "total_lines": formatted.total_lines,
                    "file_path": target.original_path,
                    "resolved_path": target.resolved_path,
                    "symlink_resolved": target.symlink_resolved,
                    "encoding": decoded.encoding,
                    "encoding_lossy": decoded.lossy,
                    "base_file_hash": hashline_mode.then(|| hashline::file_hash(&decoded.content)),
                    "hashline_mode": hashline_mode,
                }),
                new_messages: vec![],
                file_state_receipts: file_state_receipt.clone().into_iter().collect(),
                ..Default::default()
            });
        }

        if formatted.output.is_empty() {
            return Ok(ToolResult {
                data: json!({
                    "output": "(empty file)",
                    "total_lines": 0,
                    "file_path": target.original_path,
                    "resolved_path": target.resolved_path,
                    "symlink_resolved": target.symlink_resolved,
                    "encoding": decoded.encoding,
                    "encoding_lossy": decoded.lossy,
                    "truncated": false,
                    "base_file_hash": hashline_mode.then(|| hashline::file_hash(&decoded.content)),
                    "hashline_mode": hashline_mode,
                }),
                new_messages: vec![],
                file_state_receipts: file_state_receipt.clone().into_iter().collect(),
                ..Default::default()
            });
        }

        let mut output = formatted.output;
        if let Some(next_offset) = formatted.next_offset {
            output.push_str(&format!(
                "... (showing lines {}-{} of {}; use offset {} to continue)\n",
                formatted.start_line, formatted.end_line, formatted.total_lines, next_offset
            ));
        }

        let truncated_by_chars = output.chars().count() > max_chars;
        if truncated_by_chars {
            output = output.chars().take(max_chars).collect();
            output.push_str("\n... (output truncated)");
        }
        Ok(ToolResult {
            data: json!({
                "output": output,
                "total_lines": formatted.total_lines,
                "start_line": formatted.start_line,
                "end_line": formatted.end_line,
                "next_offset": formatted.next_offset,
                "truncated": formatted.line_limited || truncated_by_chars,
                "line_limited": formatted.line_limited,
                "char_limited": truncated_by_chars,
                "encoding": decoded.encoding,
                "encoding_lossy": decoded.lossy,
                "base_file_hash": hashline_mode.then(|| hashline::file_hash(&decoded.content)),
                "hashline_mode": hashline_mode,
                "file_path": target.original_path,
                "resolved_path": target.resolved_path,
                "symlink_resolved": target.symlink_resolved,
            }),
            new_messages: vec![],
            file_state_receipts: file_state_receipt.into_iter().collect(),
            ..Default::default()
        })
    }
}

impl Default for FileReadTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for FileReadTool {
    fn name(&self) -> &str {
        "Read"
    }

    async fn description(&self, _input: &Value) -> String {
        "Reads a file from the local filesystem.".to_string()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "The absolute path to the file to read"
                },
                "offset": {
                    "type": "number",
                    "description": "The line number to start reading from (1-based)"
                },
                "limit": {
                    "type": "number",
                    "description": "The number of lines to read"
                },
                "pages": {
                    "type": "string",
                    "description": "Page range for PDF files (e.g. '1-5')"
                }
            },
            "required": ["file_path"]
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    fn interrupt_behavior(&self) -> InterruptBehavior {
        InterruptBehavior::Cancel
    }

    fn get_path(&self, input: &Value) -> Option<String> {
        input
            .get("file_path")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }

    fn backfill_observable_input(&self, input: &mut serde_json::Map<String, Value>) {
        crate::interaction::observable_input::backfill_file_path(input);
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        let file_path = input
            .get("file_path")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if file_path.is_empty() {
            return ValidationResult::Error {
                message: "file_path is required".to_string(),
                error_code: 1,
            };
        }
        ValidationResult::Ok
    }

    async fn call(
        &self,
        input: Value,
        ctx: &ToolUseContext,
        _parent_message: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let (file_path, offset, limit, pages) = Self::parse_input(&input);

        if file_path.is_empty() {
            return Ok(ToolResult {
                data: json!({ "error": "file_path is required" }),
                new_messages: vec![],
                ..Default::default()
            });
        }

        let target = match Self::resolve_read_target(&file_path).await {
            Ok(target) => target,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ToolResult {
                    data: json!({ "error": format!("File not found: {}", file_path) }),
                    new_messages: vec![],
                    ..Default::default()
                });
            }
            Err(e) => {
                return Ok(ToolResult {
                    data: json!({ "error": format!("Failed to resolve file path {}: {}", file_path, e) }),
                    new_messages: vec![],
                    ..Default::default()
                });
            }
        };

        let metadata = match tokio::fs::metadata(&target.read_path).await {
            Ok(metadata) => metadata,
            Err(e) => {
                return Ok(ToolResult {
                    data: json!({ "error": format!("Failed to read metadata for {}: {}", file_path, e) }),
                    new_messages: vec![],
                    ..Default::default()
                });
            }
        };

        if metadata.is_dir() {
            return Ok(ToolResult {
                data: json!({ "error": format!("Path is a directory, not a file: {}. Use ls via Bash tool to list directory contents.", file_path) }),
                new_messages: vec![],
                ..Default::default()
            });
        }

        let read_path = target.read_path.to_string_lossy().to_string();

        // Route based on file extension BEFORE binary detection
        if Self::target_matches(&target, Self::is_image_file) {
            return match Self::read_image(&read_path).await {
                Ok(mut result) => {
                    Self::attach_read_target(&mut result, &target);
                    Ok(result)
                }
                Err(e) => Ok(ToolResult {
                    data: json!({ "error": format!("Failed to read image: {}", e) }),
                    new_messages: vec![],
                    ..Default::default()
                }),
            };
        }

        if Self::target_matches(&target, Self::is_pdf_file) {
            return match Self::read_pdf(&read_path, pages).await {
                Ok(mut result) => {
                    Self::attach_read_target(&mut result, &target);
                    Ok(result)
                }
                Err(e) => Ok(ToolResult {
                    data: json!({ "error": format!("Failed to read PDF: {}", e) }),
                    new_messages: vec![],
                    ..Default::default()
                }),
            };
        }

        if Self::target_matches(&target, Self::is_notebook_file) {
            return match Self::read_notebook(&read_path, Some((ctx, &target))).await {
                Ok(mut result) => {
                    Self::attach_read_target(&mut result, &target);
                    Ok(result)
                }
                Err(e) => Ok(ToolResult {
                    data: json!({ "error": format!("Failed to read notebook: {}", e) }),
                    new_messages: vec![],
                    ..Default::default()
                }),
            };
        }

        match Self::read_text(
            &target,
            offset,
            limit,
            self.max_result_size_chars(),
            Some(ctx),
        )
        .await
        {
            Ok(result) => Ok(result),
            Err(e) => Ok(ToolResult {
                data: json!({ "error": format!("Failed to read file: {}", e) }),
                new_messages: vec![],
                ..Default::default()
            }),
        }
    }

    async fn prompt(&self) -> String {
        "Reads a file from the local filesystem. You can access any file directly by using this tool.\n\
Assume this tool is able to read all files on the machine. If the User provides a path to a file assume that path is valid. It is okay to read a file that does not exist; an error will be returned.\n\n\
Usage:\n\
- The file_path parameter must be an absolute path, not a relative path\n\
- If limit is not provided, small files are read from the starting offset to the end; large text files are paginated and include next_offset to continue\n\
- When you already know which part of the file you need, only read that part. This can be important for larger files.\n\
- Results are returned using cat -n format, with line numbers starting at 1\n\
- When hashline edit mode is enabled, text results include base_file_hash and line#hash prefixes for use with HashEdit\n\
- Text results include encoding, resolved_path, and symlink_resolved metadata when available\n\
- This tool allows allthecodes to read images (eg PNG, JPG, etc). When reading an image file the contents are presented visually as allthecodes is a multimodal LLM.\n\
- This tool can read PDF files (.pdf). For large PDFs (more than 10 pages), you MUST provide the pages parameter to read specific page ranges (e.g., pages: \"1-5\"). Reading a large PDF without the pages parameter will fail. Maximum 20 pages per request.\n\
- This tool can read Jupyter notebooks (.ipynb files) and returns all cells with their outputs, combining code, text, and visualizations.\n\
- This tool can only read files, not directories. To read a directory, use an ls command via the Bash tool.\n\
- You will regularly be asked to read screenshots. If the user provides a path to a screenshot, ALWAYS use this tool to view the file at the path. This tool will work with all temporary file paths.\n\
- If you read a file that exists but has empty contents you will receive a system reminder warning in place of file contents.".to_string()
    }

    fn user_facing_name(&self, _input: Option<&Value>) -> String {
        "Read".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_is_image_file() {
        assert!(FileReadTool::is_image_file(Path::new("photo.png")));
        assert!(FileReadTool::is_image_file(Path::new("photo.PNG")));
        assert!(FileReadTool::is_image_file(Path::new("photo.jpg")));
        assert!(FileReadTool::is_image_file(Path::new("photo.jpeg")));
        assert!(FileReadTool::is_image_file(Path::new("photo.JPEG")));
        assert!(FileReadTool::is_image_file(Path::new("photo.gif")));
        assert!(FileReadTool::is_image_file(Path::new("photo.bmp")));
        assert!(FileReadTool::is_image_file(Path::new("photo.webp")));
        assert!(FileReadTool::is_image_file(Path::new("icon.svg")));
        assert!(!FileReadTool::is_image_file(Path::new("file.txt")));
        assert!(!FileReadTool::is_image_file(Path::new("file.rs")));
        assert!(!FileReadTool::is_image_file(Path::new("file.pdf")));
        assert!(!FileReadTool::is_image_file(Path::new("noext")));
    }

    #[test]
    fn test_is_pdf_file() {
        assert!(FileReadTool::is_pdf_file(Path::new("document.pdf")));
        assert!(FileReadTool::is_pdf_file(Path::new("document.PDF")));
        assert!(FileReadTool::is_pdf_file(Path::new("/path/to/file.pdf")));
        assert!(!FileReadTool::is_pdf_file(Path::new("file.txt")));
        assert!(!FileReadTool::is_pdf_file(Path::new("file.png")));
        assert!(!FileReadTool::is_pdf_file(Path::new("noext")));
    }

    #[test]
    fn test_is_notebook_file() {
        assert!(FileReadTool::is_notebook_file(Path::new("notebook.ipynb")));
        assert!(FileReadTool::is_notebook_file(Path::new("notebook.IPYNB")));
        assert!(FileReadTool::is_notebook_file(Path::new(
            "/path/to/nb.ipynb"
        )));
        assert!(!FileReadTool::is_notebook_file(Path::new("file.txt")));
        assert!(!FileReadTool::is_notebook_file(Path::new("file.py")));
        assert!(!FileReadTool::is_notebook_file(Path::new("noext")));
    }

    #[test]
    fn test_parse_pages() {
        assert_eq!(FileReadTool::parse_pages("3").unwrap(), (3, 3));
        assert_eq!(FileReadTool::parse_pages("1-5").unwrap(), (1, 5));
        assert_eq!(FileReadTool::parse_pages("10-20").unwrap(), (10, 20));
        assert_eq!(FileReadTool::parse_pages(" 2 - 8 ").unwrap(), (2, 8));
        assert!(FileReadTool::parse_pages("abc").is_err());
        assert!(FileReadTool::parse_pages("5-3").is_err());
        assert!(FileReadTool::parse_pages("1-abc").is_err());
    }

    #[test]
    fn test_format_with_line_numbers_without_limit_reads_to_end() {
        let content = "line1\nline2\nline3\nline4";
        let (formatted, total_lines) = FileReadTool::format_with_line_numbers(content, 0, None);

        assert_eq!(total_lines, 4);
        assert_eq!(formatted, "1\tline1\n2\tline2\n3\tline3\n4\tline4\n");
    }

    #[test]
    fn test_format_with_line_numbers_with_limit_still_works() {
        let content = "line1\nline2\nline3\nline4";
        let (formatted, total_lines) = FileReadTool::format_with_line_numbers(content, 2, Some(2));

        assert_eq!(total_lines, 4);
        assert_eq!(formatted, "2\tline2\n3\tline3\n");
    }

    #[test]
    fn test_format_text_window_auto_paginates_large_files() {
        let content = (1..=FileReadTool::DEFAULT_TEXT_LINE_LIMIT + 2)
            .map(|line| format!("line{line}"))
            .collect::<Vec<_>>()
            .join("\n");

        let formatted = FileReadTool::format_text_window(&content, 0, None);

        assert_eq!(
            formatted.total_lines,
            FileReadTool::DEFAULT_TEXT_LINE_LIMIT + 2
        );
        assert_eq!(formatted.start_line, 1);
        assert_eq!(formatted.end_line, FileReadTool::DEFAULT_TEXT_LINE_LIMIT);
        assert_eq!(
            formatted.next_offset,
            Some(FileReadTool::DEFAULT_TEXT_LINE_LIMIT + 1)
        );
        assert!(formatted.line_limited);
        assert!(formatted.output.contains(&format!(
            "{}\tline{}",
            FileReadTool::DEFAULT_TEXT_LINE_LIMIT,
            FileReadTool::DEFAULT_TEXT_LINE_LIMIT
        )));
        assert!(!formatted.output.contains(&format!(
            "{}\tline{}",
            FileReadTool::DEFAULT_TEXT_LINE_LIMIT + 1,
            FileReadTool::DEFAULT_TEXT_LINE_LIMIT + 1
        )));
    }

    #[test]
    fn test_decode_text_bytes_detects_utf8_bom() {
        let decoded = FileReadTool::decode_text_bytes(b"\xEF\xBB\xBFhello")
            .unwrap()
            .unwrap();

        assert_eq!(decoded.content, "hello");
        assert_eq!(decoded.encoding, "utf-8-bom");
        assert!(!decoded.lossy);
    }

    #[test]
    fn test_decode_text_bytes_detects_utf16le_bom() {
        let bytes = [0xFF, 0xFE, b'h', 0, b'e', 0, b'l', 0, b'l', 0, b'o', 0];
        let decoded = FileReadTool::decode_text_bytes(&bytes).unwrap().unwrap();

        assert_eq!(decoded.content, "hello");
        assert_eq!(decoded.encoding, "utf-16le-bom");
        assert!(!decoded.lossy);
    }

    #[test]
    fn test_decode_text_bytes_detects_utf16be_without_bom() {
        let bytes = [0, b'h', 0, b'e', 0, b'l', 0, b'l', 0, b'o'];
        let decoded = FileReadTool::decode_text_bytes(&bytes).unwrap().unwrap();

        assert_eq!(decoded.content, "hello");
        assert_eq!(decoded.encoding, "utf-16be");
        assert!(!decoded.lossy);
    }

    #[tokio::test]
    async fn test_read_text_reports_encoding_and_pagination_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("utf16.txt");
        let bytes = [
            0xFF, 0xFE, b'a', 0, b'l', 0, b'p', 0, b'h', 0, b'a', 0, b'\n', 0, b'b', 0, b'e', 0,
            b't', 0, b'a', 0,
        ];
        tokio::fs::write(&file_path, bytes).await.unwrap();

        let target = FileReadTool::resolve_read_target(file_path.to_str().unwrap())
            .await
            .unwrap();
        let result = FileReadTool::read_text(&target, None, Some(1), 10_000, None)
            .await
            .unwrap();

        assert_eq!(result.data["encoding"], "utf-16le-bom");
        assert_eq!(result.data["total_lines"], 2);
        assert_eq!(result.data["next_offset"], 2);
        assert_eq!(result.data["line_limited"], true);
        assert!(result.data["output"].as_str().unwrap().contains("1\talpha"));
        assert!(!result.data["output"].as_str().unwrap().contains("2\tbeta"));
    }

    #[tokio::test]
    async fn test_resolve_read_target_for_regular_file() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("regular.txt");
        tokio::fs::write(&file_path, "hello").await.unwrap();

        let target = FileReadTool::resolve_read_target(file_path.to_str().unwrap())
            .await
            .unwrap();

        assert_eq!(target.original_path, file_path.to_string_lossy());
        assert!(!target.symlink_resolved);
        assert_eq!(
            target.read_path,
            tokio::fs::canonicalize(&file_path).await.unwrap()
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_resolve_read_target_reports_unix_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("target.txt");
        let link_path = dir.path().join("link.txt");
        tokio::fs::write(&file_path, "hello").await.unwrap();
        std::os::unix::fs::symlink(&file_path, &link_path).unwrap();

        let target = FileReadTool::resolve_read_target(link_path.to_str().unwrap())
            .await
            .unwrap();

        assert!(target.symlink_resolved);
        assert_eq!(
            target.read_path,
            tokio::fs::canonicalize(&file_path).await.unwrap()
        );
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn test_resolve_read_target_reports_windows_symlink_when_available() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("target.txt");
        let link_path = dir.path().join("link.txt");
        tokio::fs::write(&file_path, "hello").await.unwrap();

        if std::os::windows::fs::symlink_file(&file_path, &link_path).is_err() {
            return;
        }

        let target = FileReadTool::resolve_read_target(link_path.to_str().unwrap())
            .await
            .unwrap();

        assert!(target.symlink_resolved);
        assert_eq!(
            target.read_path,
            tokio::fs::canonicalize(&file_path).await.unwrap()
        );
    }

    #[tokio::test]
    async fn test_read_notebook_format() {
        use std::io::Write;

        let notebook_json = json!({
            "cells": [
                {
                    "cell_type": "markdown",
                    "source": ["# Hello World\n", "This is a test notebook."],
                    "metadata": {}
                },
                {
                    "cell_type": "code",
                    "source": "print('hello')",
                    "metadata": {},
                    "outputs": [
                        {
                            "output_type": "stream",
                            "name": "stdout",
                            "text": "hello\n"
                        }
                    ]
                },
                {
                    "cell_type": "code",
                    "source": ["1 + 1"],
                    "metadata": {},
                    "outputs": [
                        {
                            "output_type": "execute_result",
                            "data": {
                                "text/plain": "2"
                            },
                            "metadata": {},
                            "execution_count": 2
                        }
                    ]
                },
                {
                    "cell_type": "code",
                    "source": ["raise ValueError('oops')"],
                    "metadata": {},
                    "outputs": [
                        {
                            "output_type": "error",
                            "ename": "ValueError",
                            "evalue": "oops",
                            "traceback": []
                        }
                    ]
                }
            ],
            "metadata": {},
            "nbformat": 4,
            "nbformat_minor": 5
        });

        // Write to a temp file
        let dir = std::env::temp_dir();
        let file_path = dir.join("test_notebook_read.ipynb");
        {
            let mut f = std::fs::File::create(&file_path).unwrap();
            f.write_all(
                serde_json::to_string_pretty(&notebook_json)
                    .unwrap()
                    .as_bytes(),
            )
            .unwrap();
        }

        let result = FileReadTool::read_notebook(file_path.to_str().unwrap(), None)
            .await
            .unwrap();

        let output = result.data.get("output").and_then(|v| v.as_str()).unwrap();

        // Verify cell headers are present
        assert!(output.contains("# Cell [1] (type: markdown)"));
        assert!(output.contains("# Cell [2] (type: code)"));
        assert!(output.contains("# Cell [3] (type: code)"));
        assert!(output.contains("# Cell [4] (type: code)"));

        // Verify source content
        assert!(output.contains("# Hello World"));
        assert!(output.contains("This is a test notebook."));
        assert!(output.contains("print('hello')"));
        assert!(output.contains("1 + 1"));

        // Verify outputs
        assert!(output.contains("# Output:"));
        assert!(output.contains("hello"));
        assert!(output.contains("2"));
        assert!(output.contains("ValueError: oops"));

        // Clean up
        let _ = std::fs::remove_file(&file_path);
    }

    #[test]
    fn test_extract_notebook_text_string() {
        let val = Value::String("hello world".to_string());
        assert_eq!(
            FileReadTool::extract_notebook_text(Some(&val)),
            "hello world"
        );
    }

    #[test]
    fn test_extract_notebook_text_array() {
        let val = json!(["line 1\n", "line 2\n", "line 3"]);
        assert_eq!(
            FileReadTool::extract_notebook_text(Some(&val)),
            "line 1\nline 2\nline 3"
        );
    }

    #[test]
    fn test_extract_notebook_text_none() {
        assert_eq!(FileReadTool::extract_notebook_text(None), "");
    }

    #[test]
    fn test_image_media_type() {
        assert_eq!(FileReadTool::image_media_type("png"), "image/png");
        assert_eq!(FileReadTool::image_media_type("jpg"), "image/jpeg");
        assert_eq!(FileReadTool::image_media_type("jpeg"), "image/jpeg");
        assert_eq!(FileReadTool::image_media_type("gif"), "image/gif");
        assert_eq!(FileReadTool::image_media_type("bmp"), "image/bmp");
        assert_eq!(FileReadTool::image_media_type("webp"), "image/webp");
        assert_eq!(FileReadTool::image_media_type("svg"), "image/svg+xml");
        assert_eq!(FileReadTool::image_media_type("PNG"), "image/png");
    }
}
