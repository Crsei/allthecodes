//! File change watcher for hooks.
//!
//! Watches filesystem paths configured in FileChanged hooks and fires
//! hooks when files change.
//!
//! Port of TypeScript `fileChangedWatcher.ts`.
//!
//! NOTE: This is a structural stub. The TypeScript version uses `chokidar`
//! for filesystem watching. In Rust, this would use the `notify` crate.
//! The full implementation requires:
//!   - `notify` crate for file system events
//!   - Integration with the hook execution system
//!   - Dynamic watch path updates from hook output

use std::collections::HashSet;
use std::sync::{LazyLock, Mutex};

/// Registered watch paths for the file changed watcher.
static WATCH_PATHS: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

/// Callback type for file change notifications.
pub type FileChangedCallback = Box<dyn Fn(&str, &str) + Send + Sync>;

static NOTIFY_CALLBACK: LazyLock<Mutex<Option<FileChangedCallback>>> =
    LazyLock::new(|| Mutex::new(None));

/// Set the file change notification callback.
pub fn set_file_changed_notifier(cb: Option<FileChangedCallback>) {
    *NOTIFY_CALLBACK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = cb;
}

/// Update the dynamic watch paths from hook output.
pub fn update_watch_paths(paths: &[String]) {
    let mut watched = WATCH_PATHS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    watched.clear();
    for p in paths {
        watched.insert(p.clone());
    }
}

/// Get current watch paths.
pub fn get_watch_paths() -> Vec<String> {
    WATCH_PATHS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .iter()
        .cloned()
        .collect()
}

/// Handle a file change event (called by the file watcher).
pub fn handle_file_event(path: &str, event: &str) {
    if let Some(cb) = NOTIFY_CALLBACK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .as_ref()
    {
        cb(path, event);
    }
}

/// Reset file changed watcher state (for testing).
pub fn reset_file_changed_watcher() {
    WATCH_PATHS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clear();
    *NOTIFY_CALLBACK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
}

/// Initialize the file changed watcher for a given working directory.
pub fn initialize_file_changed_watcher(_cwd: &str) {
    // TODO: Full implementation requires the `notify` crate:
    //
    // 1. Read FileChanged hook config from snapshot
    // 2. Resolve static paths from matcher patterns
    // 3. Merge with dynamic watch paths
    // 4. Start notify::Watcher with channel
    // 5. Handle events by calling handle_file_event()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[serial]
    #[test]
    fn test_update_and_get_watch_paths() {
        reset_file_changed_watcher();

        update_watch_paths(&["/tmp/test".into(), "/var/log".into()]);
        let paths = get_watch_paths();
        assert_eq!(paths.len(), 2);
        assert!(paths.contains(&"/tmp/test".to_string()));
    }

    #[serial]
    #[test]
    fn test_handle_file_event() {
        reset_file_changed_watcher();

        let called = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let called_clone = called.clone();

        set_file_changed_notifier(Some(Box::new(move |_path, _event| {
            called_clone.store(true, std::sync::atomic::Ordering::Relaxed);
        })));

        handle_file_event("/tmp/test.txt", "change");

        assert!(called.load(std::sync::atomic::Ordering::Relaxed));
    }

    // CS-002: Reset clears all state.
    #[serial]
    #[test]
    fn test_reset_clears_state() {
        reset_file_changed_watcher();

        // Set up some state.
        update_watch_paths(&["/tmp/foo".into(), "/tmp/bar".into()]);
        set_file_changed_notifier(Some(Box::new(|_, _| {})));

        assert!(!get_watch_paths().is_empty());

        reset_file_changed_watcher();

        assert!(get_watch_paths().is_empty());

        // Also verify no panic when calling handle_file_event after reset.
        handle_file_event("/tmp/test.txt", "change");
    }

    // CS-002: Empty watch paths update doesn't crash.
    #[serial]
    #[test]
    fn test_update_watch_paths_empty() {
        reset_file_changed_watcher();

        // First put something in, then clear with empty slice.
        update_watch_paths(&["/tmp/foo".into()]);
        assert_eq!(get_watch_paths().len(), 1);

        update_watch_paths(&[]);
        assert!(get_watch_paths().is_empty());
    }

    // CS-002: Get watch paths returns empty when nothing set.
    #[serial]
    #[test]
    fn test_get_watch_paths_empty_initial() {
        reset_file_changed_watcher();
        let paths = get_watch_paths();
        assert!(paths.is_empty());
    }

    // CS-002: Duplicate paths are de-duplicated by HashSet.
    #[serial]
    #[test]
    fn test_update_watch_paths_dedup() {
        reset_file_changed_watcher();

        update_watch_paths(&["/tmp/dup".into(), "/tmp/dup".into(), "/tmp/unique".into()]);
        let paths = get_watch_paths();
        assert_eq!(paths.len(), 2);
    }
}
