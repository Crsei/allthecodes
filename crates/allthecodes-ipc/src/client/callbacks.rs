//! Permission, question, and tool-progress callback builders.

use std::collections::HashMap;
use std::sync::Arc;

use allthecodes_ipc_protocol::BackendMessage;
pub use allthecodes_types::callbacks::CallbackHost;
use allthecodes_types::callbacks::{
    AskUserCallback, AskUserRequestPayload, PermissionCallback, PermissionEventCallback,
    PermissionEventPayload, PermissionRequestPayload, PermissionResponsePayload, ToolProgress,
};
use parking_lot::Mutex;
use tokio::sync::oneshot;

use crate::transport::FrontendSink;

const DEFAULT_RESPONSE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

struct PendingPermissionCleanup {
    pending: PendingPermissions,
    id: String,
}

impl Drop for PendingPermissionCleanup {
    fn drop(&mut self) {
        self.pending.lock().remove(&self.id);
    }
}

struct PendingQuestionCleanup {
    pending: PendingQuestions,
    id: String,
}

impl Drop for PendingQuestionCleanup {
    fn drop(&mut self) {
        self.pending.lock().remove(&self.id);
    }
}

fn register_permission_then_send(
    pending: PendingPermissions,
    id: String,
    send: impl FnOnce() -> std::io::Result<()>,
) -> std::io::Result<(
    oneshot::Receiver<PermissionResponsePayload>,
    PendingPermissionCleanup,
)> {
    let (tx, rx) = oneshot::channel();
    pending.lock().insert(id.clone(), tx);
    let cleanup = PendingPermissionCleanup { pending, id };
    send()?;
    Ok((rx, cleanup))
}

fn register_question_then_send(
    pending: PendingQuestions,
    id: String,
    send: impl FnOnce() -> std::io::Result<()>,
) -> std::io::Result<(oneshot::Receiver<String>, PendingQuestionCleanup)> {
    let (tx, rx) = oneshot::channel();
    pending.lock().insert(id.clone(), tx);
    let cleanup = PendingQuestionCleanup { pending, id };
    send()?;
    Ok((rx, cleanup))
}

/// Pending permission requests awaiting a response from the frontend.
pub type PendingPermissions =
    Arc<Mutex<HashMap<String, oneshot::Sender<PermissionResponsePayload>>>>;
/// Pending AskUserQuestion requests awaiting a response from the frontend.
pub type PendingQuestions = Arc<Mutex<HashMap<String, oneshot::Sender<String>>>>;

/// Optional host hook invoked after the user rejects ExitPlanMode approval.
pub type ExitPlanRejectedHook<H> = Arc<dyn Fn(&H, &FrontendSink) + Send + Sync>;

pub fn install_permission_callback<H>(
    host: &Arc<H>,
    pending: PendingPermissions,
    sink: FrontendSink,
    exit_plan_rejected: Option<ExitPlanRejectedHook<H>>,
) where
    H: CallbackHost + Send + Sync + 'static,
{
    install_permission_callback_with_timeout(
        host,
        pending,
        sink,
        exit_plan_rejected,
        DEFAULT_RESPONSE_TIMEOUT,
    );
}

fn install_permission_callback_with_timeout<H>(
    host: &Arc<H>,
    pending: PendingPermissions,
    sink: FrontendSink,
    exit_plan_rejected: Option<ExitPlanRejectedHook<H>>,
    response_timeout: std::time::Duration,
) where
    H: CallbackHost + Send + Sync + 'static,
{
    let host_handle = host.clone();
    let callback: PermissionCallback = Arc::new(move |request: PermissionRequestPayload| {
        let pending = pending.clone();
        let sink = sink.clone();
        let host = host_handle.clone();
        let exit_plan_rejected = exit_plan_rejected.clone();
        Box::pin(async move {
            let tool_use_id = request.tool_use_id.clone();
            let tool_name = request.tool_name.clone();
            let tool_input = request.tool_input.clone();
            let operation = request.operation.clone().unwrap_or_else(|| {
                allthecodes_tool_display::ToolClassifier::classify_permission(
                    &tool_name,
                    &tool_input,
                    Some(&request.message),
                    allthecodes_types::tool_operation::OperationStatus::InProgress,
                )
            });
            let registered =
                register_permission_then_send(pending.clone(), tool_use_id.clone(), || {
                    sink.send(&BackendMessage::PermissionRequest {
                        tool_use_id: tool_use_id.clone(),
                        tool: tool_name.clone(),
                        command: request.legacy_command(),
                        input: tool_input,
                        options: request.options,
                        operation: Some(operation),
                        security: request.security,
                    })
                });
            let Ok((rx, _cleanup)) = registered else {
                return PermissionResponsePayload::deny();
            };

            match tokio::time::timeout(response_timeout, rx).await {
                Ok(Ok(decision)) => {
                    if tool_name == "ExitPlanMode"
                        && matches!(
                            decision.normalized_decision().as_str(),
                            "deny" | "reject" | "no"
                        )
                    {
                        if let Some(hook) = exit_plan_rejected.as_ref() {
                            hook(&host, &sink);
                        }
                    }
                    decision
                }
                Ok(Err(_)) | Err(_) => PermissionResponsePayload::deny(),
            }
        })
    });
    host.set_permission_callback(callback);
}

