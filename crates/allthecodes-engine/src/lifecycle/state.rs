use std::collections::HashSet;
use std::sync::Arc;

use allthecodes_types::sdk::{PermissionDenial, UsageTracking};

use crate::observability::AuditContext;
use crate::services::session_memory::SessionMemoryService;
use crate::types::app_state::AppState;
use crate::types::message::{Message, Usage};
use crate::types::tool::{
    AskUserCallback, FileStateCache, PermissionCallback, PermissionEventCallback, ToolProgress,
    Tools,
};

use super::{types, AutoReviewTracker};

pub(crate) type QueryEngineState = EngineSharedState;

pub(crate) struct TranscriptState {
    pub(crate) messages: Vec<Message>,
    pub(crate) usage: UsageTracking,
    pub(crate) total_turn_count: usize,
}

pub(crate) struct PermissionState {
    pub(crate) denials: Vec<PermissionDenial>,
    pub(crate) permission_callback: Option<PermissionCallback>,
    pub(crate) ask_user_callback: Option<AskUserCallback>,
    pub(crate) permission_event_callback: Option<PermissionEventCallback>,
    pub(crate) auto_denial_tracker: crate::permissions::decision::DenialTracker,
    pub(crate) auto_review_tracker: AutoReviewTracker,
}

pub(crate) struct ToolRuntimeState {
    pub(crate) registry: Tools,
    pub(crate) file_state_cache: FileStateCache,
    pub(crate) discovered_skill_names: HashSet<String>,
    pub(crate) loaded_nested_memory_paths: HashSet<String>,
}

pub(crate) struct SessionRuntimeState {
    pub(crate) abort_reason: Option<super::AbortReason>,
    pub(crate) goal_runtime: types::GoalRuntimeState,
    pub(crate) bg_agent_tx: Option<allthecodes_types::agent_channel::AgentSender>,
    pub(crate) tool_progress_callback: Option<Arc<dyn Fn(ToolProgress) + Send + Sync>>,
    pub(crate) sleep_until: Option<std::time::Instant>,
    pub(crate) session_memory: SessionMemoryService,
    pub(crate) audit_ctx: AuditContext,
    pub(crate) taint_ledger: crate::security::TaintLedger,
}

/// All mutable session state behind a single `Arc<RwLock<_>>`.
///
/// The outer lock stays singular while fields are grouped by domain so callers
/// borrow the part of the engine state they are actually manipulating.
pub(crate) struct EngineSharedState {
    pub(crate) transcript: TranscriptState,
    pub(crate) permissions: PermissionState,
    pub(crate) tools: ToolRuntimeState,
    pub(crate) runtime: SessionRuntimeState,
    pub(crate) app_state: AppState,
}

impl EngineSharedState {
    pub(crate) fn append_message(&mut self, message: Message) {
        self.transcript.messages.push(message);
    }

    pub(crate) fn update_usage(&mut self, usage: &Usage, cost_usd: f64) {
        self.transcript.usage = self
            .transcript
            .usage
            .clone()
            .with_added_usage(usage, cost_usd);
        crate::bootstrap::PROCESS_STATE.write().total_cost_usd += cost_usd;
    }

    pub(crate) fn record_permission_denial(&mut self, denial: PermissionDenial) {
        self.permissions.denials.push(denial);
    }

    pub(crate) fn set_tools(&mut self, tools: Tools) {
        self.tools.registry = tools;
    }
}
