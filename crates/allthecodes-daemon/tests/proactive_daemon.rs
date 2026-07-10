#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::ffi::{OsStr, OsString};

use allthecodes_config::features::FeatureFlags;
use serial_test::serial;

struct EnvGuard {
    key: &'static str,
    previous: Option<OsString>,
}

impl EnvGuard {
    fn set(key: &'static str, value: impl AsRef<OsStr>) -> Self {
        let previous = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, previous }
    }

    fn remove(key: &'static str) -> Self {
        let previous = std::env::var_os(key);
        std::env::remove_var(key);
        Self { key, previous }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        if let Some(previous) = &self.previous {
            std::env::set_var(self.key, previous);
        } else {
            std::env::remove_var(self.key);
        }
    }
}

#[test]
#[serial]
fn proactive_daemon_mode_accepts_standalone_feature() {
    let temp = tempfile::tempdir().expect("temp ALLTHECODES_HOME");
    let _home = EnvGuard::set("ALLTHECODES_HOME", temp.path());
    let _proactive = EnvGuard::set("FEATURE_PROACTIVE", "1");
    let _kairos = EnvGuard::remove("FEATURE_KAIROS");

    let flags = FeatureFlags::from_env();

    assert!(flags.proactive);
    assert!(!flags.kairos);
}
