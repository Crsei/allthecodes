#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryProviderSelector {
    #[default]
    All,
    Mcp,
    Plugin,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum McpDiscoveryScope {
    #[default]
    All,
    User,
    Project,
    Runtime,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum PluginDiscoverySource {
    #[default]
    All,
    Installed,
    Active,
    MarketplaceCache,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryResultKindDto {
    Skill,
    McpServer,
    McpResource,
    McpCapability,
    McpSkill,
    Plugin,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DiscoverySearchQuery {
    pub q: String,
    #[serde(default)]
    pub provider: DiscoveryProviderSelector,
    #[serde(default)]
    pub mcp_scope: McpDiscoveryScope,
    #[serde(default)]
    pub plugin_source: PluginDiscoverySource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<DiscoveryResultKindDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_summaries: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryProviderKind {
    Mcp,
    Plugin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryProviderStatus {
    Ok,
    Unavailable,
    Failed,
    TimedOut,
    Truncated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DiscoveryProviderError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DiscoveryProviderState {
    pub provider: DiscoveryProviderKind,
    pub status: DiscoveryProviderStatus,
    pub returned: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<DiscoveryProviderError>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DiscoveryStatusSummaryDto {
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DiscoveryToolSummaryDto {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DiscoverySkillSummaryDto {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DiscoveryNextActionDto {
    pub label: String,
    /// Display-only command from a fixed navigation/search allowlist.
    pub command: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum DiscoverySignalDto {
    ExplicitSearch,
    PrefetchSearch,
    McpResourceDiscovery,
    PluginMarketplaceCache,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DiscoverySearchItem {
    pub rank: usize,
    pub provider: DiscoveryProviderKind,
    pub kind: DiscoveryResultKindDto,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when_to_use: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_invocable: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_invocable: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_summary: Option<DiscoveryStatusSummaryDto>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_summaries: Vec<DiscoveryToolSummaryDto>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skill_summaries: Vec<DiscoverySkillSummaryDto>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mcp_servers: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub marketplace: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub match_reasons: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_action: Option<DiscoveryNextActionDto>,
    pub discovery_signal: DiscoverySignalDto,
    pub remote_url_todo: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DiscoverySearchResponse {
    pub query: String,
    pub results: Vec<DiscoverySearchItem>,
    pub providers: Vec<DiscoveryProviderState>,
    pub partial: bool,
    pub truncated: bool,
    pub returned: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_defaults_are_safe_and_bounded_by_the_handler() {
        let query: DiscoverySearchQuery =
            serde_json::from_value(serde_json::json!({"q": "github"})).unwrap();

        assert_eq!(query.provider, DiscoveryProviderSelector::All);
        assert_eq!(query.mcp_scope, McpDiscoveryScope::All);
        assert_eq!(query.plugin_source, PluginDiscoverySource::All);
        assert_eq!(query.limit, None);
        assert_eq!(query.include_summaries, None);
    }

    #[test]
    fn unknown_enum_values_fail_deserialization() {
        let error = serde_json::from_value::<DiscoverySearchQuery>(serde_json::json!({
            "q": "github",
            "provider": "remote"
        }))
        .unwrap_err();

        assert!(error.to_string().contains("unknown variant"));
    }
}
