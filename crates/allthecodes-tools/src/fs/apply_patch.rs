use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, bail, Context, Result};
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::common::{current_dir, string_param};
use crate::fs::safe_write::{safe_write_text, SafeWriteOptions};
use crate::tool::{
    PermissionResult, Tool, ToolProgress, ToolResult, ToolUseContext, Tools, ValidationResult,
};
use allthecodes_types::message::AssistantMessage;

pub fn tools() -> Tools {
    vec![Arc::new(ApplyPatchTool), Arc::new(ApplyPatchFreeformTool)]
}

pub struct ApplyPatchTool;
pub struct ApplyPatchFreeformTool;

fn patch_param(input: &Value) -> Option<&str> {
    string_param(input, "patch").or_else(|| string_param(input, "input"))
}

fn patch_validation(input: &Value) -> ValidationResult {
    let Some(patch) = patch_param(input) else {
        return ValidationResult::Error {
            message: "patch or input is required".into(),
            error_code: 400,
        };
    };
    match parse_patch(patch) {
        Ok(_) => ValidationResult::Ok,
        Err(err) => ValidationResult::Error {
            message: err.to_string(),
            error_code: 400,
        },
    }
}

fn patch_permission(input: &Value, ctx: &ToolUseContext, display_name: &str) -> PermissionResult {
    let Some(patch) = patch_param(input) else {
        return PermissionResult::Deny {
            message: "patch or input is required".into(),
        };
    };
    let ops = match parse_patch(patch) {
        Ok(ops) => ops,
        Err(err) => {
            return PermissionResult::Deny {
                message: err.to_string(),
            }
        }
    };
    if let Some(reason) = patch_needs_explicit_permission(&ops, ctx) {
        return PermissionResult::Ask {
            message: format!(
                "Allow {display_name} to {reason}? Summary: {}",
                patch_summary(&ops)
            ),
        };
    }
    PermissionResult::Allow {
        updated_input: input.clone(),
    }
}

fn apply_patch_input_schema(primary_field: &str) -> Value {
    let description = "Patch text using *** Begin Patch / *** End Patch grammar.";
    if primary_field == "input" {
        json!({
            "type": "object",
            "properties": {
                "input": {"type": "string", "description": description}
            },
            "required": ["input"]
        })
    } else {
        json!({
            "type": "object",
            "properties": {
                "patch": {"type": "string", "description": description}
            },
            "required": ["patch"]
        })
    }
}

#[derive(Debug, Clone)]
pub(crate) enum PatchOp {
    Add {
        path: PathBuf,
        content: String,
    },
    Delete {
        path: PathBuf,
    },
    Update {
        path: PathBuf,
        move_to: Option<PathBuf>,
        lines: Vec<PatchLine>,
    },
}

#[derive(Debug, Clone)]
pub(crate) enum PatchLine {
    Context(String),
    Remove(String),
    Add(String),
    EndOfFile,
}

#[derive(Debug, Clone)]
enum PreparedPatchChange {
    Add {
        path: PathBuf,
        content: String,
    },
    Delete {
        path: PathBuf,
    },
    Update {
        path: PathBuf,
        target: PathBuf,
        remove_source_after_write: bool,
        content: String,
    },
}

struct PatchParser<'a> {
    lines: Vec<&'a str>,
    cursor: usize,
}

impl<'a> PatchParser<'a> {
    fn new(raw: &'a str) -> Self {
        Self {
            lines: raw.lines().collect(),
            cursor: 0,
        }
    }

