use super::*;
use allthecodes_bootstrap::SessionId;
use allthecodes_engine::types::app_state::AppState;
use std::path::PathBuf;
fn test_ctx(cwd: PathBuf) -> CommandContext {
    CommandContext {
        messages: Vec::new(),
        cwd,
        app_state: AppState::default(),
        session_id: SessionId::from_string("test-session"),
    }
}

#[tokio::test]
async fn test_memory_show_nonexistent_dir_returns_output() {
    let handler = MemoryHandler;
    let mut ctx = test_ctx(PathBuf::from("/nonexistent/fake/path"));
    let result = handler.execute("show", &mut ctx).await.unwrap();
    match result {
        CommandResult::Output(text) => {
            assert!(!text.is_empty());
        }
        _ => panic!("Expected Output"),
    }
}

/// Default entry point (no args) is the selector, not `show`.
/// This is the core UX change for issue #45.
#[tokio::test]
async fn test_memory_empty_args_is_selector() {
    let handler = MemoryHandler;
    let mut ctx = test_ctx(PathBuf::from("/nonexistent/fake/path"));
    let result = handler.execute("", &mut ctx).await.unwrap();
    match result {
        CommandResult::Output(text) => {
            assert!(
                text.contains("Memory selector"),
                "expected selector header, got: {}",
                text
            );
            assert!(
                text.contains("Auto-memory:"),
                "expected auto-memory header, got: {}",
                text
            );
            assert!(
                text.contains("Directory shortcuts"),
                "expected directory shortcuts block, got: {}",
                text
            );
        }
        _ => panic!("Expected Output"),
    }
}

#[tokio::test]
async fn test_memory_unknown_subcommand_shows_usage() {
    let handler = MemoryHandler;
    let mut ctx = test_ctx(PathBuf::from("/nonexistent/fake/path"));
    let result = handler.execute("unknown-subcmd", &mut ctx).await.unwrap();
    match result {
        CommandResult::Output(text) => {
            assert!(text.contains("Usage"), "expected usage info, got: {}", text);
        }
        _ => panic!("Expected Output"),
    }
}

