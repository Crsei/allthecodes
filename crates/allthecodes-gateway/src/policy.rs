use crate::{BusyPolicy, GatewayDiagnostic, GatewayError, RunId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BusySnapshot {
    pub running: usize,
    pub queued: usize,
    pub max_running: usize,
    pub max_queued: usize,
}

impl Default for BusySnapshot {
    fn default() -> Self {
        Self {
            running: 0,
            queued: 0,
            max_running: 1,
            max_queued: 32,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BusyDecision {
    StartNow,
    Queue,
    Interrupt,
    Reject(GatewayDiagnostic),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayPolicy {
    pub max_running: usize,
    pub max_queued: usize,
    pub supports_steer: bool,
}

impl Default for GatewayPolicy {
    fn default() -> Self {
        Self {
            max_running: 1,
            max_queued: 32,
            supports_steer: false,
        }
    }
}

impl GatewayPolicy {
    pub fn evaluate_busy(
        &self,
        policy: BusyPolicy,
        snapshot: BusySnapshot,
    ) -> Result<BusyDecision, GatewayError> {
        if matches!(policy, BusyPolicy::Steer) && !self.supports_steer {
            return Err(GatewayError::new(GatewayDiagnostic::new(
                "unsupported",
                "Mid-turn steer is not supported by this gateway.",
                "Use queue, reject, or interrupt until steer is implemented.",
            )));
        }

        if snapshot.running < self.max_running {
            return Ok(BusyDecision::StartNow);
        }

        match policy {
            BusyPolicy::Reject => Ok(BusyDecision::Reject(GatewayDiagnostic::new(
                "busy",
                "The gateway is already running the maximum number of runs.",
                "Retry later or submit with the queue busy policy.",
            ))),
            BusyPolicy::Queue => {
                if snapshot.queued >= self.max_queued {
                    Ok(BusyDecision::Reject(GatewayDiagnostic::new(
                        "queue_full",
                        "The gateway run queue is full.",
                        "Retry after queued runs complete or increase the queue limit.",
                    )))
                } else {
                    Ok(BusyDecision::Queue)
                }
            }
            BusyPolicy::Interrupt => Ok(BusyDecision::Interrupt),
            BusyPolicy::Steer => Ok(BusyDecision::StartNow),
        }
    }

    pub fn stale_response(run_id: &RunId) -> GatewayDiagnostic {
        GatewayDiagnostic::new(
            "stale_response",
            "The approval or ask-user response no longer matches a pending run request.",
            "Reload the run events and answer the current pending request.",
        )
        .with_context(format!("run_id={}", run_id))
    }
}

#[derive(Debug, Clone)]
pub struct GatewayRateLimiter {
    max_hits: usize,
    window: Duration,
    hits: BTreeMap<String, Vec<Instant>>,
}

impl GatewayRateLimiter {
    pub fn new(max_hits: usize, window: Duration) -> Self {
        Self {
            max_hits,
            window,
            hits: BTreeMap::new(),
        }
    }

    pub fn check(&mut self, key: impl Into<String>) -> Result<(), GatewayError> {
        let key = key.into();
        let now = Instant::now();
        let cutoff = now.checked_sub(self.window).unwrap_or(now);
        let hits = self.hits.entry(key).or_default();
        hits.retain(|hit| *hit >= cutoff);
        if hits.len() >= self.max_hits {
            return Err(GatewayError::new(GatewayDiagnostic::new(
                "rate_limited",
                "The remote-control gateway rate limit has been exceeded.",
                "Retry after the rate-limit window or reduce request frequency.",
            )));
        }
        hits.push(now);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_full_returns_stable_diagnostic() {
        let policy = GatewayPolicy::default();
        let decision = policy
            .evaluate_busy(
                BusyPolicy::Queue,
                BusySnapshot {
                    running: 1,
                    queued: 32,
                    ..BusySnapshot::default()
                },
            )
            .unwrap();

        match decision {
            BusyDecision::Reject(diagnostic) => assert_eq!(diagnostic.code, "queue_full"),
            other => panic!("unexpected decision: {other:?}"),
        }
    }

    #[test]
    fn steer_is_explicitly_unsupported() {
        let err = GatewayPolicy::default()
            .evaluate_busy(
                BusyPolicy::Steer,
                BusySnapshot {
                    running: 1,
                    ..BusySnapshot::default()
                },
            )
            .unwrap_err();

        assert_eq!(err.diagnostic().code, "unsupported");
    }

    #[test]
    fn steer_is_unsupported_even_when_idle() {
        let err = GatewayPolicy::default()
            .evaluate_busy(BusyPolicy::Steer, BusySnapshot::default())
            .unwrap_err();

        assert_eq!(err.diagnostic().code, "unsupported");
    }

    #[test]
    fn rate_limiter_returns_stable_redacted_diagnostic() {
        let mut limiter = GatewayRateLimiter::new(1, std::time::Duration::from_secs(60));
        limiter.check("token:raw-token").unwrap();
        let err = limiter.check("token:raw-token").unwrap_err();
        let json = serde_json::to_string(err.diagnostic()).unwrap();

        assert_eq!(err.diagnostic().code, "rate_limited");
        assert!(!json.contains("raw-token"));
    }
}
