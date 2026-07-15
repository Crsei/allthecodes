use std::collections::HashSet;
use std::fmt;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::Semaphore;

use crate::common::{string_param, validate_enum};
use crate::tool::{Tool, ToolProgress, ToolResult, ToolUseContext, Tools, ValidationResult};
use allthecodes_config::features::{self, Feature};
use allthecodes_types::message::AssistantMessage;

pub const DEFAULT_DISCOVERY_RESULTS: usize = 20;
pub const MAX_DISCOVERY_RESULTS: usize = 100;
pub const MAX_DISCOVERY_QUERY_CHARS: usize = 256;
pub const MAX_DISCOVERY_CANDIDATES_PER_PROVIDER: usize = 5_000;
pub const MAX_DISCOVERY_CONTRIBUTIONS: usize = 50;
pub const MAX_DISCOVERY_RESPONSE_BYTES: usize = 256 * 1024;
pub const MAX_DISCOVERY_PROVIDER_WAIT: Duration = Duration::from_millis(500);

const MAX_DISCOVERY_FIELD_BYTES: usize = 4 * 1024;
const MAX_DISCOVERY_DESCRIPTION_BYTES: usize = 8 * 1024;
const MAX_DISCOVERY_ACTION_BYTES: usize = 512;
const MAX_DISCOVERY_PROVIDER_ERROR_BYTES: usize = 512;
const DISCOVERY_BLOCKING_CONCURRENCY: usize = 4;

static DISCOVERY_BLOCKING_SEMAPHORE: LazyLock<Arc<Semaphore>> =
    LazyLock::new(|| Arc::new(Semaphore::new(DISCOVERY_BLOCKING_CONCURRENCY)));

pub type DiscoverySearchProvider = Arc<
    dyn Fn(
            &DiscoveryContext,
        ) -> std::result::Result<DiscoveryProviderCollection, DiscoveryProviderFailure>
        + Send
        + Sync
        + 'static,
>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryContext {
    workspace: PathBuf,
}

impl DiscoveryContext {
    pub fn new(workspace: impl AsRef<Path>) -> std::result::Result<Self, DiscoverySearchError> {
        let workspace = workspace.as_ref().canonicalize().map_err(|_| {
            DiscoverySearchError::with_code(
                "invalid_workspace",
                "the server-selected discovery workspace is unavailable",
            )
        })?;
        if !workspace.is_dir() {
            return Err(DiscoverySearchError::with_code(
                "invalid_workspace",
                "the server-selected discovery workspace is not a directory",
            ));
        }
        Ok(Self { workspace })
    }

