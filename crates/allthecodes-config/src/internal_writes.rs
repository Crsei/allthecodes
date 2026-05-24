//! Internal write-token mechanism.
//!
//! Prevents infinite write loops when configuration files are modified by
//! our own writes. The `InternalWriteGuard` RAII guard registers a write
//! intent before modifying a config file, so the change detector can skip
//! re-processing files that we just wrote ourselves.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

// ---------------------------------------------------------------------------
// Global write-path registry
// ---------------------------------------------------------------------------

static ACTIVE_WRITES: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();

fn active_writes() -> &'static Mutex<HashSet<PathBuf>> {
    ACTIVE_WRITES.get_or_init(|| Mutex::new(HashSet::new()))
}

// ---------------------------------------------------------------------------
// InternalWriteGuard — RAII guard
// ---------------------------------------------------------------------------

/// RAII guard that marks a file path as being written by us.
///
/// While the guard is alive, the change detector should consider the file
/// as having been intentionally modified and not trigger a reload cycle.
///
/// # Example
///
/// ```ignore
/// use std::path::Path;
/// use allthecodes_config::internal_writes::{begin_write, is_active_write};
///
/// let path = Path::new("settings.json");
/// let guard = begin_write(path);
/// assert!(is_active_write(path));
/// drop(guard);
/// assert!(!is_active_write(path));
/// ```
#[derive(Debug)]
pub struct InternalWriteGuard {
    path: PathBuf,
}

impl InternalWriteGuard {
    /// The path being written.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for InternalWriteGuard {
    fn drop(&mut self) {
        if let Ok(mut set) = active_writes().lock() {
            set.remove(&self.path);
        }
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Register a write intent for `path`, returning a guard.
///
/// Returns `None` if the global lock is poisoned (should not happen in
/// normal operation).
pub fn begin_write(path: &Path) -> Option<InternalWriteGuard> {
    let mut set = active_writes().lock().ok()?;
    set.insert(path.to_path_buf());
    Some(InternalWriteGuard {
        path: path.to_path_buf(),
    })
}

/// Explicitly commit (end) a write. Equivalent to dropping the guard.
///
/// This is a convenience for cases where the guard's lifetime is awkward
/// to manage. After calling this, the path is no longer tracked as an
/// active write.
pub fn commit_write(guard: InternalWriteGuard) {
    drop(guard);
}

/// Check whether `path` is currently being written by us.
///
/// The change detector should call this before raising a change event.
pub fn is_active_write(path: &Path) -> bool {
    active_writes()
        .lock()
        .map(|set| set.contains(path))
        .unwrap_or(false)
}

/// Clear all active write tokens. Used in tests and during shutdown.
pub fn clear_all_writes() {
    if let Ok(mut set) = active_writes().lock() {
        set.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// All global-state tests are combined into one function because
    /// `clear_all_writes` and the shared `ACTIVE_WRITES` set create races
    /// when run in parallel with other tests.
    #[test]
    fn test_global_state_operations() {
        // --- begin_write registers path ---
        let path = PathBuf::from("/tmp/test-settings.json");
        {
            let guard = begin_write(&path);
            assert!(guard.is_some());
            assert!(is_active_write(&path));
        }
        assert!(!is_active_write(&path));

        // --- commit_write ends tracking ---
        let path = PathBuf::from("/tmp/test-settings2.json");
        let guard = begin_write(&path).unwrap();
        assert!(is_active_write(&path));
        commit_write(guard);
        assert!(!is_active_write(&path));

        // --- multiple writes tracked independently ---
        let path_a = PathBuf::from("/tmp/a.json");
        let path_b = PathBuf::from("/tmp/b.json");

        let guard_a = begin_write(&path_a).unwrap();
        let guard_b = begin_write(&path_b).unwrap();

        assert!(is_active_write(&path_a));
        assert!(is_active_write(&path_b));

        drop(guard_a);
        assert!(!is_active_write(&path_a));
        assert!(is_active_write(&path_b));

        drop(guard_b);
        assert!(!is_active_write(&path_b));

        // --- clear_all_writes ---
        let path = PathBuf::from("/tmp/clear-test.json");
        let _guard = begin_write(&path).unwrap();
        assert!(is_active_write(&path));
        clear_all_writes();
        assert!(!is_active_write(&path));

        // --- guard path accessor ---
        let path = PathBuf::from("/tmp/guard-path.json");
        let guard = begin_write(&path).unwrap();
        assert_eq!(guard.path(), Path::new("/tmp/guard-path.json"));
    }

    /// This test doesn't interact with global state, so it can run in parallel.
    #[test]
    fn test_nonexistent_path() {
        assert!(!is_active_write(Path::new("/nonexistent/path.json")));
    }
}
