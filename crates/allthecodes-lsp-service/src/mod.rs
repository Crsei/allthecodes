//! LSP service — manages Language Server Protocol server connections.
//!
//! Corresponds to TypeScript: internal LSP server manager used by LSPTool.
//!
//! This module provides the interface for communicating with LSP servers,
//! using `lsp-types` for proper protocol types.
//!
//! LSP servers are managed per-language (determined by file extension).
//! Each server is a subprocess communicating via stdin/stdout using JSON-RPC 2.0.

pub mod client;
pub mod conversions;
pub mod recommendation;
pub mod tool;
pub mod types;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::LazyLock;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

pub use types::{
    CompletionItemInfo, DiagnosticRange, DocumentChange, DocumentSyncState, HoverInfo,
    LspDiagnostic, LspEvent, LspServerInfo, SourceLocation, SourceRange, SymbolInfo,
};

pub mod transport;

/// Configuration for a language server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspServerConfig {
    /// Stable server key. Defaults to `language_id`; plugin/settings configs
    /// may set this to avoid collisions between servers that share a language.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Language ID (e.g. "rust", "typescript", "python").
    #[serde(default, rename = "languageId", alias = "language_id")]
    pub language_id: String,
    /// File extensions this server handles.
    #[serde(default)]
    pub extensions: Vec<String>,
    /// Optional extension-to-language map used by plugin LSP configs.
    #[serde(
        default,
        rename = "extensionToLanguage",
        alias = "extension_to_language"
    )]
    pub extension_to_language: HashMap<String, String>,
    /// Command to launch the server.
    pub command: String,
    /// Command arguments.
    #[serde(default)]
    pub args: Vec<String>,
    /// Environment variables for the server subprocess.
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// Optional workspace folder override.
    #[serde(default, rename = "workspaceFolder", alias = "workspace_folder")]
    pub workspace_folder: Option<String>,
    /// Additional initialization options.
    #[serde(
        default,
        rename = "initializationOptions",
        alias = "init_options",
        alias = "initialization_options"
    )]
    pub init_options: Option<serde_json::Value>,
    /// Human-readable source marker: default/settings/plugin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// Config adapter used by root/plugin wiring to provide LSP server
/// contributions without this crate depending on plugin runtime internals.
pub type LspConfigProvider = Arc<dyn Fn() -> Vec<LspServerConfig> + Send + Sync>;

static CONFIG_PROVIDER: LazyLock<parking_lot::Mutex<Option<LspConfigProvider>>> =
    LazyLock::new(|| parking_lot::Mutex::new(None));

/// Install or replace the external config provider.
pub fn set_config_provider(provider: Option<LspConfigProvider>) {
    *CONFIG_PROVIDER.lock() = provider;
}

/// Global recommendation engine instance.
static RECOMMENDATION_ENGINE: LazyLock<
    parking_lot::Mutex<Option<recommendation::RecommendationEngine>>,
> = LazyLock::new(|| parking_lot::Mutex::new(None));

/// Install or replace the recommendation engine.
pub fn set_recommendation_engine(engine: recommendation::RecommendationEngine) {
    *RECOMMENDATION_ENGINE.lock() = Some(engine);
}

/// Access the recommendation engine with a closure.
///
/// Returns `f(None)` when no engine has been installed.
pub fn with_recommendation_engine<R>(
    f: impl FnOnce(Option<&recommendation::RecommendationEngine>) -> R,
) -> R {
    let guard = RECOMMENDATION_ENGINE.lock();
    f(guard.as_ref())
}

/// Generate recommendations for a project directory using the installed engine.
///
/// Returns an empty vec when no engine has been installed.
pub fn generate_recommendations(
    project_dir: &std::path::Path,
    installed: &[String],
) -> Vec<recommendation::PluginLspRecommendation> {
    let guard = RECOMMENDATION_ENGINE.lock();
    match guard.as_ref() {
        Some(engine) => engine.recommend_for_project(project_dir, installed),
        None => Vec::new(),
    }
}

/// Get a reference to the installed recommendation engine, if any.
pub fn get_recommendation_engine() -> Option<recommendation::RecommendationEngine> {
    RECOMMENDATION_ENGINE.lock().clone()
}

