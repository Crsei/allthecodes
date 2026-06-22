use super::*;
use crate::handlers::test_support::*;
use crate::state::AccountAuthSession;
use allthecodes_plugins::marketplace::{
    default_marketplace_entries, default_marketplace_source, MarketplacePluginEntry,
    MarketplaceSource, DEFAULT_MARKETPLACE_SOURCE_NAME, GLOBAL_MARKETPLACE_INDEX,
    OFFICIAL_MARKETPLACE_SOURCE_NAME,
};
use allthecodes_plugins::{PluginSource, PluginStatus};
use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde_json::json;
use serial_test::serial;

#[tokio::test]
#[serial]
async fn plugins_local_install_list_marketplace_and_uninstall_round_trip() {
    let (_home, _guard) = temp_home();
    let _marketplace_guard = MarketplaceGlobalGuard::with_official_source(PluginSource::Local {
        path: "/path/that/does/not/exist/marketplace.json".into(),
    });
    allthecodes_plugins::clear_plugins();
    let state = make_web_state();
    let plugin_source = tempfile::tempdir().expect("plugin source");
    std::fs::write(
        plugin_source.path().join("plugin.json"),
        r#"{
            "name": "local-plugin",
            "display_name": "Local Plugin",
            "version": "1.0.0",
            "description": "Local test plugin"
        }"#,
    )
    .expect("plugin manifest");

    let response = plugins_install_handler(
        State(state.clone()),
        Json(
            allthecodes_protocol::v1::plugins::PluginInstallRequest::Legacy(
                allthecodes_protocol::v1::plugins::PluginLegacyInstallRequest {
                    source: plugin_source.path().to_string_lossy().to_string(),
                    scope: Some("user".to_string()),
                },
            ),
        ),
    )
    .await
    .into_response();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["plugin"]["id"], json!("local-plugin@local"));
    assert_eq!(body["fresh_install"], json!(true));

    let response = plugins_list_handler(State(state.clone()))
        .await
        .into_response();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert!(body["plugins"]
        .as_array()
        .expect("plugins")
        .iter()
        .any(|plugin| plugin["id"] == json!("local-plugin@local")));
    assert!(body["diagnostics"]
        .as_array()
        .expect("diagnostics")
        .is_empty());

    let response = plugins_marketplace_handler(State(state.clone()))
        .await
        .into_response();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert!(body["plugins"].is_array());

    let response = plugins_uninstall_handler(
        State(state.clone()),
        AxumPath("local-plugin@local".to_string()),
        Json(PluginUninstallRequest { purge: false }),
    )
    .await
    .into_response();
    assert_eq!(response.status(), StatusCode::OK);

    let response = plugins_list_handler(State(state)).await.into_response();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert!(body["plugins"].as_array().expect("plugins").is_empty());
    allthecodes_plugins::clear_plugins();
}

#[tokio::test]
#[serial]
async fn marketplace_handler_refreshes_empty_official_cache() {
    let (_home, _guard) = temp_home();
    let dir = tempfile::tempdir().expect("marketplace dir");
    let index_path = dir.path().join("marketplace.json");
    std::fs::write(
        &index_path,
        r#"{
            "plugins": [
                {
                    "id": "eco-boost",
                    "name": "Eco Boost",
                    "description": "Eco helper",
                    "version": "0.1.0",
                    "download_url": "https://allthecodes.cc/api/downloads/plugins/eco-boost/v0.1.0/eco-boost-0.1.0.zip"
                }
            ]
        }"#,
    )
    .expect("marketplace fixture");
    let _marketplace_guard = MarketplaceGlobalGuard::with_official_source(PluginSource::Local {
        path: index_path.to_string_lossy().to_string(),
    });
    let state = make_web_state();

    let response = plugins_marketplace_handler(State(state))
        .await
        .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    let plugins = body["plugins"].as_array().expect("plugins");
    assert!(plugins
        .iter()
        .any(|plugin| plugin["id"] == json!("eco-boost")));
    assert!(plugins
        .iter()
        .any(|plugin| plugin["id"] == json!("superpowers")));
}

