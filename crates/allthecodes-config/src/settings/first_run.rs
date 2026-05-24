//! First-run initialization — seeds `~/.allthecodes/settings.json` from an
//! embedded template when no global config exists yet.

use anyhow::{Context, Result};

use super::raw::RawSettings;
use super::write::write_settings_file;

/// Embedded template for the initial user-level settings.json.
///
/// Contains auth-profile scaffolding for both Claude Code (Anthropic) and
/// Codex (OpenAI). Empty-string placeholders are stripped by
/// [`sanitize_template`] before writing so they don't shadow env-var or
/// keychain resolution at load time.
const SETTINGS_TEMPLATE: &str = r#"{
    "activeAuthProfile": "claude_code",
    "authProfiles": {
        "claude_code": {
            "backend": "native",
            "apiProvider": "anthropic"
        },
        "codex": {
            "backend": "codex",
            "apiProvider": "openai-codex"
        }
    }
}"#;

/// Initialize the global data root on first run.
///
/// If `{data_root}/settings.json` does not exist, creates the directory
/// and seeds `settings.json` from the embedded template. If the file
/// already exists, this is a no-op.
///
/// Returns `Ok(true)` if first-run initialization was performed,
/// `Ok(false)` if skipped (not first run).
pub fn initialize_first_run() -> Result<bool> {
    let root = crate::paths::data_root();
    let settings_path = root.join("settings.json");

    // Idempotency: if settings.json already exists, nothing to do.
    if settings_path.exists() {
        return Ok(false);
    }

    // Ensure the directory tree exists.
    crate::paths::ensure_data_root()
        .context("failed to create data root directory during first-run init")?;

    // Parse the template and strip empty placeholder values.
    let template: RawSettings = serde_json::from_str(SETTINGS_TEMPLATE)
        .context("BUG: embedded settings template is invalid JSON")?;
    let template = sanitize_template(template);

    write_settings_file(&settings_path, &template)
        .context("failed to write initial settings.json")?;

    tracing::info!(
        path = %settings_path.display(),
        "first-run: seeded settings.json from template"
    );
    Ok(true)
}

/// Remove empty-string placeholder fields from the template so they
/// don't interfere with env-var or keychain resolution at load time.
fn sanitize_template(mut raw: RawSettings) -> RawSettings {
    if let Some(ref mut profiles) = raw.auth_profiles {
        for profile in profiles.values_mut() {
            if profile.api_key.as_deref() == Some("") {
                profile.api_key = None;
            }
            if profile.model.as_deref() == Some("") {
                profile.model = None;
            }
        }
    }
    raw
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::{data_root, ensure_data_root};
    use serial_test::serial;

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }

    #[test]
    #[serial]
    fn fresh_install_creates_settings_json() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("fresh");
        let _g = EnvGuard::set("ALLTHECODES_HOME", root.to_str().unwrap());
        assert!(!data_root().exists());

        let created = initialize_first_run().unwrap();
        assert!(created);

        let settings = root.join("settings.json");
        assert!(settings.exists(), "settings.json should be created");

        // Verify it parses back with the expected structure.
        let raw: RawSettings =
            serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
        assert_eq!(raw.active_auth_profile, Some("claude_code".to_string()));
        let profiles = raw.auth_profiles.expect("authProfiles should exist");
        assert!(profiles.contains_key("claude_code"));
        assert!(profiles.contains_key("codex"));
    }

    #[test]
    #[serial]
    fn existing_settings_is_not_overwritten() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("existing");
        let _g = EnvGuard::set("ALLTHECODES_HOME", root.to_str().unwrap());
        ensure_data_root().unwrap();
        let custom = r#"{"model":"claude-opus-4-7"}"#;
        std::fs::write(root.join("settings.json"), custom).unwrap();

        let created = initialize_first_run().unwrap();
        assert!(!created, "should not overwrite existing settings");

        let content = std::fs::read_to_string(root.join("settings.json")).unwrap();
        assert_eq!(content, custom, "existing content must be preserved");
    }

    #[test]
    #[serial]
    fn template_parses_correctly() {
        let raw: RawSettings =
            serde_json::from_str(SETTINGS_TEMPLATE).expect("template must be valid JSON");
        assert_eq!(raw.active_auth_profile, Some("claude_code".to_string()));
        let profiles = raw.auth_profiles.expect("authProfiles must exist");
        let cc = profiles.get("claude_code").expect("claude_code profile");
        assert_eq!(cc.backend.as_deref(), Some("native"));
        assert_eq!(cc.api_provider.as_deref(), Some("anthropic"));
        let codex = profiles.get("codex").expect("codex profile");
        assert_eq!(codex.backend.as_deref(), Some("codex"));
        assert_eq!(codex.api_provider.as_deref(), Some("openai-codex"));
    }

    #[test]
    #[serial]
    fn sanitize_strips_empty_placeholders() {
        let mut raw: RawSettings =
            serde_json::from_str(SETTINGS_TEMPLATE).expect("template must be valid JSON");

        // Add empty-string placeholders.
        if let Some(ref mut profiles) = raw.auth_profiles {
            profiles.get_mut("claude_code").unwrap().api_key = Some("".to_string());
            profiles.get_mut("claude_code").unwrap().model = Some("".to_string());
        }

        let cleaned = sanitize_template(raw);
        let profiles = cleaned.auth_profiles.expect("authProfiles must exist");
        assert_eq!(
            profiles.get("claude_code").unwrap().api_key,
            None,
            "empty api_key should be None"
        );
        assert_eq!(
            profiles.get("claude_code").unwrap().model,
            None,
            "empty model should be None"
        );
    }

    #[test]
    #[serial]
    fn template_round_trips_through_write_and_load() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("roundtrip");
        let _g = EnvGuard::set("ALLTHECODES_HOME", root.to_str().unwrap());
        ensure_data_root().unwrap();

        let raw: RawSettings = serde_json::from_str(SETTINGS_TEMPLATE).unwrap();
        let settings_path = root.join("settings.json");
        write_settings_file(&settings_path, &raw).unwrap();

        let loaded: RawSettings =
            serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
        assert_eq!(loaded.active_auth_profile, Some("claude_code".to_string()));
    }
}