    fn current(&self) -> Option<&'a str> {
        self.lines.get(self.cursor).copied()
    }

    fn advance(&mut self) {
        self.cursor += 1;
    }

    fn is_done(&self) -> bool {
        self.cursor >= self.lines.len()
    }

    fn is_at_section_or_end(&self) -> bool {
        self.current()
            .map(|line| line.starts_with("*** ") && line != "*** End of File")
            .unwrap_or(true)
    }

    fn parse_add(&mut self, path: &str) -> Result<PatchOp> {
        self.advance();
        let mut content = String::new();
        while !self.is_at_section_or_end() {
            let line = self
                .current()
                .ok_or_else(|| anyhow!("unexpected end of add file section"))?;
            let Some(rest) = line.strip_prefix('+') else {
                bail!("add file lines must start with +");
            };
            content.push_str(rest);
            content.push('\n');
            self.advance();
        }
        Ok(PatchOp::Add {
            path: PathBuf::from(path),
            content,
        })
    }

    fn parse_update(&mut self, path: &str) -> Result<PatchOp> {
        self.advance();
        let mut move_to = None;
        if let Some(target) = self
            .current()
            .and_then(|line| line.strip_prefix("*** Move to: "))
        {
            move_to = Some(PathBuf::from(target));
            self.advance();
        }
        let mut patch_lines = Vec::new();
        while !self.is_at_section_or_end() {
            let current = self
                .current()
                .ok_or_else(|| anyhow!("unexpected end of update section"))?;
            if current.starts_with("@@") {
                self.advance();
                continue;
            }
            if current == "*** End of File" {
                patch_lines.push(PatchLine::EndOfFile);
                self.advance();
                continue;
            }
            let Some((prefix, text)) = current.split_at_checked(1) else {
                bail!("empty update lines must be represented as a context/add/remove line");
            };
            match prefix {
                " " => patch_lines.push(PatchLine::Context(text.to_string())),
                "-" => patch_lines.push(PatchLine::Remove(text.to_string())),
                "+" => patch_lines.push(PatchLine::Add(text.to_string())),
                _ => bail!("update lines must start with space, -, +, @@, or *** End of File"),
            }
            self.advance();
        }
        Ok(PatchOp::Update {
            path: PathBuf::from(path),
            move_to,
            lines: patch_lines,
        })
    }
}

pub(crate) fn parse_patch(raw: &str) -> Result<Vec<PatchOp>> {
    let mut parser = PatchParser::new(raw);
    if parser.current() != Some("*** Begin Patch") {
        bail!("patch must start with *** Begin Patch");
    }
    if parser.lines.last().copied() != Some("*** End Patch") {
        bail!("patch must end with *** End Patch");
    }
    parser.advance();
    if parser
        .current()
        .is_some_and(|line| line.starts_with("*** Environment ID: "))
    {
        parser.advance();
    }
    let mut ops = Vec::new();
    while !parser.is_done() {
        let Some(line) = parser.current() else {
            break;
        };
        if line == "*** End Patch" {
            parser.advance();
            break;
        }
        if let Some(path) = line.strip_prefix("*** Add File: ") {
            ops.push(parser.parse_add(path)?);
        } else if let Some(path) = line.strip_prefix("*** Delete File: ") {
            ops.push(PatchOp::Delete {
                path: PathBuf::from(path),
            });
            parser.advance();
        } else if let Some(path) = line.strip_prefix("*** Update File: ") {
            ops.push(parser.parse_update(path)?);
        } else if line.trim().is_empty() {
            parser.advance();
        } else {
            bail!("unknown patch section: {line}");
        }
    }
    if ops.is_empty() {
        bail!("patch must contain at least one operation");
    }
    Ok(ops)
}

fn apply_update(original: &str, patch: &[PatchLine]) -> Result<String> {
    let source = original.lines().map(ToOwned::to_owned).collect::<Vec<_>>();
    let mut out = Vec::new();
    let mut cursor = 0usize;
    for line in patch {
        match line {
            PatchLine::Context(expected) => {
                let pos = source[cursor..]
                    .iter()
                    .position(|actual| actual == expected)
                    .ok_or_else(|| anyhow!("context line not found: {expected}"))?;
                out.extend_from_slice(&source[cursor..cursor + pos]);
                out.push(expected.clone());
                cursor += pos + 1;
            }
            PatchLine::Remove(expected) => {
                if source.get(cursor) != Some(expected) {
                    bail!(
                        "remove line did not match current file at line {}: {}",
                        cursor + 1,
                        expected
                    );
                }
                cursor += 1;
            }
            PatchLine::Add(text) => out.push(text.clone()),
            PatchLine::EndOfFile => {
                cursor = source.len();
            }
        }
    }
    out.extend_from_slice(&source[cursor..]);
    let mut text = out.join("\n");
    if original.ends_with('\n') || patch.iter().any(|line| matches!(line, PatchLine::Add(_))) {
        text.push('\n');
    }
    Ok(text)
}