pub fn install_tool_progress_callback<H>(host: &H, sink: FrontendSink)
where
    H: CallbackHost + Send + Sync + 'static,
{
    let callback = Arc::new(move |progress: ToolProgress| {
        let data = &progress.data;
        let tool = data
            .get("tool")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let output = data
            .get("output")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let elapsed_seconds = data
            .get("elapsed_seconds")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let total_lines = data.get("total_lines").and_then(|v| v.as_u64());
        let total_bytes = data.get("total_bytes").and_then(|v| v.as_u64());
        let timeout_ms = data.get("timeout_ms").and_then(|v| v.as_u64());

        let _ = sink.send(&BackendMessage::ToolProgress {
            tool_use_id: progress.tool_use_id,
            tool,
            output,
            elapsed_seconds,
            total_lines,
            total_bytes,
            timeout_ms,
            operation: None,
        });
    });
    host.set_tool_progress_callback(callback);
}

pub fn install_ask_user_callback<H>(host: &H, pending: PendingQuestions, sink: FrontendSink)
where
    H: CallbackHost + Send + Sync + 'static,
{
    let callback: AskUserCallback = Arc::new(move |request: AskUserRequestPayload| {
        let pending = pending.clone();
        let sink = sink.clone();
        Box::pin(async move {
            let question_id = uuid::Uuid::new_v4().to_string();
            let registered =
                register_question_then_send(pending.clone(), question_id.clone(), || {
                    sink.send(&BackendMessage::QuestionRequest {
                        id: question_id.clone(),
                        text: request.question,
                        choices: request.choices,
                        allow_free_text: request.allow_free_text,
                    })
                });
            let Ok((rx, _cleanup)) = registered else {
                return String::new();
            };

            tokio::time::timeout(DEFAULT_RESPONSE_TIMEOUT, rx)
                .await
                .ok()
                .and_then(Result::ok)
                .unwrap_or_default()
        })
    });
    host.set_ask_user_callback(callback);
}