    pub fn workspace(&self) -> &Path {
        &self.workspace
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryProviderFailure {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

impl DiscoveryProviderFailure {
    pub fn new(code: impl Into<String>, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            code: normalize_error_code(&code.into()),
            message: bounded_public_text(&message.into(), MAX_DISCOVERY_PROVIDER_ERROR_BYTES),
            retryable,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiscoveryProviderCollection {
    pub items: Vec<DiscoverySearchResult>,
    pub partial_error: Option<DiscoveryProviderFailure>,
    pub truncated: bool,
}

impl DiscoveryProviderCollection {
    pub fn success(items: Vec<DiscoverySearchResult>) -> Self {
        Self {
            items,
            partial_error: None,
            truncated: false,
        }
    }

    pub fn with_partial_error(mut self, error: DiscoveryProviderFailure) -> Self {
        self.partial_error = Some(error);
        self
    }
}

#[derive(Clone, Default)]
pub struct DiscoverySearchRuntime {
    mcp_items_provider: Option<DiscoverySearchProvider>,
    plugin_items_provider: Option<DiscoverySearchProvider>,
}

impl DiscoverySearchRuntime {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_mcp_items_provider<F>(mut self, provider: F) -> Self
    where
        F: Fn() -> Vec<DiscoverySearchResult> + Send + Sync + 'static,
    {
        self.mcp_items_provider = Some(Arc::new(move |_| {
            Ok(DiscoveryProviderCollection::success(provider()))
        }));
        self
    }

    pub fn with_plugin_items_provider<F>(mut self, provider: F) -> Self
    where
        F: Fn() -> Vec<DiscoverySearchResult> + Send + Sync + 'static,
    {
        self.plugin_items_provider = Some(Arc::new(move |_| {
            Ok(DiscoveryProviderCollection::success(provider()))
        }));
        self
    }

    pub fn with_contextual_mcp_items_provider<F>(mut self, provider: F) -> Self
    where
        F: Fn(
                &DiscoveryContext,
            )
                -> std::result::Result<DiscoveryProviderCollection, DiscoveryProviderFailure>
            + Send
            + Sync
            + 'static,
    {
        self.mcp_items_provider = Some(Arc::new(provider));
        self
    }

    pub fn with_contextual_plugin_items_provider<F>(mut self, provider: F) -> Self
    where
        F: Fn(
                &DiscoveryContext,
            )
                -> std::result::Result<DiscoveryProviderCollection, DiscoveryProviderFailure>
            + Send
            + Sync
            + 'static,
    {
        self.plugin_items_provider = Some(Arc::new(provider));
        self
    }
}

static DISCOVERY_SEARCH_RUNTIME: LazyLock<RwLock<DiscoverySearchRuntime>> =
    LazyLock::new(|| RwLock::new(DiscoverySearchRuntime::new()));

pub fn install_discovery_search_runtime(runtime: DiscoverySearchRuntime) {
    *DISCOVERY_SEARCH_RUNTIME.write() = runtime;
}

pub fn installed_discovery_search_runtime() -> DiscoverySearchRuntime {
    DISCOVERY_SEARCH_RUNTIME.read().clone()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoverySearchError {
    code: &'static str,
    message: String,
}

impl DiscoverySearchError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            code: "invalid_query",
            message: message.into(),
        }
    }

    pub fn with_code(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn code(&self) -> &'static str {
        self.code
    }
}

impl fmt::Display for DiscoverySearchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for DiscoverySearchError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryResultKind {
    Skill,
    McpServer,
    McpResource,
    McpCapability,
    McpSkill,
    Plugin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoverySignal {
    ExplicitSearch,
    PrefetchSearch,
    McpResourceDiscovery,
    PluginMarketplaceCache,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryStatusSummary {
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl DiscoveryStatusSummary {
    pub fn new(state: impl Into<String>) -> Self {
        Self {
            state: state.into(),
            detail: None,
        }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryNextAction {
    pub label: String,
    pub command: String,
}

impl DiscoveryNextAction {
    pub fn new(label: impl Into<String>, command: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            command: command.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryToolSummary {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl DiscoveryToolSummary {
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: Some(description.into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoverySkillSummary {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

impl DiscoverySkillSummary {
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: Some(description.into()),
            source: None,
        }
    }

    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillPrefetchResult {
    pub query: String,
    pub results: Vec<DiscoverySearchResult>,
    pub remote_state: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoverySearchResult {
    pub kind: DiscoveryResultKind,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub when_to_use: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_invocable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_invocable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_summary: Option<DiscoveryStatusSummary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_summaries: Vec<DiscoveryToolSummary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skill_summaries: Vec<DiscoverySkillSummary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mcp_servers: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub marketplace: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub match_reasons: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_action: Option<DiscoveryNextAction>,
    pub discovery_signal: DiscoverySignal,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefetch_source: Option<String>,
    pub remote_url_todo: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_source: Option<String>,
}

impl DiscoverySearchResult {
    pub fn new(kind: DiscoveryResultKind, name: impl Into<String>) -> Self {
        Self {
            kind,
            name: name.into(),
            id: None,
            display_name: None,
            source: None,
            server_name: None,
            version: None,
            description: None,
            when_to_use: None,
            user_invocable: None,
            model_invocable: None,
            status_summary: None,
            capabilities: Vec::new(),
            tool_summaries: Vec::new(),
            skill_summaries: Vec::new(),
            skills: Vec::new(),
            tools: Vec::new(),
            mcp_servers: Vec::new(),
            marketplace: None,
            match_reasons: Vec::new(),
            next_action: None,
            discovery_signal: DiscoverySignal::ExplicitSearch,
            prefetch_source: None,
            remote_url_todo: false,
            remote_source: None,
        }
    }

    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    pub fn with_display_name(mut self, display_name: impl Into<String>) -> Self {
        self.display_name = Some(display_name.into());
        self
    }

    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }

    pub fn with_server_name(mut self, server_name: impl Into<String>) -> Self {
        self.server_name = Some(server_name.into());
        self
    }

    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn with_when_to_use(mut self, when_to_use: impl Into<String>) -> Self {
        self.when_to_use = Some(when_to_use.into());
        self
    }

    pub fn with_invocation_flags(mut self, user_invocable: bool, model_invocable: bool) -> Self {
        self.user_invocable = Some(user_invocable);
        self.model_invocable = Some(model_invocable);
        self
    }

    pub fn with_status(mut self, status: DiscoveryStatusSummary) -> Self {
        self.status_summary = Some(status);
        self
    }

    pub fn with_capabilities<I, S>(mut self, capabilities: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.capabilities = capabilities.into_iter().map(Into::into).collect();
        self
    }

    pub fn with_tool_summaries<I>(mut self, summaries: I) -> Self
    where
        I: IntoIterator<Item = DiscoveryToolSummary>,
    {
        self.tool_summaries = summaries.into_iter().collect();
        self
    }

    pub fn with_skill_summaries<I>(mut self, summaries: I) -> Self
    where
        I: IntoIterator<Item = DiscoverySkillSummary>,
    {
        self.skill_summaries = summaries.into_iter().collect();
        self
    }

    pub fn with_skills<I, S>(mut self, skills: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.skills = skills.into_iter().map(Into::into).collect();
        self
    }

    pub fn with_tools<I, S>(mut self, tools: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.tools = tools.into_iter().map(Into::into).collect();
        self
    }

    pub fn with_mcp_servers<I, S>(mut self, servers: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.mcp_servers = servers.into_iter().map(Into::into).collect();
        self
    }

    pub fn with_marketplace(mut self, marketplace: impl Into<String>) -> Self {
        self.marketplace = Some(marketplace.into());
        self
    }

    pub fn with_next_action(mut self, action: DiscoveryNextAction) -> Self {
        self.next_action = Some(action);
        self
    }

    pub fn with_signal(mut self, signal: DiscoverySignal) -> Self {
        self.discovery_signal = signal;
        self
    }

    pub fn with_prefetch_source(mut self, source: impl Into<String>) -> Self {
        self.prefetch_source = Some(source.into());
        self
    }

    pub fn with_remote_state_placeholder(mut self) -> Self {
        if features::enabled(Feature::RemoteUrlDiscovery) {
            self.remote_url_todo = true;
            self.remote_source = Some("deferred".to_string());
        } else {
            self.remote_url_todo = false;
            self.remote_source = Some("feature_disabled".to_string());
        }
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoverySearchInput {
    pub query: String,
    pub source_filter: Option<String>,
    pub max_results: usize,
    pub include_summaries: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoverySearchOutput {
    pub data: Value,
    pub display_preview: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryProviderKind {
    Mcp,
    Plugin,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DiscoveryProviderSelector {
    #[default]
    All,
    Mcp,
    Plugin,
}

impl DiscoveryProviderSelector {
    fn includes(self, provider: DiscoveryProviderKind) -> bool {
        matches!(self, Self::All)
            || matches!(
                (self, provider),
                (Self::Mcp, DiscoveryProviderKind::Mcp)
                    | (Self::Plugin, DiscoveryProviderKind::Plugin)
            )
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum McpDiscoveryScope {
    #[default]
    All,
    User,
    Project,
    Runtime,
}

impl McpDiscoveryScope {
    fn source_filter(self) -> Option<&'static str> {
        match self {
            Self::All => None,
            Self::User => Some("user"),
            Self::Project => Some("project"),
            Self::Runtime => Some("runtime"),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PluginDiscoverySource {
    #[default]
    All,
    Installed,
    Active,
    MarketplaceCache,
}

impl PluginDiscoverySource {
    fn source_filter(self) -> Option<&'static str> {
        match self {
            Self::All => None,
            Self::Installed => Some("installed"),
            Self::Active => Some("active"),
            Self::MarketplaceCache => Some("marketplace_cache"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryAggregateInput {
    pub query: String,
    pub provider: DiscoveryProviderSelector,
    pub mcp_scope: McpDiscoveryScope,
    pub plugin_source: PluginDiscoverySource,
    pub kind: Option<DiscoveryResultKind>,
    pub max_results: usize,
    pub include_summaries: bool,
    pub context: DiscoveryContext,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryProviderStatus {
    Ok,
    Unavailable,
    Failed,
    TimedOut,
    Truncated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryProviderErrorInfo {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryProviderState {
    pub provider: DiscoveryProviderKind,
    pub status: DiscoveryProviderStatus,
    pub returned: usize,
    pub error: Option<DiscoveryProviderErrorInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryAggregateItem {
    pub provider: DiscoveryProviderKind,
    pub item: DiscoverySearchResult,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryAggregateOutput {
    pub query: String,
    pub results: Vec<DiscoveryAggregateItem>,
    pub providers: Vec<DiscoveryProviderState>,
    pub partial: bool,
    pub truncated: bool,
}

#[derive(Debug)]
struct CollectedProvider {
    provider: DiscoveryProviderKind,
    status: DiscoveryProviderStatus,
    items: Vec<DiscoverySearchResult>,
    error: Option<DiscoveryProviderErrorInfo>,
}

impl CollectedProvider {
    fn state(&self, returned: usize) -> DiscoveryProviderState {
        DiscoveryProviderState {
            provider: self.provider,
            status: self.status,
            returned,
            error: self.error.clone(),
        }
    }
}

#[derive(Debug)]
struct RankedDiscoveryItem {
    score: usize,
    provider: DiscoveryProviderKind,
    item: DiscoverySearchResult,
}

#[derive(Debug, Clone)]
struct ProviderItems {
    installed: bool,
    items: Vec<DiscoverySearchResult>,
    error: Option<String>,
}

pub fn tools() -> Tools {
    vec![Arc::new(McpSearchTool), Arc::new(PluginSearchTool)]
}

pub fn search_items(
    query: &str,
    max_results: usize,
    source_filter: Option<&str>,
) -> std::result::Result<Vec<DiscoverySearchResult>, DiscoverySearchError> {
    let context = DiscoveryContext::new(".")?;
    search_items_with_context(query, max_results, source_filter, &context)
}

pub fn search_items_with_context(
    query: &str,
    max_results: usize,
    source_filter: Option<&str>,
    context: &DiscoveryContext,
) -> std::result::Result<Vec<DiscoverySearchResult>, DiscoverySearchError> {
    let runtime = installed_discovery_search_runtime();
    let mut items = provider_items(runtime.mcp_items_provider.as_ref(), context).items;
    items.extend(provider_items(runtime.plugin_items_provider.as_ref(), context).items);
    search_items_in(query, max_results, source_filter, items)
}

pub fn search_items_in(
    query: &str,
    max_results: usize,
    source_filter: Option<&str>,
    items: Vec<DiscoverySearchResult>,
) -> std::result::Result<Vec<DiscoverySearchResult>, DiscoverySearchError> {
    validate_query(query)?;
    let source_filter = normalized_filter(source_filter);
    let mut matches = items
        .into_iter()
        .filter(|item| source_matches(item, source_filter))
        .filter_map(|mut item| {
            let (score, reasons) = score_item(query, &item);
            if score == 0 {
                return None;
            }
            item.match_reasons = reasons;
            Some((score, item))
        })
        .collect::<Vec<_>>();

    matches.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| a.1.name.cmp(&b.1.name))
            .then_with(|| a.1.id.cmp(&b.1.id))
    });
    matches.truncate(max_results.clamp(1, MAX_DISCOVERY_RESULTS));
    Ok(matches.into_iter().map(|(_, item)| item).collect())
}

/// Collect, rank, redact, and bound all requested discovery providers.
///
/// Provider callbacks are synchronous because the installed MCP/plugin owners
/// currently expose synchronous snapshots. They are isolated behind a bounded
/// blocking semaphore and a per-provider timeout so Web requests never run
/// them on an async executor thread or create unbounded timed-out work.
pub async fn run_discovery_search(
    input: DiscoveryAggregateInput,
) -> std::result::Result<DiscoveryAggregateOutput, DiscoverySearchError> {
    validate_query(&input.query)?;
    if input.max_results == 0 || input.max_results > MAX_DISCOVERY_RESULTS {
        return Err(DiscoverySearchError::with_code(
            "invalid_limit",
            format!("limit must be between 1 and {MAX_DISCOVERY_RESULTS}"),
        ));
    }

    let query = input.query.trim().to_string();
    let runtime = installed_discovery_search_runtime();
    let mcp_context = input.context.clone();
    let plugin_context = input.context.clone();

    let mcp = async {
        if input.provider.includes(DiscoveryProviderKind::Mcp) {
            Some(
                collect_provider(
                    DiscoveryProviderKind::Mcp,
                    runtime.mcp_items_provider.clone(),
                    mcp_context,
                )
                .await,
            )
        } else {
            None
        }
    };
    let plugin = async {
        if input.provider.includes(DiscoveryProviderKind::Plugin) {
            Some(
                collect_provider(
                    DiscoveryProviderKind::Plugin,
                    runtime.plugin_items_provider.clone(),
                    plugin_context,
                )
                .await,
            )
        } else {
            None
        }
    };
    let (mcp, plugin) = tokio::join!(mcp, plugin);
    let mut collected = Vec::with_capacity(2);
    if let Some(mcp) = mcp {
        collected.push(mcp);
    }
    if let Some(plugin) = plugin {
        collected.push(plugin);
    }

    let mut ranked = Vec::new();
    for provider in &mut collected {
        let source_filter = match provider.provider {
            DiscoveryProviderKind::Mcp => input.mcp_scope.source_filter(),
            DiscoveryProviderKind::Plugin => input.plugin_source.source_filter(),
        };
        for item in provider.items.drain(..) {
            if !source_matches(&item, source_filter)
                || input.kind.is_some_and(|kind| item.kind != kind)
            {
                continue;
            }
            let mut item = sanitize_discovery_item(item, true);
            let (score, reasons) = score_item(&query, &item);
            if score == 0 {
                continue;
            }
            item.match_reasons = reasons;
            ranked.push(RankedDiscoveryItem {
                score,
                provider: provider.provider,
                item,
            });
        }
    }
    ranked.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| a.item.name.cmp(&b.item.name))
            .then_with(|| a.item.id.cmp(&b.item.id))
    });

    let truncated = ranked.len() > input.max_results
        || collected
            .iter()
            .any(|provider| provider.status == DiscoveryProviderStatus::Truncated);
    ranked.truncate(input.max_results);
    let results = ranked
        .into_iter()
        .map(|ranked| DiscoveryAggregateItem {
            provider: ranked.provider,
            item: sanitize_discovery_item(ranked.item, input.include_summaries),
        })
        .collect::<Vec<_>>();

    let partial = collected
        .iter()
        .any(|provider| provider.status != DiscoveryProviderStatus::Ok);
    let mut output = DiscoveryAggregateOutput {
        query,
        results,
        providers: collected.iter().map(|provider| provider.state(0)).collect(),
        partial,
        truncated,
    };
    refresh_provider_returned_counts(&mut output);

    while serde_json::to_vec(&output)
        .map_err(|_| {
            DiscoverySearchError::with_code(
                "discovery_serialization_failed",
                "failed to construct a bounded discovery response",
            )
        })?
        .len()
        > MAX_DISCOVERY_RESPONSE_BYTES
    {
        if output.results.pop().is_none() {
            return Err(DiscoverySearchError::with_code(
                "discovery_response_too_large",
                "discovery provider metadata exceeds the response budget",
            ));
        }
        output.truncated = true;
        refresh_provider_returned_counts(&mut output);
    }

    Ok(output)
}

async fn collect_provider(
    provider_kind: DiscoveryProviderKind,
    provider: Option<DiscoverySearchProvider>,
    context: DiscoveryContext,
) -> CollectedProvider {
    let Some(provider) = provider else {
        return CollectedProvider {
            provider: provider_kind,
            status: DiscoveryProviderStatus::Unavailable,
            items: Vec::new(),
            error: Some(DiscoveryProviderErrorInfo {
                code: "provider_unavailable".to_string(),
                message: "discovery provider is unavailable".to_string(),
                retryable: true,
            }),
        };
    };

    let collection = tokio::time::timeout(MAX_DISCOVERY_PROVIDER_WAIT, async move {
        let permit = DISCOVERY_BLOCKING_SEMAPHORE
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| {
                DiscoveryProviderFailure::new(
                    "provider_unavailable",
                    "discovery provider capacity is unavailable",
                    true,
                )
            })?;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            match catch_unwind(AssertUnwindSafe(|| provider(&context))) {
                Ok(result) => result,
                Err(_) => Err(DiscoveryProviderFailure::new(
                    "provider_failed",
                    "discovery provider failed",
                    true,
                )),
            }
        })
        .await
        .map_err(|_| {
            DiscoveryProviderFailure::new("provider_failed", "discovery provider failed", true)
        })?
    })
    .await;

    let mut collection = match collection {
        Err(_) => {
            return CollectedProvider {
                provider: provider_kind,
                status: DiscoveryProviderStatus::TimedOut,
                items: Vec::new(),
                error: Some(DiscoveryProviderErrorInfo {
                    code: "provider_timed_out".to_string(),
                    message: "discovery provider timed out".to_string(),
                    retryable: true,
                }),
            };
        }
        Ok(Err(error)) => {
            return CollectedProvider {
                provider: provider_kind,
                status: DiscoveryProviderStatus::Failed,
                items: Vec::new(),
                error: Some(provider_error_info(error)),
            };
        }
        Ok(Ok(collection)) => collection,
    };

    let candidate_truncated = collection.items.len() > MAX_DISCOVERY_CANDIDATES_PER_PROVIDER;
    collection
        .items
        .truncate(MAX_DISCOVERY_CANDIDATES_PER_PROVIDER);
    let error = collection.partial_error.map(provider_error_info);
    let status = if error.is_some() {
        DiscoveryProviderStatus::Failed
    } else if candidate_truncated || collection.truncated {
        DiscoveryProviderStatus::Truncated
    } else {
        DiscoveryProviderStatus::Ok
    };
    CollectedProvider {
        provider: provider_kind,
        status,
        items: collection.items,
        error,
    }
}

fn provider_error_info(error: DiscoveryProviderFailure) -> DiscoveryProviderErrorInfo {
    DiscoveryProviderErrorInfo {
        code: normalize_error_code(&error.code),
        message: bounded_public_text(&error.message, MAX_DISCOVERY_PROVIDER_ERROR_BYTES),
        retryable: error.retryable,
    }
}

fn refresh_provider_returned_counts(output: &mut DiscoveryAggregateOutput) {
    for state in &mut output.providers {
        state.returned = output
            .results
            .iter()
            .filter(|item| item.provider == state.provider)
            .count();
    }
}

pub fn run_mcp_search(
    input: DiscoverySearchInput,
) -> std::result::Result<DiscoverySearchOutput, DiscoverySearchError> {
    let context = DiscoveryContext::new(".")?;
    run_mcp_search_with_context(input, &context)
}

pub fn run_mcp_search_with_context(
    input: DiscoverySearchInput,
    context: &DiscoveryContext,
) -> std::result::Result<DiscoverySearchOutput, DiscoverySearchError> {
    validate_query(&input.query)?;
    let runtime = installed_discovery_search_runtime();
    let provider = provider_items(runtime.mcp_items_provider.as_ref(), context);
    run_provider_search(
        input,
        provider,
        "scope",
        "MCP discovery provider is not installed",
        "MCP discovery provider failed",
        DomainOutputOptions {
            include_summaries_key: "include_tool_summaries",
            redact_contributions_when_false: false,
        },
    )
}

pub fn run_plugin_search(
    input: DiscoverySearchInput,
) -> std::result::Result<DiscoverySearchOutput, DiscoverySearchError> {
    let context = DiscoveryContext::new(".")?;
    run_plugin_search_with_context(input, &context)
}

pub fn run_plugin_search_with_context(
    input: DiscoverySearchInput,
    context: &DiscoveryContext,
) -> std::result::Result<DiscoverySearchOutput, DiscoverySearchError> {
    validate_query(&input.query)?;
    let runtime = installed_discovery_search_runtime();
    let provider = provider_items(runtime.plugin_items_provider.as_ref(), context);
    run_provider_search(
        input,
        provider,
        "source",
        "Plugin discovery provider is not installed",
        "Plugin discovery provider failed",
        DomainOutputOptions {
            include_summaries_key: "include_contributions",
            redact_contributions_when_false: true,
        },
    )
}

fn validate_query(query: &str) -> std::result::Result<(), DiscoverySearchError> {
    if query.trim().is_empty() {
        return Err(DiscoverySearchError::new("query is required"));
    }
    if query.chars().count() > MAX_DISCOVERY_QUERY_CHARS {
        return Err(DiscoverySearchError::with_code(
            "invalid_query",
            format!("query must not exceed {MAX_DISCOVERY_QUERY_CHARS} characters"),
        ));
    }
    Ok(())
}

fn normalized_filter(filter: Option<&str>) -> Option<&str> {
    filter
        .map(str::trim)
        .filter(|filter| !filter.is_empty() && *filter != "all")
}

fn source_matches(item: &DiscoverySearchResult, filter: Option<&str>) -> bool {
    let Some(filter) = filter else {
        return true;
    };
    item.source.as_deref() == Some(filter)
}

fn provider_items(
    provider: Option<&DiscoverySearchProvider>,
    context: &DiscoveryContext,
) -> ProviderItems {
    let Some(provider) = provider else {
        return ProviderItems {
            installed: false,
            items: Vec::new(),
            error: None,
        };
    };

    match catch_unwind(AssertUnwindSafe(|| provider(context))) {
        Ok(Ok(collection)) => ProviderItems {
            installed: true,
            items: collection.items,
            error: collection.partial_error.map(|error| error.message),
        },
        Ok(Err(error)) => ProviderItems {
            installed: true,
            items: Vec::new(),
            error: Some(error.message),
        },
        Err(_) => ProviderItems {
            installed: true,
            items: Vec::new(),
            error: Some("provider panicked while collecting discovery data".to_string()),
        },
    }
}

struct DomainOutputOptions {
    include_summaries_key: &'static str,
    redact_contributions_when_false: bool,
}

fn run_provider_search(
    input: DiscoverySearchInput,
    provider: ProviderItems,
    filter_key: &str,
    missing_message: &str,
    failed_message: &str,
    options: DomainOutputOptions,
) -> std::result::Result<DiscoverySearchOutput, DiscoverySearchError> {
    let filter = input.source_filter.as_deref().unwrap_or("all");
    if !provider.installed {
        let data = json!({
            "query": input.query,
            filter_key: filter,
            "count": 0,
            "results": [],
            "note": missing_message,
        });
        return Ok(DiscoverySearchOutput {
            data,
            display_preview: Some(format!("{missing_message}; returning empty local results.")),
        });
    }
    if let Some(error) = provider.error {
        let data = json!({
            "query": input.query,
            filter_key: filter,
            "count": 0,
            "results": [],
            "note": failed_message,
            "error": error,
        });
        return Ok(DiscoverySearchOutput {
            data,
            display_preview: Some(format!("{failed_message}; returning empty local results.")),
        });
    }

    let mut results = search_items_in(
        &input.query,
        input.max_results,
        input.source_filter.as_deref(),
        provider.items,
    )?;
    if !input.include_summaries {
        for result in &mut results {
            result.tool_summaries.clear();
            result.skill_summaries.clear();
            if options.redact_contributions_when_false {
                result.skills.clear();
                result.tools.clear();
                result.mcp_servers.clear();
            }
        }
    }
    let data = json!({
        "query": input.query,
        filter_key: filter,
        "count": results.len(),
        "results": results,
        options.include_summaries_key: input.include_summaries,
    });
    Ok(DiscoverySearchOutput {
        data,
        display_preview: None,
    })
}

fn sanitize_discovery_item(
    mut item: DiscoverySearchResult,
    include_summaries: bool,
) -> DiscoverySearchResult {
    item.name = bounded_public_text(&item.name, MAX_DISCOVERY_FIELD_BYTES);
    item.id = sanitize_optional_text(item.id, MAX_DISCOVERY_FIELD_BYTES);
    item.display_name = sanitize_optional_text(item.display_name, MAX_DISCOVERY_FIELD_BYTES);
    item.source = sanitize_optional_text(item.source, MAX_DISCOVERY_FIELD_BYTES);
    item.server_name = sanitize_optional_text(item.server_name, MAX_DISCOVERY_FIELD_BYTES);
    item.version = sanitize_optional_text(item.version, MAX_DISCOVERY_FIELD_BYTES);
    item.description = sanitize_optional_text(item.description, MAX_DISCOVERY_DESCRIPTION_BYTES);
    item.when_to_use = sanitize_optional_text(item.when_to_use, MAX_DISCOVERY_DESCRIPTION_BYTES);
    item.marketplace = sanitize_optional_text(item.marketplace, MAX_DISCOVERY_FIELD_BYTES);
    item.prefetch_source = sanitize_optional_text(item.prefetch_source, MAX_DISCOVERY_FIELD_BYTES);
    item.remote_source = sanitize_optional_text(item.remote_source, MAX_DISCOVERY_FIELD_BYTES);
    if let Some(status) = &mut item.status_summary {
        status.state = bounded_public_text(&status.state, MAX_DISCOVERY_FIELD_BYTES);
        status.detail =
            sanitize_optional_text(status.detail.take(), MAX_DISCOVERY_DESCRIPTION_BYTES);
    }

    item.capabilities = sanitize_string_list(item.capabilities, MAX_DISCOVERY_CONTRIBUTIONS);
    item.skills = sanitize_string_list(item.skills, MAX_DISCOVERY_CONTRIBUTIONS);
    item.tools = sanitize_string_list(item.tools, MAX_DISCOVERY_CONTRIBUTIONS);
    item.mcp_servers = sanitize_string_list(item.mcp_servers, MAX_DISCOVERY_CONTRIBUTIONS);
    item.tool_summaries.truncate(MAX_DISCOVERY_CONTRIBUTIONS);
    for summary in &mut item.tool_summaries {
        summary.name = bounded_public_text(&summary.name, MAX_DISCOVERY_FIELD_BYTES);
        summary.description =
            sanitize_optional_text(summary.description.take(), MAX_DISCOVERY_DESCRIPTION_BYTES);
    }
    item.skill_summaries.truncate(MAX_DISCOVERY_CONTRIBUTIONS);
    for summary in &mut item.skill_summaries {
        summary.name = bounded_public_text(&summary.name, MAX_DISCOVERY_FIELD_BYTES);
        summary.description =
            sanitize_optional_text(summary.description.take(), MAX_DISCOVERY_DESCRIPTION_BYTES);
        summary.source = sanitize_optional_text(summary.source.take(), MAX_DISCOVERY_FIELD_BYTES);
    }
    item.match_reasons = sanitize_string_list(item.match_reasons, 20);
    item.next_action = item.next_action.and_then(sanitize_next_action);

    if !include_summaries {
        item.tool_summaries.clear();
        item.skill_summaries.clear();
        if item.kind == DiscoveryResultKind::Plugin {
            item.skills.clear();
            item.tools.clear();
            item.mcp_servers.clear();
        }
    }
    item
}

fn sanitize_optional_text(value: Option<String>, max_bytes: usize) -> Option<String> {
    value
        .map(|value| bounded_public_text(&value, max_bytes))
        .filter(|value| !value.is_empty())
}

fn sanitize_string_list(mut values: Vec<String>, max_items: usize) -> Vec<String> {
    values.truncate(max_items);
    values
        .into_iter()
        .map(|value| bounded_public_text(&value, MAX_DISCOVERY_FIELD_BYTES))
        .filter(|value| !value.is_empty())
        .collect()
}

fn sanitize_next_action(mut action: DiscoveryNextAction) -> Option<DiscoveryNextAction> {
    let command = safe_discovery_command(&action.command)?;
    action.label = bounded_public_text(&action.label, MAX_DISCOVERY_ACTION_BYTES);
    if action.label.is_empty() {
        return None;
    }
    action.command = command;
    Some(action)
}

fn safe_discovery_command(command: &str) -> Option<String> {
    const PREFIXES: &[&str] = &[
        "/mcp status ",
        "/plugin info ",
        "/plugin marketplace search ",
        "/skills ",
        "ToolSearch source=mcp query=select:",
    ];
    let command = command.trim();
    if command.len() > MAX_DISCOVERY_ACTION_BYTES || command.contains(['\n', '\r', '\0']) {
        return None;
    }
    let prefix = PREFIXES
        .iter()
        .find(|prefix| command.starts_with(**prefix))?;
    let target = command.strip_prefix(prefix)?.trim();
    if target.is_empty()
        || target.len() > 160
        || !target
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | ':' | '@'))
    {
        return None;
    }
    Some(format!("{prefix}{target}"))
}

fn normalize_error_code(code: &str) -> String {
    let code = code.trim();
    if !code.is_empty()
        && code.len() <= 64
        && code
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
    {
        code.to_string()
    } else {
        "provider_failed".to_string()
    }
}

fn bounded_public_text(value: &str, max_bytes: usize) -> String {
    let value = value.trim();
    if value.is_empty() {
        return String::new();
    }
    let lower = value.to_ascii_lowercase();
    if lower.contains("http://")
        || lower.contains("https://")
        || lower.contains("authorization:")
        || lower.contains("bearer ")
        || lower.contains("api_key")
        || lower.contains("apikey")
        || lower.contains("token=")
        || lower.contains("secret=")
        || lower.contains("password=")
        || lower.contains("input_schema")
        || lower.contains("inputschema")
        || lower.contains("\"properties\"")
        || value.split_whitespace().any(looks_like_absolute_path)
    {
        return "[redacted]".to_string();
    }
    truncate_utf8_bytes(value, max_bytes)
}

fn looks_like_absolute_path(token: &str) -> bool {
    let token = token.trim_matches(|ch: char| {
        matches!(
            ch,
            ',' | ';' | '(' | ')' | '[' | ']' | '{' | '}' | '\'' | '"'
        )
    });
    if token.starts_with('/') && token.len() > 1 {
        return true;
    }
    let bytes = token.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\')
}

fn truncate_utf8_bytes(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }
    if max_bytes <= 3 {
        return value.chars().take(max_bytes).collect();
    }
    let mut end = max_bytes - 3;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &value[..end])
}

fn score_item(query: &str, item: &DiscoverySearchResult) -> (usize, Vec<String>) {
    let fields = item_fields(item);
    let score = text_score(
        query,
        &fields
            .iter()
            .map(|(_, value)| value.as_str())
            .collect::<Vec<_>>(),
    );
    if score == 0 {
        return (0, Vec::new());
    }

    let mut reasons = Vec::new();
    for (reason, value) in fields {
        if field_matches_query(&value, query) && !reasons.contains(&reason) {
            reasons.push(reason);
        }
    }
    if reasons.is_empty() {
        reasons.push("metadata".to_string());
    }
    (score, reasons)
}

fn item_fields(item: &DiscoverySearchResult) -> Vec<(String, String)> {
    let mut fields = Vec::new();
    fields.push(("name".to_string(), item.name.clone()));
    push_opt(&mut fields, "id", &item.id);
    push_opt(&mut fields, "display_name", &item.display_name);
    push_opt(&mut fields, "source", &item.source);
    push_opt(&mut fields, "server_name", &item.server_name);
    push_opt(&mut fields, "description", &item.description);
    push_opt(&mut fields, "when_to_use", &item.when_to_use);
    push_opt(&mut fields, "version", &item.version);
    push_opt(&mut fields, "marketplace", &item.marketplace);
    if let Some(status) = &item.status_summary {
        fields.push(("status".to_string(), status.state.clone()));
        push_opt(&mut fields, "status", &status.detail);
    }
    push_joined(&mut fields, "capability", &item.capabilities);
    push_joined(&mut fields, "tool", &item.tools);
    push_joined(&mut fields, "skill", &item.skills);
    push_joined(&mut fields, "mcp_server", &item.mcp_servers);
    for summary in &item.tool_summaries {
        fields.push(("tool".to_string(), summary.name.clone()));
        push_opt(&mut fields, "tool", &summary.description);
    }
    for summary in &item.skill_summaries {
        fields.push(("skill".to_string(), summary.name.clone()));
        push_opt(&mut fields, "skill", &summary.description);
        push_opt(&mut fields, "skill", &summary.source);
    }
    fields
}

fn push_opt(fields: &mut Vec<(String, String)>, reason: &str, value: &Option<String>) {
    if let Some(value) = value.as_ref().filter(|value| !value.trim().is_empty()) {
        fields.push((reason.to_string(), value.clone()));
    }
}

fn push_joined(fields: &mut Vec<(String, String)>, reason: &str, values: &[String]) {
    if !values.is_empty() {
        fields.push((reason.to_string(), values.join(" ")));
    }
}

fn field_matches_query(field: &str, query: &str) -> bool {
    let field_text = field.to_ascii_lowercase();
    let field_tokens = search_identifier_tokens(field)
        .into_iter()
        .collect::<HashSet<_>>();
    query
        .trim()
        .to_ascii_lowercase()
        .split_whitespace()
        .map(str::trim)
        .filter(|term| !term.is_empty())
        .any(|term| field_text.contains(term) || field_tokens.contains(term))
}

fn text_score(query: &str, fields: &[&str]) -> usize {
    let query = query.trim().to_ascii_lowercase();
    if query.is_empty() {
        return 0;
    }
    let terms = query
        .split_whitespace()
        .map(str::trim)
        .filter(|term| !term.is_empty())
        .collect::<Vec<_>>();
    if terms.is_empty() {
        return 0;
    }
    let field_text = fields.join("\n").to_ascii_lowercase();
    let field_tokens = fields
        .iter()
        .flat_map(|field| search_identifier_tokens(field))
        .collect::<HashSet<_>>();
    let mut score = 0;
    if fields
        .iter()
        .any(|field| field.eq_ignore_ascii_case(&query))
    {
        score += 500;
    }
    for term in terms {
        if let Some(required) = term.strip_prefix('+') {
            if !field_text.contains(required) && !field_tokens.contains(required) {
                return 0;
            }
            score += 30;
        } else if field_tokens.contains(term) {
            score += 60;
        } else if field_text.contains(term) {
            score += 10;
        }
    }
    score
}

fn search_identifier_tokens(input: &str) -> Vec<String> {
    let mut normalized = String::with_capacity(input.len() * 2);
    let mut prev_lower_or_digit = false;
    for ch in input.chars() {
        if ch.is_ascii_uppercase() && prev_lower_or_digit {
            normalized.push(' ');
        }
        if ch.is_ascii_alphanumeric() {
            normalized.push(ch.to_ascii_lowercase());
            prev_lower_or_digit = ch.is_ascii_lowercase() || ch.is_ascii_digit();
        } else if ch == '_' || ch == '-' || ch == '.' || ch == '/' || ch == ':' {
            normalized.push(' ');
            prev_lower_or_digit = false;
        } else {
            normalized.push(ch);
            prev_lower_or_digit = false;
        }
    }
    normalized
        .split_whitespace()
        .map(str::trim)
        .filter(|term| !term.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

pub struct McpSearchTool;
pub struct PluginSearchTool;

fn domain_query(input: &Value) -> &str {
    string_param(input, "query").unwrap_or("")
}

fn domain_max_results(input: &Value) -> usize {
    input
        .get("max_results")
        .and_then(Value::as_u64)
        .unwrap_or(25)
        .clamp(1, 100) as usize
}

fn validation_error(message: impl Into<String>) -> ValidationResult {
    ValidationResult::Error {
        message: message.into(),
        error_code: 400,
    }
}

fn validate_required_query(input: &Value) -> Option<ValidationResult> {
    let query = domain_query(input);
    if query.trim().is_empty() {
        return Some(validation_error("query is required"));
    }
    if query.chars().count() > MAX_DISCOVERY_QUERY_CHARS {
        return Some(validation_error(format!(
            "query must not exceed {MAX_DISCOVERY_QUERY_CHARS} characters"
        )));
    }
    None
}

#[async_trait]
impl Tool for McpSearchTool {
    fn name(&self) -> &str {
        "McpSearch"
    }

    async fn description(&self, _input: &Value) -> String {
        "Discover MCP servers, resources, capabilities, and MCP-provided skills without returning callable tool schemas.".into()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "Text query over MCP server names, resources, capabilities, tools, and MCP skills."},
                "scope": {"type": "string", "enum": ["all", "user", "project", "runtime"], "description": "MCP discovery scope filter."},
                "max_results": {"type": "integer", "minimum": 1, "maximum": 100},
                "include_tool_summaries": {"type": "boolean", "description": "Include MCP tool names and descriptions without schemas."}
            },
            "required": ["query"]
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        if let Some(result) = validate_required_query(input) {
            return result;
        }
        if let Some(result) = validate_enum(input, "scope", &["all", "user", "project", "runtime"])
        {
            return result;
        }
        ValidationResult::Ok
    }

    async fn call(
        &self,
        input: Value,
        ctx: &ToolUseContext,
        _parent: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let context = DiscoveryContext::new(&ctx.cwd)?;
        let output = run_mcp_search_with_context(
            DiscoverySearchInput {
                query: domain_query(&input).to_string(),
                source_filter: string_param(&input, "scope").map(ToOwned::to_owned),
                max_results: domain_max_results(&input),
                include_summaries: input
                    .get("include_tool_summaries")
                    .and_then(Value::as_bool)
                    .unwrap_or(true),
            },
            &context,
        )?;
        Ok(ToolResult {
            data: output.data,
            display_preview: output.display_preview,
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        "Discover MCP servers, resources, capabilities, and MCP-provided skills. Use ToolSearch for exact MCP callable tool schemas.".into()
    }
}

#[async_trait]
impl Tool for PluginSearchTool {
    fn name(&self) -> &str {
        "PluginSearch"
    }

    async fn description(&self, _input: &Value) -> String {
        "Discover installed, active, and marketplace-cache plugins without returning callable tool schemas.".into()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "Text query over plugin names, descriptions, status, skills, tools, and MCP contributions."},
                "source": {"type": "string", "enum": ["all", "installed", "active", "marketplace_cache"], "description": "Plugin source filter."},
                "max_results": {"type": "integer", "minimum": 1, "maximum": 100},
                "include_contributions": {"type": "boolean", "description": "Include plugin skill/tool/MCP contribution summaries without schemas."}
            },
            "required": ["query"]
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        if let Some(result) = validate_required_query(input) {
            return result;
        }
        if let Some(result) = validate_enum(
            input,
            "source",
            &["all", "installed", "active", "marketplace_cache"],
        ) {
            return result;
        }
        ValidationResult::Ok
    }

    async fn call(
        &self,
        input: Value,
        ctx: &ToolUseContext,
        _parent: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let context = DiscoveryContext::new(&ctx.cwd)?;
        let output = run_plugin_search_with_context(
            DiscoverySearchInput {
                query: domain_query(&input).to_string(),
                source_filter: string_param(&input, "source").map(ToOwned::to_owned),
                max_results: domain_max_results(&input),
                include_summaries: input
                    .get("include_contributions")
                    .and_then(Value::as_bool)
                    .unwrap_or(true),
            },
            &context,
        )?;
        Ok(ToolResult {
            data: output.data,
            display_preview: output.display_preview,
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        "Discover plugin capabilities and status summaries. Use ToolSearch for exact plugin callable tool schemas.".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_config::features::{self, FeatureFlags};
    use serial_test::serial;

    struct FeatureGuard;

    impl FeatureGuard {
        fn set(flags: FeatureFlags) -> Self {
            features::set_runtime_override(flags);
            Self
        }
    }

    impl Drop for FeatureGuard {
        fn drop(&mut self) {
            features::clear_runtime_override();
        }
    }

    fn rust_plugin() -> DiscoverySearchResult {
        DiscoverySearchResult::new(DiscoveryResultKind::Plugin, "rust-tools")
            .with_id("rust-tools")
            .with_source("active")
            .with_description("Rust formatting, linting, and workspace analysis")
            .with_status(DiscoveryStatusSummary::new("installed"))
            .with_tools(["plugin__rust_tools__fmt"])
            .with_skills(["rust-review"])
            .with_next_action(DiscoveryNextAction::new(
                "Inspect plugin",
                "/plugin info rust-tools",
            ))
            .with_signal(DiscoverySignal::PluginMarketplaceCache)
            .with_remote_state_placeholder()
    }

    fn github_mcp() -> DiscoverySearchResult {
        DiscoverySearchResult::new(DiscoveryResultKind::McpCapability, "github pull requests")
            .with_server_name("github")
            .with_source("runtime")
            .with_description("Create and review pull requests through the github MCP server")
            .with_status(DiscoveryStatusSummary::new("connected"))
            .with_capabilities(["tools", "resources"])
            .with_tool_summaries([DiscoveryToolSummary::new(
                "mcp__github__create_pull_request",
                "Create a pull request",
            )])
            .with_next_action(DiscoveryNextAction::new(
                "Inspect callable schema",
                "ToolSearch source=mcp query=select:mcp__github__create_pull_request",
            ))
            .with_signal(DiscoverySignal::McpResourceDiscovery)
    }

    #[test]
    #[serial]
    fn search_items_ranks_filters_and_clamps_provider_data() {
        install_discovery_search_runtime(
            DiscoverySearchRuntime::new()
                .with_mcp_items_provider(|| vec![github_mcp()])
                .with_plugin_items_provider(|| {
                    vec![
                        rust_plugin(),
                        DiscoverySearchResult::new(DiscoveryResultKind::Plugin, "node-tools")
                            .with_id("node-tools")
                            .with_source("installed")
                            .with_description("JavaScript package helpers"),
                    ]
                }),
        );

        let results = search_items("rust formatter", 1, Some("active")).unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id.as_deref(), Some("rust-tools"));
        assert_eq!(results[0].source.as_deref(), Some("active"));
        assert_eq!(
            results[0].discovery_signal,
            DiscoverySignal::PluginMarketplaceCache
        );
        assert_eq!(results[0].match_reasons[0], "name");
    }

    #[test]
    #[serial]
    fn search_items_rejects_empty_query() {
        install_discovery_search_runtime(DiscoverySearchRuntime::new());

        let err = search_items("   ", 10, None).unwrap_err();

        assert!(err.to_string().contains("query is required"));
    }

    #[test]
    #[serial]
    fn mcp_search_without_provider_returns_empty_preview() {
        install_discovery_search_runtime(DiscoverySearchRuntime::new());

        let output = run_mcp_search(DiscoverySearchInput {
            query: "github".to_string(),
            source_filter: Some("runtime".to_string()),
            max_results: 10,
            include_summaries: true,
        })
        .unwrap();

        assert_eq!(output.data["count"], 0);
        assert!(output.data["results"].as_array().unwrap().is_empty());
        assert!(output
            .display_preview
            .as_deref()
            .unwrap()
            .contains("MCP discovery provider is not installed"));
    }

    #[test]
    #[serial]
    fn plugin_search_remote_state_fields_are_present_but_inert() {
        let mut flags = FeatureFlags::all_disabled();
        flags.remote_url_discovery = true;
        let _features = FeatureGuard::set(flags);
        install_discovery_search_runtime(
            DiscoverySearchRuntime::new().with_plugin_items_provider(|| vec![rust_plugin()]),
        );

        let output = run_plugin_search(DiscoverySearchInput {
            query: "rust".to_string(),
            source_filter: Some("all".to_string()),
            max_results: 5,
            include_summaries: true,
        })
        .unwrap();

        let result = &output.data["results"][0];
        assert_eq!(result["remote_url_todo"], true);
        assert_eq!(result["remote_source"], "deferred");
        assert!(result.get("remote_url").is_none());
    }

    #[test]
    #[serial]
    fn plugin_search_marks_remote_url_discovery_feature_disabled_when_gate_is_off() {
        let _features = FeatureGuard::set(FeatureFlags::all_disabled());
        install_discovery_search_runtime(
            DiscoverySearchRuntime::new().with_plugin_items_provider(|| vec![rust_plugin()]),
        );

        let output = run_plugin_search(DiscoverySearchInput {
            query: "rust".to_string(),
            source_filter: Some("all".to_string()),
            max_results: 5,
            include_summaries: true,
        })
        .unwrap();

        let result = &output.data["results"][0];
        assert_eq!(result["remote_url_todo"], false);
        assert_eq!(result["remote_source"], "feature_disabled");
        assert!(result.get("remote_url").is_none());
    }

    fn aggregate_input(
        workspace: &Path,
        query: &str,
    ) -> std::result::Result<DiscoveryAggregateInput, DiscoverySearchError> {
        Ok(DiscoveryAggregateInput {
            query: query.to_string(),
            provider: DiscoveryProviderSelector::All,
            mcp_scope: McpDiscoveryScope::All,
            plugin_source: PluginDiscoverySource::All,
            kind: None,
            max_results: DEFAULT_DISCOVERY_RESULTS,
            include_summaries: false,
            context: DiscoveryContext::new(workspace)?,
        })
    }

    #[tokio::test]
    #[serial]
    async fn aggregate_uses_trusted_workspace_and_preserves_global_ranking() {
        let workspace = tempfile::tempdir().unwrap();
        let expected = workspace.path().canonicalize().unwrap();
        install_discovery_search_runtime(
            DiscoverySearchRuntime::new()
                .with_contextual_mcp_items_provider(move |context| {
                    assert_eq!(context.workspace(), expected);
                    Ok(DiscoveryProviderCollection::success(vec![
                        DiscoverySearchResult::new(DiscoveryResultKind::McpServer, "zeta")
                            .with_source("project")
                            .with_description("shared exact"),
                    ]))
                })
                .with_plugin_items_provider(|| {
                    vec![
                        DiscoverySearchResult::new(DiscoveryResultKind::Plugin, "alpha")
                            .with_source("active")
                            .with_description("shared exact"),
                    ]
                }),
        );

        let output =
            run_discovery_search(aggregate_input(workspace.path(), "shared exact").unwrap())
                .await
                .unwrap();

        assert_eq!(output.results.len(), 2);
        assert_eq!(output.results[0].item.name, "alpha");
        assert_eq!(output.results[1].item.name, "zeta");
        assert_eq!(output.results[0].item.match_reasons, ["description"]);
    }

    #[tokio::test]
    #[serial]
    async fn aggregate_reports_partial_failure_without_dropping_successes() {
        let workspace = tempfile::tempdir().unwrap();
        install_discovery_search_runtime(
            DiscoverySearchRuntime::new()
                .with_mcp_items_provider(|| vec![github_mcp()])
                .with_contextual_plugin_items_provider(|_| {
                    Err(DiscoveryProviderFailure::new(
                        "provider_failed",
                        "plugin snapshot unavailable",
                        true,
                    ))
                }),
        );

        let output = run_discovery_search(aggregate_input(workspace.path(), "github").unwrap())
            .await
            .unwrap();

        assert!(output.partial);
        assert_eq!(output.results.len(), 1);
        assert_eq!(output.providers[0].status, DiscoveryProviderStatus::Ok);
        assert_eq!(output.providers[1].status, DiscoveryProviderStatus::Failed);
        assert_eq!(
            output.providers[1].error.as_ref().unwrap().code,
            "provider_failed"
        );
    }

    #[tokio::test]
    #[serial]
    async fn aggregate_maps_provider_panics_without_exposing_the_payload() {
        let workspace = tempfile::tempdir().unwrap();
        install_discovery_search_runtime(
            DiscoverySearchRuntime::new()
                .with_mcp_items_provider(|| panic!("provider-specific panic payload")),
        );
        let mut input = aggregate_input(workspace.path(), "github").unwrap();
        input.provider = DiscoveryProviderSelector::Mcp;

        let output = run_discovery_search(input).await.unwrap();

        assert!(output.partial);
        assert_eq!(output.providers[0].status, DiscoveryProviderStatus::Failed);
        let error = output.providers[0].error.as_ref().unwrap();
        assert_eq!(error.code, "provider_failed");
        assert_eq!(error.message, "discovery provider failed");
        assert!(!serde_json::to_string(&output)
            .unwrap()
            .contains("provider-specific"));
    }

    #[tokio::test]
    #[serial]
    async fn aggregate_times_out_blocking_provider_with_stable_public_error() {
        let workspace = tempfile::tempdir().unwrap();
        install_discovery_search_runtime(DiscoverySearchRuntime::new().with_mcp_items_provider(
            || {
                std::thread::sleep(Duration::from_millis(650));
                vec![github_mcp()]
            },
        ));
        let mut input = aggregate_input(workspace.path(), "github").unwrap();
        input.provider = DiscoveryProviderSelector::Mcp;

        let output = run_discovery_search(input).await.unwrap();

        assert!(output.partial);
        assert!(output.results.is_empty());
        assert_eq!(
            output.providers[0].status,
            DiscoveryProviderStatus::TimedOut
        );
        assert_eq!(
            output.providers[0].error.as_ref().unwrap().code,
            "provider_timed_out"
        );
    }

    #[tokio::test]
    #[serial]
    async fn aggregate_enforces_candidate_contribution_and_response_budgets() {
        let workspace = tempfile::tempdir().unwrap();
        install_discovery_search_runtime(DiscoverySearchRuntime::new().with_plugin_items_provider(
            || {
                (0..=MAX_DISCOVERY_CANDIDATES_PER_PROVIDER)
                    .map(|index| {
                        DiscoverySearchResult::new(
                            DiscoveryResultKind::Plugin,
                            format!("plugin-{index:05}"),
                        )
                        .with_description("plugin match")
                        .with_tools(
                            (0..(MAX_DISCOVERY_CONTRIBUTIONS + 10))
                                .map(|tool| format!("tool-{tool}")),
                        )
                    })
                    .collect()
            },
        ));
        let mut input = aggregate_input(workspace.path(), "plugin").unwrap();
        input.provider = DiscoveryProviderSelector::Plugin;
        input.include_summaries = true;
        input.max_results = MAX_DISCOVERY_RESULTS;

        let output = run_discovery_search(input).await.unwrap();

        assert!(output.truncated);
        assert_eq!(
            output.providers[0].status,
            DiscoveryProviderStatus::Truncated
        );
        assert_eq!(output.results.len(), MAX_DISCOVERY_RESULTS);
        assert!(output
            .results
            .iter()
            .all(|result| result.item.tools.len() <= MAX_DISCOVERY_CONTRIBUTIONS));
        assert!(serde_json::to_vec(&output).unwrap().len() <= MAX_DISCOVERY_RESPONSE_BYTES);
    }

    #[tokio::test]
    #[serial]
    async fn aggregate_applies_source_kind_summary_and_input_bounds() {
        let workspace = tempfile::tempdir().unwrap();
        install_discovery_search_runtime(
            DiscoverySearchRuntime::new()
                .with_mcp_items_provider(|| vec![github_mcp()])
                .with_plugin_items_provider(|| {
                    vec![
                        rust_plugin(),
                        DiscoverySearchResult::new(DiscoveryResultKind::Plugin, "rust-installed")
                            .with_source("installed"),
                    ]
                }),
        );
        let mut input = aggregate_input(workspace.path(), "rust").unwrap();
        input.provider = DiscoveryProviderSelector::Plugin;
        input.plugin_source = PluginDiscoverySource::Active;
        input.kind = Some(DiscoveryResultKind::Plugin);

        let output = run_discovery_search(input).await.unwrap();

        assert_eq!(output.results.len(), 1);
        assert_eq!(output.results[0].item.name, "rust-tools");
        assert!(output.results[0].item.tools.is_empty());
        assert!(output.results[0].item.skills.is_empty());

        let mut invalid_limit = aggregate_input(workspace.path(), "rust").unwrap();
        invalid_limit.max_results = 0;
        assert_eq!(
            run_discovery_search(invalid_limit)
                .await
                .unwrap_err()
                .code(),
            "invalid_limit"
        );

        let oversized_query = "界".repeat(MAX_DISCOVERY_QUERY_CHARS + 1);
        assert_eq!(
            run_discovery_search(aggregate_input(workspace.path(), &oversized_query).unwrap())
                .await
                .unwrap_err()
                .code(),
            "invalid_query"
        );
    }

    #[test]
    fn registry_exposes_domain_search_tools_without_losing_tool_search() {
        let names = crate::registry::get_all_tools()
            .into_iter()
            .map(|tool| tool.name().to_string())
            .collect::<Vec<_>>();

        for expected in ["ToolSearch", "SkillSearch", "McpSearch", "PluginSearch"] {
            assert!(
                names.contains(&expected.to_string()),
                "{expected} should be registered"
            );
        }
    }
}
