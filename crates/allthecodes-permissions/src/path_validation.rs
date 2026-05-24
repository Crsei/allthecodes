//! Path validation for the permission system.
//!
//! Validates that file paths are within allowed directories before
//! permitting tool access. This prevents path traversal attacks and
//! ensures tools only operate within the working directory or
//! explicitly allowed additional directories.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use allthecodes_types::permissions::ToolPermissionContext;

/// Check if a path is within the current working directory or any of the
/// allowed additional directories.
///
/// Returns `true` if the path is contained within at least one allowed
/// directory, `false` otherwise.
///
/// # Arguments
/// * `path` - The path to check (should be canonicalized or absolute).
/// * `cwd` - The current working directory.
/// * `ctx` - The tool permission context containing additional allowed directories.
pub fn is_path_within_allowed_directories(
    path: &Path,
    cwd: &Path,
    ctx: &ToolPermissionContext,
) -> bool {
    // Check if the path is within the CWD
    if is_path_in_directory(path, cwd) {
        return true;
    }

    // Check additional working directories
    for dir_info in ctx.additional_working_directories.values() {
        let dir_path = Path::new(&dir_info.path);
        if is_path_in_directory(path, dir_path) {
            return true;
        }
    }

    false
}

/// Validate and canonicalize a file path string.
///
/// Performs the following checks:
/// - Path is not empty
/// - Path does not contain null bytes
/// - Path does not contain suspicious sequences (e.g. excessive `..`)
/// - Attempts to canonicalize the path (resolves symlinks, `.`, `..`)
///
/// Returns the canonicalized `PathBuf` on success.
pub fn validate_file_path(path: &str) -> Result<PathBuf> {
    let trimmed = path.trim();

    if trimmed.is_empty() {
        bail!("File path cannot be empty");
    }

    if trimmed.contains('\0') {
        bail!("File path contains null byte");
    }

    // Detect obviously malicious patterns
    if contains_traversal_attack(trimmed) {
        bail!(
            "File path contains suspicious traversal pattern: '{}'",
            trimmed
        );
    }

    let path_buf = PathBuf::from(trimmed);

    // Try to canonicalize. If the file doesn't exist yet, we normalize
    // what we can by resolving the parent directory.
    match path_buf.canonicalize() {
        Ok(canonical) => Ok(canonical),
        Err(_) => {
            // File may not exist yet. Try to canonicalize the parent.
            if let Some(parent) = path_buf.parent() {
                if parent.as_os_str().is_empty() {
                    // Relative path with no parent -- use as-is
                    Ok(path_buf)
                } else {
                    match parent.canonicalize() {
                        Ok(canonical_parent) => {
                            let file_name = path_buf
                                .file_name()
                                .context("Path has no file name component")?;
                            Ok(canonical_parent.join(file_name))
                        }
                        Err(_) => {
                            // Parent doesn't exist either; return normalized form
                            Ok(normalize_path(&path_buf))
                        }
                    }
                }
            } else {
                Ok(path_buf)
            }
        }
    }
}

/// Check if `path` is contained within `dir`.
///
/// Both paths are compared using their string representation after
/// normalization. This function does NOT follow symlinks -- for
/// security-critical checks, canonicalize paths first.
pub fn is_path_in_directory(path: &Path, dir: &Path) -> bool {
    // Try canonical comparison first
    let canonical_path = path.canonicalize().unwrap_or_else(|_| normalize_path(path));
    let canonical_dir = dir.canonicalize().unwrap_or_else(|_| normalize_path(dir));

    canonical_path.starts_with(&canonical_dir)
}

/// Detect path traversal attack patterns.
///
/// Returns `true` if the path contains suspicious sequences that could
/// be used for directory traversal attacks.
fn contains_traversal_attack(path: &str) -> bool {
    // Count the number of `..` segments
    let traversal_count = path.split(['/', '\\']).filter(|seg| *seg == "..").count();

    // More than 5 parent directory references is suspicious
    if traversal_count > 5 {
        return true;
    }

    // Check for attempts to escape common safe directories
    let suspicious_patterns = [
        "/../../../..",
        "\\..\\..\\..\\..",
        "/etc/passwd",
        "/etc/shadow",
        "%00",    // null byte encoding
        "%2e%2e", // URL-encoded ..
    ];

    for pattern in &suspicious_patterns {
        if path.to_lowercase().contains(&pattern.to_lowercase()) {
            return true;
        }
    }

    false
}