fn ensure_fresh(path: &Path, ctx: &ToolUseContext) -> Result<()> {
    let content = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    let hash = crate::tool::FileStateCache::hash_content(&content);
    let candidates = [
        path.to_string_lossy().to_string(),
        fs::canonicalize(path)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default(),
    ];
    if candidates
        .iter()
        .filter(|candidate| !candidate.is_empty())
        .any(|candidate| {
            ctx.read_file_state
                .get(candidate)
                .map(|entry| entry.content_hash == hash)
                .unwrap_or(false)
        })
    {
        Ok(())
    } else {
        bail!("refusing to modify {} because it has not been read in this session or has changed since it was read", path.display())
    }
}

fn patch_summary(ops: &[PatchOp]) -> Value {
    let mut files = Vec::new();
    for op in ops {
        match op {
            PatchOp::Add { path, content } => {
                files.push(json!({
                    "operation": "add",
                    "path": path,
                    "bytes": content.len(),
                }));
            }
            PatchOp::Delete { path } => {
                files.push(json!({
                    "operation": "delete",
                    "path": path,
                }));
            }
            PatchOp::Update {
                path,
                move_to,
                lines,
            } => {
                let added = lines
                    .iter()
                    .filter(|line| matches!(line, PatchLine::Add(_)))
                    .count();
                let removed = lines
                    .iter()
                    .filter(|line| matches!(line, PatchLine::Remove(_)))
                    .count();
                files.push(json!({
                    "operation": if move_to.is_some() { "move_update" } else { "update" },
                    "path": path,
                    "target": move_to,
                    "added_lines": added,
                    "removed_lines": removed,
                    "truncates_at_eof": lines.iter().any(|line| matches!(line, PatchLine::EndOfFile)),
                }));
            }
        }
    }
    json!({
        "operation_count": ops.len(),
        "files": files,
    })
}

fn prepare_patch_changes(
    ops: &[PatchOp],
    ctx: &ToolUseContext,
) -> Result<Vec<PreparedPatchChange>> {
    let mut prepared = Vec::new();
    for op in ops {
        match op {
            PatchOp::Add { path, content } => {
                if path.exists() {
                    bail!(
                        "refusing to add file that already exists: {}",
                        path.display()
                    );
                }
                prepared.push(PreparedPatchChange::Add {
                    path: path.clone(),
                    content: content.clone(),
                });
            }
            PatchOp::Delete { path } => {
                ensure_fresh(path, ctx)?;
                prepared.push(PreparedPatchChange::Delete { path: path.clone() });
            }
            PatchOp::Update {
                path,
                move_to,
                lines,
            } => {
                ensure_fresh(path, ctx)?;
                let original = fs::read_to_string(path)
                    .with_context(|| format!("failed to read {}", path.display()))?;
                let updated = apply_update(&original, lines)?;
                let target = move_to.as_ref().unwrap_or(path);
                if move_to.is_some() && target.exists() {
                    bail!("refusing to move over existing file: {}", target.display());
                }
                prepared.push(PreparedPatchChange::Update {
                    path: path.clone(),
                    target: target.clone(),
                    remove_source_after_write: move_to.is_some(),
                    content: updated,
                });
            }
        }
    }
    Ok(prepared)
}

fn apply_patch_tool_result(input: Value, ctx: &ToolUseContext) -> Result<ToolResult> {
    let patch = patch_param(&input).ok_or_else(|| anyhow!("patch or input is required"))?;
    let ops = parse_patch(patch)?;
    let summary = patch_summary(&ops);
    let prepared = prepare_patch_changes(&ops, ctx)?;
    let mut changed = Vec::new();
    for change in prepared {
        match change {
            PreparedPatchChange::Add { path, content } => {
                let report = safe_write_text(
                    &path,
                    &content,
                    &SafeWriteOptions {
                        session_id: Some(ctx.session_id.clone()),
                        ..Default::default()
                    },
                )?;
                changed
                    .push(json!({"operation": "add", "path": path, "bytes": report.bytes_written}));
            }
            PreparedPatchChange::Delete { path } => {
                fs::remove_file(&path)
                    .with_context(|| format!("failed to delete {}", path.display()))?;
                ctx.read_file_state.invalidate(&path.to_string_lossy());
                changed.push(json!({"operation": "delete", "path": path}));
            }
            PreparedPatchChange::Update {
                path,
                target,
                remove_source_after_write,
                content,
            } => {
                let report = safe_write_text(
                    &target,
                    &content,
                    &SafeWriteOptions {
                        session_id: Some(ctx.session_id.clone()),
                        ..Default::default()
                    },
                )?;
                if remove_source_after_write {
                    fs::remove_file(&path).with_context(|| {
                        format!("failed to remove moved source {}", path.display())
                    })?;
                }
                ctx.read_file_state.invalidate(&path.to_string_lossy());
                changed.push(json!({"operation": if remove_source_after_write { "move_update" } else { "update" }, "path": path, "target": target, "bytes": report.bytes_written}));
            }
        }
    }
    let count = changed.len();
    Ok(ToolResult {
        data: json!({"changed": changed, "count": count, "summary": summary}),
        new_messages: vec![],
        display_preview: Some(format!("Applied patch to {count} file(s)")),
        ..Default::default()
    })
}

