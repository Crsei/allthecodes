//! Remote-control gateway route wiring for the daemon HTTP server.

use allthecodes_gateway::auth::{invalid_token_error, missing_token_error, GatewayAuthMode};
use allthecodes_gateway::{
    api, BusySnapshot, GatewayAuthVerifier, GatewayBusySnapshotProvider, GatewayConfig,
    GatewayError, GatewayPolicy, GatewayRunner, GatewayStore, SessionKeyPolicy,
};
use std::time::Duration;
use tracing::warn;

use super::gateway_bridge::GatewayDaemonBridge;
use super::process_state;

#[derive(Debug, Clone, Copy)]
struct DaemonGatewayAuth;

impl GatewayAuthVerifier for DaemonGatewayAuth {
    fn mode(&self) -> GatewayAuthMode {
        GatewayAuthMode::LoopbackDaemonToken
    }

    fn verify(&self, candidate: Option<&str>) -> Result<(), GatewayError> {
        let Some(candidate) = candidate else {
            return Err(missing_token_error());
        };
        match process_state::verify_control_token(candidate) {
            Ok(true) => Ok(()),
            Ok(false) => Err(invalid_token_error()),
            Err(error) => Err(GatewayError::new(
                allthecodes_gateway::GatewayDiagnostic::new(
                    "control_token_unavailable",
                    "The daemon control token could not be verified.",
                    "Check daemon process state and token file permissions.",
                )
                .with_context(error.to_string()),
            )),
        }
    }
}

#[derive(Debug, Clone)]
struct DaemonBusySnapshotProvider {
    policy: GatewayPolicy,
}

impl GatewayBusySnapshotProvider for DaemonBusySnapshotProvider {
    fn snapshot(&self) -> BusySnapshot {
        let active_submits = crate::automation_state::active_submit_count();
        let pending_input = crate::automation_state::pending_input_active();

        BusySnapshot {
            running: usize::from(active_submits > 0 || pending_input),
            queued: active_submits.saturating_sub(1),
            max_running: self.policy.max_running,
            max_queued: self.policy.max_queued,
        }
    }
}

pub fn gateway_routes() -> axum::Router {
    let config = load_gateway_config();
    let policy = GatewayPolicy::default();
    let runner = GatewayRunner::new(
        GatewayStore::new(config.persistence.clone(), SessionKeyPolicy::default()),
        policy.clone(),
    );
    if let Err(error) = runner.recover_on_startup(Duration::from_secs(30 * 60)) {
        warn!(
            diagnostic = ?error.diagnostic(),
            "gateway startup recovery failed"
        );
    }
    let state = api::GatewayApiState::new(
        runner,
        GatewayDaemonBridge::assistant_worker(),
        DaemonGatewayAuth,
        DaemonBusySnapshotProvider {
            policy: policy.clone(),
        },
        policy,
        config,
    );
    api::router(state)
}

fn load_gateway_config() -> GatewayConfig {
    GatewayConfig::load().unwrap_or_else(|error| {
        warn!(
            error = %error,
            "gateway config load failed; using disabled adapter defaults"
        );
        GatewayConfig::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{DaemonCommandKind, DaemonEventKind};
    use crate::supervisor::ASSISTANT_WORKER_ID;
    use allthecodes_gateway::{
        BusyPolicy, RemoteSource, RemoteTransport, RunPolicy, RunRequest, RunStatus,
    };
    use serial_test::serial;

    #[test]
    #[serial]
    fn busy_snapshot_counts_active_submit_commands() {
        let tmp = tempfile::tempdir().unwrap();
        let previous = std::env::var("ALLTHECODES_HOME").ok();
        std::env::set_var("ALLTHECODES_HOME", tmp.path());
        crate::protocol_store()
            .enqueue_command(
                ASSISTANT_WORKER_ID,
                DaemonCommandKind::Submit,
                serde_json::json!({ "text": "hello" }),
                None,
            )
            .unwrap();

        let snapshot = DaemonBusySnapshotProvider {
            policy: GatewayPolicy::default(),
        }
        .snapshot();

        assert_eq!(snapshot.running, 1);
        assert_eq!(snapshot.queued, 0);
        match previous {
            Some(value) => std::env::set_var("ALLTHECODES_HOME", value),
            None => std::env::remove_var("ALLTHECODES_HOME"),
        }
    }

    #[test]
    #[serial]
    fn busy_snapshot_counts_pending_input_as_running() {
        let tmp = tempfile::tempdir().unwrap();
        let previous = std::env::var("ALLTHECODES_HOME").ok();
        std::env::set_var("ALLTHECODES_HOME", tmp.path());
        crate::protocol_store()
            .append_event(
                ASSISTANT_WORKER_ID,
                None,
                DaemonEventKind::PermissionRequest.as_str(),
                serde_json::json!({
                    "request_id": "toolu_1",
                    "tool_use_id": "toolu_1",
                    "tool_name": "Bash",
                }),
            )
            .unwrap();

        let snapshot = DaemonBusySnapshotProvider {
            policy: GatewayPolicy::default(),
        }
        .snapshot();

        assert_eq!(snapshot.running, 1);
        assert_eq!(snapshot.queued, 0);
        match previous {
            Some(value) => std::env::set_var("ALLTHECODES_HOME", value),
            None => std::env::remove_var("ALLTHECODES_HOME"),
        }
    }

    #[test]
    #[serial]
    fn gateway_routes_loads_persisted_channel_config() {
        let tmp = tempfile::tempdir().unwrap();
        let previous = std::env::var("ALLTHECODES_HOME").ok();
        std::env::set_var("ALLTHECODES_HOME", tmp.path());

        let mut config = allthecodes_gateway::GatewayConfig::default();
        config.adapters.telegram.enabled = true;
        config.adapters.telegram.bot_token = Some("test-only-not-a-real-credential".to_string());
        config.save().unwrap();

        let loaded = load_gateway_config();

        assert!(loaded.adapters.telegram.enabled);
        assert_eq!(
            loaded.adapters.telegram.bot_token.as_deref(),
            Some("test-only-not-a-real-credential")
        );
        match previous {
            Some(value) => std::env::set_var("ALLTHECODES_HOME", value),
            None => std::env::remove_var("ALLTHECODES_HOME"),
        }
    }

    #[test]
    #[serial]
    fn gateway_routes_runs_startup_recovery() {
        let tmp = tempfile::tempdir().unwrap();
        let previous = std::env::var("ALLTHECODES_HOME").ok();
        std::env::set_var("ALLTHECODES_HOME", tmp.path());

        let store = GatewayStore::default_with_policy(SessionKeyPolicy::default());
        let created = store
            .create_run(RunRequest {
                prompt: "recover me".to_string(),
                source: RemoteSource::new(
                    RemoteTransport::Http,
                    "local",
                    "F:/AIclassmanager/cc/rust",
                    "dashboard",
                    "86186",
                    "thread-1",
                ),
                policy: RunPolicy {
                    busy: BusyPolicy::Queue,
                    ..RunPolicy::default()
                },
                idempotency_key: None,
            })
            .unwrap();
        let run_id = created.meta().run_id.clone();
        store.update_status(&run_id, RunStatus::Running).unwrap();

        let _router = gateway_routes();

        assert_eq!(
            store.load_run(&run_id).unwrap().status,
            RunStatus::Recoverable
        );
        match previous {
            Some(value) => std::env::set_var("ALLTHECODES_HOME", value),
            None => std::env::remove_var("ALLTHECODES_HOME"),
        }
    }
}
