use allthecodes_protocol::{ApiMethod, ApiOperationMetadata, API_METADATA};

use crate::api_dispatcher::{ApiDispatcherMigrationState, ApiTransportKind};

static API_OPERATION_REGISTRY: ApiOperationRegistry = ApiOperationRegistry;

#[derive(Debug, Clone, Copy)]
pub(crate) struct ApiOperationDescriptor {
    pub operation: ApiMethod,
    pub metadata: &'static ApiOperationMetadata,
    pub transport_kind: ApiTransportKind,
    pub migration_state: ApiDispatcherMigrationState,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ApiOperationRegistry;

impl ApiOperationRegistry {
    pub fn len(&self) -> usize {
        API_METADATA.len()
    }

    pub fn get(&self, operation: ApiMethod) -> Option<ApiOperationDescriptor> {
        let metadata = self.metadata(operation)?;
        let migration_state = migration_state_for(operation);
        Some(ApiOperationDescriptor {
            operation,
            metadata,
            transport_kind: transport_kind_for(metadata, migration_state),
            migration_state,
        })
    }

    pub fn migration_state(&self, operation: ApiMethod) -> ApiDispatcherMigrationState {
        self.get(operation)
            .map(|descriptor| descriptor.migration_state)
            .unwrap_or(ApiDispatcherMigrationState::LegacyRestHandler)
    }

    pub fn experimental_reason(&self, operation: ApiMethod) -> Option<&'static str> {
        self.metadata(operation)
            .and_then(|metadata| metadata.experimental)
    }

    pub fn metadata(&self, operation: ApiMethod) -> Option<&'static ApiOperationMetadata> {
        API_METADATA
            .iter()
            .find(|metadata| metadata.endpoint.operation == operation)
    }
}

pub(crate) fn api_operation_registry() -> &'static ApiOperationRegistry {
    &API_OPERATION_REGISTRY
}

pub(crate) const DISPATCHED_OPERATIONS: &[ApiMethod] = &[
    ApiMethod::Health,
    ApiMethod::Capabilities,
    ApiMethod::SessionList,
    ApiMethod::SessionSearch,
    ApiMethod::SessionCreate,
    ApiMethod::SessionDetail,
    ApiMethod::SessionReport,
    ApiMethod::SessionResume,
    ApiMethod::SessionArchive,
    ApiMethod::SessionModePatch,
    ApiMethod::SessionMessageBranch,
    ApiMethod::SessionMessageFeedback,
    ApiMethod::SessionMessageDelete,
    ApiMethod::SessionMessageRegeneratePrepare,
    ApiMethod::SessionMessageEditPrepare,
    ApiMethod::SessionMessageRollbackPreview,
    ApiMethod::SessionMessageRollback,
    ApiMethod::FilesTree,
    ApiMethod::FilesStat,
    ApiMethod::FilesRead,
    ApiMethod::FilesPreview,
    ApiMethod::FilesWrite,
    ApiMethod::FilesUpload,
    ApiMethod::FilesMkdir,
    ApiMethod::FilesRename,
    ApiMethod::FilesCopy,
    ApiMethod::FilesMove,
    ApiMethod::FilesDelete,
    ApiMethod::DiscoverySearch,
    ApiMethod::SkillsList,
    ApiMethod::SkillProposalsList,
    ApiMethod::SkillProposalDetail,
    ApiMethod::SkillProposalDiff,
    ApiMethod::SkillProposalApprove,
    ApiMethod::SkillProposalReject,
    ApiMethod::ChatModesList,
    ApiMethod::ChatModesResources,
    ApiMethod::PeopleList,
    ApiMethod::PeopleCreate,
    ApiMethod::PromptsList,
    ApiMethod::PromptsCreate,
    ApiMethod::KanbanBoards,
    ApiMethod::KanbanTaskCreate,
    ApiMethod::WorkflowDefinitionsList,
    ApiMethod::WorkflowDefinitionDetail,
    ApiMethod::WorkflowRunsList,
    ApiMethod::WorkflowRunStart,
    ApiMethod::WorkflowRunStatus,
    ApiMethod::WorkflowRunAdvance,
    ApiMethod::WorkflowRunCancel,
    ApiMethod::PluginsList,
    ApiMethod::PluginsInstalled,
    ApiMethod::PluginsMarketplace,
    ApiMethod::PluginsInstall,
    ApiMethod::PluginsUpdate,
    ApiMethod::PluginsUninstallById,
    ApiMethod::PluginsEnable,
    ApiMethod::PluginsDisable,
    ApiMethod::PluginsRestart,
    ApiMethod::GatewayStatus,
    ApiMethod::GatewaysList,
    ApiMethod::ModelsList,
    ApiMethod::AgentRuntimeDashboard,
    ApiMethod::WorktreeSessionsList,
    ApiMethod::WorktreeSessionsCurrent,
    ApiMethod::WorktreeSessionsBySession,
];

fn migration_state_for(operation: ApiMethod) -> ApiDispatcherMigrationState {
    if DISPATCHED_OPERATIONS.contains(&operation) {
        ApiDispatcherMigrationState::Dispatched
    } else if is_dedicated_transport_operation(operation) {
        ApiDispatcherMigrationState::DedicatedTransport
    } else if operation == ApiMethod::IpcWs {
        ApiDispatcherMigrationState::LegacyIpcBridge
    } else {
        ApiDispatcherMigrationState::LegacyRestHandler
    }
}

fn transport_kind_for(
    metadata: &'static ApiOperationMetadata,
    migration_state: ApiDispatcherMigrationState,
) -> ApiTransportKind {
    match migration_state {
        ApiDispatcherMigrationState::Dispatched => ApiTransportKind::JsonRpcWebSocket,
        ApiDispatcherMigrationState::LegacyIpcBridge => ApiTransportKind::IpcBridge,
        ApiDispatcherMigrationState::DedicatedTransport
            if is_websocket_endpoint(metadata.endpoint.http_method, metadata.endpoint.path) =>
        {
            ApiTransportKind::DedicatedWebSocket
        }
        ApiDispatcherMigrationState::DedicatedTransport
        | ApiDispatcherMigrationState::LegacyRestHandler => ApiTransportKind::Rest,
    }
}

fn is_websocket_endpoint(http_method: &str, path: &str) -> bool {
    http_method == "ANY" || path.ends_with("/ws")
}

fn is_dedicated_transport_operation(operation: ApiMethod) -> bool {
    matches!(
        operation,
        ApiMethod::TerminalHealth
            | ApiMethod::TerminalProfiles
            | ApiMethod::TerminalSessionsList
            | ApiMethod::TerminalSessionsCreate
            | ApiMethod::TerminalSessionDetail
            | ApiMethod::TerminalSessionOutput
            | ApiMethod::TerminalSessionDelete
            | ApiMethod::TerminalSessionWs
            | ApiMethod::TuiWs
            | ApiMethod::FilesDownload
            | ApiMethod::FilesMedia
    )
}
