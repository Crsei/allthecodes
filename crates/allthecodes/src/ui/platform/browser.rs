//! Shared read-only "list + drill-down" browser primitives used by
//! `/hooks`, `/agents`, `/doctor`, `/tasks`, and `/keybindings`.
//!
//! These commands all render a tree of items grouped by source or category
//! and link each leaf back to a config file the user can open in their
//! editor. Rather than reinvent the same formatting and file-open dance in
//! each command, they share these helpers.
//!
//! The module is deliberately text-only — no TUI drill-down state. Commands
//! execute via `CommandResult::Output(String)`, so a rendered tree plus a
//! "jump to editor" subcommand is the UX seam we actually need.

use std::path::Path;

// ---------------------------------------------------------------------------
// Tree node
// ---------------------------------------------------------------------------

/// A node in a rendered tree.
///
/// `label` is the primary display string. `detail` is a short secondary line
/// shown after the label (e.g. a description, a path). `badge` is a bracketed
/// source tag printed before the label (e.g. `[user]`, `[managed]`).
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

/// Render a list of top-level nodes as an indented tree.
///
/// Output uses ASCII box-drawing so it is portable across terminals without
/// requiring specific Unicode fonts.
pub fn render_tree(title: &str, roots: &[TreeNode]) -> String {
    let mut out = String::new();
    if !title.is_empty() {
        out.push_str(title);
        out.push('\n');
        out.push_str(&"─".repeat(title.chars().count().min(60)));
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
    let connector = if is_root { "" } else { "├─ " };
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
        out.push_str(" — ");
        out.push_str(detail);
    }
    out.push('\n');

    let child_prefix = if is_root {
        "  ".to_string()
    } else {
        format!("{}│  ", prefix)
    };
    let last_child_prefix = if is_root {
        "  ".to_string()
    } else {
        format!("{}   ", prefix)
    };
    let total = node.children.len();
    for (i, child) in node.children.iter().enumerate() {
        let is_last = i + 1 == total;
        let nested_prefix = if is_last {
            &last_child_prefix
        } else {
            &child_prefix
        };
        render_child(out, child, nested_prefix, is_last);
    }
}

fn render_child(out: &mut String, node: &TreeNode, prefix: &str, is_last: bool) {
    let connector = if is_last { "└─ " } else { "├─ " };
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
        out.push_str(" — ");
        out.push_str(detail);
    }
    out.push('\n');

    let total = node.children.len();
    if total == 0 {
        return;
    }
    let continuation = if is_last { "   " } else { "│  " };
    let deeper_prefix = format!("{}{}", prefix, continuation);
    for (i, child) in node.children.iter().enumerate() {
        render_child(out, child, &deeper_prefix, i + 1 == total);
    }
}

// ---------------------------------------------------------------------------
// Editor-jump helper
// ---------------------------------------------------------------------------

/// Outcome of attempting to open a file in the user's editor.
#[derive(Debug)]
pub enum OpenOutcome {
    /// File opened successfully (or the editor returned cleanly).
    Opened { editor: String, created: bool },
    /// No `$VISUAL`/`$EDITOR` is set — the caller should print the path.
    NoEditor { created: bool },
    /// Editor exited non-zero or failed to launch.
    Failed { editor: String, message: String },
    /// Failed to create parent directory or template.
    CreateFailed { error: String },
}

/// Ensure `path` exists (creating it with `template` if not) and open it in
/// the user's `$VISUAL`/`$EDITOR`.
///
/// The caller is expected to format the `OpenOutcome` for display. We don't
/// print directly so the command layer keeps full control over messaging.
pub fn ensure_and_open(path: &Path, template: &str) -> OpenOutcome {
    let created = match ensure_file(path, template) {
        Ok(c) => c,
        Err(e) => {
            return OpenOutcome::CreateFailed {
                error: e.to_string(),
            };
        }
    };

    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .ok()
        .filter(|s| !s.trim().is_empty());

    match editor {
        Some(ed) => {
            let status = std::process::Command::new(&ed).arg(path).status();
            match status {
                Ok(s) if s.success() => OpenOutcome::Opened {
                    editor: ed,
                    created,
                },
                Ok(s) => OpenOutcome::Failed {
                    editor: ed,
                    message: format!("exited with status {}", s.code().unwrap_or(-1)),
                },
                Err(e) => OpenOutcome::Failed {
                    editor: ed,
                    message: e.to_string(),
                },
            }
        }
        None => OpenOutcome::NoEditor { created },
    }
}

