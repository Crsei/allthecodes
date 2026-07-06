pub const COORDINATOR_CHAT_MODE: &str = "coordinator";

pub fn set_session_coordinator_mode(
    session_id: &str,
    cwd: &str,
    enabled: bool,
) -> anyhow::Result<()> {
    let value = enabled.then_some(COORDINATOR_CHAT_MODE);
    allthecodes_session::storage::set_session_chat_mode_override(session_id, value, cwd)?;
    crate::coordinator::set_coordinator_mode_enabled(enabled);
    Ok(())
}

pub fn match_session_mode(session_id: &str, _cwd: &str) -> anyhow::Result<bool> {
    if !allthecodes_session::storage::get_session_file(session_id).exists() {
        return Ok(false);
    }

    let info = allthecodes_session::storage::load_session_info(session_id)?;
    let enabled = info.chat_mode_override.as_deref() == Some(COORDINATOR_CHAT_MODE);
    crate::coordinator::set_coordinator_mode_enabled(enabled);
    Ok(enabled)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set_path(key: &'static str, value: &std::path::Path) -> Self {
            let previous = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    struct FeatureOverrideGuard;

    impl Drop for FeatureOverrideGuard {
        fn drop(&mut self) {
            allthecodes_config::features::clear_runtime_override();
        }
    }

    #[test]
    #[serial_test::serial]
    fn coordinator_session_mode_roundtrip_uses_chat_mode_override() {
        let temp = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set_path("ALLTHECODES_HOME", temp.path());
        let _features = FeatureOverrideGuard;
        allthecodes_config::features::set_runtime_override(
            allthecodes_config::features::FeatureFlags::all_disabled(),
        );

        set_session_coordinator_mode("sess-coord", "/repo", true).unwrap();
        assert!(match_session_mode("sess-coord", "/repo").unwrap());
        assert!(crate::coordinator::is_coordinator_mode_enabled());

        set_session_coordinator_mode("sess-coord", "/repo", false).unwrap();
        assert!(!match_session_mode("sess-coord", "/repo").unwrap());
        assert!(!crate::coordinator::is_coordinator_mode_enabled());
    }
}
