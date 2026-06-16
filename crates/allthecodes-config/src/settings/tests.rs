use std::collections::HashMap;
use std::path::Path;

use super::load::apply_active_auth_profile;
use super::raw::{merge_permissions, merge_str_lists};
use super::*;
use serde_json::{json, Value};
use serial_test::serial;

struct EnvGuard {
    key: &'static str,
    previous: Option<String>,
}

impl EnvGuard {
    fn set_path(key: &'static str, value: &Path) -> Self {
        let previous = std::env::var(key).ok();
        std::env::set_var(key, value);
        Self { key, previous }
    }

    fn set_value(key: &'static str, value: &str) -> Self {
        let previous = std::env::var(key).ok();
        std::env::set_var(key, value);
        Self { key, previous }
    }

    fn unset(key: &'static str) -> Self {
        let previous = std::env::var(key).ok();
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
fn project_overrides_global() {
    let global = RawSettings {
        model: Some("claude-sonnet".into()),
        backend: Some("native".into()),
        theme: Some("dark".into()),
        verbose: Some(false),
        ..Default::default()
    };
    let project = RawSettings {
        model: Some("claude-opus".into()),
        backend: Some("codex".into()),
        verbose: Some(true),
        claude_in_chrome_default_enabled: Some(true),
        ..Default::default()
    };

    let merged = merge_configs(&global, &project);
    assert_eq!(merged.model.as_deref(), Some("claude-opus"));
    assert_eq!(merged.backend.as_deref(), Some("codex"));
    assert_eq!(merged.theme.as_deref(), Some("dark"));
    assert!(merged.verbose);
    assert_eq!(merged.claude_in_chrome_default_enabled, Some(true));
}

#[test]
fn allowed_tools_dedup_merges() {
    let base: Vec<String> = vec!["Bash".into(), "FileRead".into()];
    let over: Vec<String> = vec!["FileRead".into(), "Grep".into()];
    let tools = merge_str_lists(Some(&base), Some(&over));
    assert_eq!(tools, vec!["Bash", "FileRead", "Grep"]);
}

#[test]
fn empty_configs_produce_defaults() {
    let merged = merge_configs(&RawSettings::default(), &RawSettings::default());
    assert!(merged.model.is_none());
    assert!(merged.backend.is_none());
    assert!(!merged.verbose);
    assert!(merged.allowed_tools.is_empty());
}

#[test]
fn anthropic_provider_uses_claude_code_auth_profile_name() {
    assert_eq!(
        auth_profile_name_for_provider(API_PROVIDER_ANTHROPIC),
        AUTH_PROFILE_CLAUDE_CODE
    );
    assert_eq!(
        auth_profile_lookup_names_for_provider(API_PROVIDER_ANTHROPIC),
        &[AUTH_PROFILE_CLAUDE_CODE, AUTH_PROFILE_ANTHROPIC_LEGACY]
    );
    assert_eq!(auth_profile_name_for_provider("openai-codex"), "codex");
    assert_eq!(auth_profile_name_for_provider("unknown"), "custom");
    assert_eq!(
        display_auth_profile_name(AUTH_PROFILE_ANTHROPIC_LEGACY),
        AUTH_PROFILE_CLAUDE_CODE
    );
}

#[test]
fn permissions_legacy_fallback() {
    let raw = RawSettings {
        permission_mode: Some("auto".into()),
        allowed_tools: Some(vec!["Bash".into()]),
        ..Default::default()
    };
    let eff = EffectiveSettings::from_raw(raw);
    assert_eq!(eff.permissions.default_mode.as_deref(), Some("auto"));
    assert_eq!(eff.permissions.allow, vec!["Bash".to_string()]);
    assert_eq!(eff.permission_mode.as_deref(), Some("auto"));
}

#[test]
fn permissions_nested_overrides_legacy() {
    let raw = RawSettings {
        permission_mode: Some("auto".into()),
        permissions: Some(PermissionsSettings {
            default_mode: Some("bypass".into()),
            allow: vec!["Grep".into()],
            ..Default::default()
        }),
        ..Default::default()
    };
    let eff = EffectiveSettings::from_raw(raw);
    assert_eq!(eff.permissions.default_mode.as_deref(), Some("bypass"));
    assert!(eff.permissions.allow.contains(&"Grep".to_string()));
}

#[test]
#[serial]
fn managed_permission_fields_override_user_project_local_and_env() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let project = temp.path().join("repo");
    std::fs::create_dir_all(home.join("plugins")).unwrap();
    std::fs::create_dir_all(project.join(".allthecodes")).unwrap();
    let managed_path = temp.path().join("managed.json");
    std::fs::write(
        &managed_path,
        r#"{
                "permissions": {
                    "defaultMode": "ask",
                    "enableAutoMode": false,
                    "enableBypassMode": false
                }
            }"#,
    )
    .unwrap();
    std::fs::write(
        home.join("settings.json"),
        r#"{"permissions":{"defaultMode":"bypass","enableAutoMode":true,"enableBypassMode":true}}"#,
    )
    .unwrap();
    std::fs::write(
        project.join(".allthecodes/settings.json"),
        r#"{"permissions":{"defaultMode":"auto","enableAutoMode":true,"enableBypassMode":true}}"#,
    )
    .unwrap();
    std::fs::write(
        project.join(".allthecodes/settings.local.json"),
        r#"{"permissions":{"defaultMode":"bypass","enableAutoMode":true,"enableBypassMode":true}}"#,
    )
    .unwrap();

