//! PR activity subscription tools for coordinator mode.

use std::fs;

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{constants, mailbox, types::TeammateMessage};
use allthecodes_tools::tool::{Tool, ToolProgress, ToolResult, ToolUseContext, ValidationResult};
use allthecodes_types::message::AssistantMessage;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PrActivitySubscription {
    pub id: String,
    pub owner: String,
    pub repo: String,
    pub pr_number: u64,
    #[serde(default)]
    pub events: Vec<String>,
    pub team_name: String,
    pub mailbox_name: String,
    pub created_at: String,
    #[serde(default = "default_active")]
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GithubPrActivity {
    pub owner: String,
    pub repo: String,
    pub pr_number: u64,
    pub event_name: Option<String>,
    pub action: String,
    pub sender: Option<String>,
    pub title: Option<String>,
    pub html_url: Option<String>,
    pub delivery_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrActivityRouteResult {
    pub matched: usize,
    pub delivered: usize,
}

#[derive(Debug, Deserialize)]
struct SubscribeInput {
    owner: Option<String>,
    repo: String,
    pr_number: u64,
    #[serde(default)]
    events: Vec<String>,
    #[serde(default)]
    team: Option<String>,
    #[serde(default)]
    mailbox: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UnsubscribeInput {
    #[serde(default)]
    subscription_id: Option<String>,
    #[serde(default)]
    owner: Option<String>,
    #[serde(default)]
    repo: Option<String>,
    #[serde(default)]
    pr_number: Option<u64>,
    #[serde(default)]
    team: Option<String>,
}

pub struct SubscribePrActivityTool;
pub struct SubscribePrTool;
pub struct UnsubscribePrActivityTool;

#[async_trait]
impl Tool for SubscribePrActivityTool {
    fn name(&self) -> &str {
        "subscribe_pr_activity"
    }

    async fn description(&self, _input: &Value) -> String {
        "Subscribe the active coordinator team to GitHub pull request activity.".to_string()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "owner": {
                    "type": "string",
                    "description": "Repository owner. Optional when repo is owner/name."
                },
                "repo": {
                    "type": "string",
                    "description": "Repository name, or owner/name."
                },
                "pr_number": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Pull request number to watch."
                },
                "team": {
                    "type": "string",
                    "description": "Optional team name. Defaults to the active team."
                },
                "events": {
                    "type": "array",
                    "items": {
                        "type": "string",
                        "enum": ["comment", "review", "ci", "merge", "close"]
                    },
                    "description": "Optional event filters. Empty means all PR activity."
                },
                "mailbox": {
                    "type": "string",
                    "description": "Optional mailbox recipient. Defaults to team-lead."
                }
            },
            "required": ["repo", "pr_number"]
        })
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        if input
            .get("repo")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .is_empty()
        {
            return ValidationResult::Error {
                message: "'repo' is required".to_string(),
                error_code: 400,
            };
        }
        if input.get("pr_number").and_then(Value::as_u64).unwrap_or(0) == 0 {
            return ValidationResult::Error {
                message: "'pr_number' must be a positive integer".to_string(),
                error_code: 400,
            };
        }
        ValidationResult::Ok
    }

    async fn call(
        &self,
        input: Value,
        ctx: &ToolUseContext,
        _parent: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let params: SubscribeInput = serde_json::from_value(input)?;
        let app_state = (ctx.get_app_state)();
        let team_name = params
            .team
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .or_else(|| {
                app_state
                    .team_context
                    .as_ref()
                    .map(|team| team.team_name.clone())
            })
            .ok_or_else(|| anyhow::anyhow!("No active team. Start coordinator mode first."))?;
        let (owner, repo) = normalize_repo(params.owner.as_deref(), &params.repo)?;
        let mailbox_name = params
            .mailbox
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(constants::TEAM_LEAD_NAME)
            .to_string();
        let subscription = subscribe_with_events(
            owner,
            repo,
            params.pr_number,
            normalize_events(&params.events),
            team_name,
            mailbox_name,
        )?;

        Ok(ToolResult {
            data: json!({
                "subscribed": true,
                "subscription": subscription,
            }),
            new_messages: vec![],
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        "Subscribe to GitHub PR activity and route matching webhook events into the coordinator mailbox."
            .to_string()
    }
}

#[async_trait]
impl Tool for SubscribePrTool {
    fn name(&self) -> &str {
        "SubscribePR"
    }

    async fn description(&self, _input: &Value) -> String {
        "Subscribe to GitHub pull request activity via the allthecodes KAIROS webhook subsystem."
            .to_string()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "repo": {
                    "type": "string",
                    "description": "Repository in owner/repo format."
                },
                "pr_number": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Pull request number to subscribe to."
                },
                "events": {
                    "type": "array",
                    "items": {
                        "type": "string",
                        "enum": ["comment", "review", "ci", "merge", "close"]
                    },
                    "description": "Event types to subscribe to. Defaults to all PR activity."
                },
                "team": {
                    "type": "string",
                    "description": "Optional team name. Defaults to the active team."
                },
                "mailbox": {
                    "type": "string",
                    "description": "Optional mailbox recipient. Defaults to team-lead."
                }
            },
            "required": ["repo", "pr_number"]
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        if input
            .get("repo")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .is_empty()
        {
            return ValidationResult::Error {
                message: "'repo' is required".to_string(),
                error_code: 400,
            };
        }
        if input.get("pr_number").and_then(Value::as_u64).unwrap_or(0) == 0 {
            return ValidationResult::Error {
                message: "'pr_number' must be a positive integer".to_string(),
                error_code: 400,
            };
        }
        ValidationResult::Ok
    }

    async fn call(
        &self,
        input: Value,
        ctx: &ToolUseContext,
        _parent: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let params: SubscribeInput = serde_json::from_value(input)?;
        let app_state = (ctx.get_app_state)();
        let team_name = params
            .team
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .or_else(|| {
                app_state
                    .team_context
                    .as_ref()
                    .map(|team| team.team_name.clone())
            })
            .ok_or_else(|| anyhow::anyhow!("No active team. Start coordinator mode first."))?;
        let (owner, repo) = normalize_repo(params.owner.as_deref(), &params.repo)?;
        let mailbox_name = params
            .mailbox
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(constants::TEAM_LEAD_NAME)
            .to_string();
        let events = normalize_events(&params.events);
        let subscription = subscribe_with_events(
            owner,
            repo,
            params.pr_number,
            events,
            team_name,
            mailbox_name,
        )?;

        Ok(ToolResult {
            data: json!({
                "subscribed": true,
                "subscription_id": subscription.id,
                "subscription": subscription,
            }),
            new_messages: vec![],
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        "Subscribe to GitHub pull request activity. Events are delivered into the coordinator team's mailbox through the KAIROS GitHub webhook endpoint.".to_string()
    }
}

#[async_trait]
impl Tool for UnsubscribePrActivityTool {
    fn name(&self) -> &str {
        "unsubscribe_pr_activity"
    }

    async fn description(&self, _input: &Value) -> String {
        "Remove a GitHub pull request activity subscription.".to_string()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "subscription_id": {
                    "type": "string",
                    "description": "Subscription id returned by subscribe_pr_activity."
                },
                "owner": {
                    "type": "string",
                    "description": "Repository owner."
                },
                "repo": {
                    "type": "string",
                    "description": "Repository name, or owner/name."
                },
                "pr_number": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Pull request number."
                },
                "team": {
                    "type": "string",
                    "description": "Optional team name to narrow the unsubscribe."
                }
            }
        })
    }

    async fn call(
        &self,
        input: Value,
        _ctx: &ToolUseContext,
        _parent: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let params: UnsubscribeInput = serde_json::from_value(input)?;
        let removed = unsubscribe(params)?;

        Ok(ToolResult {
            data: json!({
                "unsubscribed": removed > 0,
                "removed": removed,
            }),
            new_messages: vec![],
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        "Unsubscribe a coordinator team from GitHub PR activity.".to_string()
    }
}

pub fn parse_github_pr_activity(
    payload: &Value,
    event_name: Option<&str>,
    delivery_id: Option<&str>,
) -> Option<GithubPrActivity> {
    if !matches!(
        event_name,
        None | Some("pull_request")
            | Some("pull_request_review")
            | Some("issue_comment")
            | Some("check_run")
            | Some("check_suite")
    ) {
        return None;
    }

    let repository = payload.get("repository")?;
    let repo = repository.get("name")?.as_str()?.to_string();
    let owner = repository
        .get("owner")
        .and_then(|owner| owner.get("login").or_else(|| owner.get("name")))
        .and_then(Value::as_str)?
        .to_string();
    let pr = match event_name {
        Some("check_run") => payload
            .get("check_run")
            .and_then(|check_run| check_run.get("pull_requests"))
            .and_then(Value::as_array)
            .and_then(|pull_requests| pull_requests.first())?,
        Some("check_suite") => payload
            .get("check_suite")
            .and_then(|check_suite| check_suite.get("pull_requests"))
            .and_then(Value::as_array)
            .and_then(|pull_requests| pull_requests.first())?,
        _ => payload.get("pull_request").or_else(|| {
            payload
                .get("issue")
                .filter(|issue| issue.get("pull_request").is_some())
        })?,
    };
    let pr_number = pr.get("number")?.as_u64()?;
    let mut action = pr_activity_action(payload, event_name);
    if action == "closed" && pr.get("merged").and_then(Value::as_bool).unwrap_or(false) {
        action = "merged".to_string();
    }
    let sender = payload
        .get("sender")
        .and_then(|sender| sender.get("login"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let title = pr
        .get("title")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let html_url = pr
        .get("html_url")
        .and_then(Value::as_str)
        .or_else(|| {
            payload
                .get("comment")
                .and_then(|comment| comment.get("html_url"))
                .and_then(Value::as_str)
        })
        .or_else(|| {
            payload
                .get("check_run")
                .and_then(|check_run| check_run.get("html_url"))
                .and_then(Value::as_str)
        })
        .or_else(|| {
            payload
                .get("check_suite")
                .and_then(|check_suite| check_suite.get("html_url"))
                .and_then(Value::as_str)
        })
        .map(ToOwned::to_owned);

    Some(GithubPrActivity {
        owner,
        repo,
        pr_number,
        event_name: event_name.map(ToOwned::to_owned),
        action,
        sender,
        title,
        html_url,
        delivery_id: delivery_id.map(ToOwned::to_owned),
    })
}

fn pr_activity_action(payload: &Value, event_name: Option<&str>) -> String {
    let action = payload
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    match event_name {
        Some("check_run") => {
            let check_run = payload.get("check_run");
            let name = check_run
                .and_then(|check_run| check_run.get("name"))
                .and_then(Value::as_str);
            let status = check_run
                .and_then(|check_run| check_run.get("conclusion"))
                .or_else(|| check_run.and_then(|check_run| check_run.get("status")))
                .and_then(Value::as_str);
            format_ci_action("check_run", action, name, status)
        }
        Some("check_suite") => {
            let check_suite = payload.get("check_suite");
            let status = check_suite
                .and_then(|check_suite| check_suite.get("conclusion"))
                .or_else(|| check_suite.and_then(|check_suite| check_suite.get("status")))
                .and_then(Value::as_str);
            format_ci_action("check_suite", action, None, status)
        }
        _ => action.to_string(),
    }
}

fn format_ci_action(kind: &str, action: &str, name: Option<&str>, status: Option<&str>) -> String {
    let mut parts = vec!["ci".to_string(), kind.to_string(), action.to_string()];
    if let Some(status) = status.filter(|value| !value.is_empty()) {
        parts.push(status.to_string());
    }
    if let Some(name) = name.filter(|value| !value.is_empty()) {
        parts.push(name.to_string());
    }
    parts.join(" ")
}

pub fn route_github_pr_activity(activity: &GithubPrActivity) -> Result<PrActivityRouteResult> {
    let subscriptions = load_subscriptions()?;
    let matches: Vec<PrActivitySubscription> = subscriptions
        .into_iter()
        .filter(|subscription| {
            subscription.active
                && subscription.owner.eq_ignore_ascii_case(&activity.owner)
                && subscription.repo.eq_ignore_ascii_case(&activity.repo)
                && subscription.pr_number == activity.pr_number
                && subscription_events_match(
                    &subscription.events,
                    activity.event_name.as_deref(),
                    &activity.action,
                )
        })
        .collect();

    let mut delivered = 0;
    for subscription in &matches {
        let message = TeammateMessage {
            from: "github".to_string(),
            text: format_activity_message(activity),
            timestamp: chrono::Utc::now().to_rfc3339(),
            read: false,
            color: None,
            summary: Some(format!(
                "PR activity: {}/{}#{} {}",
                activity.owner, activity.repo, activity.pr_number, activity.action
            )),
        };
        mailbox::write_to_mailbox(&subscription.mailbox_name, message, &subscription.team_name)?;
        delivered += 1;
    }

    Ok(PrActivityRouteResult {
        matched: matches.len(),
        delivered,
    })
}

#[cfg(test)]
pub(crate) fn subscribe(
    owner: String,
    repo: String,
    pr_number: u64,
    team_name: String,
    mailbox_name: String,
) -> Result<PrActivitySubscription> {
    subscribe_with_events(owner, repo, pr_number, Vec::new(), team_name, mailbox_name)
}

pub(crate) fn subscribe_with_events(
    owner: String,
    repo: String,
    pr_number: u64,
    events: Vec<String>,
    team_name: String,
    mailbox_name: String,
) -> Result<PrActivitySubscription> {
    let mut subscriptions = load_subscriptions()?;
    if let Some(existing) = subscriptions.iter_mut().find(|subscription| {
        subscription.owner.eq_ignore_ascii_case(&owner)
            && subscription.repo.eq_ignore_ascii_case(&repo)
            && subscription.pr_number == pr_number
            && subscription.team_name == team_name
            && subscription.mailbox_name == mailbox_name
    }) {
        existing.active = true;
        existing.events = events;
        let subscription = existing.clone();
        save_subscriptions(&subscriptions)?;
        return Ok(subscription);
    }

    let subscription = PrActivitySubscription {
        id: uuid::Uuid::new_v4().to_string(),
        owner,
        repo,
        pr_number,
        events,
        team_name,
        mailbox_name,
        created_at: chrono::Utc::now().to_rfc3339(),
        active: true,
    };
    subscriptions.push(subscription.clone());
    save_subscriptions(&subscriptions)?;
    Ok(subscription)
}

fn normalize_events(events: &[String]) -> Vec<String> {
    let mut normalized = events
        .iter()
        .map(|event| event.trim().to_ascii_lowercase())
        .filter(|event| {
            matches!(
                event.as_str(),
                "comment" | "review" | "ci" | "merge" | "close"
            )
        })
        .collect::<Vec<_>>();
    normalized.sort();
    normalized.dedup();
    normalized
}

fn subscription_events_match(events: &[String], event_name: Option<&str>, action: &str) -> bool {
    if events.is_empty() {
        return true;
    }
    let action = action.to_ascii_lowercase();
    let event_name = event_name.unwrap_or_default().to_ascii_lowercase();
    events.iter().any(|event| match event.as_str() {
        "comment" => event_name == "issue_comment" || action.contains("comment"),
        "review" => event_name == "pull_request_review" || action.contains("review"),
        "ci" => {
            matches!(event_name.as_str(), "check_run" | "check_suite" | "status")
                || action.contains("check")
                || action.contains("status")
                || action.contains("ci")
        }
        "merge" => action.contains("merge"),
        "close" => {
            event_name == "pull_request" && (action.contains("close") || action.contains("closed"))
        }
        other => action.contains(other),
    })
}

fn unsubscribe(params: UnsubscribeInput) -> Result<usize> {
    let mut subscriptions = load_subscriptions()?;
    let mut removed = 0;
    let repo_parts = params
        .repo
        .as_deref()
        .map(|repo| normalize_repo(params.owner.as_deref(), repo))
        .transpose()?;

    for subscription in &mut subscriptions {
        let matched = if let Some(id) = params.subscription_id.as_deref() {
            subscription.id == id
        } else if let Some((owner, repo)) = repo_parts.as_ref() {
            subscription.owner.eq_ignore_ascii_case(owner)
                && subscription.repo.eq_ignore_ascii_case(repo)
                && params.pr_number == Some(subscription.pr_number)
                && params
                    .team
                    .as_deref()
                    .map(|team| team == subscription.team_name)
                    .unwrap_or(true)
        } else {
            false
        };
        if matched && subscription.active {
            subscription.active = false;
            removed += 1;
        }
    }

    if removed > 0 {
        save_subscriptions(&subscriptions)?;
    }
    Ok(removed)
}

fn load_subscriptions() -> Result<Vec<PrActivitySubscription>> {
    let path = crate::storage_paths::pr_activity_subscriptions_path();
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str(&raw).with_context(|| format!("failed to parse {}", path.display()))
}

fn save_subscriptions(subscriptions: &[PrActivitySubscription]) -> Result<()> {
    let path = crate::storage_paths::pr_activity_subscriptions_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let raw = serde_json::to_string_pretty(subscriptions)?;
    fs::write(&path, raw).with_context(|| format!("failed to write {}", path.display()))
}

fn normalize_repo(owner: Option<&str>, repo: &str) -> Result<(String, String)> {
    let repo = repo.trim();
    if repo.is_empty() {
        bail!("repo is required");
    }
    if let Some((parsed_owner, parsed_repo)) = repo.split_once('/') {
        let parsed_owner = parsed_owner.trim();
        let parsed_repo = parsed_repo.trim();
        if parsed_owner.is_empty() || parsed_repo.is_empty() {
            bail!("repo must be in owner/name format when it contains '/'");
        }
        return Ok((parsed_owner.to_string(), parsed_repo.to_string()));
    }

    let Some(owner) = owner.map(str::trim).filter(|value| !value.is_empty()) else {
        bail!("owner is required when repo is not owner/name");
    };
    Ok((owner.to_string(), repo.to_string()))
}

fn format_activity_message(activity: &GithubPrActivity) -> String {
    let mut text = format!(
        "GitHub PR activity: {}/{}#{} {}",
        activity.owner, activity.repo, activity.pr_number, activity.action
    );
    if let Some(sender) = activity.sender.as_deref() {
        text.push_str(&format!(" by {sender}"));
    }
    if let Some(title) = activity.title.as_deref() {
        text.push_str(&format!("\nTitle: {title}"));
    }
    if let Some(url) = activity.html_url.as_deref() {
        text.push_str(&format!("\nURL: {url}"));
    }
    if let Some(delivery_id) = activity.delivery_id.as_deref() {
        text.push_str(&format!("\nDelivery: {delivery_id}"));
    }
    text
}

fn default_active() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::helpers;
    use serial_test::serial;

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

    fn sample_payload() -> Value {
        json!({
            "action": "synchronize",
            "repository": {
                "name": "allthecodes",
                "owner": { "login": "AIclassmanager" }
            },
            "pull_request": {
                "number": 42,
                "title": "Coordinator phase",
                "html_url": "https://github.com/AIclassmanager/allthecodes/pull/42"
            },
            "sender": { "login": "octocat" }
        })
    }

    fn sample_check_run_payload() -> Value {
        json!({
            "action": "completed",
            "repository": {
                "name": "allthecodes",
                "owner": { "login": "AIclassmanager" }
            },
            "check_run": {
                "name": "ci/build",
                "conclusion": "success",
                "html_url": "https://github.com/AIclassmanager/allthecodes/runs/123",
                "pull_requests": [
                    {
                        "number": 42,
                        "html_url": "https://github.com/AIclassmanager/allthecodes/pull/42"
                    }
                ]
            },
            "sender": { "login": "github-actions[bot]" }
        })
    }

    #[test]
    #[serial]
    fn subscribe_dedupes_and_unsubscribe_is_idempotent() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path().to_str().unwrap());

        let first = subscribe(
            "owner".into(),
            "repo".into(),
            7,
            "team".into(),
            constants::TEAM_LEAD_NAME.into(),
        )
        .unwrap();
        let second = subscribe(
            "OWNER".into(),
            "repo".into(),
            7,
            "team".into(),
            constants::TEAM_LEAD_NAME.into(),
        )
        .unwrap();
        assert_eq!(first.id, second.id);
        assert_eq!(load_subscriptions().unwrap().len(), 1);

        let removed = unsubscribe(UnsubscribeInput {
            subscription_id: Some(first.id),
            owner: None,
            repo: None,
            pr_number: None,
            team: None,
        })
        .unwrap();
        assert_eq!(removed, 1);
        let removed_again = unsubscribe(UnsubscribeInput {
            subscription_id: Some("missing".into()),
            owner: None,
            repo: None,
            pr_number: None,
            team: None,
        })
        .unwrap();
        assert_eq!(removed_again, 0);
    }

    #[test]
    #[serial]
    fn webhook_activity_routes_to_matching_team_mailbox() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path().to_str().unwrap());
        helpers::create_team("phase4", None, None, ".").unwrap();
        subscribe(
            "AIclassmanager".into(),
            "allthecodes".into(),
            42,
            "phase4".into(),
            constants::TEAM_LEAD_NAME.into(),
        )
        .unwrap();
        let activity =
            parse_github_pr_activity(&sample_payload(), Some("pull_request"), Some("delivery-1"))
                .unwrap();

        let routed = route_github_pr_activity(&activity).unwrap();

        assert_eq!(routed.matched, 1);
        assert_eq!(routed.delivered, 1);
        let inbox = mailbox::read_mailbox(constants::TEAM_LEAD_NAME, "phase4").unwrap();
        assert_eq!(inbox.len(), 1);
        assert!(inbox[0].text.contains("AIclassmanager/allthecodes#42"));
        assert!(inbox[0].text.contains("delivery-1"));
    }

    #[test]
    fn parse_github_pr_activity_rejects_unrelated_events() {
        assert!(parse_github_pr_activity(&sample_payload(), Some("push"), None).is_none());
    }

    #[test]
    fn parse_github_pr_activity_accepts_ci_events() {
        let activity =
            parse_github_pr_activity(&sample_check_run_payload(), Some("check_run"), None).unwrap();

        assert_eq!(activity.owner, "AIclassmanager");
        assert_eq!(activity.repo, "allthecodes");
        assert_eq!(activity.pr_number, 42);
        assert_eq!(activity.event_name.as_deref(), Some("check_run"));
        assert!(activity.action.contains("ci"));
        assert!(activity.action.contains("success"));
        assert!(subscription_events_match(
            &["ci".to_string()],
            activity.event_name.as_deref(),
            &activity.action,
        ));
    }
}
