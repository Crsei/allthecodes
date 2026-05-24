//! Onboarding state — first-run wizard progress persisted across sessions.
//!
//! Shared between `/logout` (issue #43), which must reset the state, and
//! `/team-onboarding` (issue #63), which reads the current user's progress
//! to decide what sections to include in the teammate-facing guide.
//!
//! File layout: `{data_root}/onboarding.json`.
//!
//! The schema is intentionally small — only the fields we actually
//! inspect today. Adding a new flag is a matter of extending the struct
//! with `#[serde(default)]` so old files keep deserializing.

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Schema version. Bumped when we change the meaning of an existing field.
/// Adding a new field with `#[serde(default)]` does NOT require a bump.
pub const ONBOARDING_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Error)]
pub enum OnboardingError {
    #[error("I/O error touching {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to decode onboarding state at {path}: {source}")]
    Decode {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("failed to encode onboarding state: {0}")]
    Encode(#[source] serde_json::Error),
}

/// Persistent onboarding progress.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OnboardingState {
    #[serde(default = "default_version")]
    pub version: u32,
    /// Whether the user has run through the first-time setup at least once.
    #[serde(default)]
    pub has_completed_onboarding: bool,
    /// Whether the IDE integration dialog was accepted or dismissed.
    #[serde(default)]
    pub ide_onboarding_done: bool,
    /// Whether the auth setup step was completed. Used by `/logout` so a
    /// freshly-logged-out user re-runs auth on next start.
    #[serde(default)]
    pub auth_onboarding_done: bool,
    /// Whether the theme / statusline customization step ran.
    #[serde(default)]
    pub ui_onboarding_done: bool,
    /// Optional friendly name collected during onboarding (team-onboarding
    /// templates the welcome paragraph around it when present).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// Timestamp of the most recent successful onboarding completion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
}

fn default_version() -> u32 {
    ONBOARDING_SCHEMA_VERSION
}

impl Default for OnboardingState {
    fn default() -> Self {
        Self {
            version: ONBOARDING_SCHEMA_VERSION,
            has_completed_onboarding: false,
            ide_onboarding_done: false,
            auth_onboarding_done: false,
            ui_onboarding_done: false,
            display_name: None,
            completed_at: None,
        }
    }
}

impl OnboardingState {
    /// Reset every field that should vanish when the user logs out. Keeps
    /// the file around (the wrapper `OnboardingStore::reset()` deletes it
    /// instead); this is exposed for in-memory reset in tests or UI flows.
    pub fn reset_for_logout(&mut self) {
        self.has_completed_onboarding = false;
        self.ide_onboarding_done = false;
        self.auth_onboarding_done = false;
        self.ui_onboarding_done = false;
        self.completed_at = None;
        // display_name is preserved — it's a user preference, not an auth
        // artifact — unless explicitly cleared by the UI.
    }