    let _managed = EnvGuard::set_path("ALLTHECODES_MANAGED_SETTINGS", &managed_path);
    let _home = EnvGuard::set_path("ALLTHECODES_HOME", &home);
    let _env = EnvGuard::set_value("CLAUDE_PERMISSION_MODE", "bypass");

    let loaded = load_effective(&project).unwrap();
    assert_eq!(loaded.effective.permission_mode.as_deref(), Some("ask"));
    assert_eq!(
        loaded.effective.permissions.default_mode.as_deref(),
        Some("ask")
    );
    assert_eq!(loaded.effective.permissions.enable_auto_mode, Some(false));
    assert_eq!(loaded.effective.permissions.enable_bypass_mode, Some(false));
    assert_eq!(
        loaded.source_of("permissions.enableAutoMode"),
        SettingsSource::Managed
    );
}

#[test]
#[serial]
fn managed_only_sandbox_lists_ignore_lower_sources() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let project = temp.path().join("repo");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(project.join(".allthecodes")).unwrap();
    let managed_path = temp.path().join("managed.json");
    std::fs::write(
        &managed_path,
        r#"{
                "sandbox": {
                    "allowManagedReadPathsOnly": true,
                    "allowManagedDomainsOnly": true,
                    "filesystem": {"allowRead": ["/managed/read"]},
                    "network": {"allowedDomains": ["managed.example.com"]}
                }
            }"#,
    )
    .unwrap();
    std::fs::write(
        home.join("settings.json"),
        r#"{
                "sandbox": {
                    "filesystem": {"allowRead": ["/user/read"]},
                    "network": {"allowedDomains": ["user.example.com"]}
                }
            }"#,
    )
    .unwrap();
    std::fs::write(
        project.join(".allthecodes/settings.local.json"),
        r#"{
                "sandbox": {
                    "filesystem": {"allowRead": ["/local/read"]},
                    "network": {"allowedDomains": ["local.example.com"]}
                }
            }"#,
    )
    .unwrap();

    let _managed = EnvGuard::set_path("ALLTHECODES_MANAGED_SETTINGS", &managed_path);
    let _home = EnvGuard::set_path("ALLTHECODES_HOME", &home);
    let _perm = EnvGuard::unset("CLAUDE_PERMISSION_MODE");

    let loaded = load_effective(&project).unwrap();
    assert_eq!(
        loaded.effective.sandbox.filesystem.allow_read,
        vec!["/managed/read".to_string()]
    );
    assert_eq!(
        loaded.effective.sandbox.network.allowed_domains,
        vec!["managed.example.com".to_string()]
    );
    assert_eq!(
        loaded.effective.sandbox.allow_managed_read_paths_only,
        Some(true)
    );
    assert_eq!(
        loaded.effective.sandbox.allow_managed_domains_only,
        Some(true)
    );
}

#[test]
fn permissions_auto_mode_parses_and_preserves_extra() {
    let raw: RawSettings = serde_json::from_str(
        r#"{
                "permissions": {
                    "autoMode": {
                        "environment": ["Trusted repo: github.example.com/acme"],
                        "allow": ["Read project docs"],
                        "softDeny": ["Avoid package manager install commands"],
                        "futurePolicy": {"owner": "security"}
                    }
                }
            }"#,
    )
    .unwrap();

    let auto = raw
        .permissions
        .as_ref()
        .and_then(|p| p.auto_mode.as_ref())
        .expect("auto mode settings parsed");
    assert_eq!(
        auto.environment,
        vec!["Trusted repo: github.example.com/acme".to_string()]
    );
    assert_eq!(auto.allow, vec!["Read project docs".to_string()]);
    assert_eq!(
        auto.soft_deny,
        vec!["Avoid package manager install commands".to_string()]
    );
    assert!(auto.extra.contains_key("futurePolicy"));
}

#[test]
fn permissions_auto_mode_merges_lists_and_extra() {
    let base = PermissionsSettings {
        auto_mode: Some(AutoModeSettings {
            environment: vec!["Trusted repo".into()],
            allow: vec!["Run cargo test".into()],
            soft_deny: vec!["Network writes".into()],
            extra: HashMap::from([("baseOnly".to_string(), json!(true))]),
        }),
        ..Default::default()
    };
    let over = PermissionsSettings {
        auto_mode: Some(AutoModeSettings {
            environment: vec!["Trusted repo".into(), "CI machine".into()],
            allow: vec!["Run cargo test".into(), "Read docs".into()],
            soft_deny: vec!["Privilege escalation".into()],
            extra: HashMap::from([("overrideOnly".to_string(), json!("x"))]),
        }),
        ..Default::default()
    };

    let merged = merge_permissions(Some(base), over);
    let auto = merged.auto_mode.expect("auto mode merged");
    assert_eq!(auto.environment, vec!["Trusted repo", "CI machine"]);
    assert_eq!(auto.allow, vec!["Run cargo test", "Read docs"]);
    assert_eq!(
        auto.soft_deny,
        vec!["Network writes", "Privilege escalation"]
    );
    assert_eq!(auto.extra.get("baseOnly"), Some(&json!(true)));
    assert_eq!(auto.extra.get("overrideOnly"), Some(&json!("x")));
}

