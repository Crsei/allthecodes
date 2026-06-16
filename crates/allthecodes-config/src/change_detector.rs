//! Configuration file change detection.
//!
//! Tracks modification timestamps of settings files so the runtime can
//! detect when a file has been externally modified and needs to be reloaded.
//! Uses a simple mtime-based polling approach (no inotify/kqueue).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// A set of file paths grouped by whether they have changed.
#[derive(Debug, Clone, Default)]
pub struct ChangeSet {
    /// Paths whose mtime has changed (or are newly detected).
    pub changed: Vec<PathBuf>,
    /// Paths whose mtime is unchanged.
    pub unchanged: Vec<PathBuf>,
}

impl ChangeSet {
    /// Whether any files have changed.
    pub fn has_any_changed(&self) -> bool {
        !self.changed.is_empty()
    }

    /// Number of changed files.
    pub fn changed_count(&self) -> usize {
        self.changed.len()
    }

    /// Number of unchanged files.
    pub fn unchanged_count(&self) -> usize {
        self.unchanged.len()
    }

    /// Merge another ChangeSet into this one.
    pub fn merge(&mut self, other: ChangeSet) {
        self.changed.extend(other.changed);
        self.unchanged.extend(other.unchanged);
    }
}

/// Tracks file modification timestamps using mtime-based polling.
///
/// # Example
///
/// ```ignore
/// use std::path::PathBuf;
/// use allthecodes_config::change_detector::ChangeDetector;
///
/// let paths = vec![PathBuf::from("settings.json")];
/// let detector = ChangeDetector::watch(&paths);
/// // ... later ...
/// if detector.has_changed(Path::new("settings.json")) {
///     println!("settings.json was modified");
/// }
/// ```
#[derive(Debug, Clone)]
pub struct ChangeDetector {
    /// Known file paths and their last-seen modification times.
    mtimes: HashMap<PathBuf, SystemTime>,
}

impl ChangeDetector {
    /// Create a new detector, recording the current mtime for each path.
    ///
    /// Paths that do not exist at watch time are recorded with no mtime
    /// and will be reported as changed when they first appear.
    pub fn watch(paths: &[PathBuf]) -> Self {
        let mut mtimes = HashMap::with_capacity(paths.len());
        for path in paths {
            let mtime = std::fs::metadata(path)
                .ok()
                .and_then(|meta| meta.modified().ok());
            if let Some(mtime) = mtime {
                mtimes.insert(path.clone(), mtime);
            }
            // If the file doesn't exist yet, we don't insert it. When
            // `has_changed` is called later and the file exists, it will
            // be detected as changed.
        }
        Self { mtimes }
    }

    /// Check whether a single file has changed since it was watched.
    ///
    /// Returns `true` if:
    /// - The file was not previously tracked and now exists.
    /// - The file's mtime is different from the recorded value.
    /// - The file was previously tracked but no longer exists.
    ///
    /// Returns `false` if the file is tracked and its mtime matches.
    pub fn has_changed(&self, path: &Path) -> bool {
        let current_mtime = std::fs::metadata(path)
            .ok()
            .and_then(|meta| meta.modified().ok());

        match (self.mtimes.get(path), current_mtime) {
            (Some(old), Some(new)) => *old != new,
            (Some(_), None) => true, // file was deleted
            (None, Some(_)) => true, // file just appeared
            (None, None) => false,   // never tracked and still doesn't exist
        }
    }

    /// Check a set of paths and return a `ChangeSet` categorising them.
    pub fn changes(&self, paths: &[PathBuf]) -> ChangeSet {
        let mut changed = Vec::new();
        let mut unchanged = Vec::new();
        for path in paths {
            if self.has_changed(path) {
                changed.push(path.clone());
            } else {
                unchanged.push(path.clone());
            }
        }
        ChangeSet { changed, unchanged }
    }

    /// Update the stored mtime for a path to the current time.
    ///
    /// Call after you have processed a change (e.g. reloaded a file) so
    /// subsequent `has_changed` calls return `false` until the next real
    /// modification.
    pub fn refresh(&mut self, path: &Path) {
        if let Some(mtime) = std::fs::metadata(path)
            .ok()
            .and_then(|meta| meta.modified().ok())
        {
            self.mtimes.insert(path.to_path_buf(), mtime);
        } else {
            self.mtimes.remove(path);
        }
    }