    /// Does the state look meaningfully populated? Used by `/team-onboarding`
    /// to decide whether to show "first-run" language or not.
    pub fn is_first_run(&self) -> bool {
        !self.has_completed_onboarding
            && !self.auth_onboarding_done
            && !self.ide_onboarding_done
            && !self.ui_onboarding_done
    }
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

/// File-backed onboarding state. Safe across threads in one process; across
/// processes the worst case is a lost-update on the final write, which is
/// acceptable for the onboarding flow (it only runs once per install).
pub struct OnboardingStore {
    path: PathBuf,
    inner: Mutex<()>,
}

impl OnboardingStore {
    pub fn default_path() -> PathBuf {
        allthecodes_config::paths::data_root().join("onboarding.json")
    }

    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            inner: Mutex::new(()),
        }
    }

    pub fn open_default() -> Self {
        Self::new(Self::default_path())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Read the current state. Missing file → default state (first run).
    pub fn load(&self) -> Result<OnboardingState, OnboardingError> {
        let _guard = self.inner.lock();
        self.read_from_disk()
    }

    /// Mutate the state through a closure. Reads, mutates in-place, writes
    /// back atomically. This is the only write path today — `/logout`
    /// calls it with `OnboardingState::reset_for_logout` to preserve
    /// preferences (display_name) while clearing identity artifacts.
    pub fn update<F>(&self, f: F) -> Result<OnboardingState, OnboardingError>
    where
        F: FnOnce(&mut OnboardingState),
    {
        let _guard = self.inner.lock();
        let mut state = self.read_from_disk()?;
        f(&mut state);
        self.write_to_disk(&state)?;
        Ok(state)
    }

    // -----------------------------------------------------------------
    // Internals
    // -----------------------------------------------------------------

    fn read_from_disk(&self) -> Result<OnboardingState, OnboardingError> {
        if !self.path.exists() {
            return Ok(OnboardingState::default());
        }
        let mut file = File::open(&self.path).map_err(|e| OnboardingError::Io {
            path: self.path.clone(),
            source: e,
        })?;
        let mut buf = String::new();
        file.read_to_string(&mut buf)
            .map_err(|e| OnboardingError::Io {
                path: self.path.clone(),
                source: e,
            })?;
        if buf.trim().is_empty() {
            return Ok(OnboardingState::default());
        }
        let parsed = serde_json::from_str(&buf).map_err(|e| OnboardingError::Decode {
            path: self.path.clone(),
            source: e,
        })?;
        Ok(parsed)
    }

    fn write_to_disk(&self, state: &OnboardingState) -> Result<(), OnboardingError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|e| OnboardingError::Io {
                path: parent.to_path_buf(),
                source: e,
            })?;
        }
        let bytes = serde_json::to_vec_pretty(state).map_err(OnboardingError::Encode)?;
        let tmp_path = self.path.with_extension("json.tmp");
        {
            let mut tmp = File::create(&tmp_path).map_err(|e| OnboardingError::Io {
                path: tmp_path.clone(),
                source: e,
            })?;
            tmp.write_all(&bytes).map_err(|e| OnboardingError::Io {
                path: tmp_path.clone(),
                source: e,
            })?;
            tmp.flush().map_err(|e| OnboardingError::Io {
                path: tmp_path.clone(),
                source: e,
            })?;
        }
        fs::rename(&tmp_path, &self.path).map_err(|e| OnboardingError::Io {
            path: self.path.clone(),
            source: e,
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn fresh_store() -> (tempfile::TempDir, OnboardingStore) {
        let dir = tempdir().unwrap();
        let store = OnboardingStore::new(dir.path().join("onboarding.json"));
        (dir, store)
    }

    #[test]
    fn missing_file_returns_default_state() {
        let (_dir, store) = fresh_store();
        let state = store.load().unwrap();
        assert_eq!(state, OnboardingState::default());
        assert!(state.is_first_run());
    }

    #[test]
    fn update_applies_closure() {
        let (_dir, store) = fresh_store();
        let out = store
            .update(|s| {
                s.auth_onboarding_done = true;
                s.display_name = Some("Sam".into());
            })
            .unwrap();
        assert!(out.auth_onboarding_done);
        assert_eq!(out.display_name, Some("Sam".into()));
        let loaded = store.load().unwrap();
        assert_eq!(loaded, out);
    }

    #[test]
    fn reset_for_logout_preserves_display_name() {
        let mut state = OnboardingState {
            has_completed_onboarding: true,
            auth_onboarding_done: true,
            display_name: Some("Sam".into()),
            completed_at: Some(Utc::now()),
            ..OnboardingState::default()
        };
        state.reset_for_logout();
        assert!(!state.has_completed_onboarding);
        assert!(!state.auth_onboarding_done);
        assert_eq!(state.display_name, Some("Sam".into()));
    }

    #[test]
    fn unknown_fields_dont_break_decode() {
        let (_dir, store) = fresh_store();
        fs::write(
            store.path(),
            r#"{"version":1,"has_completed_onboarding":true,"unknown_future_field":"x"}"#,
        )
        .unwrap();
        let state = store.load().unwrap();
        assert!(state.has_completed_onboarding);
    }
}
