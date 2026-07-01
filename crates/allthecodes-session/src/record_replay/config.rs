/// Runtime flags controlling durable session record/replay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordReplayConfig {
    pub enabled: bool,
    pub read_prefer_replay: bool,
    pub include_raw_stream: bool,
    pub include_tool_progress: bool,
    pub fsync_on_turn_finish: bool,
    pub redaction_enabled: bool,
    pub fsync_on_flush: bool,
}

impl Default for RecordReplayConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            read_prefer_replay: true,
            include_raw_stream: false,
            include_tool_progress: false,
            fsync_on_turn_finish: true,
            redaction_enabled: true,
            fsync_on_flush: false,
        }
    }
}

impl RecordReplayConfig {
    pub fn from_env() -> Self {
        let mut config = Self::default();
        if env_flag_is_false("ALLTHECODES_RECORD_REPLAY") {
            config.enabled = false;
        }
        if env_flag_is_true("ALLTHECODES_RECORD_REPLAY_READ_PREFER_LEGACY") {
            config.read_prefer_replay = false;
        }
        config
    }
}

fn env_flag_is_false(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .map(|value| matches!(value.trim(), "0" | "false" | "FALSE" | "off" | "OFF"))
        .unwrap_or(false)
}

fn env_flag_is_true(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .map(|value| matches!(value.trim(), "1" | "true" | "TRUE" | "on" | "ON"))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
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
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    #[test]
    fn default_keeps_diagnostic_streams_disabled() {
        let config = RecordReplayConfig::default();
        assert!(config.enabled);
        assert!(config.read_prefer_replay);
        assert!(!config.include_raw_stream);
        assert!(!config.include_tool_progress);
    }

    #[test]
    #[serial]
    fn env_can_disable_record_replay_and_prefer_legacy_reads() {
        let _record = EnvGuard::set("ALLTHECODES_RECORD_REPLAY", "0");
        let _read = EnvGuard::set("ALLTHECODES_RECORD_REPLAY_READ_PREFER_LEGACY", "1");

        let config = RecordReplayConfig::from_env();
        assert!(!config.enabled);
        assert!(!config.read_prefer_replay);
    }
}
