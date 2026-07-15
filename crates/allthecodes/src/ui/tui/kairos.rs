use std::path::PathBuf;
use std::time::Duration;

use allthecodes_types::kairos::{KairosLifecycleState, KairosRuntimeSnapshot};
use tokio::sync::mpsc;

use crate::ui::app::KairosUiStatus;

pub(super) struct KairosPollUpdate {
    pub(super) status: KairosUiStatus,
}

pub(super) fn spawn_poller(cwd: PathBuf, tx: mpsc::UnboundedSender<KairosPollUpdate>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(2));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            let poll_cwd = cwd.clone();
            let result = tokio::task::spawn_blocking(move || {
                allthecodes_daemon::process_state::kairos_snapshot(&poll_cwd)
            })
            .await;
            let (cache, status) = match result {
                Ok(Ok(snapshot)) => {
                    let status = status_from_snapshot(&snapshot);
                    (Ok(snapshot), status)
                }
                Ok(Err(error)) => {
                    let message = error.to_string();
                    (
                        Err(message),
                        KairosUiStatus {
                            label: "KAIROS error".to_string(),
                            includes_proactive: false,
                        },
                    )
                }
                Err(error) => (
                    Err(format!("KAIROS poll task failed: {error}")),
                    KairosUiStatus {
                        label: "KAIROS error".to_string(),
                        includes_proactive: false,
                    },
                ),
            };
            crate::ui::command_surface::set_kairos_snapshot(cache);
            if tx.send(KairosPollUpdate { status }).is_err() {
                break;
            }
        }
    });
}

pub(super) fn status_from_snapshot(snapshot: &KairosRuntimeSnapshot) -> KairosUiStatus {
    let worker_count = snapshot.workers.len();
    let label = match snapshot.lifecycle {
        KairosLifecycleState::Starting => "KAIROS starting".to_string(),
        KairosLifecycleState::Restarting => "KAIROS restarting".to_string(),
        KairosLifecycleState::Stopping => "KAIROS stopping".to_string(),
        KairosLifecycleState::Stale => "KAIROS stale".to_string(),
        KairosLifecycleState::Failed => "KAIROS error".to_string(),
        KairosLifecycleState::Ready if snapshot.restart_required => {
            "KAIROS restart required".to_string()
        }
        KairosLifecycleState::Ready => snapshot
            .automation
            .as_ref()
            .filter(|automation| automation.status == "sleeping")
            .map(|_| "KAIROS sleeping".to_string())
            .unwrap_or_else(|| format!("KAIROS ready · {worker_count} workers")),
        KairosLifecycleState::Stopped if snapshot.desired.enabled => "KAIROS stopped".to_string(),
        KairosLifecycleState::Stopped => "KAIROS off".to_string(),
    };
    KairosUiStatus {
        label,
        includes_proactive: snapshot.lifecycle == KairosLifecycleState::Ready
            && snapshot.effective.proactive,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_types::kairos::KairosFeatureProfile;

    #[test]
    fn ready_status_reports_workers_and_suppresses_duplicate_proactive() {
        let status = status_from_snapshot(&KairosRuntimeSnapshot {
            lifecycle: KairosLifecycleState::Ready,
            effective: KairosFeatureProfile {
                enabled: true,
                proactive: true,
                ..Default::default()
            },
            workers: vec![Default::default(), Default::default()],
            ..Default::default()
        });
        assert_eq!(status.label, "KAIROS ready · 2 workers");
        assert!(status.includes_proactive);
    }
}
