use std::collections::{BTreeMap, HashSet};

use allthecodes_protocol::{ApiError as ProtocolApiError, ApiMethod, ALL_ENDPOINTS};
use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, delete, get, patch, post, put, MethodRouter};
use axum::{Json, Router};
use serde::Serialize;
use tracing::error;

use crate::api_operation_registry::api_operation_registry;
use crate::handlers;
use crate::state::WebState;
use crate::ws;

#[derive(Clone)]
pub struct HandlerEntry {
    pub operation: ApiMethod,
    pub router: MethodRouter<WebState>,
}

#[derive(Clone, Default)]
pub struct HandlerRegistry {
    entries: Vec<HandlerEntry>,
    unimplemented: Vec<ApiMethod>,
}

impl HandlerRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn handle(mut self, operation: ApiMethod, router: MethodRouter<WebState>) -> Self {
        self.entries.push(HandlerEntry { operation, router });
        self
    }

    pub fn unimplemented(mut self, operation: ApiMethod) -> Self {
        self.unimplemented.push(operation);
        self
    }

    pub fn extend(mut self, other: HandlerRegistry) -> Self {
        self.entries.extend(other.entries);
        self.unimplemented.extend(other.unimplemented);
        self
    }

    pub fn entries(&self) -> &[HandlerEntry] {
        &self.entries
    }

    pub fn unimplemented_operations(&self) -> &[ApiMethod] {
        &self.unimplemented
    }

    pub fn validate(&self) -> Result<(), RegistryValidationError> {
        let mut operations = HashSet::new();

        for entry in &self.entries {
            if protocol_endpoint(entry.operation).is_none() {
                return Err(RegistryValidationError::MissingEndpointDefinition {
                    operation: entry.operation,
                });
            }
            if !operations.insert(entry.operation) {
                return Err(RegistryValidationError::DuplicateHandler {
                    operation: entry.operation,
                });
            }
        }

        for operation in &self.unimplemented {
            if protocol_endpoint(*operation).is_none() {
                return Err(RegistryValidationError::MissingEndpointDefinition {
                    operation: *operation,
                });
            }
            if !operations.insert(*operation) {
                return Err(RegistryValidationError::DuplicateHandler {
                    operation: *operation,
                });
            }
        }

        for endpoint in ALL_ENDPOINTS {
            if !operations.contains(&endpoint.operation) {
                return Err(RegistryValidationError::MissingHandler {
                    operation: endpoint.operation,
                });
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistryValidationError {
    MissingEndpointDefinition { operation: ApiMethod },
    MissingHandler { operation: ApiMethod },
    DuplicateHandler { operation: ApiMethod },
}

pub fn all_api_handlers() -> HandlerRegistry {
    HandlerRegistry::new()
        .extend(handlers::health::handlers())
        .extend(kairos_handlers())
        .extend(chat_handlers())
        .extend(launchpad_handlers())
        .extend(handlers::sessions::handlers())
        .extend(handlers::capabilities::handlers())
        .extend(chat_mode_handlers())
        .extend(agent_handlers())
        .extend(people_handlers())
        .extend(hook_handlers())
        .extend(prompt_handlers())
        .extend(mcp_server_handlers())
        .extend(plugin_handlers())
        .extend(channel_handlers())
        .extend(gateway_handlers())
        .extend(integration_handlers())
        .extend(settings_handlers())
        .extend(memory_handlers())
        .extend(handlers::discovery::handlers())
        .extend(workspace_handlers())
        .extend(auth_handlers())
        .extend(profile_handlers())
        .extend(provider_handlers())
        .extend(model_handlers())
        .extend(credential_handlers())
        .extend(log_handlers())
        .extend(terminal_handlers())
        .extend(sidebar_handlers())
        .extend(file_handlers())
        .extend(skill_handlers())
        .extend(kanban_handlers())
        .extend(handlers::workflows::handlers())
        .extend(job_handlers())
        .extend(group_chat_handlers())
        .extend(backend_service_handlers())
        .extend(handlers::queue::handlers())
        .extend(handlers::tasks::handlers())
        .extend(handlers::agent_runtime::handlers())
        .extend(handlers::mentions::handlers())
        .extend(handlers::sidebar::handlers())
        .extend(handlers::messaging::handlers())
        .extend(handlers::image_generate::handlers())
        .extend(handlers::voice::handlers())
        .extend(handlers::worktree_sessions::handlers())
}

pub fn kairos_handlers() -> HandlerRegistry {
    HandlerRegistry::new()
        .handle(ApiMethod::KairosStatus, get(handlers::kairos::status))
        .handle(
            ApiMethod::KairosConfigUpdate,
            put(handlers::kairos::configure),
        )
        .handle(ApiMethod::KairosStart, post(handlers::kairos::start))
        .handle(ApiMethod::KairosStop, post(handlers::kairos::stop))
        .handle(ApiMethod::KairosRestart, post(handlers::kairos::restart))
}

pub fn chat_handlers() -> HandlerRegistry {
    HandlerRegistry::new()
        .handle(ApiMethod::Chat, post(handlers::chat_handler))
        .handle(ApiMethod::Abort, post(handlers::abort_handler))
        .handle(
            ApiMethod::ChatPermissionResponse,
            post(handlers::chat_permission_response_handler),
        )
        .handle(ApiMethod::State, get(handlers::state_handler))
        .handle(
            ApiMethod::SystemPrompt,
            get(handlers::system_prompt_handler),
        )
        .handle(
            ApiMethod::CodingAgentsStatus,
            get(handlers::coding_agent_status_handler),
        )
}

pub fn launchpad_handlers() -> HandlerRegistry {
    HandlerRegistry::new().handle(
        ApiMethod::LaunchpadSnapshotCreate,
        post(handlers::launchpad_snapshot_create_handler),
    )
}

pub fn chat_mode_handlers() -> HandlerRegistry {
    handlers::chat_modes::handlers()
        .handle(
            ApiMethod::ChatModesUpsert,
            put(handlers::chat_modes_upsert_handler),
        )
        .handle(
            ApiMethod::ChatModesEnable,
            post(handlers::chat_modes_enable_handler),
        )
        .handle(
            ApiMethod::ChatModesDisable,
            post(handlers::chat_modes_disable_handler),
        )
        .handle(
            ApiMethod::ChatModesDelete,
            delete(handlers::chat_modes_delete_handler),
        )
}

pub fn agent_handlers() -> HandlerRegistry {
    HandlerRegistry::new()
        .handle(ApiMethod::AgentsList, get(handlers::agents_list_handler))
        .handle(
            ApiMethod::AgentsCreate,
            post(handlers::agents_create_handler),
        )
        .handle(
            ApiMethod::AgentsDetail,
            get(handlers::agents_detail_handler),
        )
        .handle(
            ApiMethod::AgentsUpdate,
            patch(handlers::agents_update_handler),
        )
        .handle(
            ApiMethod::AgentsDelete,
            delete(handlers::agents_delete_handler),
        )
        .handle(
            ApiMethod::AgentsRestore,
            post(handlers::agents_restore_handler),
        )
}

pub fn people_handlers() -> HandlerRegistry {
    handlers::people::handlers()
        .handle(
            ApiMethod::PeopleDetail,
            get(handlers::people_detail_handler),
        )
        .handle(
            ApiMethod::PeopleUpdate,
            patch(handlers::people_update_handler),
        )
        .handle(
            ApiMethod::PeopleDelete,
            delete(handlers::people_delete_handler),
        )
}

pub fn hook_handlers() -> HandlerRegistry {
    HandlerRegistry::new()
        .handle(ApiMethod::HooksList, get(handlers::hooks_list_handler))
        .handle(ApiMethod::HooksCreate, post(handlers::hooks_create_handler))
        .handle(ApiMethod::HooksTest, post(handlers::hooks_test_handler))
        .handle(ApiMethod::HooksDetail, get(handlers::hooks_detail_handler))
        .handle(
            ApiMethod::HooksUpdate,
            patch(handlers::hooks_update_handler),
        )
        .handle(
            ApiMethod::HooksDelete,
            delete(handlers::hooks_delete_handler),
        )
}

pub fn prompt_handlers() -> HandlerRegistry {
    handlers::prompts::handlers()
        .handle(
            ApiMethod::PromptsDetail,
            get(handlers::prompts_detail_handler),
        )
        .handle(
            ApiMethod::PromptsUpdate,
            patch(handlers::prompts_update_handler),
        )
        .handle(
            ApiMethod::PromptsDelete,
            delete(handlers::prompts_delete_handler),
        )
}

pub fn mcp_server_handlers() -> HandlerRegistry {
    HandlerRegistry::new()
        .handle(
            ApiMethod::McpServersList,
            get(handlers::mcp_servers_list_handler),
        )
        .handle(
            ApiMethod::McpServersCreate,
            post(handlers::mcp_servers_create_handler),
        )
        .handle(
            ApiMethod::McpServersMarketplace,
            get(handlers::mcp_servers_marketplace_handler),
        )
        .handle(
            ApiMethod::McpServersHealth,
            get(handlers::mcp_servers_health_handler),
        )
        .handle(
            ApiMethod::McpServersProbe,
            post(handlers::mcp_servers_probe_handler),
        )
        .handle(
            ApiMethod::McpServersDetail,
            get(handlers::mcp_servers_detail_handler),
        )
        .handle(
            ApiMethod::McpServersUpdate,
            patch(handlers::mcp_servers_update_handler),
        )
        .handle(
            ApiMethod::McpServersDelete,
            delete(handlers::mcp_servers_delete_handler),
        )
        .handle(
            ApiMethod::McpServersAuthStart,
            post(handlers::mcp_servers_auth_start_handler),
        )
        .handle(
            ApiMethod::McpServersAuthComplete,
            post(handlers::mcp_servers_auth_complete_handler),
        )
        .handle(
            ApiMethod::McpServersAuthStatus,
            get(handlers::mcp_servers_auth_status_handler),
        )
        .handle(
            ApiMethod::McpServersAuthClear,
            delete(handlers::mcp_servers_auth_clear_handler),
        )
}

pub fn plugin_handlers() -> HandlerRegistry {
    handlers::plugins::handlers().handle(
        ApiMethod::PluginsUninstall,
        post(handlers::plugins_uninstall_handler),
    )
}

pub fn channel_handlers() -> HandlerRegistry {
    HandlerRegistry::new()
        .handle(
            ApiMethod::ChannelsList,
            get(handlers::channels_list_handler),
        )
        .handle(
            ApiMethod::ChannelsCapabilities,
            get(handlers::channels_capabilities_handler),
        )
        .handle(
            ApiMethod::ChannelsConfig,
            get(handlers::channels_config_handler),
        )
        .handle(
            ApiMethod::ChannelsConfigUpdate,
            patch(handlers::channels_config_update_handler),
        )
        .handle(
            ApiMethod::ChannelsEnable,
            post(handlers::channels_enable_handler),
        )
        .handle(
            ApiMethod::ChannelsDisable,
            post(handlers::channels_disable_handler),
        )
        .handle(
            ApiMethod::ChannelsConnect,
            post(handlers::channels_connect_handler),
        )
        .handle(
            ApiMethod::ChannelsTest,
            post(handlers::channels_test_handler),
        )
}

pub fn gateway_handlers() -> HandlerRegistry {
    handlers::gateways::handlers()
        .handle(
            ApiMethod::GatewayStart,
            post(handlers::gateway_start_handler),
        )
        .handle(ApiMethod::GatewayStop, post(handlers::gateway_stop_handler))
}

pub fn integration_handlers() -> HandlerRegistry {
    HandlerRegistry::new()
        .handle(
            ApiMethod::ComputerUseStatus,
            get(handlers::computer_use_status_handler),
        )
        .handle(
            ApiMethod::ComputerUsePermissionRequest,
            post(handlers::computer_use_permission_request_handler),
        )
        .handle(
            ApiMethod::ComputerUseTest,
            post(handlers::computer_use_test_handler),
        )
        .handle(
            ApiMethod::AppshotsStatus,
            get(handlers::appshots_status_handler),
        )
        .handle(
            ApiMethod::AppshotsCapture,
            post(handlers::appshots_capture_handler),
        )
        .handle(
            ApiMethod::ChromeRelayStatus,
            get(handlers::chrome_relay_status_handler),
        )
        .handle(
            ApiMethod::ChromeRelayLaunch,
            post(handlers::chrome_relay_launch_handler),
        )
        .handle(
            ApiMethod::ChromeRelayTokenRegenerate,
            post(handlers::chrome_relay_token_regenerate_handler),
        )
        .handle(
            ApiMethod::ActivityRecorderStatus,
            get(handlers::activity_recorder_status_handler),
        )
        .handle(
            ApiMethod::ActivityRecorderSessions,
            get(handlers::activity_recorder_sessions_handler),
        )
        .handle(
            ApiMethod::ActivityRecorderClear,
            post(handlers::activity_recorder_clear_handler),
        )
}

pub fn settings_handlers() -> HandlerRegistry {
    HandlerRegistry::new()
        .handle(ApiMethod::SettingsApply, post(handlers::settings_handler))
        .handle(
            ApiMethod::SettingsLayers,
            get(handlers::settings_layers_handler),
        )
        .handle(ApiMethod::CommandRun, post(handlers::command_handler))
        .handle(
            ApiMethod::MemoryConfigGet,
            get(handlers::memory_config_get_handler),
        )
        .handle(
            ApiMethod::MemoryConfigPatch,
            patch(handlers::memory_config_patch_handler),
        )
        .handle(
            ApiMethod::SpeechModels,
            get(handlers::speech_models_handler),
        )
        .handle(
            ApiMethod::SpeechModelDownload,
            post(handlers::speech_model_download_handler),
        )
        .handle(
            ApiMethod::SpeechModelDelete,
            delete(handlers::speech_model_delete_handler),
        )
        .handle(
            ApiMethod::SearchCookiesExport,
            post(handlers::search_cookies_export_handler),
        )
        .handle(
            ApiMethod::SearchCookiesImport,
            post(handlers::search_cookies_import_handler),
        )
        .handle(
            ApiMethod::SearchCookiesClear,
            post(handlers::search_cookies_clear_handler),
        )
        .handle(ApiMethod::DataExport, post(handlers::data_export_handler))
        .handle(ApiMethod::DataImport, post(handlers::data_import_handler))
        .handle(
            ApiMethod::TokenSavings,
            get(handlers::token_savings_handler),
        )
        .handle(ApiMethod::DebugState, get(handlers::debug_state_handler))
        .handle(
            ApiMethod::DebugSessionTrace,
            get(handlers::debug_session_trace_handler),
        )
        .handle(ApiMethod::DebugAction, post(handlers::debug_action_handler))
        .handle(ApiMethod::ProtocolRoutes, get(protocol_routes_handler))
}

pub fn memory_handlers() -> HandlerRegistry {
    HandlerRegistry::new()
        .handle(ApiMethod::MemoryList, get(handlers::memory_list_handler))
        .handle(
            ApiMethod::MemoryUpdate,
            patch(handlers::memory_update_handler),
        )
        .handle(
            ApiMethod::MemoryDreamList,
            get(handlers::memory_dream_list_handler),
        )
        .handle(
            ApiMethod::MemoryDreamDetail,
            get(handlers::memory_dream_detail_handler),
        )
        .handle(
            ApiMethod::MemoryProposalList,
            get(handlers::memory_proposal_list_handler),
        )
        .handle(
            ApiMethod::MemoryProposalDetail,
            get(handlers::memory_proposal_detail_handler),
        )
        .handle(
            ApiMethod::MemoryProposalApprove,
            post(handlers::memory_proposal_approve_handler),
        )
        .handle(
            ApiMethod::MemoryProposalReject,
            post(handlers::memory_proposal_reject_handler),
        )
}

pub fn workspace_handlers() -> HandlerRegistry {
    HandlerRegistry::new()
        .handle(
            ApiMethod::WorkspacesList,
            get(handlers::workspaces_list_handler),
        )
        .handle(
            ApiMethod::WorkspacePatch,
            patch(handlers::workspace_patch_handler),
        )
        .handle(
            ApiMethod::WorkspaceOpen,
            post(handlers::workspace_open_handler),
        )
        .handle(
            ApiMethod::WorkspaceSessionsArchive,
            post(handlers::workspace_sessions_archive_handler),
        )
}

pub fn auth_handlers() -> HandlerRegistry {
    HandlerRegistry::new()
        .handle(ApiMethod::AuthStatus, get(handlers::auth_status_handler))
        .handle(ApiMethod::AuthLogin, post(handlers::auth_login_handler))
        .handle(ApiMethod::AuthLogout, post(handlers::auth_logout_handler))
        .handle(ApiMethod::AuthRefresh, post(handlers::auth_refresh_handler))
        .handle(
            ApiMethod::AccountAuthLoginStart,
            post(handlers::account_auth_login_start_handler),
        )
        .handle(
            ApiMethod::AccountAuthLoginComplete,
            post(handlers::account_auth_login_complete_handler),
        )
        .handle(
            ApiMethod::AccountAuthStatus,
            get(handlers::account_auth_status_handler),
        )
        .handle(
            ApiMethod::AccountAuthRefresh,
            post(handlers::account_auth_refresh_handler),
        )
        .handle(
            ApiMethod::AccountAuthLogout,
            post(handlers::account_auth_logout_handler),
        )
        .handle(
            ApiMethod::AccountAuthBilling,
            get(handlers::account_billing_snapshot_handler),
        )
        .handle(
            ApiMethod::AccountAuthBillingLedger,
            get(handlers::account_billing_ledger_handler),
        )
        .handle(
            ApiMethod::AccountAuthBillingOrder,
            get(handlers::account_billing_order_handler),
        )
}

pub fn profile_handlers() -> HandlerRegistry {
    HandlerRegistry::new()
        .handle(
            ApiMethod::ProfilesList,
            get(handlers::profiles_list_handler),
        )
        .handle(
            ApiMethod::ProfilesCreate,
            post(handlers::profiles_create_handler),
        )
        .handle(
            ApiMethod::ProfilesImport,
            post(handlers::profiles_import_handler),
        )
        .handle(
            ApiMethod::ProfilesDetail,
            get(handlers::profiles_detail_handler),
        )
        .handle(
            ApiMethod::ProfilesUpdate,
            patch(handlers::profiles_update_handler),
        )
        .handle(
            ApiMethod::ProfilesDelete,
            delete(handlers::profiles_delete_handler),
        )
        .handle(
            ApiMethod::ProfilesSwitch,
            post(handlers::profiles_switch_handler),
        )
        .handle(
            ApiMethod::ProfilesExport,
            get(handlers::profiles_export_handler),
        )
}

pub fn provider_handlers() -> HandlerRegistry {
    HandlerRegistry::new()
        .handle(
            ApiMethod::ProvidersList,
            get(handlers::providers_list_handler),
        )
        .handle(
            ApiMethod::ProvidersCreate,
            post(handlers::providers_create_handler),
        )
        .handle(
            ApiMethod::ProvidersOpenaiCodexLocalStatus,
            get(handlers::codex_local_status_handler),
        )
        .handle(
            ApiMethod::ProvidersOpenaiCodexApplyLocal,
            post(handlers::codex_apply_local_handler),
        )
        .handle(
            ApiMethod::ProvidersUpdate,
            patch(handlers::providers_update_handler),
        )
        .handle(
            ApiMethod::ProvidersDelete,
            delete(handlers::providers_delete_handler),
        )
        .handle(
            ApiMethod::ProvidersProbe,
            post(handlers::providers_probe_handler),
        )
        .handle(
            ApiMethod::ProvidersRefreshModels,
            post(handlers::providers_refresh_models_handler),
        )
}

pub fn model_handlers() -> HandlerRegistry {
    handlers::models::handlers()
        .handle(
            ApiMethod::ModelsUpdate,
            patch(handlers::models_update_handler),
        )
        .handle(
            ApiMethod::ModelsSetDefault,
            post(handlers::models_set_default_handler),
        )
}

pub fn credential_handlers() -> HandlerRegistry {
    HandlerRegistry::new()
        .handle(ApiMethod::Credentials, get(handlers::credentials_handler))
        .handle(ApiMethod::OAuthStart, post(handlers::oauth_start_handler))
        .handle(ApiMethod::OAuthPoll, post(handlers::oauth_poll_handler))
}

pub fn log_handlers() -> HandlerRegistry {
    HandlerRegistry::new()
        .handle(ApiMethod::Logs, get(handlers::logs_handler))
        .handle(ApiMethod::LogsExport, get(handlers::logs_export_handler))
        .handle(
            ApiMethod::DiagnosticsSnapshot,
            get(handlers::diagnostics_snapshot_handler),
        )
        .handle(
            ApiMethod::DiagnosticsTraces,
            get(handlers::diagnostics_traces_handler),
        )
}

pub fn terminal_handlers() -> HandlerRegistry {
    HandlerRegistry::new()
        .handle(ApiMethod::TerminalHealth, get(ws::terminal::health_handler))
        .handle(
            ApiMethod::TerminalProfiles,
            get(ws::terminal::profiles_handler),
        )
        .handle(
            ApiMethod::TerminalSessionsList,
            get(ws::terminal::list_sessions_handler),
        )
        .handle(
            ApiMethod::TerminalSessionsCreate,
            post(ws::terminal::create_session_handler),
        )
        .handle(
            ApiMethod::TerminalSessionDetail,
            get(ws::terminal::session_detail_handler),
        )
        .handle(
            ApiMethod::TerminalSessionOutput,
            get(ws::terminal::session_output_handler),
        )
        .handle(
            ApiMethod::TerminalSessionDelete,
            delete(ws::terminal::delete_session_handler),
        )
        .handle(
            ApiMethod::TerminalSessionWs,
            any(ws::terminal::session_ws_handler),
        )
        .handle(ApiMethod::TuiWs, any(ws::tui::tui_ws_handler))
        .handle(ApiMethod::IpcWs, any(ws::ipc::ipc_ws_handler))
}

pub fn sidebar_handlers() -> HandlerRegistry {
    HandlerRegistry::new()
        .handle(ApiMethod::GitLog, get(handlers::git_log_handler))
        .handle(ApiMethod::GitDiff, get(handlers::git_diff_handler))
        .handle(ApiMethod::GitMetadata, get(handlers::git_metadata_handler))
        .handle(
            ApiMethod::GitWorktrees,
            get(handlers::git_worktrees_handler),
        )
        .handle(ApiMethod::Proxy, get(handlers::proxy_handler))
        .handle(ApiMethod::Usage, get(handlers::usage_handler))
}

pub fn file_handlers() -> HandlerRegistry {
    HandlerRegistry::new()
        .handle(ApiMethod::FilesTree, get(handlers::files_tree_handler))
        .handle(ApiMethod::FilesStat, get(handlers::files_stat_handler))
        .handle(ApiMethod::FilesRead, get(handlers::files_read_handler))
        .handle(
            ApiMethod::FilesPreview,
            get(handlers::files_preview_handler),
        )
        .handle(ApiMethod::FilesMedia, get(handlers::files_media_handler))
        .handle(ApiMethod::FilesWrite, put(handlers::files_write_handler))
        .handle(ApiMethod::FilesUpload, post(handlers::files_upload_handler))
        .handle(
            ApiMethod::FilesDownload,
            get(handlers::files_download_handler),
        )
        .handle(ApiMethod::FilesMkdir, post(handlers::files_mkdir_handler))
        .handle(ApiMethod::FilesRename, post(handlers::files_rename_handler))
        .handle(ApiMethod::FilesCopy, post(handlers::files_copy_handler))
        .handle(ApiMethod::FilesMove, post(handlers::files_move_handler))
        .handle(
            ApiMethod::FilesDelete,
            delete(handlers::files_delete_handler),
        )
}

pub fn skill_handlers() -> HandlerRegistry {
    handlers::skills::handlers()
        .handle(
            ApiMethod::SkillsDetail,
            get(handlers::skills_detail_handler),
        )
        .handle(
            ApiMethod::SkillsPatch,
            patch(handlers::skills_patch_handler),
        )
        .handle(ApiMethod::SkillsFiles, get(handlers::skills_files_handler))
}

pub fn kanban_handlers() -> HandlerRegistry {
    handlers::kanban::handlers()
        .handle(
            ApiMethod::KanbanBoardDetail,
            get(handlers::kanban_board_detail_handler),
        )
        .handle(
            ApiMethod::KanbanTaskUpdate,
            patch(handlers::kanban_task_update_handler),
        )
        .handle(
            ApiMethod::KanbanTaskComment,
            post(handlers::kanban_task_comment_handler),
        )
}

pub fn job_handlers() -> HandlerRegistry {
    HandlerRegistry::new()
        .handle(ApiMethod::JobsList, get(handlers::jobs_list_handler))
        .handle(ApiMethod::JobsCreate, post(handlers::jobs_create_handler))
        .handle(ApiMethod::JobsUpdate, patch(handlers::jobs_update_handler))
        .handle(ApiMethod::JobsDelete, delete(handlers::jobs_delete_handler))
        .handle(ApiMethod::JobsPause, post(handlers::jobs_pause_handler))
        .handle(ApiMethod::JobsResume, post(handlers::jobs_resume_handler))
        .handle(ApiMethod::JobsRun, post(handlers::jobs_run_handler))
        .handle(ApiMethod::CronHistory, get(handlers::cron_history_handler))
}

pub fn group_chat_handlers() -> HandlerRegistry {
    HandlerRegistry::new()
        .handle(
            ApiMethod::GroupChatRoomsList,
            get(handlers::group_chat_rooms_handler),
        )
        .handle(
            ApiMethod::GroupChatRoomCreate,
            post(handlers::group_chat_room_create_handler),
        )
        .handle(
            ApiMethod::GroupChatRoomDetail,
            get(handlers::group_chat_room_detail_handler),
        )
        .handle(
            ApiMethod::GroupChatRoomDelete,
            delete(handlers::group_chat_room_delete_handler),
        )
        .handle(
            ApiMethod::GroupChatRoomClone,
            post(handlers::group_chat_room_clone_handler),
        )
        .handle(
            ApiMethod::GroupChatInvite,
            get(handlers::group_chat_invite_handler),
        )
        .handle(
            ApiMethod::GroupChatAgentAdd,
            post(handlers::group_chat_agent_add_handler),
        )
        .handle(
            ApiMethod::GroupChatAgentUpdate,
            patch(handlers::group_chat_agent_update_handler),
        )
        .handle(
            ApiMethod::GroupChatAgentDelete,
            delete(handlers::group_chat_agent_delete_handler),
        )
        .handle(
            ApiMethod::GroupChatMessage,
            post(handlers::group_chat_message_handler),
        )
        .handle(
            ApiMethod::GroupChatCompression,
            post(handlers::group_chat_compression_handler),
        )
        .handle(
            ApiMethod::GroupChatStream,
            get(handlers::group_chat_stream_handler),
        )
}

pub fn backend_service_handlers() -> HandlerRegistry {
    HandlerRegistry::new()
        .handle(
            ApiMethod::BackendServices,
            get(handlers::backend_services_handler),
        )
        .handle(
            ApiMethod::BackendServicesSessionsSync,
            post(handlers::backend_services_sessions_sync_handler),
        )
        .handle(
            ApiMethod::BackendServicesContextCompressionRun,
            post(handlers::backend_services_context_compression_run_handler),
        )
        .handle(
            ApiMethod::BackendServicesAgentBridgeRetry,
            post(handlers::backend_services_agent_bridge_retry_handler),
        )
        .handle(
            ApiMethod::BackendServicesMigrationsRun,
            post(handlers::backend_services_migrations_run_handler),
        )
        .handle(
            ApiMethod::BackendServicesBackups,
            post(handlers::backend_services_backup_handler),
        )
}

pub fn register_protocol_routes(
    mut router: Router<WebState>,
    registry: &HandlerRegistry,
) -> Router<WebState> {
    if let Err(error) = registry.validate() {
        error!("protocol handler registry validation failed: {:?}", error);
    }

    let mut routes: BTreeMap<String, MethodRouter<WebState>> = BTreeMap::new();
    for entry in registry.entries() {
        let Some(endpoint) = protocol_endpoint(entry.operation) else {
            error!(
                "registered handler has no protocol endpoint definition: {:?}",
                entry.operation
            );
            continue;
        };

        let method_router = match experimental_reason(entry.operation) {
            Some(reason) => experimental_gate(entry.router.clone(), reason),
            None => entry.router.clone(),
        };

        routes
            .entry(endpoint.path.to_string())
            .and_modify(|router| {
                *router = router.clone().merge(method_router.clone());
            })
            .or_insert_with(|| method_router.clone());

        routes
            .entry(versioned_api_path(endpoint.path))
            .and_modify(|router| {
                *router = router.clone().merge(method_router.clone());
            })
            .or_insert(method_router);
    }

    for (path, method_router) in routes {
        router = router.route(&path, method_router);
    }

    router
}

pub async fn protocol_routes_handler() -> Json<Vec<ProtocolRouteInfo>> {
    let registry = all_api_handlers();
    let api_registry = api_operation_registry();
    debug_assert_eq!(api_registry.len(), ALL_ENDPOINTS.len());
    let registered: HashSet<ApiMethod> = registry
        .entries()
        .iter()
        .map(|entry| entry.operation)
        .collect();
    let unimplemented: HashSet<ApiMethod> = registry
        .unimplemented_operations()
        .iter()
        .copied()
        .collect();

    Json(
        ALL_ENDPOINTS
            .iter()
            .map(|endpoint| {
                let descriptor = api_registry.get(endpoint.operation);
                ProtocolRouteInfo {
                    operation: descriptor
                        .map(|descriptor| format!("{:?}", descriptor.operation))
                        .unwrap_or_else(|| format!("{:?}", endpoint.operation)),
                    http_method: endpoint.http_method,
                    path: endpoint.path,
                    v2_path: versioned_api_path(endpoint.path),
                    registered: registered.contains(&endpoint.operation),
                    unimplemented: unimplemented.contains(&endpoint.operation),
                    experimental: descriptor
                        .and_then(|descriptor| descriptor.metadata.experimental),
                    transport_kind: descriptor
                        .map(|descriptor| format!("{:?}", descriptor.transport_kind))
                        .unwrap_or_else(|| "Rest".to_string()),
                    migration_state: descriptor
                        .map(|descriptor| format!("{:?}", descriptor.migration_state))
                        .unwrap_or_else(|| "LegacyRestHandler".to_string()),
                    any_method: endpoint.http_method == "ANY",
                    websocket: endpoint.http_method == "ANY" || endpoint.path.ends_with("/ws"),
                }
            })
            .collect(),
    )
}

fn protocol_endpoint(operation: ApiMethod) -> Option<&'static allthecodes_protocol::ApiEndpoint> {
    ALL_ENDPOINTS
        .iter()
        .find(|endpoint| endpoint.operation == operation)
}

fn experimental_reason(operation: ApiMethod) -> Option<&'static str> {
    api_operation_registry().experimental_reason(operation)
}

fn experimental_gate(
    router: MethodRouter<WebState>,
    reason: &'static str,
) -> MethodRouter<WebState> {
    router.route_layer(middleware::from_fn(
        move |request: Request, next: Next| async move {
            if experimental_apis_enabled() {
                next.run(request).await
            } else {
                protocol_error_response(ProtocolApiError::Experimental(reason.to_string()))
            }
        },
    ))
}

fn experimental_apis_enabled() -> bool {
    std::env::var("ALLTHECODES_ENABLE_EXPERIMENTAL_API")
        .ok()
        .is_some_and(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "on"))
}