#[tokio::test]
async fn test_memory_set_get_rm_roundtrip() {
    let tmp = std::env::temp_dir().join(format!("cc_rust_mem_cmd_test_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp).unwrap();

    let handler = MemoryHandler;
    let mut ctx = test_ctx(tmp.clone());

    // set
    let result = handler
        .execute("set my-key hello world", &mut ctx)
        .await
        .unwrap();
    match &result {
        CommandResult::Output(text) => assert!(text.contains("Saved")),
        _ => panic!("Expected Output"),
    }

    // get
    let result = handler.execute("get my-key", &mut ctx).await.unwrap();
    match &result {
        CommandResult::Output(text) => {
            assert!(text.contains("my-key"));
            assert!(text.contains("hello world"));
        }
        _ => panic!("Expected Output"),
    }

    // list
    let result = handler.execute("list", &mut ctx).await.unwrap();
    match &result {
        CommandResult::Output(text) => assert!(text.contains("my-key")),
        _ => panic!("Expected Output"),
    }

    // search
    let result = handler.execute("search hello", &mut ctx).await.unwrap();
    match &result {
        CommandResult::Output(text) => assert!(text.contains("my-key")),
        _ => panic!("Expected Output"),
    }

    // rm
    let result = handler.execute("rm my-key", &mut ctx).await.unwrap();
    match &result {
        CommandResult::Output(text) => assert!(text.contains("Deleted")),
        _ => panic!("Expected Output"),
    }

    // get again (should be not found)
    let result = handler.execute("get my-key", &mut ctx).await.unwrap();
    match &result {
        CommandResult::Output(text) => assert!(text.contains("not found")),
        _ => panic!("Expected Output"),
    }

    let _ = std::fs::remove_dir_all(&tmp);
}

#[tokio::test]
async fn test_memory_set_with_category() {
    let tmp = std::env::temp_dir().join(format!("cc_rust_mem_cat_test_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp).unwrap();

    let handler = MemoryHandler;
    let mut ctx = test_ctx(tmp.clone());

    let result = handler
        .execute("set pref dark-mode --category=ui", &mut ctx)
        .await
        .unwrap();
    match &result {
        CommandResult::Output(text) => assert!(text.contains("Saved")),
        _ => panic!("Expected Output"),
    }

    let result = handler.execute("get pref", &mut ctx).await.unwrap();
    match &result {
        CommandResult::Output(text) => {
            assert!(text.contains("ui"));
            assert!(text.contains("dark-mode"));
        }
        _ => panic!("Expected Output"),
    }

    let _ = std::fs::remove_dir_all(&tmp);
}

/// Auto-toggle updates the in-memory setting. We pin `ALLTHECODES_HOME`
/// to a tempdir so the persistence side-effect lands there instead of
/// the real `~/.allthecodes/settings.json`.
#[tokio::test]
async fn test_memory_auto_toggle_updates_state() {
    let root = std::env::temp_dir().join(format!("cc_rust_mem_auto_test_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();

    let handler = MemoryHandler;
    let mut ctx = test_ctx(root.clone());
    assert_eq!(ctx.app_state.settings.auto_memory_enabled, None);

    // auto on
    let result = handler.execute("auto on", &mut ctx).await.unwrap();
    match &result {
        CommandResult::Output(text) => assert!(text.contains("ON")),
        _ => panic!("Expected Output"),
    }
    assert_eq!(ctx.app_state.settings.auto_memory_enabled, Some(true));

    // status
    let result = handler.execute("auto status", &mut ctx).await.unwrap();
    match &result {
        CommandResult::Output(text) => assert!(text.contains("ON")),
        _ => panic!("Expected Output"),
    }

    // auto off
    let result = handler.execute("auto off", &mut ctx).await.unwrap();
    match &result {
        CommandResult::Output(text) => assert!(text.contains("OFF")),
        _ => panic!("Expected Output"),
    }
    assert_eq!(ctx.app_state.settings.auto_memory_enabled, Some(false));

    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test]
async fn test_memory_auto_invalid_existing_settings_aborts_without_rewrite() {
    let root = std::env::temp_dir().join(format!("cc_rust_mem_auto_bad_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let settings_path = root.join("settings.json");
    let original = "{ invalid json\n";
    std::fs::write(&settings_path, original).unwrap();

    let handler = MemoryHandler;
    let mut ctx = test_ctx(root.clone());
    assert_eq!(ctx.app_state.settings.auto_memory_enabled, None);

    let err = match handler.execute("auto on", &mut ctx).await {
        Ok(_) => panic!("expected invalid settings to abort"),
        Err(err) => err,
    };
    assert!(
        format!("{err:#}").contains("Failed to load existing settings"),
        "unexpected error: {err:#}"
    );
    assert_eq!(ctx.app_state.settings.auto_memory_enabled, None);
    assert_eq!(std::fs::read_to_string(&settings_path).unwrap(), original);

    let _ = std::fs::remove_dir_all(&root);
}

/// `/memory open` prints the path for each valid scope and rejects
/// unknown scopes.
#[tokio::test]
async fn test_memory_open_scope_paths() {
    let tmp = std::env::temp_dir().join(format!("cc_rust_mem_open_test_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp).unwrap();

    let handler = MemoryHandler;
    let mut ctx = test_ctx(tmp.clone());

    for scope in ["auto", "team", "global", "project"] {
        let result = handler
            .execute(&format!("open {}", scope), &mut ctx)
            .await
            .unwrap();
        match &result {
            CommandResult::Output(text) => {
                assert!(
                    text.contains("directory:"),
                    "expected directory line for {}, got: {}",
                    scope,
                    text
                );
            }
            _ => panic!("Expected Output"),
        }
    }

    let result = handler.execute("open bogus", &mut ctx).await.unwrap();
    match &result {
        CommandResult::Output(text) => assert!(text.contains("Unknown scope")),
        _ => panic!("Expected Output"),
    }

    let _ = std::fs::remove_dir_all(&tmp);
}

#[tokio::test]
async fn test_memory_selector_reflects_auto_state() {
    let tmp = std::env::temp_dir().join(format!("cc_rust_mem_sel_test_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp).unwrap();

    let handler = MemoryHandler;
    let mut ctx = test_ctx(tmp.clone());

    // Default (None -> OFF)
    let result = handler.execute("", &mut ctx).await.unwrap();
    match &result {
        CommandResult::Output(text) => assert!(text.contains("Auto-memory: OFF")),
        _ => panic!("Expected Output"),
    }

    // Flip directly in app_state to sidestep persistence.
    ctx.app_state.settings.auto_memory_enabled = Some(true);
    let result = handler.execute("", &mut ctx).await.unwrap();
    match &result {
        CommandResult::Output(text) => assert!(text.contains("Auto-memory: ON")),
        _ => panic!("Expected Output"),
    }

    let _ = std::fs::remove_dir_all(&tmp);
}
