//! Bounded MCP/plugin discovery search.

use std::path::Path;

use allthecodes_protocol::v1::discovery::{
    DiscoveryNextActionDto, DiscoveryProviderError, DiscoveryProviderKind,
    DiscoveryProviderSelector, DiscoveryProviderState, DiscoveryProviderStatus,
    DiscoveryResultKindDto, DiscoverySearchItem, DiscoverySearchQuery, DiscoverySearchResponse,
    DiscoverySignalDto, DiscoverySkillSummaryDto, DiscoveryStatusSummaryDto,
    DiscoveryToolSummaryDto, McpDiscoveryScope, PluginDiscoverySource,
};
use allthecodes_protocol::{ApiError as ProtocolApiError, ApiMethod};
use allthecodes_tools::discovery_search as runtime;
use async_trait::async_trait;
use axum::extract::{Query, State};
use axum::response::Response;
use axum::routing::get;

use crate::api_dispatcher::rest_processor_response;
use crate::handler_registry::HandlerRegistry;
use crate::processors::Processor;
use crate::state::WebState;

#[derive(Clone)]
pub struct DiscoverySearchProcessor {
    state: WebState,
}

impl From<WebState> for DiscoverySearchProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for DiscoverySearchProcessor {
    type Request = DiscoverySearchQuery;
    type Response = DiscoverySearchResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "discovery.search"
    }

    async fn handle(&self, query: Self::Request) -> Result<Self::Response, Self::Error> {
        let engine = self.state.engine();
        let context =
            runtime::DiscoveryContext::new(Path::new(engine.cwd())).map_err(map_discovery_error)?;
        let output = runtime::run_discovery_search(runtime::DiscoveryAggregateInput {
            query: query.q,
            provider: map_provider_selector(query.provider),
            mcp_scope: map_mcp_scope(query.mcp_scope),
            plugin_source: map_plugin_source(query.plugin_source),
            kind: query.kind.map(map_result_kind_to_runtime),
            max_results: query
                .limit
                .map(usize::from)
                .unwrap_or(runtime::DEFAULT_DISCOVERY_RESULTS),
            include_summaries: query.include_summaries.unwrap_or(false),
            context,
        })
        .await
        .map_err(map_discovery_error)?;

        Ok(map_response(output))
    }
}

async fn discovery_search_handler(
    State(state): State<WebState>,
    Query(query): Query<DiscoverySearchQuery>,
) -> Response {
    rest_processor_response::<DiscoverySearchProcessor>(state, ApiMethod::DiscoverySearch, query)
        .await
}

pub(crate) fn handlers() -> HandlerRegistry {
    HandlerRegistry::new().handle(ApiMethod::DiscoverySearch, get(discovery_search_handler))
}

fn map_discovery_error(error: runtime::DiscoverySearchError) -> ProtocolApiError {
    match error.code() {
        "invalid_query" => ProtocolApiError::BadRequest {
            code: "invalid_query",
            message: error.to_string(),
        },
        "invalid_limit" => ProtocolApiError::BadRequest {
            code: "invalid_limit",
            message: error.to_string(),
        },
        "invalid_workspace" => ProtocolApiError::Internal {
            message: "the active discovery workspace is unavailable".to_string(),
        },
        _ => ProtocolApiError::Internal {
            message: "failed to construct discovery response".to_string(),
        },
    }
}

fn map_provider_selector(value: DiscoveryProviderSelector) -> runtime::DiscoveryProviderSelector {
    match value {
        DiscoveryProviderSelector::All => runtime::DiscoveryProviderSelector::All,
        DiscoveryProviderSelector::Mcp => runtime::DiscoveryProviderSelector::Mcp,
        DiscoveryProviderSelector::Plugin => runtime::DiscoveryProviderSelector::Plugin,
    }
}

fn map_mcp_scope(value: McpDiscoveryScope) -> runtime::McpDiscoveryScope {
    match value {
        McpDiscoveryScope::All => runtime::McpDiscoveryScope::All,
        McpDiscoveryScope::User => runtime::McpDiscoveryScope::User,
        McpDiscoveryScope::Project => runtime::McpDiscoveryScope::Project,
        McpDiscoveryScope::Runtime => runtime::McpDiscoveryScope::Runtime,
    }
}

fn map_plugin_source(value: PluginDiscoverySource) -> runtime::PluginDiscoverySource {
    match value {
        PluginDiscoverySource::All => runtime::PluginDiscoverySource::All,
        PluginDiscoverySource::Installed => runtime::PluginDiscoverySource::Installed,
        PluginDiscoverySource::Active => runtime::PluginDiscoverySource::Active,
        PluginDiscoverySource::MarketplaceCache => runtime::PluginDiscoverySource::MarketplaceCache,
    }
}

