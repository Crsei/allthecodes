//! Project instruction file loading and context building.
//!
//! Searches for `AGENTS.md` (primary) and `CLAUDE.md` (fallback) files in the
//! working directory and its ancestors, loads their contents, and assembles
//! them into a single context string that is injected into the system prompt.
//!
//! Resolution order per directory:
//! 1. If `AGENTS.md` exists, use it and log a warning if `CLAUDE.md` is also
//!    present (CLAUDE.md is ignored in that case).
//! 2. Otherwise, fall back to `CLAUDE.md`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

const AGENTS_MD: &str = "AGENTS.md";
const CLAUDE_MD: &str = "CLAUDE.md";

// ---------------------------------------------------------------------------
// AGENTS.md support (primary)
// ---------------------------------------------------------------------------

/// Search for project instruction files starting at `cwd` and walking up to
/// the current project boundary.
///
/// In each directory, `AGENTS.md` is preferred over `CLAUDE.md`. When both
/// exist in the same directory a warning is emitted to stderr and `CLAUDE.md`
/// is skipped for that directory.
///
/// Returns paths ordered from the **most distant ancestor** first (root-most)
/// to the working directory last, mirroring how context should be layered
/// (general first, specific last).
pub fn find_agents_md_files(cwd: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut dir = cwd.to_path_buf();
    let boundary = find_project_boundary(cwd);

    loop {
        let agents = dir.join(AGENTS_MD);
        let claude = dir.join(CLAUDE_MD);

        if agents.is_file() {
            if claude.is_file() {
                eprintln!(
                    "warning: {} ignored because {} exists in {}",
                    CLAUDE_MD,
                    AGENTS_MD,
                    dir.display()
                );
            }
            found.push(agents);
        } else if claude.is_file() {
            // Fallback: use CLAUDE.md when AGENTS.md is absent.
            found.push(claude);
        }

        if boundary.as_ref().is_some_and(|root| root == &dir) {
            break;
        }
        if !dir.pop() {
            break;
        }
    }

    // Reverse so root-most files come first, project-level files last.
    found.reverse();
    found
}

fn find_project_boundary(cwd: &Path) -> Option<PathBuf> {
    let mut dir = cwd.to_path_buf();
    loop {
        if has_repo_marker(&dir) || has_cargo_workspace_marker(&dir) {
            return Some(dir);
        }
        if !dir.pop() {
            break;
        }
    }
    Some(cwd.to_path_buf())
}

fn has_repo_marker(dir: &Path) -> bool {
    let marker = dir.join(".git");
    marker.is_dir() || marker.is_file()
}

fn has_cargo_workspace_marker(dir: &Path) -> bool {
    let manifest = dir.join("Cargo.toml");
    let Ok(contents) = std::fs::read_to_string(manifest) else {
        return false;
    };
    contents
        .lines()
        .any(|line| line.trim_start().starts_with("[workspace]"))
}

/// Read the contents of a single project instruction file.
pub fn load_agents_md(path: &Path) -> Result<String> {
    std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read project instructions at {}", path.display()))
}

/// Find all project instruction files from `cwd` upwards and combine their
/// contents into a single context string.
///
/// `AGENTS.md` is preferred over `CLAUDE.md` in each directory. Each file's
/// content is prefixed with a header indicating its source path. Files are
/// concatenated in order from root-most to project-level so that more specific
/// instructions appear last and can override general ones.
///
/// Returns `Ok(String)` with the combined context, or an empty string if no
/// instruction files were found.
pub fn build_agents_md_context(cwd: &Path) -> Result<String> {
    let files = find_agents_md_files(cwd);

    if files.is_empty() {
        return Ok(String::new());
    }

    let mut sections: Vec<String> = Vec::with_capacity(files.len());

    for path in &files {
        let content = load_agents_md(path)?;
        if content.trim().is_empty() {
            continue;
        }

        let display_path = path.display();
        sections.push(format!(
            "Contents of {} (project instructions, checked into the codebase):\n\n{}",
            display_path,
            content.trim()
        ));
    }

    Ok(sections.join("\n\n---\n\n"))
}

// ---------------------------------------------------------------------------
// CLAUDE.md (deprecated fallback)
// ---------------------------------------------------------------------------

/// Search for `CLAUDE.md` files starting at `cwd` and walking up to the
/// filesystem root.
///
/// **Deprecated**: Use [`find_agents_md_files`] instead, which prefers
/// `AGENTS.md` over `CLAUDE.md` in each directory. This function is retained
/// only for backward compatibility and returns an empty vector to encourage
/// migration.
#[deprecated(note = "Use find_agents_md_files instead; this returns an empty vec")]
pub fn find_claude_md_files(_cwd: &Path) -> Vec<PathBuf> {
    Vec::new()
}