/// Known default LSP server configurations.
pub fn default_server_configs() -> Vec<LspServerConfig> {
    vec![
        builtin_config("rust", &["rs"], "rust-analyzer", &[]),
        builtin_config(
            "typescript",
            &["ts", "tsx", "js", "jsx"],
            "typescript-language-server",
            &["--stdio"],
        ),
        builtin_config("python", &["py"], "pylsp", &[]),
        builtin_config("go", &["go"], "gopls", &[]),
        builtin_config("c", &["c", "h", "cpp", "hpp", "cc"], "clangd", &[]),
        builtin_config("java", &["java"], "jdtls", &[]),
    ]
}

pub fn builtin_config(
    language_id: &str,
    extensions: &[&str],
    command: &str,
    args: &[&str],
) -> LspServerConfig {
    LspServerConfig {
        name: None,
        language_id: language_id.to_string(),
        extensions: extensions.iter().map(|s| (*s).to_string()).collect(),
        extension_to_language: HashMap::new(),
        command: command.to_string(),
        args: args.iter().map(|s| (*s).to_string()).collect(),
        env: HashMap::new(),
        workspace_folder: None,
        init_options: None,
        source: Some("default".to_string()),
    }
}

// ---------------------------------------------------------------------------
// Server configuration
// ---------------------------------------------------------------------------

/// Resolve LSP server configs from plugins/settings/defaults.
///
/// Plugin and settings configs are ordered before defaults so project-specific
/// or plugin-provided servers can claim an extension before the built-in
/// fallback does.
pub fn configured_server_configs() -> Vec<LspServerConfig> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut configs = Vec::new();
    configs.extend(adapter_server_configs());
    configs.extend(settings_server_configs(&cwd));
    configs.extend(default_server_configs());

    let mut seen = HashSet::new();
    configs
        .into_iter()
        .map(normalize_config)
        .filter(|cfg| {
            let key = server_key(cfg);
            if seen.contains(&key) {
                false
            } else {
                seen.insert(key);
                true
            }
        })
        .collect()
}

fn normalize_extension(ext: &str) -> String {
    ext.trim().trim_start_matches('.').to_ascii_lowercase()
}

fn normalize_config(mut config: LspServerConfig) -> LspServerConfig {
    config.extensions = config
        .extensions
        .into_iter()
        .map(|e| normalize_extension(&e))
        .filter(|e| !e.is_empty())
        .collect();

    config.extension_to_language = config
        .extension_to_language
        .into_iter()
        .map(|(ext, lang)| (normalize_extension(&ext), lang))
        .filter(|(ext, lang)| !ext.is_empty() && !lang.trim().is_empty())
        .collect();

    for ext in config.extension_to_language.keys() {
        if !config.extensions.iter().any(|existing| existing == ext) {
            config.extensions.push(ext.clone());
        }
    }

    if config.language_id.trim().is_empty() {
        if let Some(language_id) = config.extension_to_language.values().next() {
            config.language_id = language_id.clone();
        } else if let Some(name) = config.name.clone() {
            config.language_id = name;
        }
    }

    config
}

fn server_key(config: &LspServerConfig) -> String {
    config
        .name
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(config.language_id.as_str())
        .to_string()
}

fn config_supports_extension(config: &LspServerConfig, ext: &str) -> bool {
    let ext = normalize_extension(ext);
    config.extensions.iter().any(|candidate| candidate == &ext)
        || config.extension_to_language.contains_key(&ext)
}

fn language_for_extension_in_config(config: &LspServerConfig, ext: &str) -> Option<String> {
    let ext = normalize_extension(ext);
    config
        .extension_to_language
        .get(&ext)
        .cloned()
        .or_else(|| {
            config
                .extensions
                .iter()
                .any(|candidate| candidate == &ext)
                .then(|| config.language_id.clone())
        })
        .filter(|language| !language.trim().is_empty())
}

fn settings_server_configs(cwd: &Path) -> Vec<LspServerConfig> {
    let Ok(loaded) = allthecodes_config::settings::load_effective(cwd) else {
        return Vec::new();
    };
    let Some(value) = loaded
        .effective
        .extra
        .get("lspServers")
        .or_else(|| loaded.effective.extra.get("lsp_servers"))
    else {
        return Vec::new();
    };
    parse_server_config_value(value, "settings")
}

fn adapter_server_configs() -> Vec<LspServerConfig> {
    CONFIG_PROVIDER
        .lock()
        .as_ref()
        .map(|provider| provider())
        .unwrap_or_default()
}