fn protocol_error_response(error: ProtocolApiError) -> Response {
    let status =
        StatusCode::from_u16(error.status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (status, Json(error.into_body())).into_response()
}

fn versioned_api_path(path: &str) -> String {
    path.strip_prefix("/api")
        .map(|suffix| format!("/api/v2{suffix}"))
        .unwrap_or_else(|| path.to_string())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProtocolRouteInfo {
    pub operation: String,
    pub http_method: &'static str,
    pub path: &'static str,
    pub v2_path: String,
    pub registered: bool,
    pub unimplemented: bool,
    pub experimental: Option<&'static str>,
    pub transport_kind: String,
    pub migration_state: String,
    pub any_method: bool,
    pub websocket: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_handlers_cover_current_protocol_endpoints() {
        all_api_handlers()
            .validate()
            .expect("all protocol endpoints should have handlers or explicit markers");
    }

    #[test]
    fn protocol_endpoints_cover_all_registered_handlers() {
        let registry = all_api_handlers();
        for entry in registry.entries() {
            assert!(
                protocol_endpoint(entry.operation).is_some(),
                "registered handler has no protocol endpoint: {:?}",
                entry.operation
            );
        }
    }

    #[test]
    fn same_path_multiple_methods_are_aggregated() {
        let registry = all_api_handlers();
        let agents_entries: Vec<ApiMethod> = registry
            .entries()
            .iter()
            .filter_map(|entry| {
                let endpoint = protocol_endpoint(entry.operation)?;
                (endpoint.path == "/api/agents").then_some(entry.operation)
            })
            .collect();

        assert_eq!(
            agents_entries,
            vec![ApiMethod::AgentsList, ApiMethod::AgentsCreate]
        );

        let _router = register_protocol_routes(Router::new(), &registry);
    }

    #[test]
    fn versioned_api_path_adds_v2_prefix() {
        assert_eq!(versioned_api_path("/api/sessions"), "/api/v2/sessions");
        assert_eq!(versioned_api_path("/api/-/routes"), "/api/v2/-/routes");
    }

    #[test]
    fn transport_inventory_keeps_public_web_entries_visible() {
        let registry = all_api_handlers();
        let registered: HashSet<ApiMethod> = registry
            .entries()
            .iter()
            .map(|entry| entry.operation)
            .collect();

        for method in [
            ApiMethod::ProtocolRoutes,
            ApiMethod::Health,
            ApiMethod::SessionList,
            ApiMethod::Capabilities,
            ApiMethod::McpServersHealth,
            ApiMethod::McpServersProbe,
            ApiMethod::TerminalHealth,
            ApiMethod::TerminalProfiles,
            ApiMethod::TerminalSessionsList,
            ApiMethod::TerminalSessionWs,
            ApiMethod::TuiWs,
            ApiMethod::IpcWs,
        ] {
            assert!(registered.contains(&method), "{method:?} is not registered");
        }
    }

    #[test]
    fn rest_routes_have_v2_mirror_paths() {
        for endpoint in ALL_ENDPOINTS {
            if endpoint.path.starts_with("/api/") {
                assert!(
                    versioned_api_path(endpoint.path).starts_with("/api/v2/"),
                    "missing v2 mirror for {}",
                    endpoint.path
                );
            }
        }
    }

    #[tokio::test]
    async fn dedicated_transports_are_marked_as_websocket_routes() {
        let routes = protocol_routes_handler().await.0;

        for operation in ["TerminalSessionWs", "TuiWs", "IpcWs"] {
            let route = routes
                .iter()
                .find(|route| route.operation == operation)
                .unwrap_or_else(|| panic!("missing route inventory entry for {operation}"));
            assert!(route.websocket, "{operation} must remain websocket-marked");
            assert!(
                route.any_method,
                "{operation} must accept websocket upgrade"
            );
        }
    }

    #[tokio::test]
    async fn protocol_routes_include_operation_registry_metadata() {
        let routes = protocol_routes_handler().await.0;

        let capabilities = routes
            .iter()
            .find(|route| route.operation == "Capabilities")
            .expect("capabilities route");
        assert_eq!(capabilities.transport_kind, "JsonRpcWebSocket");
        assert_eq!(capabilities.migration_state, "Dispatched");

        let terminal_ws = routes
            .iter()
            .find(|route| route.operation == "TerminalSessionWs")
            .expect("terminal websocket route");
        assert_eq!(terminal_ws.transport_kind, "DedicatedWebSocket");
        assert_eq!(terminal_ws.migration_state, "DedicatedTransport");

        let ipc_ws = routes
            .iter()
            .find(|route| route.operation == "IpcWs")
            .expect("ipc websocket route");
        assert_eq!(ipc_ws.transport_kind, "IpcBridge");
        assert_eq!(ipc_ws.migration_state, "LegacyIpcBridge");
    }

    #[test]
    fn non_web_transports_are_not_registered_as_web_api_routes() {
        let paths: HashSet<&str> = ALL_ENDPOINTS.iter().map(|endpoint| endpoint.path).collect();

        for path in ["/events", "/api/mcp", "/chrome/native-host"] {
            assert!(
                !paths.contains(path),
                "{path} belongs to daemon, MCP, or browser transport boundaries"
            );
        }
    }
}