/// Read the contents of a single `CLAUDE.md` file.
///
/// **Deprecated**: Use [`load_agents_md`] instead.
#[deprecated(note = "Use load_agents_md instead")]
pub fn load_claude_md(path: &Path) -> Result<String> {
    load_agents_md(path)
}

/// Find all `CLAUDE.md` files from `cwd` upwards and combine their contents
/// into a single context string.
///
/// **Deprecated**: Use [`build_agents_md_context`] instead.
#[deprecated(note = "Use build_agents_md_context instead")]
pub fn build_claude_md_context(cwd: &Path) -> Result<String> {
    build_agents_md_context(cwd)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Generate a unique temp directory to avoid collisions between test runs.
    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let id = uuid::Uuid::new_v4().to_string();
        std::env::temp_dir().join(format!("{}_{}", prefix, &id[..8]))
    }

    #[test]
    fn test_find_agents_md_empty_dir() {
        let dir = unique_temp_dir("agents_md_empty");
        let _ = fs::create_dir_all(&dir);
        let result = find_agents_md_files(&dir);
        // May or may not find files depending on the temp dir hierarchy,
        // but should not panic.
        let _ = result;
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_build_context_no_files() {
        let dir = unique_temp_dir("agents_md_nofiles");
        let _ = fs::create_dir_all(&dir);
        let ctx = build_agents_md_context(&dir).unwrap();
        let _ = ctx;
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_build_context_with_agents_md() {
        let dir = unique_temp_dir("agents_md_withfile");
        fs::create_dir_all(&dir).unwrap();
        let md_path = dir.join(AGENTS_MD);
        fs::write(&md_path, "# Test instructions\nDo the thing.").unwrap();

        let ctx = build_agents_md_context(&dir).unwrap();
        assert!(ctx.contains("Test instructions"));
        assert!(ctx.contains("Do the thing."));
        assert!(ctx.contains("AGENTS.md"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_agents_md_takes_priority_over_claude_md() {
        let dir = unique_temp_dir("agents_md_priority");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(AGENTS_MD), "# AGENTS content").unwrap();
        fs::write(dir.join(CLAUDE_MD), "# CLAUDE content").unwrap();

        let result = find_agents_md_files(&dir);
        assert_eq!(result.len(), 1);
        assert!(result[0].to_string_lossy().contains(AGENTS_MD));

        let ctx = build_agents_md_context(&dir).unwrap();
        assert!(ctx.contains("AGENTS content"));
        assert!(!ctx.contains("CLAUDE content"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_claude_md_fallback_when_no_agents_md() {
        let dir = unique_temp_dir("claude_md_fallback");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(CLAUDE_MD), "# CLAUDE fallback content").unwrap();

        let result = find_agents_md_files(&dir);
        assert_eq!(result.len(), 1);
        assert!(result[0].to_string_lossy().contains(CLAUDE_MD));

        let ctx = build_agents_md_context(&dir).unwrap();
        assert!(ctx.contains("CLAUDE fallback content"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_parent_claude_md_outside_repo_boundary_is_ignored() {
        let parent = unique_temp_dir("claude_md_boundary");
        let repo = parent.join("repo");
        let nested = repo.join("crate").join("src");
        fs::create_dir_all(&nested).unwrap();
        fs::write(parent.join(CLAUDE_MD), "# Parent CLAUDE content").unwrap();
        fs::create_dir_all(repo.join(".git")).unwrap();
        fs::write(repo.join(AGENTS_MD), "# Repo AGENTS content").unwrap();

        let result = find_agents_md_files(&nested);
        assert_eq!(result, vec![repo.join(AGENTS_MD)]);

        let ctx = build_agents_md_context(&nested).unwrap();
        assert!(ctx.contains("Repo AGENTS content"));
        assert!(!ctx.contains("Parent CLAUDE content"));

        let _ = fs::remove_dir_all(&parent);
    }

    #[test]
    fn test_cargo_workspace_boundary_is_used_without_git() {
        let parent = unique_temp_dir("agents_md_workspace_boundary");
        let workspace = parent.join("workspace");
        let nested = workspace.join("member").join("src");
        fs::create_dir_all(&nested).unwrap();
        fs::write(parent.join(CLAUDE_MD), "# Parent CLAUDE content").unwrap();
        fs::write(workspace.join("Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();
        fs::write(workspace.join(CLAUDE_MD), "# Workspace CLAUDE fallback").unwrap();

        let ctx = build_agents_md_context(&nested).unwrap();
        assert!(ctx.contains("Workspace CLAUDE fallback"));
        assert!(!ctx.contains("Parent CLAUDE content"));

        let _ = fs::remove_dir_all(&parent);
    }
}