fn parse_server_config_value(value: &serde_json::Value, source: &str) -> Vec<LspServerConfig> {
    let mut out = Vec::new();
    match value {
        serde_json::Value::Object(map) => {
            if map.contains_key("command") {
                match serde_json::from_value::<LspServerConfig>(value.clone()) {
                    Ok(mut config) => {
                        config.source = Some(source.to_string());
                        out.push(config);
                    }
                    Err(err) => {
                        tracing::warn!(source, error = %err, "LSP: invalid server config");
                    }
                }
            } else {
                for (name, raw) in map {
                    match serde_json::from_value::<LspServerConfig>(raw.clone()) {
                        Ok(mut config) => {
                            if config.name.is_none() {
                                config.name = Some(name.clone());
                            }
                            config.source = Some(source.to_string());
                            out.push(config);
                        }
                        Err(err) => {
                            tracing::warn!(
                                server = %name,
                                source,
                                error = %err,
                                "LSP: invalid server config"
                            );
                        }
                    }
                }
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                match serde_json::from_value::<LspServerConfig>(item.clone()) {
                    Ok(mut config) => {
                        config.source = Some(source.to_string());
                        out.push(config);
                    }
                    Err(err) => {
                        tracing::warn!(source, error = %err, "LSP: invalid server config");
                    }
                }
            }
        }
        _ => {
            tracing::warn!(source, "LSP: expected lspServers to be an object or array");
        }
    }
    out
}

pub fn load_manifest_lsp_declaration(
    declaration: &serde_json::Value,
    plugin_path: &Path,
    plugin_id: &str,
) -> Vec<LspServerConfig> {
    match declaration {
        serde_json::Value::String(relative_path) => {
            let Some(path) = resolve_plugin_relative_path(plugin_path, relative_path) else {
                tracing::warn!(
                    plugin = %plugin_id,
                    path = %relative_path,
                    "Plugin: rejected LSP config path outside plugin directory"
                );
                return Vec::new();
            };
            match std::fs::read_to_string(&path)
                .ok()
                .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            {
                Some(value) => parse_server_config_value(&value, &format!("plugin:{plugin_id}")),
                None => {
                    tracing::warn!(
                        plugin = %plugin_id,
                        path = %path.display(),
                        "Plugin: failed to read LSP config declaration"
                    );
                    Vec::new()
                }
            }
        }
        serde_json::Value::Array(items) => items
            .iter()
            .flat_map(|item| load_manifest_lsp_declaration(item, plugin_path, plugin_id))
            .collect(),
        serde_json::Value::Object(_) => {
            parse_server_config_value(declaration, &format!("plugin:{plugin_id}"))
        }
        _ => Vec::new(),
    }
}

fn resolve_plugin_relative_path(plugin_path: &Path, relative_path: &str) -> Option<PathBuf> {
    let base = plugin_path.canonicalize().ok()?;
    let candidate = plugin_path.join(relative_path).canonicalize().ok()?;
    candidate.starts_with(base).then_some(candidate)
}

/// Get the language ID for a file extension.
pub fn language_for_extension(ext: &str) -> Option<String> {
    let ext = normalize_extension(ext);
    for config in configured_server_configs() {
        if let Some(language_id) = language_for_extension_in_config(&config, &ext) {
            return Some(language_id);
        }
    }
    None
}

/// Get the server config for a file path.
pub fn config_for_file(path: &Path) -> Option<LspServerConfig> {
    let ext = normalize_extension(path.extension()?.to_str()?);
    configured_server_configs()
        .into_iter()
        .find(|c| config_supports_extension(c, &ext))
}

// ---------------------------------------------------------------------------
// Server state tracking
// ---------------------------------------------------------------------------

/// Connection state for an LSP server.
#[derive(Debug, Clone, PartialEq)]
pub enum ServerState {
    NotStarted,
    Starting,
    Running,
    Stopped,
    Error(String),
}

/// Global LSP client instances, keyed by language_id.
/// Uses tokio::sync::Mutex because LspClient methods are async.
static LSP_CLIENTS: LazyLock<tokio::sync::Mutex<HashMap<String, client::LspClient>>> =
    LazyLock::new(|| tokio::sync::Mutex::new(HashMap::new()));

/// Latest diagnostics per document URI.
static DIAGNOSTICS: LazyLock<parking_lot::Mutex<HashMap<String, Vec<LspDiagnostic>>>> =
    LazyLock::new(|| parking_lot::Mutex::new(HashMap::new()));

