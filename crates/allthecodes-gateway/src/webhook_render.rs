use axum::http::HeaderMap;
use serde_json::Value;

use crate::{RemoteSource, RemoteTransport};

use super::webhook::WebhookRouteConfig;

pub(crate) fn source_for(
    route: &WebhookRouteConfig,
    _headers: &HeaderMap,
    payload: &Value,
    idempotency_key: Option<&str>,
) -> RemoteSource {
    let client_id = route
        .default_source
        .client_id
        .clone()
        .unwrap_or_else(|| route.provider.as_source_client().to_string());
    let user_id = route
        .default_source
        .user_id
        .clone()
        .or_else(|| value_path(payload, &["sender", "login"]))
        .or_else(|| value_path(payload, &["user", "id"]))
        .unwrap_or_else(|| "webhook".to_string());
    let thread_id = route
        .default_source
        .thread_id
        .clone()
        .or_else(|| value_path(payload, &["pull_request", "number"]).map(|n| format!("pr-{n}")))
        .or_else(|| value_path(payload, &["repository", "full_name"]))
        .unwrap_or_else(|| route.route_id.clone());

    let mut source = RemoteSource::new(
        RemoteTransport::Webhook,
        route.default_source.tenant.clone(),
        route.default_source.workspace.clone(),
        client_id,
        user_id,
        thread_id,
    )
    .with_metadata("route_id", route.route_id.clone())
    .with_metadata("provider", route.provider.as_source_client());
    if let Some(idempotency_key) = idempotency_key {
        source = source.with_message_id(idempotency_key.to_string());
    }
    source
}

pub(crate) fn render_prompt(
    route: &WebhookRouteConfig,
    event: Option<&str>,
    payload: &Value,
) -> String {
    let repository = value_path(payload, &["repository", "full_name"])
        .or_else(|| value_path(payload, &["repository", "name"]))
        .unwrap_or_else(|| "unknown".to_string());
    let action = payload
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    route
        .prompt_template
        .replace("{{route_id}}", &route.route_id)
        .replace("{{provider}}", route.provider.as_source_client())
        .replace("{{event}}", event.unwrap_or("unknown"))
        .replace("{{repository}}", &repository)
        .replace("{{action}}", action)
        .replace("{{body}}", &payload.to_string())
}

fn value_path(payload: &Value, path: &[&str]) -> Option<String> {
    let mut current = payload;
    for key in path {
        current = current.get(*key)?;
    }
    current
        .as_str()
        .map(ToOwned::to_owned)
        .or_else(|| current.as_u64().map(|value| value.to_string()))
}