#[tokio::test]
#[serial]
async fn marketplace_handler_keeps_builtin_entries_when_official_refresh_fails() {
    let (_home, _guard) = temp_home();
    let _marketplace_guard = MarketplaceGlobalGuard::with_official_source(PluginSource::Local {
        path: "/path/that/does/not/exist/marketplace.json".into(),
    });
    let state = make_web_state();

    let response = plugins_marketplace_handler(State(state))
        .await
        .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    let plugins = body["plugins"].as_array().expect("plugins");
    assert!(plugins
        .iter()
        .any(|plugin| plugin["id"] == json!("superpowers")));
}

#[test]
fn install_request_deserializes_legacy_and_official_shapes() {
    let legacy: allthecodes_protocol::v1::plugins::PluginInstallRequest =
        serde_json::from_value(json!({"source": "./plugin", "scope": "user"})).unwrap();
    assert!(matches!(
        legacy,
        allthecodes_protocol::v1::plugins::PluginInstallRequest::Legacy(_)
    ));

    let official: allthecodes_protocol::v1::plugins::PluginInstallRequest = serde_json::from_value(
        json!({
            "id": "eco-boost",
            "version": "0.1.0",
            "download_url": "https://allthecodes.cc/api/downloads/plugins/eco-boost/v0.1.0/eco-boost-0.1.0.zip",
            "sha256": null
        }),
    )
    .unwrap();
    assert!(matches!(
        official,
        allthecodes_protocol::v1::plugins::PluginInstallRequest::Official(_)
    ));
}

#[tokio::test]
#[serial]
async fn lifecycle_handlers_reject_missing_plugin_ids() {
    let state = make_web_state();
    let response = plugins_enable_handler(
        State(state.clone()),
        Json(allthecodes_protocol::v1::plugins::PluginIdRequest { id: "".into() }),
    )
    .await
    .into_response();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let response = plugins_uninstall_by_id_handler(
        State(state),
        Json(
            allthecodes_protocol::v1::plugins::PluginUninstallByIdRequest {
                id: "".into(),
                purge: false,
            },
        ),
    )
    .await
    .into_response();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
#[serial]
async fn install_refreshes_plugin_contributed_skills() {
    let (_home, _guard) = temp_home();
    allthecodes_plugins::clear_plugins();
    allthecodes_skills::clear_skills();
    let state = make_web_state();
    let plugin_source = tempfile::tempdir().expect("plugin source");
    let skill_dir = plugin_source.path().join("skills").join("blunt");
    std::fs::create_dir_all(&skill_dir).expect("skill dir");
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "---\ndescription: Use concise communication.\n---\nBe concise.\n",
    )
    .expect("skill file");
    std::fs::write(
        plugin_source.path().join("plugin.json"),
        r#"{
            "name": "local-skill-plugin",
            "display_name": "Local Skill Plugin",
            "version": "1.0.0",
            "description": "Local test plugin with skills",
            "skills": [
                {
                    "name": "blunt",
                    "path": "skills/blunt/SKILL.md",
                    "description": "Use concise communication."
                }
            ]
        }"#,
    )
    .expect("plugin manifest");

    let response = plugins_install_handler(
        State(state),
        Json(
            allthecodes_protocol::v1::plugins::PluginInstallRequest::Legacy(
                allthecodes_protocol::v1::plugins::PluginLegacyInstallRequest {
                    source: plugin_source.path().to_string_lossy().to_string(),
                    scope: Some("user".to_string()),
                },
            ),
        ),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    let skill = allthecodes_skills::find_skill("blunt").expect("plugin skill loaded");
    assert!(matches!(
        skill.source,
        allthecodes_skills::SkillSource::Plugin(ref id) if id == "local-skill-plugin@local"
    ));

    allthecodes_skills::clear_skills();
    allthecodes_plugins::clear_plugins();
}