/// Delivered diagnostic keys per URI. Used by the tool-facing snapshot path
/// to suppress repeats across turns until a document changes.
static DELIVERED_DIAGNOSTICS: LazyLock<parking_lot::Mutex<HashMap<String, HashSet<String>>>> =
    LazyLock::new(|| parking_lot::Mutex::new(HashMap::new()));

/// Event sender for subsystem events (injected by headless event loop).
static EVENT_TX: LazyLock<parking_lot::Mutex<Option<tokio::sync::broadcast::Sender<LspEvent>>>> =
    LazyLock::new(|| parking_lot::Mutex::new(None));

/// Inject the event sender from the headless event loop.
pub fn set_event_sender(tx: tokio::sync::broadcast::Sender<LspEvent>) {
    *EVENT_TX.lock() = Some(tx);
}

/// Emit a subsystem event (no-op if no sender is set).
pub fn emit_event(event: LspEvent) {
    if let Some(tx) = EVENT_TX.lock().as_ref() {
        let _ = tx.send(event);
    }
}

pub(crate) fn record_diagnostics_event(event: LspEvent) {
    if let LspEvent::DiagnosticsPublished { uri, diagnostics } = &event {
        if diagnostics.is_empty() {
            DIAGNOSTICS.lock().remove(uri);
            DELIVERED_DIAGNOSTICS.lock().remove(uri);
        } else {
            DIAGNOSTICS.lock().insert(uri.clone(), diagnostics.clone());
        }
    }
    emit_event(event);
}

pub(crate) fn clear_delivered_diagnostics(uri: &str) {
    DELIVERED_DIAGNOSTICS.lock().remove(uri);
}

