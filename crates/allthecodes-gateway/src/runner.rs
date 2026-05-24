use crate::{
    BusyDecision, BusySnapshot, CallbackDeliverySink, ChannelDeliverySink, DeliveryRecord,
    DeliveryRouter, GatewayDiagnostic, GatewayError, GatewayPolicy, GatewayStore, RunEvent,
    RunEventKind, RunMeta, RunRequest, RunStatus, SessionKeyPolicy,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

/// Gateway-neutral command names for the daemon integration layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GatewayCommandKind {
    Submit,
    Abort,
    PermissionResponse,
    AskUserResponse,
}

/// Command envelope emitted by the gateway runner.
///
/// The gateway crate deliberately keeps this daemon-neutral. The daemon bridge
/// is responsible for mapping the envelope onto worker ids and local protocol
/// files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayCommand {
    pub kind: GatewayCommandKind,
    pub run_id: String,
    pub session_key: String,
    pub payload: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
}

impl GatewayCommand {
    pub fn submit(meta: &RunMeta, prompt: &str) -> Self {
        Self {
            kind: GatewayCommandKind::Submit,
            run_id: meta.run_id.to_string(),
            session_key: meta.session_key.to_string(),
            payload: json!({ "text": prompt }),
            idempotency_key: meta.request.idempotency_key.clone(),
        }
    }

    pub fn abort(meta: &RunMeta, reason: &str) -> Self {
        Self {
            kind: GatewayCommandKind::Abort,
            run_id: meta.run_id.to_string(),
            session_key: meta.session_key.to_string(),
            payload: json!({ "reason": reason }),
            idempotency_key: None,
        }
    }

    pub fn permission_response(
        meta: &RunMeta,
        tool_use_id: &str,
        approved: bool,
        reason: Option<&str>,
    ) -> Self {
        Self {
            kind: GatewayCommandKind::PermissionResponse,
            run_id: meta.run_id.to_string(),
            session_key: meta.session_key.to_string(),
            payload: json!({
                "toolUseId": tool_use_id,
                "approved": approved,
                "reason": reason,
            }),
            idempotency_key: None,
        }
    }

    pub fn ask_user_response(meta: &RunMeta, question_id: &str, response: &str) -> Self {
        Self {
            kind: GatewayCommandKind::AskUserResponse,
            run_id: meta.run_id.to_string(),
            session_key: meta.session_key.to_string(),
            payload: json!({
                "questionId": question_id,
                "response": response,
            }),
            idempotency_key: None,
        }
    }
}

/// A daemon or test bridge that can accept gateway commands.
pub trait GatewayCommandSink {
    fn dispatch(&self, command: GatewayCommand) -> Result<GatewayCommandReceipt, GatewayError>;
}