#[tokio::test]
#[serial]
async fn web_state_provides_account_token_to_official_plugin_mcp_discovery() {
    let (_home, _guard) = temp_home();
    allthecodes_plugins::clear_plugins();
    let state = make_web_state();
    state.account_auth.lock().session = Some(AccountAuthSession {
        account_site_url: "https://allthecodes.cc".to_string(),
        access_token: "account-token".to_string(),
        expires_at: "2999-01-01T00:00:00Z".to_string(),
        user: json!({ "id": "user-1" }),
        subscription: json!({}),
        entitlements: json!({}),
        credits: json!({}),
        agent_collaboration: json!({}),
    });
    let plugin_root = tempfile::tempdir().expect("plugin root");
    std::fs::write(
        plugin_root.path().join("plugin.json"),
        r#"{
            "name": "eco-boost",
            "display_name": "Eco Boost",
            "version": "1.0.0",
            "description": "Official test plugin",
            "mcp_servers": [
                { "name": "mcp-cli-bridge", "command": "bin/mcp-cli-bridge", "args": [] }
            ]
        }"#,
    )
    .expect("plugin manifest");
    allthecodes_plugins::register_plugin(allthecodes_plugins::PluginEntry {
        id: "eco-boost".to_string(),
        name: "Eco Boost".to_string(),
        version: "1.0.0".to_string(),
        description: "Official test plugin".to_string(),
        source: PluginSource::Local {
            path: plugin_root.path().to_string_lossy().to_string(),
        },
        status: PluginStatus::Installed,
        marketplace: Some(OFFICIAL_MARKETPLACE_SOURCE_NAME.to_string()),
        cache_path: Some(plugin_root.path().to_path_buf()),
        installed_version: Some("1.0.0".to_string()),
        official: true,
        download_url: None,
        homepage: None,
        sha256: None,
        tools: Vec::new(),
        skills: Vec::new(),
        mcp_servers: vec!["mcp-cli-bridge".to_string()],
        installed_at: None,
        updated_at: None,
    });

    let discovered = allthecodes_plugins::discover_plugin_mcp_servers_scoped();

    assert_eq!(discovered.len(), 1);
    assert_eq!(discovered[0].0, "eco-boost");
    assert_eq!(discovered[0].1.name, "mcp-cli-bridge");
    assert_eq!(
        discovered[0]
            .1
            .env
            .as_ref()
            .and_then(|env| env.get("ALLTHECODES_COM_ACCESS_TOKEN")),
        Some(&"account-token".to_string())
    );

    allthecodes_plugins::clear_plugins();
    allthecodes_plugins::set_plugin_account_token_provider(None);
}

