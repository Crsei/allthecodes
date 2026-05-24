//! Text-only browser primitives for slash-command list/detail output.

use std::path::Path;

#[derive(Debug, Clone, Default)]
pub struct TreeNode {
    pub label: String,
    pub detail: Option<String>,
    pub badge: Option<String>,
    pub children: Vec<TreeNode>,
}

impl TreeNode {
    pub fn leaf(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            ..Default::default()
        }
    }

    pub fn with_badge(mut self, badge: impl Into<String>) -> Self {
        self.badge = Some(badge.into());
        self
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn push_child(&mut self, child: TreeNode) {
        self.children.push(child);
    }
}

pub fn render_tree(title: &str, roots: &[TreeNode]) -> String {
    let mut out = String::new();
    if !title.is_empty() {
        out.push_str(title);
        out.push('\n');
        out.push_str(&"-".repeat(title.chars().count().min(60)));
        out.push('\n');
    }
    if roots.is_empty() {
        out.push_str("  (no entries)\n");
        return out;
    }
    for node in roots {
        render_node(&mut out, node, "", true);
    }
    out
}

fn render_node(out: &mut String, node: &TreeNode, prefix: &str, is_root: bool) {
    let connector = if is_root { "" } else { "+-- " };
    let line_prefix = if is_root {
        String::new()
    } else {
        prefix.to_string()
    };
    let badge = node
        .badge
        .as_deref()
        .map(|b| format!("[{}] ", b))
        .unwrap_or_default();

    out.push_str(&line_prefix);
    out.push_str(connector);
    out.push_str(&badge);
    out.push_str(&node.label);
    if let Some(detail) = &node.detail {
        out.push_str(" - ");
        out.push_str(detail);
    }
    out.push('\n');

    let child_prefix = if is_root {
        "  ".to_string()
    } else {
        format!("{}|   ", prefix)
    };
    let last_child_prefix = if is_root {
        "  ".to_string()
    } else {
        format!("{}    ", prefix)
    };
    let total = node.children.len();
    for (i, child) in node.children.iter().enumerate() {
        let nested_prefix = if i + 1 == total {
            &last_child_prefix
        } else {
            &child_prefix
        };
        render_child(out, child, nested_prefix, i + 1 == total);
    }
}

fn render_child(out: &mut String, node: &TreeNode, prefix: &str, is_last: bool) {
    let connector = if is_last { "`-- " } else { "+-- " };
    let badge = node
        .badge
        .as_deref()
        .map(|b| format!("[{}] ", b))
        .unwrap_or_default();
    out.push_str(prefix);
    out.push_str(connector);
    out.push_str(&badge);
    out.push_str(&node.label);
    if let Some(detail) = &node.detail {
        out.push_str(" - ");
        out.push_str(detail);
    }
    out.push('\n');

    let total = node.children.len();
    if total == 0 {
        return;
    }
    let continuation = if is_last { "    " } else { "|   " };
    let deeper_prefix = format!("{}{}", prefix, continuation);
    for (i, child) in node.children.iter().enumerate() {
        render_child(out, child, &deeper_prefix, i + 1 == total);
    }
}

pub fn display_path(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

pub fn shorten_path(p: &Path) -> String {
    if let Some(home) = dirs::home_dir() {
        if let Ok(rel) = p.strip_prefix(&home) {
            let rel = rel.to_string_lossy().replace('\\', "/");
            if rel.is_empty() {
                return "~".to_string();
            }
            return format!("~/{}", rel);
        }
    }
    display_path(p)
}

pub fn render_with_footer(title: &str, roots: &[TreeNode], footer: &str) -> String {
    let mut out = render_tree(title, roots);
    if !footer.is_empty() {
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out.push('\n');
        out.push_str(footer);
    }
    out
}

#[derive(Debug)]
pub enum OpenOutcome {
    Opened { editor: String, created: bool },
    NoEditor { created: bool },
    Failed { editor: String, message: String },
    CreateFailed { error: String },
}

pub fn ensure_and_open(path: &Path, template: &str) -> OpenOutcome {
    let created = match ensure_file(path, template) {
        Ok(created) => created,
        Err(error) => {
            return OpenOutcome::CreateFailed {
                error: error.to_string(),
            };
        }
    };

    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .ok()
        .filter(|value| !value.trim().is_empty());

    match editor {
        Some(editor) => match std::process::Command::new(&editor).arg(path).status() {
            Ok(status) if status.success() => OpenOutcome::Opened { editor, created },
            Ok(status) => OpenOutcome::Failed {
                editor,
                message: format!("exited with status {}", status.code().unwrap_or(-1)),
            },
            Err(error) => OpenOutcome::Failed {
                editor,
                message: error.to_string(),
            },
        },
        None => OpenOutcome::NoEditor { created },
    }
}

pub fn format_open_outcome(outcome: &OpenOutcome, path: &Path) -> String {
    match outcome {
        OpenOutcome::Opened { editor, created } => format!(
            "{}Opened {} in {}.",
            if *created { "Created and " } else { "" },
            path.display(),
            editor
        ),
        OpenOutcome::NoEditor { created } => format!(
            "{}File: {}\n(Set $VISUAL or $EDITOR to auto-open in an editor.)",
            if *created { "Created template.\n" } else { "" },
            path.display()
        ),
        OpenOutcome::Failed { editor, message } => {
            format!(
                "Error launching '{}': {}. File: {}",
                editor,
                message,
                path.display()
            )
        }
        OpenOutcome::CreateFailed { error } => {
            format!("Error: could not prepare {}: {}", path.display(), error)
        }
    }
}

fn ensure_file(path: &Path, template: &str) -> std::io::Result<bool> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    if path.exists() {
        return Ok(false);
    }
    std::fs::write(path, template)?;
    Ok(true)
}
