//! Remote-control gateway foundation.
//!
//! This crate owns remote source identity, deterministic session-key
//! derivation, and gateway-local configuration models. Runtime daemon,
//! TUI, IPC, and `QueryEngine` integration belong in `allthecodes`
//! adapter modules so dependency direction stays one-way.

pub mod adapters;
pub mod api;
mod api_support;
pub mod auth;
pub mod config;
pub mod delivery;
pub mod events;
pub mod policy;
pub mod run;
pub mod runner;
pub mod scheduled;
pub mod session_key;
pub mod source;
pub mod store;
pub mod webhook;
mod webhook_hmac;
mod webhook_render;

pub use adapters::{
    AdapterProvider, AdapterRegistry, AdapterState, AdapterStatus, AdapterTestMessage,
    RemoteAdapter,
};
pub use allthecodes_types::output::{
    EventSeq, OutputEvent, OutputLifecycleState, OutputReadBatch, OutputStream,
};
pub use api::{GatewayApiState, GatewayBusySnapshotProvider, StaticBusySnapshotProvider};
pub use auth::{GatewayAuthMode, GatewayAuthVerifier, RemoteGatewayAuth};
pub use config::{GatewayConfig, GatewayLimits, GatewayPersistence, GatewaySecurityConfig};
pub use delivery::{
    CallbackDeliverySink, ChannelDeliverySink, DeliveryPayload, DeliveryRecord, DeliveryRouter,
    DeliveryStatus, DeliveryTarget,
};
pub use events::{RunEvent, RunEventKind};
pub use policy::{BusyDecision, BusySnapshot, GatewayPolicy, GatewayRateLimiter};
pub use run::{
    BusyPolicy, CreateRunOutcome, GatewayDiagnostic, GatewayError, RunId, RunMeta, RunPolicy,
    RunRequest, RunStatus,
};
pub use runner::{
    GatewayCommand, GatewayCommandKind, GatewayCommandReceipt, GatewayCommandSink,
    GatewayRunAction, GatewayRunSubmission, GatewayRunner,
};
pub use scheduled::{scheduled_task_run_request, scheduled_task_source};
pub use session_key::{SessionKey, SessionKeyPolicy};
pub use source::{RemoteSource, RemoteSourceMetadata, RemoteTransport};
pub use store::{GatewayRecoveryReport, GatewayStore, SessionLock, SessionLockOutcome};
pub use webhook::{WebhookRouteConfig, WebhookVerifier};

#[cfg(test)]
mod scheduled_tests {
    use super::*;
    use allthecodes_tasks::{ScheduleSpec, ScheduledAgentTask};

    fn scheduled_task() -> ScheduledAgentTask {
        ScheduledAgentTask {
            id: "daily-review".to_string(),
            prompt: "summarize today's work".to_string(),
            cwd: "/repo".to_string(),
            schedule: ScheduleSpec::Interval {
                every_seconds: 3600,
            },
            enabled: true,
            last_run_at: None,
            next_run_at: Some("2026-07-04T12:00:00+00:00".to_string()),
        }
    }

    #[test]
    fn scheduled_task_maps_to_gateway_run_request() {
        let request = scheduled_task_run_request(&scheduled_task());

        assert_eq!(request.prompt, "summarize today's work");
        assert_eq!(request.source.transport, RemoteTransport::Scheduled);
        assert_eq!(request.source.workspace, "/repo");
        assert_eq!(request.source.client_id, "scheduled_agent");
        assert_eq!(request.source.user_id, "daily-review");
        assert_eq!(request.source.thread_id, "daily-review");
        assert_eq!(
            request.source.metadata.get("scheduled_task_id"),
            Some(&"daily-review".to_string())
        );
        assert_eq!(
            request.source.metadata.get("next_run_at"),
            Some(&"2026-07-04T12:00:00+00:00".to_string())
        );
        assert_eq!(
            request.idempotency_key.as_deref(),
            Some("scheduled:daily-review:2026-07-04T12:00:00+00:00")
        );
        assert_eq!(request.policy.busy, BusyPolicy::Queue);
    }
}
