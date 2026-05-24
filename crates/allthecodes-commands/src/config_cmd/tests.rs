use super::*;
use allthecodes_bootstrap::SessionId;
use allthecodes_engine::types::app_state::AppState;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, OnceLock};

fn test_ctx() -> CommandContext {
    test_ctx_with_cwd(PathBuf::from("/test/project"))
}

fn test_ctx_with_cwd(cwd: PathBuf) -> CommandContext {
    CommandContext {
        messages: Vec::new(),
        cwd,
        app_state: AppState::default(),
        session_id: SessionId::from_string("test-session"),
    }
}

#[tokio::test]
async fn test_config_show() {
    let handler = ConfigHandler;
    let mut ctx = test_ctx();
    let result = handler.execute("show", &mut ctx).await.unwrap();
    match result {
        CommandResult::Output(text) => {
            assert!(text.contains("model"));
            assert!(text.contains("backend"));
            assert!(text.contains("permissionMode"));
            assert!(text.contains("File locations"));
        }
        _ => panic!("Expected Output result"),
    }
}

#[tokio::test]
#[serial_test::serial]
async fn test_config_set_model_is_read_only() {
    // Use a tempdir as ALLTHECODES_HOME so we don't clobber the real user file.
    let dir = tempfile::tempdir().unwrap();
    let _g = EnvGuard::set("ALLTHECODES_HOME", dir.path().to_str().unwrap());
    let handler = ConfigHandler;
    let mut ctx = test_ctx();
    let result = handler
        .execute("set model claude-opus", &mut ctx)
        .await
        .unwrap();
    match result {
        CommandResult::Output(text) => {
            assert!(text.contains("read-only"));
            assert!(text.contains("/model"));
        }
        _ => panic!("Expected Output result"),
    }
    assert_ne!(ctx.app_state.main_loop_model, "claude-opus");
    assert!(!dir.path().join("settings.json").exists());
}

#[tokio::test]
async fn test_config_set_model_rejects_write_entry() {
    let handler = ConfigHandler;
    let mut ctx = test_ctx();
    let result = handler.execute("set model sonnet", &mut ctx).await.unwrap();
    match result {
        CommandResult::Output(text) => assert!(text.contains("read-only")),
        _ => panic!("Expected Output result"),
    }
    assert_ne!(ctx.app_state.main_loop_model, "sonnet");
}

#[tokio::test]
#[serial_test::serial]
async fn test_config_set_model_reasoning_effort() {
    let dir = tempfile::tempdir().unwrap();
    let _g = EnvGuard::set("ALLTHECODES_HOME", dir.path().to_str().unwrap());
    let handler = ConfigHandler;
    let mut ctx = test_ctx();
    let result = handler
        .execute("set model_reasoning_effort xhigh", &mut ctx)
        .await
        .unwrap();
    let CommandResult::Output(text) = result else {
        panic!("expected output")
    };
    assert!(text.contains("read-only"));
    assert!(text.contains("/effort"));
    assert!(ctx.app_state.settings.model_reasoning_effort.is_none());
    assert!(!dir.path().join("settings.json").exists());
}

#[tokio::test]
#[serial_test::serial]
async fn test_config_set_permission_mode_updates_live_context() {
    let dir = tempfile::tempdir().unwrap();
    let _g = EnvGuard::set("ALLTHECODES_HOME", dir.path().to_str().unwrap());
    let handler = ConfigHandler;
    let mut ctx = test_ctx();
    let result = handler
        .execute("set permissionMode dontAsk", &mut ctx)
        .await
        .unwrap();
    let CommandResult::Output(text) = result else {
        panic!("expected output")
    };
    assert!(text.contains("Permission mode set to: dontAsk"));
    assert_eq!(
        ctx.app_state.tool_permission_context.mode,
        PermissionMode::DontAsk
    );
}

#[tokio::test]
#[serial_test::serial]
async fn test_config_set_permission_mode_full_access_canonicalizes_to_bypass() {
    let dir = tempfile::tempdir().unwrap();
    let _g = EnvGuard::set("ALLTHECODES_HOME", dir.path().to_str().unwrap());
    let handler = ConfigHandler;
    let mut ctx = test_ctx();
    let result = handler
        .execute("set permissionMode full access", &mut ctx)
        .await
        .unwrap();
    let CommandResult::Output(text) = result else {
        panic!("expected output")
    };
    assert!(text.contains("Permission mode set to: bypass"));
    assert_eq!(
        ctx.app_state.tool_permission_context.mode,
        PermissionMode::Bypass
    );

    let settings: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.path().join("settings.json")).unwrap())
            .unwrap();
    assert_eq!(settings["permissionMode"], "bypass");
}