/// Normalize a path by resolving `.` and `..` components without
/// filesystem access.
fn normalize_path(path: &Path) -> PathBuf {
    let mut components = Vec::new();

    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                // Pop the last component if possible
                if !components.is_empty() {
                    components.pop();
                }
            }
            std::path::Component::CurDir => {
                // Skip `.`
            }
            other => {
                components.push(other);
            }
        }
    }

    components.iter().collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_types::permissions::{
        AdditionalWorkingDirectory, PermissionMode, ToolPermissionContext,
    };
    use std::collections::HashMap;

    fn default_ctx() -> ToolPermissionContext {
        ToolPermissionContext {
            mode: PermissionMode::Default,
            additional_working_directories: HashMap::new(),
            always_allow_rules: HashMap::new(),
            always_deny_rules: HashMap::new(),
            always_ask_rules: HashMap::new(),
            session_allow_rules: HashMap::new(),
            auto_mode_stripped_always_allow_rules: Vec::new(),
            auto_mode_stripped_session_allow_rules: Vec::new(),
            is_bypass_permissions_mode_available: false,
            is_auto_mode_available: None,
            pre_plan_mode: None,
        }
    }

    #[test]
    fn test_path_in_directory() {
        assert!(is_path_in_directory(
            Path::new("/home/user/project/src/main.rs"),
            Path::new("/home/user/project")
        ));

        // Path equal to directory is considered "in" the directory
        assert!(is_path_in_directory(
            Path::new("/home/user/project"),
            Path::new("/home/user/project")
        ));
    }

    #[test]
    fn test_path_not_in_directory() {
        // Without canonicalization, simple prefix check
        let in_dir = is_path_in_directory(
            Path::new("/home/other/file.txt"),
            Path::new("/home/user/project"),
        );
        assert!(!in_dir);
    }

    #[test]
    fn test_validate_empty_path() {
        assert!(validate_file_path("").is_err());
        assert!(validate_file_path("   ").is_err());
    }

    #[test]
    fn test_validate_null_byte() {
        assert!(validate_file_path("file\0name.txt").is_err());
    }

    #[test]
    fn test_validate_traversal_attack() {
        assert!(validate_file_path("/../../../../../etc/passwd").is_err());
    }

    #[test]
    fn test_validate_normal_path() {
        // This may or may not succeed depending on whether the path exists,
        // but it should not error on the validation checks.
        let result = validate_file_path("src/main.rs");
        // It should at least not error on validation
        assert!(result.is_ok());
    }

    #[test]
    fn test_allowed_directories_cwd() {
        let ctx = default_ctx();
        let cwd = Path::new("/home/user/project");
        let path = Path::new("/home/user/project/src/file.rs");

        assert!(is_path_within_allowed_directories(path, cwd, &ctx));
    }

    #[test]
    fn test_allowed_directories_additional() {
        let mut ctx = default_ctx();
        ctx.additional_working_directories.insert(
            "shared".to_string(),
            AdditionalWorkingDirectory {
                path: "/home/user/shared-libs".to_string(),
                read_only: false,
            },
        );

        let cwd = Path::new("/home/user/project");
        let path = Path::new("/home/user/shared-libs/utils.rs");

        assert!(is_path_within_allowed_directories(path, cwd, &ctx));
    }

    #[test]
    fn test_outside_allowed_directories() {
        let ctx = default_ctx();
        let cwd = Path::new("/home/user/project");
        let path = Path::new("/tmp/malicious.sh");

        assert!(!is_path_within_allowed_directories(path, cwd, &ctx));
    }

    #[test]
    fn test_normalize_path() {
        let p = normalize_path(Path::new("/home/user/../user/project/./src"));
        assert_eq!(p, PathBuf::from("/home/user/project/src"));
    }

    #[test]
    fn test_traversal_detection() {
        assert!(contains_traversal_attack("/../../../../../etc/passwd"));
        assert!(contains_traversal_attack("/foo/%2e%2e/bar"));
        assert!(!contains_traversal_attack("../src/main.rs"));
        assert!(!contains_traversal_attack("../../Cargo.toml"));
    }
}
