use parking_lot::Mutex;
use std::path::{Path, PathBuf};

/// Global working directory override.
///
/// When set, this is used instead of `std::env::current_dir()` to determine
/// the effective working directory for tool execution. This allows the
/// application to track a logical working directory that can differ from
/// the process's actual cwd (e.g., after a `cd` command in Bash tool).
static CWD: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Set the effective working directory.
///
/// This overrides the process cwd for all subsequent calls to `get_cwd()`.
/// Pass an absolute path for predictable behavior.
pub fn set_cwd(path: &str) {
    let path_buf = PathBuf::from(path);
    let mut guard = CWD.lock();
    *guard = Some(path_buf);
}

/// Get the effective working directory.
///
/// Returns the directory set via `set_cwd()`, or falls back to the
/// process's current working directory if no override has been set.
pub fn get_cwd() -> PathBuf {
    let guard = CWD.lock();
    match guard.as_ref() {
        Some(path) => path.clone(),
        None => std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    }
}

/// Reset the working directory override, reverting to the process cwd.
pub fn reset_cwd() {
    let mut guard = CWD.lock();
    *guard = None;
}

/// Resolve a potentially relative path against the effective working directory.
///
/// If `path` is absolute, returns it as-is.
/// If `path` is relative, joins it with `get_cwd()`.
pub fn resolve_path(path: &str) -> PathBuf {
    let p = Path::new(path);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        get_cwd().join(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_cwd() {
        // Reset to ensure clean state
        reset_cwd();
        let cwd = get_cwd();
        // Should return a valid path (process cwd)
        assert!(!cwd.as_os_str().is_empty());
    }

    #[test]
    fn test_set_and_get_cwd() {
        set_cwd("/tmp/test-dir");
        let cwd = get_cwd();
        assert_eq!(cwd, PathBuf::from("/tmp/test-dir"));

        // Cleanup
        reset_cwd();
    }

    #[test]
    fn test_reset_cwd() {
        set_cwd("/tmp/custom");
        reset_cwd();
        let cwd = get_cwd();
        // After reset, should fall back to process cwd, not /tmp/custom
        assert_ne!(cwd, PathBuf::from("/tmp/custom"));
    }

    #[test]
    fn test_resolve_absolute_path() {
        set_cwd("/home/user/project");
        let resolved = resolve_path("/etc/config");
        assert_eq!(resolved, PathBuf::from("/etc/config"));
        reset_cwd();
    }

    #[test]
    fn test_resolve_relative_path() {
        set_cwd("/home/user/project");
        let resolved = resolve_path("src/main.rs");
        assert_eq!(resolved, PathBuf::from("/home/user/project/src/main.rs"));
        reset_cwd();
    }
}