pub fn install_permission_event_callback<H>(host: &H, sink: FrontendSink)
where
    H: CallbackHost + Send + Sync + 'static,
{
    let callback: PermissionEventCallback = Arc::new(move |event: PermissionEventPayload| {
        let _ = match event {
            PermissionEventPayload::HookDecision { event } => {
                sink.send(&BackendMessage::HookPermissionDecision { event })
            }
            PermissionEventPayload::DecisionDebug { event } => {
                sink.send(&BackendMessage::PermissionDecisionDebug { event })
            }
            PermissionEventPayload::AutoReview { event } => {
                sink.send(&BackendMessage::PermissionAutoReview { event })
            }
        };
    });
    host.set_permission_event_callback(callback);
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_types::callbacks::ToolProgress;
    use allthecodes_types::tool_operation::OperationKind;
    use parking_lot::Mutex;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicBool, Ordering};

    type ToolProgressCallback = Arc<dyn Fn(ToolProgress) + Send + Sync>;

    #[derive(Default)]
    struct MockHost {
        permission: Mutex<Option<PermissionCallback>>,
        ask_user: Mutex<Option<AskUserCallback>>,
        tool_progress: Mutex<Option<ToolProgressCallback>>,
    }

    impl CallbackHost for MockHost {
        fn set_permission_callback(&self, cb: PermissionCallback) {
            *self.permission.lock() = Some(cb);
        }

        fn set_ask_user_callback(&self, cb: AskUserCallback) {
            *self.ask_user.lock() = Some(cb);
        }

        fn set_permission_event_callback(&self, _cb: PermissionEventCallback) {}

        fn set_tool_progress_callback(&self, cb: Arc<dyn Fn(ToolProgress) + Send + Sync>) {
            *self.tool_progress.lock() = Some(cb);
        }
    }

    #[tokio::test]
    async fn permission_callback_sends_request_and_resolves_decision() {
        let host = Arc::new(MockHost::default());
        let pending: PendingPermissions = Arc::new(Mutex::new(HashMap::new()));
        let sink = FrontendSink::memory();

        install_permission_callback(&host, pending.clone(), sink.clone(), None);

        let callback = host
            .permission
            .lock()
            .clone()
            .expect("permission callback installed");
        let task = tokio::spawn(callback(PermissionRequestPayload {
            tool_use_id: "tool-1".to_string(),
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({"command":"echo hi"}),
            message: "echo hi".to_string(),
            options: vec!["allow".to_string(), "deny".to_string()],
            operation: None,
            security: None,
        }));

        wait_until(|| pending.lock().contains_key("tool-1")).await;

        let captured = sink.captured();
        let BackendMessage::PermissionRequest {
            tool_use_id,
            tool,
            command,
            input,
            options,
            operation: Some(operation),
            ..
        } = &captured[0]
        else {
            panic!("expected permission request with operation metadata");
        };
        assert_eq!(tool_use_id, "tool-1");
        assert_eq!(tool, "Bash");
        assert_eq!(command, "Bash: echo hi");
        assert_eq!(input, &serde_json::json!({"command":"echo hi"}));
        assert_eq!(options, &vec!["allow".to_string(), "deny".to_string()]);
        assert_eq!(operation.kind, OperationKind::Permission);

        let tx = pending.lock().remove("tool-1").expect("pending sender");
        tx.send(PermissionResponsePayload::decision("allow"))
            .unwrap();
        assert_eq!(
            task.await.unwrap(),
            PermissionResponsePayload::decision("allow")
        );
    }

    #[tokio::test]
    async fn exit_plan_rejection_invokes_host_hook() {
        let host = Arc::new(MockHost::default());
        let pending: PendingPermissions = Arc::new(Mutex::new(HashMap::new()));
        let sink = FrontendSink::memory();
        let rejected = Arc::new(AtomicBool::new(false));
        let rejected_hook = {
            let rejected = rejected.clone();
            Arc::new(move |_host: &MockHost, _sink: &FrontendSink| {
                rejected.store(true, Ordering::SeqCst);
            })
        };

        install_permission_callback(&host, pending.clone(), sink, Some(rejected_hook));

        let callback = host
            .permission
            .lock()
            .clone()
            .expect("permission callback installed");
        let task = tokio::spawn(callback(PermissionRequestPayload {
            tool_use_id: "exit-plan".to_string(),
            tool_name: "ExitPlanMode".to_string(),
            tool_input: serde_json::json!({"plan":"approve plan"}),
            message: "approve plan".to_string(),
            options: vec!["allow".to_string(), "deny".to_string()],
            operation: None,
            security: None,
        }));

        wait_until(|| pending.lock().contains_key("exit-plan")).await;

        let tx = pending.lock().remove("exit-plan").expect("pending sender");
        tx.send(PermissionResponsePayload::decision("deny"))
            .unwrap();
        assert_eq!(
            task.await.unwrap(),
            PermissionResponsePayload::decision("deny")
        );
        assert!(rejected.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn ask_user_callback_sends_question_and_resolves_response() {
        let host = MockHost::default();
        let pending: PendingQuestions = Arc::new(Mutex::new(HashMap::new()));
        let sink = FrontendSink::memory();

        install_ask_user_callback(&host, pending.clone(), sink.clone());

        let callback = host
            .ask_user
            .lock()
            .clone()
            .expect("ask-user callback installed");
        let task = tokio::spawn(callback(AskUserRequestPayload {
            question: "Continue?".to_string(),
            choices: vec![],
            allow_free_text: true,
        }));

        wait_until(|| !pending.lock().is_empty()).await;

        let captured = sink.captured();
        let BackendMessage::QuestionRequest {
            id,
            text,
            choices,
            allow_free_text,
        } = &captured[0]
        else {
            panic!("expected question request");
        };
        assert_eq!(text, "Continue?");
        assert!(choices.is_empty());
        assert!(*allow_free_text);

        let tx = pending.lock().remove(id).expect("pending sender");
        tx.send("yes".to_string()).unwrap();
        assert_eq!(task.await.unwrap(), "yes");
    }

    #[tokio::test]
    async fn unanswered_permission_times_out_and_cleans_pending() {
        let host = Arc::new(MockHost::default());
        let pending: PendingPermissions = Arc::new(Mutex::new(HashMap::new()));
        install_permission_callback_with_timeout(
            &host,
            pending.clone(),
            FrontendSink::memory(),
            None,
            std::time::Duration::from_millis(20),
        );
        let callback = host.permission.lock().clone().unwrap();
        let task = tokio::spawn(callback(PermissionRequestPayload {
            tool_use_id: "tool-timeout".into(),
            tool_name: "Bash".into(),
            tool_input: serde_json::json!({}),
            message: "wait".into(),
            options: vec![],
            operation: None,
            security: None,
        }));
        tokio::task::yield_now().await;
        assert!(pending.lock().contains_key("tool-timeout"));
        assert_eq!(task.await.unwrap(), PermissionResponsePayload::deny());
        assert!(pending.lock().is_empty());
    }

    #[tokio::test]
    async fn cancelling_question_wait_cleans_pending() {
        let host = MockHost::default();
        let pending: PendingQuestions = Arc::new(Mutex::new(HashMap::new()));
        install_ask_user_callback(&host, pending.clone(), FrontendSink::memory());
        let callback = host.ask_user.lock().clone().unwrap();
        let task = tokio::spawn(callback(AskUserRequestPayload {
            question: "cancel?".into(),
            choices: vec![],
            allow_free_text: true,
        }));
        wait_until(|| !pending.lock().is_empty()).await;
        task.abort();
        let _ = task.await;
        assert!(pending.lock().is_empty());
    }

    #[tokio::test]
    async fn permission_is_pending_before_send_and_fast_response_is_not_lost() {
        let pending: PendingPermissions = Arc::new(Mutex::new(HashMap::new()));
        let responder_pending = pending.clone();
        let (rx, _cleanup) =
            register_permission_then_send(pending.clone(), "fast".into(), move || {
                let tx = responder_pending
                    .lock()
                    .remove("fast")
                    .expect("registered before send");
                tx.send(PermissionResponsePayload::decision("allow"))
                    .unwrap();
                Ok(())
            })
            .unwrap();

        assert_eq!(
            rx.await.unwrap(),
            PermissionResponsePayload::decision("allow")
        );
        assert!(pending.lock().is_empty());
    }

    #[test]
    fn question_send_failure_cleans_pending() {
        let pending: PendingQuestions = Arc::new(Mutex::new(HashMap::new()));
        let result = register_question_then_send(pending.clone(), "failed".into(), || {
            Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "closed",
            ))
        });

        assert!(result.is_err());
        assert!(pending.lock().is_empty());
    }

    #[test]
    fn tool_progress_callback_maps_payload_fields() {
        let host = MockHost::default();
        let sink = FrontendSink::memory();

        install_tool_progress_callback(&host, sink.clone());

        let callback = host
            .tool_progress
            .lock()
            .clone()
            .expect("tool-progress callback installed");
        callback(ToolProgress {
            tool_use_id: "tool-1".to_string(),
            data: serde_json::json!({
                "tool": "Bash",
                "output": "line",
                "elapsed_seconds": 3,
                "total_lines": 7,
                "total_bytes": 12,
                "timeout_ms": 5000
            }),
        });

        assert!(matches!(
            &sink.captured()[0],
            BackendMessage::ToolProgress {
                tool_use_id,
                tool,
                output,
                elapsed_seconds,
                total_lines,
                total_bytes,
                timeout_ms,
                ..
            } if tool_use_id == "tool-1"
                && tool == "Bash"
                && output == "line"
                && *elapsed_seconds == 3
                && *total_lines == Some(7)
                && *total_bytes == Some(12)
                && *timeout_ms == Some(5000)
        ));
    }

    async fn wait_until(mut predicate: impl FnMut() -> bool) {
        for _ in 0..50 {
            if predicate() {
                return;
            }
            tokio::task::yield_now().await;
        }
        panic!("condition was not met");
    }
}