fn path_within_allowed(path: &Path, ctx: &ToolUseContext) -> bool {
    let Ok(validated) =
        allthecodes_permissions::path_validation::validate_file_path(&path.to_string_lossy())
    else {
        return false;
    };
    let app_state = (ctx.get_app_state)();
    allthecodes_permissions::path_validation::is_path_within_allowed_directories(
        &validated,
        &current_dir(),
        &app_state.tool_permission_context,
    )
}

fn patch_needs_explicit_permission(ops: &[PatchOp], ctx: &ToolUseContext) -> Option<String> {
    for op in ops {
        match op {
            PatchOp::Add { path, .. } => {
                if !path_within_allowed(path, ctx) {
                    return Some(format!(
                        "add outside allowed directories: {}",
                        path.display()
                    ));
                }
            }
            PatchOp::Delete { path } => {
                return Some(format!("delete file: {}", path.display()));
            }
            PatchOp::Update { path, move_to, .. } => {
                if !path_within_allowed(path, ctx) {
                    return Some(format!(
                        "update outside allowed directories: {}",
                        path.display()
                    ));
                }
                if let Some(target) = move_to {
                    if !path_within_allowed(target, ctx) {
                        return Some(format!(
                            "move target outside allowed directories: {}",
                            target.display()
                        ));
                    }
                    return Some(format!(
                        "move file: {} -> {}",
                        path.display(),
                        target.display()
                    ));
                }
            }
        }
    }
    None
}

#[async_trait]
impl Tool for ApplyPatchTool {
    fn name(&self) -> &str {
        "ApplyPatch"
    }

    async fn description(&self, _input: &Value) -> String {
        "Apply a Codex patch grammar payload to local files using safe writes.".into()
    }

    fn input_json_schema(&self) -> Value {
        apply_patch_input_schema("patch")
    }

    fn is_destructive(&self, _input: &Value) -> bool {
        true
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        patch_validation(input)
    }

    async fn check_permissions(&self, input: &Value, ctx: &ToolUseContext) -> PermissionResult {
        patch_permission(input, ctx, self.name())
    }

    async fn call(
        &self,
        input: Value,
        ctx: &ToolUseContext,
        _parent: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        apply_patch_tool_result(input, ctx)
    }

    async fn prompt(&self) -> String {
        "ApplyPatch is a legacy JSON alias for `apply_patch`; pass a `patch` string in Codex patch grammar. Read existing files first before update/delete patches.".into()
    }
}

#[async_trait]
impl Tool for ApplyPatchFreeformTool {
    fn name(&self) -> &str {
        "apply_patch"
    }

    async fn description(&self, _input: &Value) -> String {
        "Apply a Codex patch grammar payload to local files using safe writes.".into()
    }

    fn input_json_schema(&self) -> Value {
        apply_patch_input_schema("input")
    }

    fn is_destructive(&self, _input: &Value) -> bool {
        true
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        patch_validation(input)
    }

    async fn check_permissions(&self, input: &Value, ctx: &ToolUseContext) -> PermissionResult {
        patch_permission(input, ctx, self.name())
    }

    async fn call(
        &self,
        input: Value,
        ctx: &ToolUseContext,
        _parent: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        apply_patch_tool_result(input, ctx)
    }

    async fn prompt(&self) -> String {
        "`apply_patch` is exposed as a Codex-compatible freeform grammar tool for OpenAI Codex Responses. On JSON-only providers, pass the patch text in `input`. Read existing files first before update/delete patches.".into()
    }
}