    /// Refresh all tracked paths at once.
    pub fn refresh_all(&mut self) {
        let keys: Vec<PathBuf> = self.mtimes.keys().cloned().collect();
        for key in keys {
            self.refresh(&key);
        }
    }

    /// The number of tracked paths.
    pub fn tracked_count(&self) -> usize {
        self.mtimes.len()
    }

    /// Return all tracked paths.
    pub fn tracked_paths(&self) -> Vec<PathBuf> {
        self.mtimes.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_watch_empty() {
        let detector = ChangeDetector::watch(&[]);
        assert_eq!(detector.tracked_count(), 0);
    }

    #[test]
    fn test_watch_nonexistent_file() {
        let path = PathBuf::from("/nonexistent/file.json");
        let detector = ChangeDetector::watch(&[path.clone()]);
        // File doesn't exist, so it shouldn't be tracked.
        assert_eq!(detector.tracked_count(), 0);
        // has_changed should return false for a never-tracked, still-missing file.
        assert!(!detector.has_changed(&path));
    }

    #[test]
    fn test_detects_new_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("new.json");

        // Watch before the file exists.
        let detector = ChangeDetector::watch(&[path.clone()]);
        assert!(!detector.has_changed(&path));

        // Create the file.
        std::fs::write(&path, "{}").unwrap();
        assert!(detector.has_changed(&path));
    }

    #[test]
    fn test_detects_modification() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.json");
        std::fs::write(&path, "v1").unwrap();

        // Sleep to ensure mtime advances (filesystem ms resolution on ext4).
        std::thread::sleep(Duration::from_millis(1100));

        let detector = ChangeDetector::watch(&[path.clone()]);
        assert!(!detector.has_changed(&path));

        // Modify the file.
        std::fs::write(&path, "v2").unwrap();
        assert!(detector.has_changed(&path));
    }

    #[test]
    fn test_refresh_resets_change_state() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.json");
        std::fs::write(&path, "v1").unwrap();

        std::thread::sleep(Duration::from_millis(1100));

        let mut detector = ChangeDetector::watch(&[path.clone()]);
        assert!(!detector.has_changed(&path));

        std::fs::write(&path, "v2").unwrap();
        assert!(detector.has_changed(&path));

        // Refresh and check again.
        detector.refresh(&path);
        assert!(!detector.has_changed(&path));
    }

    #[test]
    fn test_changeset_merge() {
        let mut cs1 = ChangeSet {
            changed: vec![PathBuf::from("a.json")],
            unchanged: vec![],
        };
        let cs2 = ChangeSet {
            changed: vec![PathBuf::from("b.json")],
            unchanged: vec![PathBuf::from("c.json")],
        };
        cs1.merge(cs2);

        assert_eq!(cs1.changed_count(), 2);
        assert_eq!(cs1.unchanged_count(), 1);
        assert!(cs1.has_any_changed());
    }

    #[test]
    fn test_changes_method() {
        let dir = tempfile::tempdir().unwrap();
        let path_a = dir.path().join("a.json");
        let path_b = dir.path().join("b.json");
        std::fs::write(&path_a, "a").unwrap();
        std::fs::write(&path_b, "b").unwrap();

        // Sleep to ensure mtime advances.
        std::thread::sleep(Duration::from_millis(1100));

        let detector = ChangeDetector::watch(&[path_a.clone(), path_b.clone()]);
        let cs = detector.changes(&[path_a.clone(), path_b.clone()]);
        assert!(!cs.has_any_changed());

        std::fs::write(&path_a, "a2").unwrap();
        let cs = detector.changes(&[path_a.clone(), path_b.clone()]);
        assert!(cs.has_any_changed());
        assert_eq!(cs.changed_count(), 1);
        assert_eq!(cs.unchanged_count(), 1);
    }

    #[test]
    fn test_delete_detected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tmp.json");
        std::fs::write(&path, "x").unwrap();

        // Sleep to ensure mtime advances.
        std::thread::sleep(Duration::from_millis(1100));

        let detector = ChangeDetector::watch(&[path.clone()]);
        std::fs::remove_file(&path).unwrap();
        assert!(detector.has_changed(&path));
    }
}