#[tokio::test]
#[serial_test::serial]
async fn test_config_set_permission_mode_auto_respects_disabled_policy() {
    let dir = tempfile::tempdir().unwrap();
    let _g = EnvGuard::set("ALLTHECODES_HOME", dir.path().to_str().unwrap());
    let handler = ConfigHandler;
    let mut ctx = test_ctx();
    ctx.app_state.tool_permission_context.is_auto_mode_available = Some(false);

    let result = handler.execute("set permissionMode auto", &mut ctx).await;

    match result {
        Ok(_) => panic!("auto mode should be rejected when disabled"),
        Err(err) => assert!(
            err.to_string().contains("permissions.enableAutoMode=false"),
            "unexpected error: {err:#}"
        ),
    }
    assert_eq!(
        ctx.app_state.tool_permission_context.mode,
        PermissionMode::Default
    );
    assert!(!dir.path().join("settings.json").exists());
}

#[tokio::test]
async fn test_config_set_invalid_bool_is_visible_and_does_not_mutate() {
    let handler = ConfigHandler;
    let mut ctx = test_ctx();

    let result = handler
        .execute("set voiceEnabled definitely", &mut ctx)
        .await;

    match result {
        Ok(_) => panic!("invalid boolean should fail visibly"),
        Err(err) => assert!(err.to_string().contains("Invalid boolean")),
    }
    assert_eq!(ctx.app_state.settings.voice_enabled, None);
}

#[tokio::test]
async fn test_config_set_invalid_existing_file_does_not_publish_staged_state() {
    let dir = tempfile::tempdir().unwrap();
    let project_root = dir.path().join("workspace");
    let nested = project_root.join("src");
    let project_dir = project_root.join(".allthecodes");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(project_dir.join("settings.json"), "{ invalid json").unwrap();

    let handler = ConfigHandler;
    let mut ctx = test_ctx_with_cwd(nested);
    let result = handler.execute("set theme light --project", &mut ctx).await;

    match result {
        Ok(_) => panic!("invalid existing settings should fail visibly"),
        Err(err) => assert!(!err.to_string().is_empty()),
    }
    assert_eq!(ctx.app_state.settings.theme, None);
    assert!(!ctx.app_state.settings.sources.contains_key("theme"));
}

#[tokio::test]
async fn test_config_set_project_from_subdir_preserves_existing_settings() {
    let dir = tempfile::tempdir().unwrap();
    let project_root = dir.path().join("workspace");
    let nested = project_root.join("src").join("nested");
    let project_dir = project_root.join(".allthecodes");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::create_dir_all(&project_dir).unwrap();

    let existing = serde_json::json!({
        "model": "claude-opus",
        "verbose": true,
        "theme": "dark"
    });
    std::fs::write(
        project_dir.join("settings.json"),
        serde_json::to_string_pretty(&existing).unwrap(),
    )
    .unwrap();

    let handler = ConfigHandler;
    let mut ctx = test_ctx_with_cwd(nested.clone());
    handler
        .execute("set theme light --project", &mut ctx)
        .await
        .unwrap();

    let written: RawSettings =
        serde_json::from_str(&std::fs::read_to_string(project_dir.join("settings.json")).unwrap())
            .unwrap();
    assert_eq!(written.model.as_deref(), Some("claude-opus"));
    assert_eq!(written.verbose, Some(true));
    assert_eq!(written.theme.as_deref(), Some("light"));
    assert!(!nested.join(".allthecodes").join("settings.json").exists());
}

#[tokio::test]
async fn test_config_unknown_subcommand() {
    let handler = ConfigHandler;
    let mut ctx = test_ctx();
    let result = handler.execute("delete", &mut ctx).await.unwrap();
    match result {
        CommandResult::Output(text) => {
            assert!(text.contains("Unknown config subcommand"));
        }
        _ => panic!("Expected Output result"),
    }
}

#[tokio::test]
async fn test_config_schema_emits_object() {
    let handler = ConfigHandler;
    let mut ctx = test_ctx();
    let result = handler.execute("schema", &mut ctx).await.unwrap();
    match result {
        CommandResult::Output(text) => {
            assert!(text.contains("\"properties\""));
            assert!(text.contains("\"permissions\""));
        }
        _ => panic!("Expected Output result"),
    }
}

#[tokio::test]
async fn test_config_sources_empty_default() {
    let handler = ConfigHandler;
    let mut ctx = test_ctx();
    let result = handler.execute("sources", &mut ctx).await.unwrap();
    match result {
        CommandResult::Output(text) => {
            assert!(text.contains("no settings overrides") || text.contains("Per-key sources"));
        }
        _ => panic!("Expected Output result"),
    }
}

/// Process-env guard for tests that mutate ALLTHECODES_HOME.
struct EnvGuard {
    key: &'static str,
    previous: Option<String>,
    _lock: MutexGuard<'static, ()>,
}

impl EnvGuard {
    fn set(key: &'static str, value: &str) -> Self {
        static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        let lock = ENV_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .expect("env test lock poisoned");
        let previous = std::env::var(key).ok();
        std::env::set_var(key, value);
        Self {
            key,
            previous,
            _lock: lock,
        }
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