/// Render an `OpenOutcome` back to a user-facing string given the target
/// path. Commands call this to produce the final `CommandResult::Output`.
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
        OpenOutcome::Failed { editor, message } => format!(
            "Error launching '{}': {}. File: {}",
            editor,
            message,
            path.display()
        ),
        OpenOutcome::CreateFailed { error } => {
            format!("Error: could not prepare {}: {}", path.display(), error)
        }
    }
}

/// Ensure `path` exists, creating it (and its parent directory) from
/// `template` when absent. Returns whether a new file was created.
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

// ---------------------------------------------------------------------------
// Path helpers
// ---------------------------------------------------------------------------

/// Render a path suitable for display (always forward-slashed).
pub fn display_path(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

/// Shorten a path relative to `$HOME` using the `~` convention when possible.
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

/// Convenience: turn a `Vec<TreeNode>` plus optional header/footer into a
/// single string.
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn render_tree_prints_title_and_nodes() {
        let mut node = TreeNode::leaf("alpha")
            .with_badge("user")
            .with_detail("first entry");
        node.push_child(TreeNode::leaf("a-child"));
        let out = render_tree("Things", &[node]);
        assert!(out.contains("Things"));
        assert!(out.contains("[user]"));
        assert!(out.contains("alpha"));
        assert!(out.contains("first entry"));
        assert!(out.contains("a-child"));
    }

    #[test]
    fn render_tree_empty_says_no_entries() {
        let out = render_tree("Hooks", &[]);
        assert!(out.contains("Hooks"));
        assert!(out.contains("(no entries)"));
    }

    #[test]
    fn ensure_and_open_creates_template_when_missing() {
        let dir = std::env::temp_dir().join(format!("allthecodes-browser-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("config.json");
        // Keep editor empty so the helper returns NoEditor without spawning.
        let prev_visual = std::env::var_os("VISUAL");
        let prev_editor = std::env::var_os("EDITOR");
        std::env::remove_var("VISUAL");
        std::env::remove_var("EDITOR");

        let outcome = ensure_and_open(&path, "{}");
        match &outcome {
            OpenOutcome::NoEditor { created } => assert!(*created),
            _ => panic!("expected NoEditor, got {:?}", outcome),
        }
        assert!(path.exists(), "template should be created");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{}");

        // Running again should not recreate.
        let second = ensure_and_open(&path, "{\"changed\": true}");
        match &second {
            OpenOutcome::NoEditor { created } => assert!(!*created),
            _ => panic!("expected NoEditor, got {:?}", second),
        }
        // Template must be preserved — no overwrite.
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{}");

        // Restore env.
        if let Some(v) = prev_visual {
            std::env::set_var("VISUAL", v);
        }
        if let Some(v) = prev_editor {
            std::env::set_var("EDITOR", v);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn format_open_outcome_prints_path() {
        let path = PathBuf::from("/tmp/x.json");
        let out = format_open_outcome(&OpenOutcome::NoEditor { created: true }, &path);
        assert!(out.contains("/tmp/x.json"));
        assert!(out.contains("Created template"));
    }

    #[test]
    fn shorten_path_strips_home_prefix() {
        if let Some(home) = dirs::home_dir() {
            let inside = home.join(".allthecodes/settings.json");
            let short = shorten_path(&inside);
            assert!(short.starts_with("~/"));
            assert!(short.contains(".allthecodes"));
        }
    }
}