#[tokio::test]
#[serial]
async fn web_state_provides_stored_account_token_to_official_plugin_mcp_discovery() {
    let (home, _guard) = temp_home();
    allthecodes_plugins::clear_plugins();
    std::fs::write(
        home.path().join("auth.json"),
        r#"{
            "auth_mode": "allthecodes",
            "tokens": {
                "id_token": "id-token",
                "access_token": "stored-account-token",
                "refresh_token": "refresh-token",
                "account_id": "account-1"
            },
            "last_refresh": "2026-06-21T12:56:57.967Z"
        }"#,
    )
    .expect("stored account auth");
    let _state = make_web_state();
    let plugin_root = tempfile::tempdir().expect("plugin root");
    std::fs::write(
        plugin_root.path().join("plugin.json"),
        r#"{
            "name": "eco-boost",
            "display_name": "Eco Boost",
            "version": "1.0.0",
            "description": "Official test plugin",
            "mcp_servers": [
                { "name": "mcp-cli-bridge", "command": "bin/mcp-cli-bridge", "args": [] }
            ]
        }"#,
    )
    .expect("plugin manifest");
    allthecodes_plugins::register_plugin(allthecodes_plugins::PluginEntry {
        id: "eco-boost".to_string(),
        name: "Eco Boost".to_string(),
        version: "1.0.0".to_string(),
        description: "Official test plugin".to_string(),
        source: PluginSource::Local {
            path: plugin_root.path().to_string_lossy().to_string(),
        },
        status: PluginStatus::Installed,
        marketplace: Some(OFFICIAL_MARKETPLACE_SOURCE_NAME.to_string()),
        cache_path: Some(plugin_root.path().to_path_buf()),
        installed_version: Some("1.0.0".to_string()),
        official: true,
        download_url: None,
        homepage: None,
        sha256: None,
        tools: Vec::new(),
        skills: Vec::new(),
        mcp_servers: vec!["mcp-cli-bridge".to_string()],
        installed_at: None,
        updated_at: None,
    });

    let discovered = allthecodes_plugins::discover_plugin_mcp_servers_scoped();

    assert_eq!(discovered.len(), 1);
    assert_eq!(
        discovered[0]
            .1
            .env
            .as_ref()
            .and_then(|env| env.get("ALLTHECODES_COM_ACCESS_TOKEN")),
        Some(&"stored-account-token".to_string())
    );

    let scoped = allthecodes_mcp::discovery::discover_mcp_servers_scoped(home.path())
        .expect("scoped mcp discovery");
    let mcp = scoped
        .iter()
        .find(|server| server.config.name == "mcp-cli-bridge")
        .expect("plugin mcp server from global discovery");
    assert_eq!(
        mcp.config
            .env
            .as_ref()
            .and_then(|env| env.get("ALLTHECODES_COM_ACCESS_TOKEN")),
        Some(&"stored-account-token".to_string())
    );

    allthecodes_plugins::clear_plugins();
    allthecodes_plugins::set_plugin_account_token_provider(None);
}

struct MarketplaceGlobalGuard {
    sources: Vec<MarketplaceSource>,
    entries: Vec<(String, Vec<MarketplacePluginEntry>)>,
}

impl MarketplaceGlobalGuard {
    fn with_official_source(source: PluginSource) -> Self {
        let guard = Self::capture();
        Self::replace_with_official_source(source);
        guard
    }

    fn capture() -> Self {
        let sources = GLOBAL_MARKETPLACE_INDEX.list_sources();
        let entries = sources
            .iter()
            .map(|source| {
                (
                    source.name.clone(),
                    GLOBAL_MARKETPLACE_INDEX.get_marketplace_entries(&source.name),
                )
            })
            .collect();
        Self { sources, entries }
    }

    fn replace_with_official_source(source: PluginSource) {
        for source in GLOBAL_MARKETPLACE_INDEX.list_sources() {
            GLOBAL_MARKETPLACE_INDEX.unregister_source(&source.name);
        }
        GLOBAL_MARKETPLACE_INDEX.clear_cache();
        GLOBAL_MARKETPLACE_INDEX.register_source(default_marketplace_source());
        GLOBAL_MARKETPLACE_INDEX.set_marketplace_entries(
            DEFAULT_MARKETPLACE_SOURCE_NAME,
            default_marketplace_entries(),
        );
        GLOBAL_MARKETPLACE_INDEX.register_source(MarketplaceSource {
            name: OFFICIAL_MARKETPLACE_SOURCE_NAME.to_string(),
            source,
            description: "Official test marketplace".to_string(),
            auto_update: true,
            priority: 0,
        });
    }
}

impl Drop for MarketplaceGlobalGuard {
    fn drop(&mut self) {
        for source in GLOBAL_MARKETPLACE_INDEX.list_sources() {
            GLOBAL_MARKETPLACE_INDEX.unregister_source(&source.name);
        }
        GLOBAL_MARKETPLACE_INDEX.clear_cache();
        for source in &self.sources {
            GLOBAL_MARKETPLACE_INDEX.register_source(source.clone());
        }
        for (name, entries) in &self.entries {
            GLOBAL_MARKETPLACE_INDEX.set_marketplace_entries(name, entries.clone());
        }
    }
}