fn map_result_kind_to_runtime(value: DiscoveryResultKindDto) -> runtime::DiscoveryResultKind {
    match value {
        DiscoveryResultKindDto::Skill => runtime::DiscoveryResultKind::Skill,
        DiscoveryResultKindDto::McpServer => runtime::DiscoveryResultKind::McpServer,
        DiscoveryResultKindDto::McpResource => runtime::DiscoveryResultKind::McpResource,
        DiscoveryResultKindDto::McpCapability => runtime::DiscoveryResultKind::McpCapability,
        DiscoveryResultKindDto::McpSkill => runtime::DiscoveryResultKind::McpSkill,
        DiscoveryResultKindDto::Plugin => runtime::DiscoveryResultKind::Plugin,
    }
}

fn map_response(output: runtime::DiscoveryAggregateOutput) -> DiscoverySearchResponse {
    DiscoverySearchResponse {
        query: output.query,
        returned: output.results.len(),
        results: output
            .results
            .into_iter()
            .enumerate()
            .map(|(index, result)| map_result(index + 1, result))
            .collect(),
        providers: output
            .providers
            .into_iter()
            .map(map_provider_state)
            .collect(),
        partial: output.partial,
        truncated: output.truncated,
    }
}

fn map_result(rank: usize, result: runtime::DiscoveryAggregateItem) -> DiscoverySearchItem {
    let item = result.item;
    DiscoverySearchItem {
        rank,
        provider: map_provider_kind(result.provider),
        kind: map_result_kind(item.kind),
        name: item.name,
        id: item.id,
        display_name: item.display_name,
        source: item.source,
        server_name: item.server_name,
        version: item.version,
        description: item.description,
        when_to_use: item.when_to_use,
        user_invocable: item.user_invocable,
        model_invocable: item.model_invocable,
        status_summary: item.status_summary.map(|status| DiscoveryStatusSummaryDto {
            state: status.state,
            detail: status.detail,
        }),
        capabilities: item.capabilities,
        tool_summaries: item
            .tool_summaries
            .into_iter()
            .map(|summary| DiscoveryToolSummaryDto {
                name: summary.name,
                description: summary.description,
            })
            .collect(),
        skill_summaries: item
            .skill_summaries
            .into_iter()
            .map(|summary| DiscoverySkillSummaryDto {
                name: summary.name,
                description: summary.description,
                source: summary.source,
            })
            .collect(),
        skills: item.skills,
        tools: item.tools,
        mcp_servers: item.mcp_servers,
        marketplace: item.marketplace,
        match_reasons: item.match_reasons,
        next_action: item.next_action.map(|action| DiscoveryNextActionDto {
            label: action.label,
            command: action.command,
        }),
        discovery_signal: match item.discovery_signal {
            runtime::DiscoverySignal::ExplicitSearch => DiscoverySignalDto::ExplicitSearch,
            runtime::DiscoverySignal::PrefetchSearch => DiscoverySignalDto::PrefetchSearch,
            runtime::DiscoverySignal::McpResourceDiscovery => {
                DiscoverySignalDto::McpResourceDiscovery
            }
            runtime::DiscoverySignal::PluginMarketplaceCache => {
                DiscoverySignalDto::PluginMarketplaceCache
            }
        },
        remote_url_todo: item.remote_url_todo,
        remote_source: item.remote_source,
    }
}

fn map_result_kind(value: runtime::DiscoveryResultKind) -> DiscoveryResultKindDto {
    match value {
        runtime::DiscoveryResultKind::Skill => DiscoveryResultKindDto::Skill,
        runtime::DiscoveryResultKind::McpServer => DiscoveryResultKindDto::McpServer,
        runtime::DiscoveryResultKind::McpResource => DiscoveryResultKindDto::McpResource,
        runtime::DiscoveryResultKind::McpCapability => DiscoveryResultKindDto::McpCapability,
        runtime::DiscoveryResultKind::McpSkill => DiscoveryResultKindDto::McpSkill,
        runtime::DiscoveryResultKind::Plugin => DiscoveryResultKindDto::Plugin,
    }
}

fn map_provider_kind(value: runtime::DiscoveryProviderKind) -> DiscoveryProviderKind {
    match value {
        runtime::DiscoveryProviderKind::Mcp => DiscoveryProviderKind::Mcp,
        runtime::DiscoveryProviderKind::Plugin => DiscoveryProviderKind::Plugin,
    }
}

