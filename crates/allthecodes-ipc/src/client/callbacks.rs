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
    let host_handle = host.clone();
    let callback: PermissionCallback = Arc::new(move |request: PermissionRequestPayload| {
        let pending = pending.clone();
        let sink = sink.clone();
        let host = host_handle.clone();
        let exit_plan_rejected = exit_plan_rejected.clone();
        Box::pin(async move {
            let tool_use_id = request.tool_use_id.clone();
            let tool_name = request.tool_name.clone();
            let _ = sink.send(&BackendMessage::PermissionRequest {
                tool_use_id: tool_use_id.clone(),
                tool: tool_name.clone(),
                command: request.legacy_command(),
                input: request.tool_input,
                options: request.options,
            });

            let (tx, rx) = oneshot::channel();
            pending.lock().insert(tool_use_id, tx);

            match rx.await {
                Ok(decision) => {
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
                Err(_) => PermissionResponsePayload::deny(),
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
            let _ = sink.send(&BackendMessage::QuestionRequest {
                id: question_id.clone(),
                text: request.question,
                choices: request.choices,
                allow_free_text: request.allow_free_text,
            });

            let (tx, rx) = oneshot::channel();
            pending.lock().insert(question_id, tx);

            rx.await.unwrap_or_default()
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
        };
    });
    host.set_permission_event_callback(callback);
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_types::callbacks::ToolProgress;
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
        }));

        wait_until(|| pending.lock().contains_key("tool-1")).await;

        assert!(matches!(
            &sink.captured()[0],
            BackendMessage::PermissionRequest {
                tool_use_id,
                tool,
                command,
                input,
                options,
            } if tool_use_id == "tool-1"
                && tool == "Bash"
                && command == "Bash: echo hi"
                && input == &serde_json::json!({"command":"echo hi"})
                && options == &vec!["allow".to_string(), "deny".to_string()]
        ));

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
