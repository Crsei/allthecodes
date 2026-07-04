//! Webhook signature verification, declarative route handling, and payload parsing.

use allthecodes_gateway::{GatewayCommand, GatewayCommandKind, GatewayCommandSink};
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::Json;
use hmac::{Hmac, Mac};
use serde_json::{json, Value};
use sha2::Sha256;

use super::channels::{channel_submit_payload, ChannelEvent, ChannelOrigin};
use super::state::DaemonState;

type HmacSha256 = Hmac<Sha256>;

/// Verify GitHub webhook signature (X-Hub-Signature-256).
///
/// GitHub sends a header `X-Hub-Signature-256: sha256=<hex>` on each webhook
/// delivery. This function recomputes the HMAC-SHA256 over the raw body using
/// the shared secret and compares it to the provided signature in constant time.
pub fn verify_github_signature(body: &[u8], signature: &str, secret: &str) -> bool {
    let Some(hex_sig) = signature.strip_prefix("sha256=") else {
        return false;
    };
    let Ok(mut mac) = HmacSha256::new_from_slice(secret.as_bytes()) else {
        return false;
    };
    mac.update(body);
    let Ok(expected) = hex::decode(hex_sig) else {
        return false;
    };
    mac.verify_slice(&expected).is_ok()
}

/// Verify Slack webhook signature (X-Slack-Signature / X-Slack-Request-Timestamp).
///
/// Slack computes `v0=HMAC-SHA256(signing_secret, "v0:{timestamp}:{body}")` and
/// sends it as `X-Slack-Signature`. This function rebuilds the base string and
/// compares the result.
pub fn verify_slack_signature(
    body: &[u8],
    timestamp: &str,
    signature: &str,
    signing_secret: &str,
) -> bool {
    let sig_basestring = format!("v0:{}:{}", timestamp, String::from_utf8_lossy(body));
    let Ok(mut mac) = HmacSha256::new_from_slice(signing_secret.as_bytes()) else {
        return false;
    };
    mac.update(sig_basestring.as_bytes());
    let result = mac.finalize();
    let expected = format!("v0={}", hex::encode(result.into_bytes()));
    expected == signature
}

pub async fn webhook_github(headers: HeaderMap, body: Bytes) -> Json<Value> {
    handle_deliver_only_webhook(
        declarative_route("github").with_deliver_only(true),
        headers,
        body,
    )
    .await
}

pub async fn webhook_slack(headers: HeaderMap, body: Bytes) -> Json<Value> {
    handle_deliver_only_webhook(
        declarative_route("slack").with_deliver_only(true),
        headers,
        body,
    )
    .await
}

pub async fn webhook_generic(headers: HeaderMap, body: Bytes) -> Json<Value> {
    handle_deliver_only_webhook(
        declarative_route("generic").with_deliver_only(true),
        headers,
        body,
    )
    .await
}