impl<T> GatewayCommandSink for Arc<T>
where
    T: GatewayCommandSink + ?Sized,
{
    fn dispatch(&self, command: GatewayCommand) -> Result<GatewayCommandReceipt, GatewayError> {
        self.as_ref().dispatch(command)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayCommandReceipt {
    pub command_id: String,
    pub target: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GatewayRunAction {
    Started,
    Queued,
    Existing,
    Rejected,
    Interrupted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayRunSubmission {
    pub meta: RunMeta,
    pub action: GatewayRunAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<GatewayCommandReceipt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostic: Option<GatewayDiagnostic>,
}

#[derive(Debug, Clone)]
pub struct GatewayRunner {
    store: GatewayStore,
    policy: GatewayPolicy,
}

impl GatewayRunner {
    pub fn new(store: GatewayStore, policy: GatewayPolicy) -> Self {
        Self { store, policy }
    }

    pub fn default_with_policy(session_policy: SessionKeyPolicy, policy: GatewayPolicy) -> Self {
        Self::new(GatewayStore::default_with_policy(session_policy), policy)
    }

    pub fn submit_run(
        &self,
        request: RunRequest,
        snapshot: BusySnapshot,
        sink: &(impl GatewayCommandSink + ?Sized),
    ) -> Result<GatewayRunSubmission, GatewayError> {
        let outcome = self.store.create_run(request)?;
        let meta = outcome.meta().clone();
        if matches!(outcome, crate::CreateRunOutcome::Existing(_)) {
            return Ok(GatewayRunSubmission {
                meta,
                action: GatewayRunAction::Existing,
                command: None,
                diagnostic: None,
            });
        }

        match self
            .policy
            .evaluate_busy(meta.request.policy.busy, snapshot)?
        {
            BusyDecision::StartNow => self.start_run(meta, sink),
            BusyDecision::Queue => Ok(GatewayRunSubmission {
                meta,
                action: GatewayRunAction::Queued,
                command: None,
                diagnostic: None,
            }),
            BusyDecision::Interrupt => self.interrupt_then_start(meta, sink),
            BusyDecision::Reject(diagnostic) => {
                let meta = self.fail_created_run(meta, diagnostic.clone())?;
                Ok(GatewayRunSubmission {
                    meta,
                    action: GatewayRunAction::Rejected,
                    command: None,
                    diagnostic: Some(diagnostic),
                })
            }
        }
    }

    pub fn stop_run(
        &self,
        run_id: &crate::RunId,
        sink: &(impl GatewayCommandSink + ?Sized),
    ) -> Result<GatewayRunSubmission, GatewayError> {
        let meta = self.store.load_run(run_id)?;
        if meta.status.is_terminal() {
            let diagnostic = GatewayDiagnostic::new(
                "run_already_terminal",
                "The requested run is already terminal.",
                "Reload the run before sending another stop request.",
            )
            .with_context(format!("run_id={}", meta.run_id));
            return Ok(GatewayRunSubmission {
                meta,
                action: GatewayRunAction::Rejected,
                command: None,
                diagnostic: Some(diagnostic),
            });
        }

        let receipt = sink.dispatch(GatewayCommand::abort(&meta, "gateway stop request"))?;
        let meta = self.store.update_status(run_id, RunStatus::Cancelled)?;
        Ok(GatewayRunSubmission {
            meta,
            action: GatewayRunAction::Interrupted,
            command: Some(receipt),
            diagnostic: None,
        })
    }

    pub fn approve_run(
        &self,
        run_id: &crate::RunId,
        tool_use_id: &str,
        approved: bool,
        reason: Option<&str>,
        sink: &(impl GatewayCommandSink + ?Sized),
    ) -> Result<GatewayRunSubmission, GatewayError> {
        let meta = self.store.load_run(run_id)?;
        if meta.status != RunStatus::WaitingApproval
            || !self.has_pending_approval(run_id, tool_use_id)?
        {
            return Ok(stale_response(meta, run_id));
        }

        let receipt = sink.dispatch(GatewayCommand::permission_response(
            &meta,
            tool_use_id,
            approved,
            reason,
        ))?;
        let sequence = self.store.read_events(run_id)?.len() as u64 + 1;
        self.store.append_event(&RunEvent::new(
            run_id.clone(),
            sequence,
            RunEventKind::Custom {
                name: "approval_response".to_string(),
                payload: json!({
                    "toolUseId": tool_use_id,
                    "approved": approved,
                }),
            },
        ))?;
        let meta = self.store.update_status(run_id, RunStatus::Running)?;
        Ok(GatewayRunSubmission {
            meta,
            action: GatewayRunAction::Started,
            command: Some(receipt),
            diagnostic: None,
        })
    }

    pub fn answer_user(
        &self,
        run_id: &crate::RunId,
        question_id: &str,
        response: &str,
        sink: &(impl GatewayCommandSink + ?Sized),
    ) -> Result<GatewayRunSubmission, GatewayError> {
        let meta = self.store.load_run(run_id)?;
        if meta.status != RunStatus::WaitingUser
            || !self.has_pending_question(run_id, question_id)?
        {
            return Ok(stale_response(meta, run_id));
        }

        let receipt = sink.dispatch(GatewayCommand::ask_user_response(
            &meta,
            question_id,
            response,
        ))?;
        let sequence = self.store.read_events(run_id)?.len() as u64 + 1;
        self.store.append_event(&RunEvent::new(
            run_id.clone(),
            sequence,
            RunEventKind::Custom {
                name: "ask_user_response".to_string(),
                payload: json!({
                    "questionId": question_id,
                }),
            },
        ))?;
        let meta = self.store.update_status(run_id, RunStatus::Running)?;
        Ok(GatewayRunSubmission {
            meta,
            action: GatewayRunAction::Started,
            command: Some(receipt),
            diagnostic: None,
        })
    }

    pub fn deliver_event(
        &self,
        run_id: &crate::RunId,
        event: &RunEvent,
        callback_sink: &impl CallbackDeliverySink,
        channel_sink: &impl ChannelDeliverySink,
    ) -> Result<Vec<DeliveryRecord>, GatewayError> {
        let meta = self.store.load_run(run_id)?;
        let records = DeliveryRouter::default().deliver_event(
            &self.store,
            &meta,
            event,
            callback_sink,
            channel_sink,
        )?;
        for diagnostic in records
            .iter()
            .filter_map(|record| record.diagnostic.clone())
        {
            self.store
                .append_event(&crate::delivery::delivery_failed_event(
                    run_id.clone(),
                    self.store.read_events(run_id)?.len() as u64 + 1,
                    diagnostic,
                ))?;
        }
        Ok(records)
    }

    pub fn recover_on_startup(
        &self,
        pending_timeout: Duration,
    ) -> Result<crate::GatewayRecoveryReport, GatewayError> {
        self.store.recover_on_startup(pending_timeout)
    }

    pub fn start_next_queued(
        &self,
        snapshot: BusySnapshot,
        sink: &(impl GatewayCommandSink + ?Sized),
    ) -> Result<Option<GatewayRunSubmission>, GatewayError> {
        if snapshot.running >= self.policy.max_running {
            return Ok(None);
        }
        let Some(meta) = self.store.queued_runs()?.into_iter().next() else {
            return Ok(None);
        };
        self.start_run(meta, sink).map(Some)
    }

    fn start_run(
        &self,
        meta: RunMeta,
        sink: &(impl GatewayCommandSink + ?Sized),
    ) -> Result<GatewayRunSubmission, GatewayError> {
        let prompt = meta.request.prompt.clone();
        let receipt = sink.dispatch(GatewayCommand::submit(&meta, &prompt))?;
        let meta = self.store.update_status(&meta.run_id, RunStatus::Running)?;
        Ok(GatewayRunSubmission {
            meta,
            action: GatewayRunAction::Started,
            command: Some(receipt),
            diagnostic: None,
        })
    }

    fn interrupt_then_start(
        &self,
        meta: RunMeta,
        sink: &(impl GatewayCommandSink + ?Sized),
    ) -> Result<GatewayRunSubmission, GatewayError> {
        let _ = sink.dispatch(GatewayCommand::abort(&meta, "gateway interrupt policy"))?;
        let prompt = meta.request.prompt.clone();
        let receipt = sink.dispatch(GatewayCommand::submit(&meta, &prompt))?;
        let meta = self.store.update_status(&meta.run_id, RunStatus::Running)?;
        Ok(GatewayRunSubmission {
            meta,
            action: GatewayRunAction::Interrupted,
            command: Some(receipt),
            diagnostic: None,
        })
    }

    fn fail_created_run(
        &self,
        meta: RunMeta,
        diagnostic: GatewayDiagnostic,
    ) -> Result<RunMeta, GatewayError> {
        self.store.append_event(&RunEvent::new(
            meta.run_id.clone(),
            self.store.read_events(&meta.run_id)?.len() as u64 + 1,
            RunEventKind::Diagnostic { diagnostic },
        ))?;
        self.store.update_status(&meta.run_id, RunStatus::Failed)
    }

    fn has_pending_approval(
        &self,
        run_id: &crate::RunId,
        tool_use_id: &str,
    ) -> Result<bool, GatewayError> {
        Ok(self.store.read_events(run_id)?.iter().rev().any(|event| {
            matches!(
                &event.kind,
                RunEventKind::ApprovalRequested { tool_use_id: pending } if pending == tool_use_id
            )
        }))
    }

    fn has_pending_question(
        &self,
        run_id: &crate::RunId,
        question_id: &str,
    ) -> Result<bool, GatewayError> {
        Ok(self.store.read_events(run_id)?.iter().rev().any(|event| {
            matches!(
                &event.kind,
                RunEventKind::AskUserRequested { question_id: pending } if pending == question_id
            )
        }))
    }
}

fn stale_response(meta: RunMeta, run_id: &crate::RunId) -> GatewayRunSubmission {
    GatewayRunSubmission {
        meta,
        action: GatewayRunAction::Rejected,
        command: None,
        diagnostic: Some(GatewayPolicy::stale_response(run_id)),
    }
}