pub fn diagnostics_snapshot(uri: Option<&str>) -> Vec<(String, Vec<LspDiagnostic>)> {
    let diagnostics = DIAGNOSTICS.lock();
    match uri {
        Some(uri) => diagnostics
            .get(uri)
            .cloned()
            .map(|items| vec![(uri.to_string(), items)])
            .unwrap_or_default(),
        None => diagnostics
            .iter()
            .map(|(uri, items)| (uri.clone(), items.clone()))
            .collect(),
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Get or start an LSP client for the file at the given URI.
/// Returns the server key to look up the client.
async fn get_or_start_client(
    uri: &str,
    clients: &mut HashMap<String, client::LspClient>,
) -> Result<String> {
    // Extract file path from URI
    let path = uri
        .strip_prefix("file:///")
        .or_else(|| uri.strip_prefix("file://"))
        .unwrap_or(uri);
    let file_path = Path::new(path);
    let config = config_for_file(file_path)
        .with_context(|| format!("No LSP server configured for: {}", path))?;

    let key = server_key(&config);

    // Check if existing client is alive
    if let Some(existing) = clients.get_mut(&key) {
        if existing.is_alive() {
            return Ok(key);
        }
        tracing::warn!(server = %key, "LSP server died, will restart");
        clients.remove(&key);
        emit_event(LspEvent::ServerStateChanged {
            language_id: key.clone(),
            state: "stopped".to_string(),
            error: Some("server process died".to_string()),
        });
    }

    // Start new client
    let root_path = config
        .workspace_folder
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let new_client = client::LspClient::start(&config, &root_path).await?;
    clients.insert(key.clone(), new_client);
    emit_event(LspEvent::ServerStateChanged {
        language_id: key.clone(),
        state: "running".to_string(),
        error: None,
    });
    Ok(key)
}

/// Reconstruct a `CallHierarchyItem` JSON from a [`SymbolInfo`] for call
/// hierarchy requests (incoming/outgoing calls).
///
/// The SymbolInfo's 1-based positions are converted back to 0-based LSP
/// positions.  `SymbolKind::FUNCTION` is used as the default kind because
/// we don't store the original numeric kind value.
fn symbol_info_to_call_hierarchy_json(item: &SymbolInfo) -> Result<serde_json::Value> {
    let uri = conversions::file_path_to_uri(&item.location.file_path)?;

    // Convert 1-based back to 0-based
    let start_line = item.location.line.saturating_sub(1);
    let start_char = item.location.character.saturating_sub(1);
    let end_line = item
        .location
        .end_line
        .unwrap_or(item.location.line)
        .saturating_sub(1);
    let end_char = item
        .location
        .end_character
        .unwrap_or(item.location.character)
        .saturating_sub(1);

    // SymbolKind::FUNCTION = 12 in LSP specification
    let kind_value = serde_json::to_value(lsp_types::SymbolKind::FUNCTION)
        .unwrap_or(serde_json::Value::Number(12.into()));

    Ok(serde_json::json!({
        "name": item.name,
        "kind": kind_value,
        "uri": uri.as_str(),
        "range": {
            "start": { "line": start_line, "character": start_char },
            "end": { "line": end_line, "character": end_char }
        },
        "selectionRange": {
            "start": { "line": start_line, "character": start_char },
            "end": { "line": end_line, "character": end_char }
        }
    }))
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Start a server by server key or language id.
pub async fn start_server(language_id_or_key: &str) -> Result<()> {
    let config = configured_server_configs()
        .into_iter()
        .find(|cfg| server_key(cfg) == language_id_or_key || cfg.language_id == language_id_or_key)
        .with_context(|| format!("No LSP server configured for: {language_id_or_key}"))?;
    let key = server_key(&config);
    let root_path = config
        .workspace_folder
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    let mut clients = LSP_CLIENTS.lock().await;
    if let Some(existing) = clients.get_mut(&key) {
        if existing.is_alive() {
            emit_event(LspEvent::ServerStateChanged {
                language_id: key,
                state: "running".to_string(),
                error: None,
            });
            return Ok(());
        }
        clients.remove(&key);
    }

    let client = client::LspClient::start(&config, &root_path).await?;
    clients.insert(key.clone(), client);
    emit_event(LspEvent::ServerStateChanged {
        language_id: key,
        state: "running".to_string(),
        error: None,
    });
    Ok(())
}

/// Stop a running server by server key or language id.
pub async fn stop_server(language_id_or_key: &str) -> Result<()> {
    let key = resolve_running_key(language_id_or_key).await;
    let Some(key) = key else {
        return Ok(());
    };
    let mut clients = LSP_CLIENTS.lock().await;
    if let Some(client) = clients.remove(&key) {
        drop(clients);
        let _ = client.shutdown().await;
        emit_event(LspEvent::ServerStateChanged {
            language_id: key,
            state: "stopped".to_string(),
            error: None,
        });
    }
    Ok(())
}

pub async fn restart_server(language_id_or_key: &str) -> Result<()> {
    stop_server(language_id_or_key).await?;
    start_server(language_id_or_key).await
}

async fn resolve_running_key(language_id_or_key: &str) -> Option<String> {
    let clients = LSP_CLIENTS.lock().await;
    if clients.contains_key(language_id_or_key) {
        return Some(language_id_or_key.to_string());
    }
    clients
        .iter()
        .find(|(_, client)| client.language_id == language_id_or_key)
        .map(|(key, _)| key.clone())
}

/// Open a live editor document and notify the language server.
pub async fn open_document(
    uri: &str,
    language_id: Option<String>,
    text: String,
) -> Result<DocumentSyncState> {
    let mut clients = LSP_CLIENTS.lock().await;
    let key = get_or_start_client(uri, &mut clients).await?;
    let client = clients.get_mut(&key).expect("LSP client must exist after get_or_start_client");
    let state = client.open_document(uri, language_id, text).await?;
    let _ = client.drain_notifications(Duration::from_millis(250)).await;
    emit_document_synced("open", &state);
    Ok(state)
}

/// Apply full-document or ranged incremental edits to a live document.
pub async fn change_document(
    uri: &str,
    full_text: Option<String>,
    changes: Vec<DocumentChange>,
    version: Option<i32>,
) -> Result<DocumentSyncState> {
    if full_text.is_none() && changes.is_empty() {
        bail!("change_document requires either full text or at least one range change");
    }
    let mut clients = LSP_CLIENTS.lock().await;
    let key = get_or_start_client(uri, &mut clients).await?;
    let client = clients.get_mut(&key).expect("LSP client must exist after get_or_start_client");
    let state = client
        .change_document(uri, full_text, changes, version)
        .await?;
    let _ = client.drain_notifications(Duration::from_millis(350)).await;
    emit_document_synced("change", &state);
    Ok(state)
}

pub async fn save_document(uri: &str, text: Option<String>) -> Result<DocumentSyncState> {
    let mut clients = LSP_CLIENTS.lock().await;
    let key = get_or_start_client(uri, &mut clients).await?;
    let client = clients.get_mut(&key).expect("LSP client must exist after get_or_start_client");
    let state = client.save_document(uri, text).await?;
    let _ = client.drain_notifications(Duration::from_millis(350)).await;
    emit_document_synced("save", &state);
    Ok(state)
}

pub async fn close_document(uri: &str) -> Result<Option<DocumentSyncState>> {
    let mut clients = LSP_CLIENTS.lock().await;
    let path = uri
        .strip_prefix("file:///")
        .or_else(|| uri.strip_prefix("file://"))
        .unwrap_or(uri);
    let Some(config) = config_for_file(Path::new(path)) else {
        return Ok(None);
    };
    let key = server_key(&config);
    let Some(client) = clients.get_mut(&key) else {
        return Ok(None);
    };
    let state = client.close_document(uri).await?;
    if let Some(state) = state.as_ref() {
        record_diagnostics_event(LspEvent::DiagnosticsPublished {
            uri: uri.to_string(),
            diagnostics: Vec::new(),
        });
        emit_document_synced("close", state);
    }
    Ok(state)
}

fn emit_document_synced(kind: &str, state: &DocumentSyncState) {
    emit_event(LspEvent::DocumentSynced {
        uri: state.uri.clone(),
        language_id: state.language_id.clone(),
        version: state.version,
        change_kind: kind.to_string(),
    });
}

/// Completion suggestions at a live editor position.
pub async fn completion(
    uri: &str,
    line: u32,
    character: u32,
    trigger_character: Option<String>,
) -> Result<Vec<CompletionItemInfo>> {
    let mut clients = LSP_CLIENTS.lock().await;
    let key = get_or_start_client(uri, &mut clients).await?;
    let client = clients.get_mut(&key).expect("LSP client must exist after get_or_start_client");
    client.ensure_file_open(uri).await?;

    let mut params = serde_json::json!({
        "textDocument": { "uri": uri },
        "position": { "line": line, "character": character }
    });
    if let Some(trigger) = trigger_character {
        params["context"] = serde_json::json!({
            "triggerKind": 2,
            "triggerCharacter": trigger
        });
    }

    let response = client.request("textDocument/completion", params).await?;
    conversions::parse_completion_response(response)
}

pub fn server_info_snapshot() -> Vec<LspServerInfo> {
    let mut open_counts = HashMap::<String, usize>::new();
    if let Ok(clients) = LSP_CLIENTS.try_lock() {
        for (key, client) in clients.iter() {
            open_counts.insert(key.clone(), client.open_files_count());
        }
    }

    configured_server_configs()
        .into_iter()
        .map(|cfg| {
            let key = server_key(&cfg);
            let open_files_count = open_counts.get(&key).copied().unwrap_or(0);
            let running = open_counts.contains_key(&key);
            LspServerInfo {
                language_id: key,
                state: if running {
                    "running".to_string()
                } else {
                    "not_started".to_string()
                },
                extensions: cfg.extensions.iter().map(|e| format!(".{}", e)).collect(),
                open_files_count,
                error: None,
            }
        })
        .collect()
}

/// Go to definition at position.
pub async fn go_to_definition(uri: &str, line: u32, character: u32) -> Result<Vec<SourceLocation>> {
    let mut clients = LSP_CLIENTS.lock().await;
    let lang = get_or_start_client(uri, &mut clients).await?;
    let client = clients.get_mut(&lang).expect("LSP client must exist after get_or_start_client");
    client.ensure_file_open(uri).await?;

    let params = serde_json::json!({
        "textDocument": { "uri": uri },
        "position": { "line": line, "character": character }
    });
    let response = client.request("textDocument/definition", params).await?;
    conversions::parse_location_response(response)
}

/// Go to implementation at position.
pub async fn go_to_implementation(
    uri: &str,
    line: u32,
    character: u32,
) -> Result<Vec<SourceLocation>> {
    let mut clients = LSP_CLIENTS.lock().await;
    let lang = get_or_start_client(uri, &mut clients).await?;
    let client = clients.get_mut(&lang).expect("LSP client must exist after get_or_start_client");
    client.ensure_file_open(uri).await?;

    let params = serde_json::json!({
        "textDocument": { "uri": uri },
        "position": { "line": line, "character": character }
    });
    let response = client
        .request("textDocument/implementation", params)
        .await?;
    conversions::parse_location_response(response)
}

/// Find references at position.
pub async fn find_references(uri: &str, line: u32, character: u32) -> Result<Vec<SourceLocation>> {
    let mut clients = LSP_CLIENTS.lock().await;
    let lang = get_or_start_client(uri, &mut clients).await?;
    let client = clients.get_mut(&lang).expect("LSP client must exist after get_or_start_client");
    client.ensure_file_open(uri).await?;

    let params = serde_json::json!({
        "textDocument": { "uri": uri },
        "position": { "line": line, "character": character },
        "context": { "includeDeclaration": true }
    });
    let response = client.request("textDocument/references", params).await?;
    conversions::parse_location_response(response)
}

/// Hover at position.
pub async fn hover(uri: &str, line: u32, character: u32) -> Result<HoverInfo> {
    let mut clients = LSP_CLIENTS.lock().await;
    let lang = get_or_start_client(uri, &mut clients).await?;
    let client = clients.get_mut(&lang).expect("LSP client must exist after get_or_start_client");
    client.ensure_file_open(uri).await?;

    let params = serde_json::json!({
        "textDocument": { "uri": uri },
        "position": { "line": line, "character": character }
    });
    let response = client.request("textDocument/hover", params).await?;
    conversions::parse_hover_response(response)
}

/// List document symbols.
pub async fn document_symbols(uri: &str) -> Result<Vec<SymbolInfo>> {
    let mut clients = LSP_CLIENTS.lock().await;
    let lang = get_or_start_client(uri, &mut clients).await?;
    let client = clients.get_mut(&lang).expect("LSP client must exist after get_or_start_client");
    client.ensure_file_open(uri).await?;

    let params = serde_json::json!({
        "textDocument": { "uri": uri }
    });
    let response = client
        .request("textDocument/documentSymbol", params)
        .await?;
    conversions::parse_document_symbols_response(response)
}

/// Search workspace symbols.
pub async fn workspace_symbols(query: &str) -> Result<Vec<SymbolInfo>> {
    let mut clients = LSP_CLIENTS.lock().await;

    // Use any running client — workspace/symbol is not file-specific.
    let lang = {
        let mut found = None;
        for (lang, c) in clients.iter_mut() {
            if c.is_alive() {
                found = Some(lang.clone());
                break;
            }
        }
        found
    };

    let lang = lang.ok_or_else(|| anyhow::anyhow!("No LSP server running"))?;
    let client = clients.get_mut(&lang).expect("LSP client must exist after get_or_start_client");

    let params = serde_json::json!({
        "query": query
    });
    let response = client.request("workspace/symbol", params).await?;
    conversions::parse_workspace_symbols_response(response)
}

/// Prepare call hierarchy.
pub async fn prepare_call_hierarchy(
    uri: &str,
    line: u32,
    character: u32,
) -> Result<Vec<SymbolInfo>> {
    let mut clients = LSP_CLIENTS.lock().await;
    let lang = get_or_start_client(uri, &mut clients).await?;
    let client = clients.get_mut(&lang).expect("LSP client must exist after get_or_start_client");
    client.ensure_file_open(uri).await?;

    let params = serde_json::json!({
        "textDocument": { "uri": uri },
        "position": { "line": line, "character": character }
    });
    let response = client
        .request("textDocument/prepareCallHierarchy", params)
        .await?;
    conversions::parse_call_hierarchy_items(response)
}

/// Get incoming calls.
pub async fn incoming_calls(item: &SymbolInfo) -> Result<Vec<SymbolInfo>> {
    let uri_str = conversions::file_path_to_uri(&item.location.file_path)?
        .as_str()
        .to_string();

    let mut clients = LSP_CLIENTS.lock().await;
    let lang = get_or_start_client(&uri_str, &mut clients).await?;
    let client = clients.get_mut(&lang).expect("LSP client must exist after get_or_start_client");

    let call_item = symbol_info_to_call_hierarchy_json(item)?;
    let params = serde_json::json!({ "item": call_item });
    let response = client
        .request("callHierarchy/incomingCalls", params)
        .await?;
    conversions::parse_incoming_calls(response)
}

/// Get outgoing calls.
pub async fn outgoing_calls(item: &SymbolInfo) -> Result<Vec<SymbolInfo>> {
    let uri_str = conversions::file_path_to_uri(&item.location.file_path)?
        .as_str()
        .to_string();

    let mut clients = LSP_CLIENTS.lock().await;
    let lang = get_or_start_client(&uri_str, &mut clients).await?;
    let client = clients.get_mut(&lang).expect("LSP client must exist after get_or_start_client");

    let call_item = symbol_info_to_call_hierarchy_json(item)?;
    let params = serde_json::json!({ "item": call_item });
    let response = client
        .request("callHierarchy/outgoingCalls", params)
        .await?;
    conversions::parse_outgoing_calls(response)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_language_for_extension() {
        assert_eq!(language_for_extension("rs"), Some("rust".to_string()));
        assert_eq!(language_for_extension("ts"), Some("typescript".to_string()));
        assert_eq!(language_for_extension("py"), Some("python".to_string()));
        assert_eq!(language_for_extension("go"), Some("go".to_string()));
        assert_eq!(language_for_extension("cpp"), Some("c".to_string()));
        assert_eq!(language_for_extension("xyz"), None);
    }

    #[test]
    fn test_config_for_file() {
        let config = config_for_file(Path::new("main.rs"));
        assert!(config.is_some());
        let c = config.unwrap();
        assert_eq!(c.language_id, "rust");
        assert_eq!(c.command, "rust-analyzer");

        let config = config_for_file(Path::new("app.tsx"));
        assert!(config.is_some());
        assert_eq!(config.unwrap().language_id, "typescript");
    }

    #[test]
    fn test_config_for_unknown_file() {
        assert!(config_for_file(Path::new("data.csv")).is_none());
        assert!(config_for_file(Path::new("Makefile")).is_none());
    }

    #[test]
    fn test_default_configs_not_empty() {
        let configs = default_server_configs();
        assert!(configs.len() >= 6);
        for c in &configs {
            assert!(!c.language_id.is_empty());
            assert!(!c.command.is_empty());
            assert!(!c.extensions.is_empty());
        }
    }

    #[test]
    fn test_server_config_serde() {
        let config = LspServerConfig {
            name: Some("rust-custom".into()),
            language_id: "rust".into(),
            extensions: vec!["rs".into()],
            extension_to_language: HashMap::new(),
            command: "rust-analyzer".into(),
            args: vec!["--log-file".into(), "/tmp/ra.log".into()],
            env: HashMap::new(),
            workspace_folder: None,
            init_options: Some(serde_json::json!({"checkOnSave": true})),
            source: Some("test".into()),
        };
        let json = serde_json::to_string(&config).unwrap();
        let back: LspServerConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.language_id, "rust");
        assert_eq!(back.args.len(), 2);
        assert!(back.init_options.is_some());
    }

    #[test]
    fn test_parse_settings_lsp_config_map() {
        let value = serde_json::json!({
            "demo-lsp": {
                "languageId": "demo",
                "extensions": [".demo"],
                "extensionToLanguage": {"demo2": "demo-special"},
                "command": "demo-language-server",
                "args": ["--stdio"],
                "env": {"DEMO": "1"}
            }
        });
        let configs = parse_server_config_value(&value, "settings");
        assert_eq!(configs.len(), 1);
        let config = normalize_config(configs[0].clone());
        assert_eq!(server_key(&config), "demo-lsp");
        assert!(config.extensions.contains(&"demo".to_string()));
        assert!(config.extensions.contains(&"demo2".to_string()));
        assert_eq!(
            language_for_extension_in_config(&config, "demo2").as_deref(),
            Some("demo-special")
        );
        assert_eq!(config.command, "demo-language-server");
        assert_eq!(config.env.get("DEMO").map(String::as_str), Some("1"));
    }

    #[test]
    fn test_manifest_lsp_path_resolution_stays_inside_plugin() {
        let plugin_dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            plugin_dir.path().join("lsp.json"),
            serde_json::to_string(&serde_json::json!({
                "demo": {"languageId": "demo", "extensions": ["demo"], "command": "demo-ls"}
            }))
            .unwrap(),
        )
        .unwrap();

        let configs =
            load_manifest_lsp_declaration(&serde_json::json!("lsp.json"), plugin_dir.path(), "p");
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].name.as_deref(), Some("demo"));

        let outside = load_manifest_lsp_declaration(
            &serde_json::json!("../outside.json"),
            plugin_dir.path(),
            "p",
        );
        assert!(outside.is_empty());
    }

    #[test]
    fn test_server_state_variants() {
        let states = [
            ServerState::NotStarted,
            ServerState::Starting,
            ServerState::Running,
            ServerState::Stopped,
            ServerState::Error("timeout".into()),
        ];
        assert_eq!(states.len(), 5);
        assert_ne!(ServerState::NotStarted, ServerState::Running);
    }
}
