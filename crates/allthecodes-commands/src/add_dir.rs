//! /add-dir command -- add additional working directories to the session.
//!
//! Validates the path and adds it to `additional_working_directories`
//! in the permission context, allowing tools to access files outside
//! the primary working directory.

use std::path::{Path, PathBuf};

use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandHandler, CommandResult};
use allthecodes_types::permissions::AdditionalWorkingDirectory;

pub struct AddDirHandler;

#[async_trait]
impl CommandHandler for AddDirHandler {
    async fn execute(&self, args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        let raw_path = args.trim();

        if raw_path.is_empty() {
            // No argument: list current additional directories
            let dirs = &ctx
                .app_state
                .tool_permission_context
                .additional_working_directories;
            if dirs.is_empty() {
                return Ok(CommandResult::Output(
                    "No additional working directories.\nUsage: /add-dir <path>".to_string(),
                ));
            }
            let mut lines = vec!["Additional working directories:".to_string()];
            for (name, info) in dirs {
                let ro = if info.read_only { " (read-only)" } else { "" };
                lines.push(format!("  {} → {}{}", name, info.path, ro));
            }
            return Ok(CommandResult::Output(lines.join("\n")));
        }

        // Resolve the path
        let resolved = resolve_dir_path(raw_path, &ctx.cwd);

        // Validate
        match validate_directory(&resolved) {
            Err(msg) => Ok(CommandResult::Output(msg)),
            Ok(canonical) => {
                let canonical_str = canonical.to_string_lossy().to_string();

                // Check if already in CWD
                if is_subdir(&canonical, &ctx.cwd) {
                    return Ok(CommandResult::Output(format!(
                        "Directory '{}' is already within the current working directory.",
                        canonical.display()
                    )));
                }

                // Check if already added
                let dirs = &ctx
                    .app_state
                    .tool_permission_context
                    .additional_working_directories;
                for info in dirs.values() {
                    let existing = PathBuf::from(&info.path);
                    if let Ok(existing_canon) = existing.canonicalize() {
                        if existing_canon == canonical {
                            return Ok(CommandResult::Output(format!(
                                "Directory '{}' is already added.",
                                canonical.display()
                            )));
                        }
                    }
                }

                // Derive a short name from the directory
                let dir_name = canonical
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| canonical_str.clone());

                // Add to state
                ctx.app_state
                    .tool_permission_context
                    .additional_working_directories
                    .insert(
                        dir_name.clone(),
                        AdditionalWorkingDirectory {
                            path: canonical_str.clone(),
                            read_only: false,
                        },
                    );

                Ok(CommandResult::Output(format!(
                    "Added working directory: {} ({})",
                    dir_name, canonical_str
                )))
            }
        }
    }
}

/// Resolve a potentially relative path against the cwd, expanding `~`.
fn resolve_dir_path(raw: &str, cwd: &std::path::Path) -> PathBuf {
    let expanded = if raw.starts_with('~') {
        if let Some(home) = dirs::home_dir() {
            home.join(
                raw.strip_prefix("~/")
                    .or(raw.strip_prefix("~"))
                    .unwrap_or(raw),
            )
        } else {
            PathBuf::from(raw)
        }
    } else {
        PathBuf::from(raw)
    };

    if expanded.is_absolute() {
        expanded
    } else {
        cwd.join(expanded)
    }
}

/// Validate that a path exists and is a directory. Returns the canonical path.
fn validate_directory(path: &Path) -> std::result::Result<PathBuf, String> {
    match path.canonicalize() {
        Ok(canonical) => {
            if canonical.is_dir() {
                Ok(canonical)
            } else {
                Err(format!("'{}' is not a directory.", path.display()))
            }
        }
        Err(_) => Err(format!("Directory not found: '{}'", path.display())),
    }
}

/// Check if `child` is a subdirectory of (or equal to) `parent`.
fn is_subdir(child: &std::path::Path, parent: &std::path::Path) -> bool {
    let parent_canon = parent
        .canonicalize()
        .unwrap_or_else(|_| parent.to_path_buf());
    child.starts_with(&parent_canon)
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_bootstrap::SessionId;
    use std::fs;
    use std::path::PathBuf;
    use uuid::Uuid;

    fn test_ctx(cwd: PathBuf) -> CommandContext {
        CommandContext {
            messages: Vec::new(),
            cwd,
            app_state: Default::default(),
            session_id: SessionId::from_string("test-session"),
        }
    }

    fn sibling_temp_dirs(label: &str) -> (PathBuf, PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("cc_rust_add_dir_{label}_{}", Uuid::new_v4()));
        let cwd = root.join("cwd");
        let target = root.join("target");
        fs::create_dir_all(&cwd).unwrap();
        fs::create_dir_all(&target).unwrap();
        (root, cwd, target)
    }

    #[tokio::test]
    async fn test_add_dir_no_args_empty() {
        let handler = AddDirHandler;
        let mut ctx = test_ctx(PathBuf::from("."));
        let result = handler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => assert!(text.contains("Usage")),
            _ => panic!("Expected Output"),
        }
    }

    #[tokio::test]
    async fn test_add_dir_nonexistent() {
        let handler = AddDirHandler;
        let mut ctx = test_ctx(PathBuf::from("."));
        let result = handler
            .execute("/nonexistent/path/xyz_abc_123", &mut ctx)
            .await
            .unwrap();
        match result {
            CommandResult::Output(text) => assert!(text.contains("not found")),
            _ => panic!("Expected Output"),
        }
    }

    #[tokio::test]
    async fn test_add_dir_success() {
        let (root, cwd, target) = sibling_temp_dirs("success");

        let handler = AddDirHandler;
        let mut ctx = test_ctx(cwd);
        let result = handler
            .execute(&target.to_string_lossy(), &mut ctx)
            .await
            .unwrap();

        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("Added working directory"));
            }
            _ => panic!("Expected Output"),
        }

        // Verify it was added to state
        assert!(!ctx
            .app_state
            .tool_permission_context
            .additional_working_directories
            .is_empty());

        let _ = fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn test_add_dir_duplicate() {
        let (root, cwd, target) = sibling_temp_dirs("dup");

        let handler = AddDirHandler;
        let mut ctx = test_ctx(cwd);

        // First add
        handler
            .execute(&target.to_string_lossy(), &mut ctx)
            .await
            .unwrap();

        // Second add (duplicate)
        let result = handler
            .execute(&target.to_string_lossy(), &mut ctx)
            .await
            .unwrap();
        match result {
            CommandResult::Output(text) => assert!(text.contains("already added")),
            _ => panic!("Expected Output"),
        }

        let _ = fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn test_add_dir_is_file() {
        let tmp = std::env::temp_dir().join("cc_rust_add_dir_file_test.txt");
        fs::write(&tmp, "test").unwrap();

        let handler = AddDirHandler;
        let mut ctx = test_ctx(PathBuf::from("/"));
        let result = handler
            .execute(&tmp.to_string_lossy(), &mut ctx)
            .await
            .unwrap();
        match result {
            CommandResult::Output(text) => assert!(text.contains("not a directory")),
            _ => panic!("Expected Output"),
        }

        let _ = fs::remove_file(&tmp);
    }
}