pub async fn webhook_declarative(
    State(state): State<DaemonState>,
    Path(route_id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Json<Value> {
    let route = declarative_route(&route_id);
    match allthecodes_gateway::webhook::WebhookRouter::handle(&route, &headers, &body) {
        Ok(allthecodes_gateway::webhook::WebhookRouteOutcome::Ignored { event }) => Json(json!({
            "status": "ignored",
            "source": route.provider.as_source_client(),
            "routeId": route.route_id,
            "event": event,
        })),
        Ok(allthecodes_gateway::webhook::WebhookRouteOutcome::DeliverOnly {
            event,
            prompt,
            source,
            idempotency_key,
        }) => deliver_webhook_event(&route, event, prompt, source, idempotency_key),
        Ok(allthecodes_gateway::webhook::WebhookRouteOutcome::Run { request, event }) => {
            submit_webhook_run(state, route, request, event)
        }
        Err(error) => webhook_error(error),
    }
}

async fn handle_deliver_only_webhook(
    route: allthecodes_gateway::webhook::WebhookRouteConfig,
    headers: HeaderMap,
    body: Bytes,
) -> Json<Value> {
    match allthecodes_gateway::webhook::WebhookRouter::handle(&route, &headers, &body) {
        Ok(allthecodes_gateway::webhook::WebhookRouteOutcome::Ignored { event }) => Json(json!({
            "status": "ignored",
            "source": route.provider.as_source_client(),
            "event": event,
        })),
        Ok(allthecodes_gateway::webhook::WebhookRouteOutcome::DeliverOnly {
            event,
            prompt,
            source,
            idempotency_key,
        }) => deliver_webhook_event(&route, event, prompt, source, idempotency_key),
        Ok(allthecodes_gateway::webhook::WebhookRouteOutcome::Run { request, event }) => {
            Json(json!({
                "status": "received",
                "source": route.provider.as_source_client(),
                "routeId": route.route_id,
                "event": event,
                "prompt": request.prompt,
            }))
        }
        Err(error) => webhook_error(error),
    }
}

fn submit_webhook_run(
    _state: DaemonState,
    route: allthecodes_gateway::webhook::WebhookRouteConfig,
    request: allthecodes_gateway::RunRequest,
    event: Option<String>,
) -> Json<Value> {
    let policy = allthecodes_gateway::GatewayPolicy::default();
    let config = allthecodes_gateway::GatewayConfig::default();
    let runner = allthecodes_gateway::GatewayRunner::new(
        allthecodes_gateway::GatewayStore::new(
            config.persistence,
            allthecodes_gateway::SessionKeyPolicy::default(),
        ),
        policy.clone(),
    );
    let snapshot = allthecodes_gateway::BusySnapshot {
        running: usize::from(
            crate::automation_state::assistant_command_active()
                || crate::automation_state::pending_input_active(),
        ),
        queued: 0,
        max_running: policy.max_running,
        max_queued: policy.max_queued,
    };

    match runner.submit_run(
        request,
        snapshot,
        &crate::gateway_bridge::GatewayDaemonBridge::assistant_worker(),
    ) {
        Ok(submission) => Json(json!({
            "status": "received",
            "source": route.provider.as_source_client(),
            "routeId": route.route_id,
            "event": event,
            "runId": submission.meta.run_id,
            "runStatus": submission.meta.status,
            "action": submission.action,
        })),
        Err(error) => webhook_error(error),
    }
}

fn deliver_webhook_event(
    route: &allthecodes_gateway::webhook::WebhookRouteConfig,
    event: Option<String>,
    prompt: String,
    source: allthecodes_gateway::RemoteSource,
    idempotency_key: Option<String>,
) -> Json<Value> {
    if route.provider == allthecodes_gateway::webhook::WebhookProvider::GitHub {
        let payload = prompt
            .split_once('\n')
            .and_then(|(_, body)| serde_json::from_str::<Value>(body).ok())
            .unwrap_or(Value::Null);
        let outcome = match crate::runtime::route_github_pr_activity(
            &payload,
            event.as_deref(),
            idempotency_key.as_deref(),
        ) {
            Ok(Some(outcome)) => outcome,
            Ok(None) => {
                return Json(json!({
                    "status": "ignored",
                    "source": "github",
                    "event": event,
                }));
            }
            Err(error) => {
                return Json(json!({
                    "status": "error",
                    "source": "github",
                    "routeId": route.route_id,
                    "message": error.to_string(),
                }));
            }
        };
        return Json(json!({
            "status": "received",
            "source": "github",
            "routeId": route.route_id,
            "event": event,
            "deliverOnly": true,
            "matched": outcome.matched,
            "delivered": outcome.delivered,
        }));
    }

    let receipt = match submit_webhook_channel_event(
        route,
        event.as_deref(),
        &prompt,
        source,
        idempotency_key.clone(),
    ) {
        Ok(receipt) => receipt,
        Err(error) => return webhook_error(error),
    };

    Json(json!({
        "status": "received",
        "source": route.provider.as_source_client(),
        "routeId": route.route_id,
        "event": event,
        "deliverOnly": true,
        "idempotencyKey": idempotency_key,
        "commandId": receipt.command_id,
    }))
}

fn submit_webhook_channel_event(
    route: &allthecodes_gateway::webhook::WebhookRouteConfig,
    event: Option<&str>,
    prompt: &str,
    source: allthecodes_gateway::RemoteSource,
    idempotency_key: Option<String>,
) -> Result<allthecodes_gateway::GatewayCommandReceipt, allthecodes_gateway::GatewayError> {
    let delivery_id = idempotency_key.clone().unwrap_or_else(|| {
        format!(
            "{}:{}:{}",
            route.route_id,
            event.unwrap_or("event"),
            source.message_id.as_deref().unwrap_or("message")
        )
    });
    let channel_event = ChannelEvent {
        source: route.provider.as_source_client().to_string(),
        sender: Some(source.user_id.clone()),
        content: prompt.to_string(),
        meta: json!({
            "routeId": route.route_id,
            "event": event,
            "remoteSource": source.redacted_json(),
            "idempotencyKey": idempotency_key,
        }),
        origin: ChannelOrigin::Webhook {
            endpoint: route.route_id.clone(),
        },
    };
    let bridge = crate::gateway_bridge::GatewayDaemonBridge::assistant_worker();
    bridge.dispatch(GatewayCommand {
        kind: GatewayCommandKind::Submit,
        run_id: format!("run_webhook_{}", sanitize_webhook_id(&delivery_id)),
        session_key: format!(
            "webhook:{}:{}:{}",
            route.route_id, source.client_id, source.thread_id
        ),
        payload: channel_submit_payload(&channel_event),
        idempotency_key: Some(delivery_id),
    })
}

fn sanitize_webhook_id(raw: &str) -> String {
    let sanitized: String = raw
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    if sanitized.is_empty() {
        "delivery".to_string()
    } else {
        sanitized
    }
}

fn webhook_error(error: allthecodes_gateway::GatewayError) -> Json<Value> {
    let diagnostic = error.into_diagnostic();
    Json(json!({
        "status": "error",
        "error": diagnostic,
    }))
}

fn declarative_route(route_id: &str) -> allthecodes_gateway::webhook::WebhookRouteConfig {
    let secret = webhook_secret(route_id);
    match route_id {
        "github" => allthecodes_gateway::webhook::WebhookRouteConfig::github(route_id, secret),
        "slack" => allthecodes_gateway::webhook::WebhookRouteConfig::slack(route_id, secret),
        _ => allthecodes_gateway::webhook::WebhookRouteConfig::generic(route_id, secret),
    }
}

fn webhook_secret(route_id: &str) -> Option<String> {
    let normalized = route_id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect::<String>();
    let route_key = format!("ALLTHECODES_WEBHOOK_{}_SECRET", normalized);
    std::env::var(&route_key)
        .or_else(|_| match route_id {
            "github" => std::env::var("ALLTHECODES_GITHUB_WEBHOOK_SECRET"),
            "slack" => std::env::var("ALLTHECODES_SLACK_WEBHOOK_SECRET"),
            _ => std::env::var("ALLTHECODES_GENERIC_WEBHOOK_SECRET"),
        })
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::DaemonCommandKind;
    use crate::supervisor::ASSISTANT_WORKER_ID;

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    // ---- GitHub ----

    #[test]
    fn test_github_signature_valid() {
        let secret = "my-github-secret";
        let body = b"{ \"action\": \"opened\" }";

        // Compute the real HMAC so we have a known-good signature.
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body);
        let sig_hex = hex::encode(mac.finalize().into_bytes());
        let header = format!("sha256={sig_hex}");

        assert!(verify_github_signature(body, &header, secret));
    }

    #[test]
    fn test_github_signature_invalid() {
        let secret = "my-github-secret";
        let body = b"{ \"action\": \"opened\" }";
        let bad_header = "sha256=0000000000000000000000000000000000000000000000000000000000000000";

        assert!(!verify_github_signature(body, bad_header, secret));
    }

    #[test]
    fn test_github_signature_missing_prefix() {
        // No "sha256=" prefix should be rejected immediately.
        assert!(!verify_github_signature(b"body", "deadbeef", "secret"));
    }

    #[test]
    fn test_github_signature_invalid_hex() {
        // Non-hex characters after prefix should be rejected.
        assert!(!verify_github_signature(b"body", "sha256=ZZZZ", "secret"));
    }

    // ---- Slack ----

    #[test]
    fn test_slack_signature_valid() {
        let signing_secret = "my-slack-signing-secret";
        let timestamp = "1631234567";
        let body = b"token=abc123&event=url_verification";

        // Compute expected signature.
        let sig_basestring = format!("v0:{}:{}", timestamp, String::from_utf8_lossy(body));
        let mut mac = HmacSha256::new_from_slice(signing_secret.as_bytes()).unwrap();
        mac.update(sig_basestring.as_bytes());
        let expected = format!("v0={}", hex::encode(mac.finalize().into_bytes()));

        assert!(verify_slack_signature(
            body,
            timestamp,
            &expected,
            signing_secret
        ));
    }

    #[test]
    fn test_slack_signature_invalid() {
        let signing_secret = "my-slack-signing-secret";
        let timestamp = "1631234567";
        let body = b"token=abc123&event=url_verification";
        let bad_sig = "v0=0000000000000000000000000000000000000000000000000000000000000000";

        assert!(!verify_slack_signature(
            body,
            timestamp,
            bad_sig,
            signing_secret
        ));
    }

    #[test]
    fn test_slack_signature_wrong_timestamp() {
        let signing_secret = "my-slack-signing-secret";
        let body = b"payload";

        // Compute with one timestamp, verify with a different one.
        let ts_sign = "1000000000";
        let ts_verify = "9999999999";

        let sig_basestring = format!("v0:{}:{}", ts_sign, String::from_utf8_lossy(body));
        let mut mac = HmacSha256::new_from_slice(signing_secret.as_bytes()).unwrap();
        mac.update(sig_basestring.as_bytes());
        let sig = format!("v0={}", hex::encode(mac.finalize().into_bytes()));

        assert!(!verify_slack_signature(
            body,
            ts_verify,
            &sig,
            signing_secret
        ));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn declarative_deliver_only_webhook_queues_assistant_submit() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path().to_str().unwrap());
        let secret = "generic-secret";
        let _secret = EnvGuard::set("ALLTHECODES_WEBHOOK_GENERIC_SECRET", secret);
        let route = declarative_route("generic").with_deliver_only(true);
        let body = serde_json::to_vec(&json!({
            "action": "opened",
            "repository": { "full_name": "acme/project" },
            "message": "investigate flaky test"
        }))
        .unwrap();
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(&body);
        let mut headers = HeaderMap::new();
        headers.insert(
            allthecodes_gateway::webhook::GITHUB_SIGNATURE_HEADER,
            format!("sha256={}", hex::encode(mac.finalize().into_bytes()))
                .parse()
                .unwrap(),
        );
        headers.insert("idempotency-key", "delivery-1".parse().unwrap());

        let Json(response) =
            handle_deliver_only_webhook(route, headers, axum::body::Bytes::from(body)).await;
        let commands = crate::protocol_store()
            .read_worker_commands(ASSISTANT_WORKER_ID)
            .unwrap();

        assert_eq!(response["status"], "received");
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].kind, DaemonCommandKind::Submit);
        assert_eq!(commands[0].payload["source"], "channel");
        assert_eq!(commands[0].payload["channel"]["source"], "generic");
        assert!(commands[0].payload["gateway"]["runId"]
            .as_str()
            .unwrap()
            .starts_with("run_webhook_"));
        assert!(commands[0].payload["gateway"]["sessionKey"]
            .as_str()
            .unwrap()
            .contains("webhook:generic"));
        assert!(commands[0].payload["text"]
            .as_str()
            .unwrap()
            .contains("Webhook generic/generic received"));
    }
}