#[test]
fn skip_bypass_prompt_uses_trusted_source_or_semantics() {
    let base = PermissionsSettings {
        skip_dangerous_mode_permission_prompt: Some(true),
        ..Default::default()
    };
    let over = PermissionsSettings {
        skip_dangerous_mode_permission_prompt: Some(false),
        ..Default::default()
    };
    let merged = merge_permissions(Some(base), over);
    assert_eq!(merged.skip_dangerous_mode_permission_prompt, Some(true));

    let base = PermissionsSettings {
        skip_dangerous_mode_permission_prompt: Some(false),
        ..Default::default()
    };
    let over = PermissionsSettings {
        skip_dangerous_mode_permission_prompt: Some(false),
        ..Default::default()
    };
    let merged = merge_permissions(Some(base), over);
    assert_eq!(merged.skip_dangerous_mode_permission_prompt, Some(false));
}

#[test]
fn project_settings_cannot_skip_bypass_prompt() {
    let mut acc = RawSettings::default();
    let mut sources = SourceMap::new();

    acc.merge_from(
        RawSettings {
            permissions: Some(PermissionsSettings {
                skip_dangerous_mode_permission_prompt: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        },
        SettingsSource::Project,
        &mut sources,
    );
    assert!(acc.permissions.is_none());

    acc.merge_from(
        RawSettings {
            permissions: Some(PermissionsSettings {
                skip_dangerous_mode_permission_prompt: Some(false),
                ..Default::default()
            }),
            ..Default::default()
        },
        SettingsSource::User,
        &mut sources,
    );
    assert_eq!(
        acc.permissions
            .as_ref()
            .and_then(|p| p.skip_dangerous_mode_permission_prompt),
        Some(false)
    );

    acc.merge_from(
        RawSettings {
            permissions: Some(PermissionsSettings {
                skip_dangerous_mode_permission_prompt: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        },
        SettingsSource::Local,
        &mut sources,
    );
    assert_eq!(
        acc.permissions
            .as_ref()
            .and_then(|p| p.skip_dangerous_mode_permission_prompt),
        Some(true)
    );

    acc.merge_from(
        RawSettings {
            permissions: Some(PermissionsSettings {
                skip_dangerous_mode_permission_prompt: Some(false),
                default_mode: Some("bypass".into()),
                ..Default::default()
            }),
            ..Default::default()
        },
        SettingsSource::Project,
        &mut sources,
    );
    let permissions = acc.permissions.expect("trusted skip setting remains");
    assert_eq!(permissions.default_mode.as_deref(), Some("bypass"));
    assert_eq!(
        permissions.skip_dangerous_mode_permission_prompt,
        Some(true)
    );
}

#[test]
fn source_map_tracks_layer() {
    let user = RawSettings {
        model: Some("sonnet".into()),
        ..Default::default()
    };
    let project = RawSettings {
        model: Some("opus".into()),
        theme: Some("dark".into()),
        ..Default::default()
    };
    let mut acc = RawSettings::default();
    let mut sources = SourceMap::new();
    acc.merge_from(user, SettingsSource::User, &mut sources);
    acc.merge_from(project, SettingsSource::Project, &mut sources);
    assert_eq!(sources.get("model"), Some(&SettingsSource::Project));
    assert_eq!(sources.get("theme"), Some(&SettingsSource::Project));
}

#[test]
fn source_rank_matches_merge_precedence() {
    assert!(SettingsSource::Managed.rank() > SettingsSource::Default.rank());
    assert!(SettingsSource::Project.rank() > SettingsSource::User.rank());
    assert!(SettingsSource::Local.rank() > SettingsSource::Project.rank());
    assert!(SettingsSource::Env.rank() > SettingsSource::Local.rank());
    assert!(SettingsSource::Cli.rank() > SettingsSource::Env.rank());
}

#[test]
#[serial]
fn loaders_sources_prompt_and_extra_use_allthecodes_paths() {
    let temp = tempfile::tempdir().expect("tempdir");
    let data_root = temp.path().join("home");
    let managed_path = temp.path().join("managed-settings.json");
    let project_root = temp.path().join("project");
    let nested = project_root.join("nested");
    let project_config_dir = project_root.join(".allthecodes");
    std::fs::create_dir_all(&data_root).unwrap();
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::create_dir_all(&project_config_dir).unwrap();

    let _allthecodes_home = EnvGuard::set_path("ALLTHECODES_HOME", &data_root);
    let _managed = EnvGuard::set_path("ALLTHECODES_MANAGED_SETTINGS", &managed_path);
    let _claude_model = EnvGuard::unset("CLAUDE_MODEL");
    let _cc_backend = EnvGuard::unset("CC_BACKEND");
    let _claude_backend = EnvGuard::unset("CLAUDE_BACKEND");
    let _api_key = EnvGuard::unset("ANTHROPIC_API_KEY");
    let _verbose = EnvGuard::unset("CLAUDE_VERBOSE");
    let _permission_mode = EnvGuard::unset("CLAUDE_PERMISSION_MODE");
    let _language = EnvGuard::unset("CLAUDE_LANGUAGE");
    let _output_style = EnvGuard::unset("CLAUDE_OUTPUT_STYLE");
    let _theme = EnvGuard::unset("CLAUDE_THEME");

    write_settings_file(
        &managed_path,
        &RawSettings {
            backend: Some("native".into()),
            ..Default::default()
        },
    )
    .unwrap();
    write_settings_file(
        &user_settings_path(),
        &RawSettings {
            model: Some("user-model".into()),
            system_prompt: Some("user prompt".into()),
            extra: HashMap::from([("unknownUser".to_string(), json!(true))]),
            ..Default::default()
        },
    )
    .unwrap();
    write_settings_file(
        &project_config_dir.join("settings.json"),
        &RawSettings {
            model: Some("project-model".into()),
            system_prompt: Some("project prompt".into()),
            allowed_tools: Some(vec!["Bash".into()]),
            ..Default::default()
        },
    )
    .unwrap();
    write_settings_file(
        &project_config_dir.join("settings.local.json"),
        &RawSettings {
            system_prompt: Some("local prompt".into()),
            extra: HashMap::from([("unknownLocal".to_string(), json!({"owner": "test"}))]),
            ..Default::default()
        },
    )
    .unwrap();

    let global = load_global_config().unwrap();
    assert_eq!(global.model.as_deref(), Some("user-model"));
    assert_eq!(global.system_prompt.as_deref(), Some("user prompt"));
    assert_eq!(global.extra.get("unknownUser"), Some(&json!(true)));

    let project = load_project_config(&nested).unwrap();
    assert_eq!(project.model.as_deref(), Some("project-model"));
    assert_eq!(project.allowed_tools, Some(vec!["Bash".to_string()]));

    let local = load_local_config(&nested).unwrap();
    assert_eq!(local.system_prompt.as_deref(), Some("local prompt"));

    let managed = load_managed_config().unwrap();
    assert_eq!(managed.backend.as_deref(), Some("native"));

    let merged = merge_configs(&global, &project);
    assert_eq!(merged.model.as_deref(), Some("project-model"));
    assert_eq!(merged.system_prompt.as_deref(), Some("project prompt"));
    assert_eq!(merged.allowed_tools, vec!["Bash".to_string()]);

    let loaded = load_effective(&nested).unwrap();
    assert_eq!(loaded.source_of("backend"), SettingsSource::Managed);
    assert_eq!(loaded.source_of("model"), SettingsSource::Project);
    assert_eq!(loaded.source_of("systemPrompt"), SettingsSource::Local);
    assert_eq!(loaded.source_of("unknownLocal"), SettingsSource::Local);
    assert_eq!(loaded.source_of("missing"), SettingsSource::Default);
    assert_eq!(loaded.effective.backend.as_deref(), Some("native"));
    assert_eq!(loaded.effective.model.as_deref(), Some("project-model"));
    assert_eq!(
        loaded.effective.system_prompt.as_deref(),
        Some("local prompt")
    );
    assert_eq!(
        loaded.effective.extra.get("unknownLocal"),
        Some(&json!({"owner": "test"}))
    );
    assert_eq!(loaded.loaded_paths.len(), 4);
}

#[test]
#[serial]
fn strict_trust_disables_untrusted_project_and_local_layers() {
    let temp = tempfile::tempdir().expect("tempdir");
    let data_root = temp.path().join("home");
    let project_root = temp.path().join("project");
    let config_dir = project_root.join(".allthecodes");
    std::fs::create_dir_all(&data_root).unwrap();
    std::fs::create_dir_all(&config_dir).unwrap();

    let _home = EnvGuard::set_path("ALLTHECODES_HOME", &data_root);
    let _requirements =
        EnvGuard::set_path("ALLTHECODES_REQUIREMENTS", &temp.path().join("none.toml"));
    std::fs::write(
        data_root.join("settings.json"),
        serde_json::to_vec(&RawSettings {
            model: Some("user-model".to_string()),
            ..Default::default()
        })
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        config_dir.join("settings.json"),
        serde_json::to_vec(&RawSettings {
            model: Some("project-model".to_string()),
            ..Default::default()
        })
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        config_dir.join("settings.local.json"),
        serde_json::to_vec(&RawSettings {
            theme: Some("local-theme".to_string()),
            ..Default::default()
        })
        .unwrap(),
    )
    .unwrap();

    let loaded = load_effective_with_options(
        &project_root,
        LoadOptions {
            trust_policy: ProjectTrustPolicy::TrustConfiguredOnly,
            enforce_requirements: false,
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(loaded.effective.model.as_deref(), Some("user-model"));
    assert!(loaded.effective.theme.is_none());
    assert_eq!(loaded.source_of("model"), SettingsSource::User);
    assert!(loaded
        .layers
        .iter()
        .any(|layer| layer.source == SettingsSource::Project
            && layer.status == ConfigLayerStatus::Disabled));
    assert!(loaded.entries.iter().any(|entry| entry.path == "model"
        && entry.disabled_reason == Some(ConfigLayerDisabledReason::ProjectNotTrusted)));
}

#[test]
#[serial]
fn profile_partial_override_inherits_user_settings() {
    let temp = tempfile::tempdir().expect("tempdir");
    let data_root = temp.path().join("home");
    let project_root = temp.path().join("project");
    std::fs::create_dir_all(&data_root).unwrap();
    std::fs::create_dir_all(&project_root).unwrap();
    let _home = EnvGuard::set_path("ALLTHECODES_HOME", &data_root);
    let _requirements =
        EnvGuard::set_path("ALLTHECODES_REQUIREMENTS", &temp.path().join("none.toml"));

    std::fs::write(
        data_root.join("settings.json"),
        serde_json::to_vec(&RawSettings {
            model: Some("base-model".to_string()),
            theme: Some("base-theme".to_string()),
            config_profiles: Some(HashMap::from([(
                "work".to_string(),
                RawSettings {
                    theme: Some("profile-theme".to_string()),
                    ..Default::default()
                },
            )])),
            ..Default::default()
        })
        .unwrap(),
    )
    .unwrap();

    let loaded = load_effective_with_options(
        &project_root,
        LoadOptions {
            profile: Some("work".to_string()),
            enforce_requirements: false,
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(loaded.effective.model.as_deref(), Some("base-model"));
    assert_eq!(loaded.effective.theme.as_deref(), Some("profile-theme"));
    assert_eq!(loaded.source_of("theme"), SettingsSource::UserProfile);
}

#[test]
fn extra_values_merge_recursively() {
    let mut acc = RawSettings {
        extra: HashMap::from([(
            "nested".to_string(),
            json!({ "one": true, "shared": { "a": 1, "b": 1 }, "array": [1] }),
        )]),
        ..Default::default()
    };
    let mut sources = SourceMap::new();
    acc.merge_from(
        RawSettings {
            extra: HashMap::from([(
                "nested".to_string(),
                json!({ "two": true, "shared": { "b": 2 }, "array": [2] }),
            )]),
            ..Default::default()
        },
        SettingsSource::Project,
        &mut sources,
    );

    assert_eq!(
        acc.extra.get("nested"),
        Some(&json!({
            "one": true,
            "two": true,
            "shared": { "a": 1, "b": 2 },
            "array": [2],
        }))
    );
}

#[test]
#[serial]
fn path_fields_resolve_relative_to_config_file() {
    let temp = tempfile::tempdir().expect("tempdir");
    let data_root = temp.path().join("home");
    let project_root = temp.path().join("project");
    std::fs::create_dir_all(&data_root).unwrap();
    std::fs::create_dir_all(&project_root).unwrap();
    let _home = EnvGuard::set_path("ALLTHECODES_HOME", &data_root);
    let _requirements =
        EnvGuard::set_path("ALLTHECODES_REQUIREMENTS", &temp.path().join("none.toml"));

    std::fs::write(
        data_root.join("settings.json"),
        serde_json::to_vec(&RawSettings {
            cloud_sync_path: Some("sync".to_string()),
            permissions: Some(PermissionsSettings {
                additional_directories: vec!["extra".to_string()],
                ..Default::default()
            }),
            sandbox: Some(SandboxSettings {
                filesystem: SandboxFilesystemSettings {
                    allow_write: vec!["writes".to_string()],
                    ..Default::default()
                },
                ..Default::default()
            }),
            ..Default::default()
        })
        .unwrap(),
    )
    .unwrap();

    let loaded = load_effective_with_options(
        &project_root,
        LoadOptions {
            enforce_requirements: false,
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(
        loaded.effective.cloud_sync_path.as_deref(),
        Some(data_root.join("sync").to_string_lossy().as_ref())
    );
    assert_eq!(
        loaded.effective.permissions.additional_directories,
        vec![data_root.join("extra").to_string_lossy().to_string()]
    );
    assert_eq!(
        loaded.effective.sandbox.filesystem.allow_write,
        vec![data_root.join("writes").to_string_lossy().to_string()]
    );
}

#[test]
#[serial]
fn requirements_report_diagnostics_and_can_enforce() {
    let temp = tempfile::tempdir().expect("tempdir");
    let data_root = temp.path().join("home");
    let project_root = temp.path().join("project");
    let requirements = temp.path().join("requirements.toml");
    std::fs::create_dir_all(&data_root).unwrap();
    std::fs::create_dir_all(&project_root).unwrap();
    let _home = EnvGuard::set_path("ALLTHECODES_HOME", &data_root);
    let _requirements = EnvGuard::set_path("ALLTHECODES_REQUIREMENTS", &requirements);

    std::fs::write(
        &requirements,
        r#"
[requirements.backend]
allowedValues = ["codex"]

[requirements."permissions.defaultMode"]
deniedValues = ["bypass"]
"#,
    )
    .unwrap();
    std::fs::write(
        data_root.join("settings.json"),
        serde_json::to_vec(&RawSettings {
            backend: Some("native".to_string()),
            permissions: Some(PermissionsSettings {
                default_mode: Some("bypass".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        })
        .unwrap(),
    )
    .unwrap();

    let loaded = load_effective_with_options(
        &project_root,
        LoadOptions {
            enforce_requirements: false,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(loaded.requirement_violations.len(), 2);

    let err = load_effective_with_options(
        &project_root,
        LoadOptions {
            enforce_requirements: true,
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("settings requirements violated"));
}

#[test]
#[serial]
fn env_cli_runtime_layers_follow_declared_precedence() {
    let temp = tempfile::tempdir().expect("tempdir");
    let data_root = temp.path().join("home");
    let project_root = temp.path().join("project");
    std::fs::create_dir_all(&data_root).unwrap();
    std::fs::create_dir_all(&project_root).unwrap();
    let _home = EnvGuard::set_path("ALLTHECODES_HOME", &data_root);
    let _requirements =
        EnvGuard::set_path("ALLTHECODES_REQUIREMENTS", &temp.path().join("none.toml"));
    let _env_model = EnvGuard::set_value("CLAUDE_MODEL", "env-model");

    std::fs::write(
        data_root.join("settings.json"),
        serde_json::to_vec(&RawSettings {
            model: Some("user-model".to_string()),
            backend: Some("user-backend".to_string()),
            ..Default::default()
        })
        .unwrap(),
    )
    .unwrap();

    let loaded = load_effective_with_options(
        &project_root,
        LoadOptions {
            cli_overrides: Some(RawSettings {
                model: Some("cli-model".to_string()),
                backend: Some("cli-backend".to_string()),
                ..Default::default()
            }),
            runtime_overrides: Some(RawSettings {
                backend: Some("runtime-backend".to_string()),
                ..Default::default()
            }),
            enforce_requirements: false,
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(loaded.effective.model.as_deref(), Some("cli-model"));
    assert_eq!(loaded.effective.backend.as_deref(), Some("runtime-backend"));
    assert_eq!(loaded.source_of("model"), SettingsSource::Cli);
    assert_eq!(loaded.source_of("backend"), SettingsSource::Runtime);
    assert!(loaded
        .layers
        .iter()
        .any(|layer| layer.source == SettingsSource::Env));
}

#[test]
fn unknown_keys_land_in_extra() {
    let raw: RawSettings = serde_json::from_str(
        r#"{ "model": "opus", "customFlag": true, "anotherNested": {"a":1} }"#,
    )
    .unwrap();
    assert_eq!(raw.model.as_deref(), Some("opus"));
    assert!(raw.extra.contains_key("customFlag"));
    assert!(raw.extra.contains_key("anotherNested"));
}

#[test]
fn env_key_parses_as_typed_map_not_extra() {
    let raw: RawSettings = serde_json::from_str(
        r#"{
                "env": {
                    "ANTHROPIC_MODEL": "deepseek-v4-pro",
                    "CC_BACKEND": "codex"
                }
            }"#,
    )
    .unwrap();

    let env = raw.env.expect("env parsed");
    assert_eq!(
        env.get("ANTHROPIC_MODEL").map(String::as_str),
        Some("deepseek-v4-pro")
    );
    assert_eq!(env.get("CC_BACKEND").map(String::as_str), Some("codex"));
    assert!(!raw.extra.contains_key("env"));
}

#[test]
fn env_values_must_be_strings() {
    let err = serde_json::from_str::<RawSettings>(
        r#"{"env": {"ANTHROPIC_MODEL": ["not", "a", "string"]}}"#,
    )
    .expect_err("non-string env values should fail");
    assert!(err.to_string().contains("invalid type"));
}

#[test]
#[serial]
fn settings_env_layers_merge_in_settings_priority_order() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path().join("home");
    let project = temp.path().join("repo");
    let managed_path = temp.path().join("managed.json");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(project.join(".allthecodes")).unwrap();

    std::fs::write(
        &managed_path,
        r#"{"env":{"FROM_MANAGED":"managed","SHARED":"managed"}}"#,
    )
    .unwrap();
    std::fs::write(
        home.join("settings.json"),
        r#"{"env":{"FROM_USER":"user","SHARED":"user"}}"#,
    )
    .unwrap();
    std::fs::write(
        project.join(".allthecodes/settings.json"),
        r#"{"env":{"FROM_PROJECT":"project","SHARED":"project"}}"#,
    )
    .unwrap();
    std::fs::write(
        project.join(".allthecodes/settings.local.json"),
        r#"{"env":{"FROM_LOCAL":"local","SHARED":"local"}}"#,
    )
    .unwrap();

    let _managed = EnvGuard::set_path("ALLTHECODES_MANAGED_SETTINGS", &managed_path);
    let _home = EnvGuard::set_path("ALLTHECODES_HOME", &home);

    let loaded = load_effective(&project).unwrap();
    assert_eq!(
        loaded.effective.env.get("FROM_MANAGED").map(String::as_str),
        Some("managed")
    );
    assert_eq!(
        loaded.effective.env.get("FROM_USER").map(String::as_str),
        Some("user")
    );
    assert_eq!(
        loaded.effective.env.get("FROM_PROJECT").map(String::as_str),
        Some("project")
    );
    assert_eq!(
        loaded.effective.env.get("FROM_LOCAL").map(String::as_str),
        Some("local")
    );
    assert_eq!(
        loaded.effective.env.get("SHARED").map(String::as_str),
        Some("local")
    );
    assert_eq!(loaded.source_of("env"), SettingsSource::Local);
}

#[test]
#[serial]
fn apply_runtime_env_fills_missing_without_overwriting_process_env() {
    const APPLIED: &str = "ALLTHECODES_TEST_SETTINGS_ENV_APPLIED";
    const SKIPPED: &str = "ALLTHECODES_TEST_SETTINGS_ENV_SKIPPED";
    let _applied = EnvGuard::unset(APPLIED);
    let _skipped = EnvGuard::set_value(SKIPPED, "from-process");
    let env = HashMap::from([
        (APPLIED.to_string(), "from-settings".to_string()),
        (SKIPPED.to_string(), "from-settings".to_string()),
    ]);

    let report = apply_runtime_env(&env).unwrap();

    assert_eq!(report.applied, 1);
    assert_eq!(report.skipped, 1);
    assert_eq!(std::env::var(APPLIED).as_deref(), Ok("from-settings"));
    assert_eq!(std::env::var(SKIPPED).as_deref(), Ok("from-process"));
}

#[test]
#[serial]
fn apply_startup_runtime_env_overrides_provider_env_only() {
    const GENERIC: &str = "ALLTHECODES_TEST_SETTINGS_ENV_GENERIC";
    let _model = EnvGuard::set_value("ANTHROPIC_MODEL", "claude-opus-4-20250514");
    let _base_url = EnvGuard::set_value("ANTHROPIC_BASE_URL", "https://old.example.com");
    let _generic = EnvGuard::set_value(GENERIC, "from-process");
    let env = HashMap::from([
        ("ANTHROPIC_MODEL".to_string(), "deepseek-v4-pro".to_string()),
        (
            "ANTHROPIC_BASE_URL".to_string(),
            "https://inferaichat.com".to_string(),
        ),
        (GENERIC.to_string(), "from-settings".to_string()),
    ]);

    let report = apply_startup_runtime_env(&env).unwrap();

    assert_eq!(report.applied, 0);
    assert_eq!(report.overridden, 2);
    assert_eq!(report.skipped, 1);
    assert_eq!(
        std::env::var("ANTHROPIC_MODEL").as_deref(),
        Ok("deepseek-v4-pro")
    );
    assert_eq!(
        std::env::var("ANTHROPIC_BASE_URL").as_deref(),
        Ok("https://inferaichat.com")
    );
    assert_eq!(std::env::var(GENERIC).as_deref(), Ok("from-process"));
}

#[test]
fn active_auth_profile_projects_runtime_fields_and_env() {
    let raw: RawSettings = serde_json::from_str(
        r#"{
                "model": "legacy-model",
                "backend": "native",
                "apiProvider": "anthropic",
                "activeAuthProfile": "claude_code",
                "authProfiles": {
                    "claude_code": {
                        "backend": "native",
                        "apiProvider": "anthropic",
                        "model": "deepseek-v4-pro",
                        "baseUrl": "https://inferaichat.com/",
                        "env": {
                            "ANTHROPIC_AUTH_TOKEN": "custom-token",
                            "ANTHROPIC_DEFAULT_SONNET_MODEL": "deepseek-v4-pro"
                        }
                    }
                }
            }"#,
    )
    .unwrap();
    let mut sources = SourceMap::new();
    let mut effective = EffectiveSettings::from_raw(raw);
    sources.insert("authProfiles".to_string(), SettingsSource::User);

    apply_active_auth_profile(&mut effective, &mut sources);

    assert_eq!(effective.model.as_deref(), Some("deepseek-v4-pro"));
    assert_eq!(effective.backend.as_deref(), Some("native"));
    assert_eq!(
        effective.env.get("ANTHROPIC_BASE_URL").map(String::as_str),
        Some("https://inferaichat.com")
    );
    assert_eq!(
        effective.env.get("ANTHROPIC_MODEL").map(String::as_str),
        Some("deepseek-v4-pro")
    );
    assert_eq!(
        effective
            .env
            .get("ANTHROPIC_AUTH_TOKEN")
            .map(String::as_str),
        Some("custom-token")
    );
    assert_eq!(sources.get("model"), Some(&SettingsSource::User));
}

#[test]
fn legacy_anthropic_auth_profile_still_projects_runtime_fields() {
    let raw: RawSettings = serde_json::from_str(
        r#"{
                "activeAuthProfile": "anthropic",
                "authProfiles": {
                    "anthropic": {
                        "backend": "native",
                        "apiProvider": "anthropic",
                        "model": "deepseek-v4-pro",
                        "baseUrl": "https://inferaichat.com"
                    }
                }
            }"#,
    )
    .unwrap();
    let mut sources = SourceMap::new();
    let mut effective = EffectiveSettings::from_raw(raw);
    sources.insert("authProfiles".to_string(), SettingsSource::User);

    apply_active_auth_profile(&mut effective, &mut sources);

    assert_eq!(effective.model.as_deref(), Some("deepseek-v4-pro"));
    assert_eq!(effective.api_provider.as_deref(), Some("anthropic"));
    assert_eq!(
        effective.env.get("ANTHROPIC_BASE_URL").map(String::as_str),
        Some("https://inferaichat.com")
    );
}

#[test]
fn active_codex_profile_projects_codex_env() {
    let raw: RawSettings = serde_json::from_str(
        r#"{
                "activeAuthProfile": "codex",
                "authProfiles": {
                    "codex": {
                        "backend": "codex",
                        "apiProvider": "openai-codex",
                        "model": "gpt-5.4",
                        "baseUrl": "https://example.com/codex",
                        "availableModels": ["gpt-5.4", "gpt-5.5"],
                        "modelReasoningEffort": "high",
                        "modelCapabilities": {
                            "gpt-5.4": {
                                "displayName": "gpt-5.4",
                                "defaultReasoningLevel": "medium",
                                "supportedReasoningLevels": ["low", "medium", "high", "xhigh"],
                                "contextWindow": 272000,
                                "supportsFastMode": true,
                                "supportsSearchTool": true
                            }
                        }
                    }
                }
            }"#,
    )
    .unwrap();
    let mut sources = SourceMap::new();
    let mut effective = EffectiveSettings::from_raw(raw);
    sources.insert("authProfiles".to_string(), SettingsSource::User);

    apply_active_auth_profile(&mut effective, &mut sources);

    assert_eq!(effective.backend.as_deref(), Some("codex"));
    assert_eq!(effective.api_provider.as_deref(), Some("openai-codex"));
    assert_eq!(effective.model.as_deref(), Some("gpt-5.4"));
    assert_eq!(
        effective.env.get("OPENAI_CODEX_MODEL").map(String::as_str),
        Some("gpt-5.4")
    );
    assert_eq!(
        effective
            .env
            .get("OPENAI_CODEX_BASE_URL")
            .map(String::as_str),
        Some("https://example.com/codex")
    );
    assert_eq!(
        effective.available_models,
        vec!["gpt-5.4".to_string(), "gpt-5.5".to_string()]
    );
    assert_eq!(effective.model_reasoning_effort.as_deref(), Some("high"));
    let capability = effective
        .model_capabilities
        .get("gpt-5.4")
        .expect("profile model capability projected");
    assert_eq!(capability.context_window, Some(272_000));
    assert!(capability.supports_fast_mode);
    assert!(capability.supports_search_tool);
}

#[test]
fn raw_settings_preserve_thinking_and_output_config_effort() {
    let raw: RawSettings = serde_json::from_str(
        r#"{
            "thinking": { "type": "enabled" },
            "output_config": { "effort": "max" }
        }"#,
    )
    .unwrap();

    let effective = EffectiveSettings::from_raw(raw);
    assert_eq!(
        effective
            .thinking
            .as_ref()
            .and_then(|value| value.get("type"))
            .and_then(serde_json::Value::as_str),
        Some("enabled")
    );
    assert_eq!(
        effective
            .output_config
            .as_ref()
            .and_then(|value| value.get("effort"))
            .and_then(serde_json::Value::as_str),
        Some("max")
    );
}

#[test]
fn schema_has_known_keys() {
    let s = settings_schema();
    let props = s
        .pointer("/properties")
        .and_then(|v| v.as_object())
        .expect("schema has /properties");
    for key in [
        "model",
        "backend",
        "activeAuthProfile",
        "authProfiles",
        "permissions",
        "sandbox",
        "statusLine",
        "outputStyle",
        "spinnerTips",
        "availableModels",
        "defaultModel",
        "fallbackModel",
        "fastModel",
        "sotaModel",
        "motaModel",
        "fotaModel",
        "thinking",
        "output_config",
        "model_reasoning_effort",
        "fastMode",
        "env",
    ] {
        assert!(props.contains_key(key), "missing schema key: {}", key);
    }
}

/// The committed schema file is the canonical doc. This test makes
/// sure it never drifts from the runtime [`settings_schema`] output.
/// To regenerate the file, run:
/// `cargo test schema_file_matches_runtime -- --ignored` (then update).
#[test]
fn schema_file_matches_runtime() {
    // docs/ lives at the workspace root, two levels above the crate.
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("docs")
        .join("schemas")
        .join("settings.schema.json");

    let on_disk: Value = serde_json::from_str(
        &std::fs::read_to_string(&path).expect("docs/schemas/settings.schema.json missing"),
    )
    .expect("schema file is not valid JSON");

    let runtime = settings_schema();

    // Compare the `properties` object specifically — the human-curated
    // file may carry additional doc-only metadata but its property
    // shape must match. (We only assert the property keys are equal.)
    let on_disk_keys: std::collections::BTreeSet<_> = on_disk
        .pointer("/properties")
        .and_then(|v| v.as_object())
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();
    let runtime_keys: std::collections::BTreeSet<_> = runtime
        .pointer("/properties")
        .and_then(|v| v.as_object())
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();
    assert_eq!(
        on_disk_keys, runtime_keys,
        "settings.schema.json drift — committed file is missing keys \
             present in settings_schema(), or vice versa. Update \
             docs/schemas/settings.schema.json to match.",
    );
}

#[test]
fn write_creates_backup_and_prunes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("settings.json");

    let raw1 = RawSettings {
        model: Some("claude-a".into()),
        ..Default::default()
    };
    write_settings_file(&path, &raw1).unwrap();
    assert!(path.exists());

    // Do several more writes separated by one second to get unique
    // backup timestamps (format is YYYYMMDD-HHMMSS).
    for i in 0..(MAX_SETTINGS_BACKUPS + 2) {
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let raw = RawSettings {
            model: Some(format!("claude-{}", i)),
            ..Default::default()
        };
        write_settings_file(&path, &raw).unwrap();
    }

    let entries: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            n.starts_with("settings.json.") && n.ends_with(".bak")
        })
        .collect();
    assert!(
        entries.len() <= MAX_SETTINGS_BACKUPS,
        "too many backups: {}",
        entries.len()
    );
}