fn map_provider_state(value: runtime::DiscoveryProviderState) -> DiscoveryProviderState {
    DiscoveryProviderState {
        provider: map_provider_kind(value.provider),
        status: match value.status {
            runtime::DiscoveryProviderStatus::Ok => DiscoveryProviderStatus::Ok,
            runtime::DiscoveryProviderStatus::Unavailable => DiscoveryProviderStatus::Unavailable,
            runtime::DiscoveryProviderStatus::Failed => DiscoveryProviderStatus::Failed,
            runtime::DiscoveryProviderStatus::TimedOut => DiscoveryProviderStatus::TimedOut,
            runtime::DiscoveryProviderStatus::Truncated => DiscoveryProviderStatus::Truncated,
        },
        returned: value.returned,
        error: value.error.map(|error| DiscoveryProviderError {
            code: error.code,
            message: error.message,
            retryable: error.retryable,
        }),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use allthecodes_tools::discovery_search::{
        install_discovery_search_runtime, DiscoveryNextAction, DiscoveryProviderCollection,
        DiscoveryProviderFailure, DiscoverySearchResult, DiscoverySearchRuntime,
    };
    use serial_test::serial;

    use super::*;
    use crate::handlers::test_support::make_web_state_with_cwd;

    fn query(text: &str) -> DiscoverySearchQuery {
        DiscoverySearchQuery {
            q: text.to_string(),
            provider: DiscoveryProviderSelector::All,
            mcp_scope: McpDiscoveryScope::All,
            plugin_source: PluginDiscoverySource::All,
            kind: None,
            limit: None,
            include_summaries: None,
        }
    }

    #[tokio::test]
    #[serial]
    async fn processor_passes_the_engine_workspace_to_project_discovery() {
        let workspace = tempfile::tempdir().unwrap();
        let expected = workspace.path().canonicalize().unwrap();
        let observed = Arc::new(Mutex::new(None));
        let observed_for_provider = observed.clone();
        install_discovery_search_runtime(
            DiscoverySearchRuntime::new().with_contextual_mcp_items_provider(move |context| {
                *observed_for_provider.lock().unwrap() = Some(context.workspace().to_path_buf());
                Ok(DiscoveryProviderCollection::success(vec![
                    DiscoverySearchResult::new(runtime::DiscoveryResultKind::McpServer, "github")
                        .with_source("project"),
                ]))
            }),
        );
        let state = make_web_state_with_cwd(workspace.path());
        let mut request = query("github");
        request.provider = DiscoveryProviderSelector::Mcp;
        request.mcp_scope = McpDiscoveryScope::Project;

        let response = DiscoverySearchProcessor::from(state)
            .handle(request)
            .await
            .unwrap();

        assert_eq!(*observed.lock().unwrap(), Some(expected));
        assert_eq!(response.returned, 1);
        assert_eq!(response.results[0].source.as_deref(), Some("project"));
    }

    #[tokio::test]
    #[serial]
    async fn one_provider_failure_preserves_other_results() {
        let workspace = tempfile::tempdir().unwrap();
        install_discovery_search_runtime(
            DiscoverySearchRuntime::new()
                .with_mcp_items_provider(|| {
                    vec![DiscoverySearchResult::new(
                        runtime::DiscoveryResultKind::McpServer,
                        "github",
                    )]
                })
                .with_contextual_plugin_items_provider(|_| {
                    Err(DiscoveryProviderFailure::new(
                        "provider_failed",
                        "plugin provider failed",
                        true,
                    ))
                }),
        );

        let response = DiscoverySearchProcessor::from(make_web_state_with_cwd(workspace.path()))
            .handle(query("github"))
            .await
            .unwrap();

        assert!(response.partial);
        assert_eq!(response.returned, 1);
        assert_eq!(response.providers.len(), 2);
        assert_eq!(
            response.providers[1].status,
            DiscoveryProviderStatus::Failed
        );
    }

    #[tokio::test]
    #[serial]
    async fn default_projection_removes_contributions_paths_urls_and_unsafe_actions() {
        let workspace = tempfile::tempdir().unwrap();
        install_discovery_search_runtime(DiscoverySearchRuntime::new().with_plugin_items_provider(
            || {
                vec![DiscoverySearchResult::new(
                    runtime::DiscoveryResultKind::Plugin,
                    "unsafe-plugin",
                )
                .with_description("config at /home/user/plugin and https://example.invalid")
                .with_skills(["secret-skill"])
                .with_tools(["plugin__unsafe__run"])
                .with_mcp_servers(["private-mcp"])
                .with_next_action(DiscoveryNextAction::new("Run", "rm -rf /home/user"))]
            },
        ));
        let mut request = query("unsafe");
        request.provider = DiscoveryProviderSelector::Plugin;

        let response = DiscoverySearchProcessor::from(make_web_state_with_cwd(workspace.path()))
            .handle(request)
            .await
            .unwrap();

        let result = &response.results[0];
        assert_eq!(result.description.as_deref(), Some("[redacted]"));
        assert!(result.skills.is_empty());
        assert!(result.tools.is_empty());
        assert!(result.mcp_servers.is_empty());
        assert!(result.next_action.is_none());
        assert!(serde_json::to_string(result).unwrap().len() < 256 * 1024);
    }

    #[tokio::test]
    #[serial]
    async fn query_and_limit_validation_use_stable_bad_request_codes() {
        let workspace = tempfile::tempdir().unwrap();
        install_discovery_search_runtime(DiscoverySearchRuntime::new());
        let processor = DiscoverySearchProcessor::from(make_web_state_with_cwd(workspace.path()));

        let empty = processor.handle(query("   ")).await.unwrap_err();
        assert_eq!(empty.code(), "invalid_query");

        let mut over_limit = query("github");
        over_limit.limit = Some(101);
        let invalid_limit = processor.handle(over_limit).await.unwrap_err();
        assert_eq!(invalid_limit.code(), "invalid_limit");
    }
}
