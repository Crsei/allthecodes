use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;

use allthecodes_config::settings::AutoModeSettings;
use allthecodes_permissions::decision::{
    AutoClassifierDecision, AutoClassifierStage, AutoClassifierVerdict,
};
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use regex::Regex;
use serde_json::{json, Value};
use uuid::Uuid;

use allthecodes_engine::types::tool::PermissionMode;
use allthecodes_query::deps::{ModelCallParams, QueryDeps};
use allthecodes_types::message::{
    AssistantMessage, ContentBlock, Message, MessageContent, SystemSubtype, ToolResultContent,
    UserMessage,
};

const AUTO_MODE_SYSTEM_PROMPT: &str = r#"You are the cc-rust safety classifier for tool use.
Return exactly one JSON object and no prose.
Allowed verdicts are "allow", "deny", and "ask".
Use "allow" only when the tool use is clearly safe under the policy and transcript.
Use "deny" for destructive, credential-exposing, privilege-escalating, or policy-violating actions.
Use "ask" for ambiguity, missing context, or cases needing user confirmation.
Do not reveal or rely on secrets; redacted values are intentionally unavailable."#;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SafetyClassifierPurpose {
    AutoModeToolUse,
    PlanAllowedPrompt,
}

impl SafetyClassifierPurpose {
    fn as_str(self) -> &'static str {
        match self {
            SafetyClassifierPurpose::AutoModeToolUse => "auto_mode_tool_use",
            SafetyClassifierPurpose::PlanAllowedPrompt => "plan_allowed_prompt",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClassifierBudget {
    pub max_transcript_bytes: usize,
    pub max_prompt_bytes: usize,
    pub max_estimated_tokens: usize,
}

impl Default for ClassifierBudget {
    fn default() -> Self {
        Self {
            max_transcript_bytes: 48 * 1024,
            max_prompt_bytes: 64 * 1024,
            max_estimated_tokens: 16_000,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SafetyClassifierRequest {
    pub purpose: SafetyClassifierPurpose,
    pub tool_name: String,
    pub tool_input: Value,
    pub tool_classifier_input: Value,
    pub transcript: Vec<Message>,
    pub cwd: PathBuf,
    pub permission_mode: PermissionMode,
    pub sandbox_mode: Option<String>,
    pub auto_mode_policy: AutoModeSettings,
    pub hook_context: Option<Value>,
}

pub struct AutoModeToolClassifierInput {
    pub tool_name: String,
    pub tool_input: Value,
    pub tool_classifier_input: Value,
    pub transcript: Vec<Message>,
    pub cwd: PathBuf,
    pub permission_mode: PermissionMode,
    pub sandbox_mode: Option<String>,
    pub auto_mode_policy: AutoModeSettings,
}

impl SafetyClassifierRequest {
    pub fn auto_mode_tool(
        tool_name: impl Into<String>,
        tool_input: Value,
        transcript: Vec<Message>,
        cwd: PathBuf,
        permission_mode: PermissionMode,
        sandbox_mode: Option<String>,
        auto_mode_policy: AutoModeSettings,
    ) -> Self {
        Self::auto_mode_tool_with_classifier_input(AutoModeToolClassifierInput {
            tool_name: tool_name.into(),
            tool_input: tool_input.clone(),
            tool_classifier_input: tool_input,
            transcript,
            cwd,
            permission_mode,
            sandbox_mode,
            auto_mode_policy,
        })
    }

    pub fn auto_mode_tool_with_classifier_input(input: AutoModeToolClassifierInput) -> Self {
        Self {
            purpose: SafetyClassifierPurpose::AutoModeToolUse,
            tool_name: input.tool_name,
            tool_input: input.tool_input,
            tool_classifier_input: input.tool_classifier_input,
            transcript: input.transcript,
            cwd: input.cwd,
            permission_mode: input.permission_mode,
            sandbox_mode: input.sandbox_mode,
            auto_mode_policy: input.auto_mode_policy,
            hook_context: None,
        }
    }

    fn is_high_risk(&self) -> bool {
        let command = shell_command_from_input(&self.tool_input)
            .or_else(|| shell_command_from_input(&self.tool_classifier_input));
        let Some(command) = command else {
            return false;
        };

        match self.tool_name.as_str() {
            "Bash" => allthecodes_permissions::dangerous::is_dangerous_command(&command).is_some(),
            "PowerShell" => {
                allthecodes_permissions::dangerous::is_dangerous_powershell_command(&command)
                    .is_some()
            }
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SafetyClassifierPrompt {
    pub system: String,
    pub user: String,
}

#[async_trait]
pub trait SafetyClassifierModel: Send + Sync {
    fn model_name(&self, stage: AutoClassifierStage) -> String;
    async fn complete(
        &self,
        stage: AutoClassifierStage,
        prompt: SafetyClassifierPrompt,
    ) -> Result<String>;
}

pub struct QueryDepsSafetyClassifierModel {
    deps: Arc<dyn QueryDeps>,
    model: Option<String>,
    max_output_tokens: usize,
}

impl QueryDepsSafetyClassifierModel {
    pub fn new(deps: Arc<dyn QueryDeps>, model: Option<String>) -> Self {
        Self {
            deps,
            model,
            max_output_tokens: 512,
        }
    }
}

#[async_trait]
impl SafetyClassifierModel for QueryDepsSafetyClassifierModel {
    fn model_name(&self, _stage: AutoClassifierStage) -> String {
        self.model
            .clone()
            .unwrap_or_else(|| "main-loop-model".to_string())
    }

    async fn complete(
        &self,
        stage: AutoClassifierStage,
        prompt: SafetyClassifierPrompt,
    ) -> Result<String> {
        let response = self
            .deps
            .call_model(ModelCallParams {
                messages: vec![Message::User(UserMessage {
                    uuid: Uuid::new_v4(),
                    timestamp: chrono::Utc::now().timestamp_millis(),
                    role: "user".to_string(),
                    content: MessageContent::Text(prompt.user),
                    is_meta: false,
                    tool_use_result: None,
                    source_tool_assistant_uuid: None,
                })],
                system_prompt: vec![prompt.system],
                tools: Vec::new(),
                model: self.model.clone(),
                max_output_tokens: Some(self.max_output_tokens),
                skip_cache_write: Some(true),
                thinking_enabled: Some(stage == AutoClassifierStage::Thinking),
                effort_value: Some(match stage {
                    AutoClassifierStage::Fast => "low".to_string(),
                    AutoClassifierStage::Thinking => "medium".to_string(),
                }),
                advisor_model: None,
            })
            .await?;

        Ok(assistant_text(&response.assistant_message))
    }
}

pub struct SharedSafetyClassifier {
    model: Arc<dyn SafetyClassifierModel>,
    budget: ClassifierBudget,
}

impl SharedSafetyClassifier {
    pub fn new(model: Arc<dyn SafetyClassifierModel>) -> Self {
        Self {
            model,
            budget: ClassifierBudget::default(),
        }
    }

    pub fn with_budget(mut self, budget: ClassifierBudget) -> Self {
        self.budget = budget;
        self
    }

    pub async fn classify(&self, request: &SafetyClassifierRequest) -> AutoClassifierDecision {
        let fast = self.run_stage(request, AutoClassifierStage::Fast).await;
        if fast.decision.unavailable || fast.decision.transcript_too_long {
            return fast.decision;
        }
        if fast.decision.verdict == AutoClassifierVerdict::Deny {
            return fast.decision;
        }
        if fast.escalate
            || fast.decision.verdict == AutoClassifierVerdict::Ask
            || request.is_high_risk()
        {
            return self
                .run_stage(request, AutoClassifierStage::Thinking)
                .await
                .decision;
        }
        fast.decision
    }

    async fn run_stage(
        &self,
        request: &SafetyClassifierRequest,
        stage: AutoClassifierStage,
    ) -> StageResult {
        let model = self.model.model_name(stage);
        let prompt = match build_stage_prompt(request, stage, self.budget) {
            Ok(prompt) => prompt,
            Err(PromptBuildError::TranscriptTooLong(reason)) => {
                return StageResult {
                    decision: AutoClassifierDecision::transcript_too_long(model, reason),
                    escalate: false,
                };
            }
        };

        let raw = match self.model.complete(stage, prompt).await {
            Ok(raw) => raw,
            Err(err) => {
                return StageResult {
                    decision: AutoClassifierDecision::unavailable(
                        model,
                        format!("classifier unavailable: {err}"),
                    ),
                    escalate: false,
                };
            }
        };

        match parse_classifier_response(&raw, model.clone(), stage) {
            Ok(parsed) => parsed,
            Err(err) => StageResult {
                decision: AutoClassifierDecision::unavailable(
                    model,
                    format!("malformed classifier response: {err}"),
                ),
                escalate: false,
            },
        }
    }
}

#[derive(Debug, Clone)]
struct StageResult {
    decision: AutoClassifierDecision,
    escalate: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PromptBuildError {
    TranscriptTooLong(String),
}

fn build_stage_prompt(
    request: &SafetyClassifierRequest,
    stage: AutoClassifierStage,
    budget: ClassifierBudget,
) -> std::result::Result<SafetyClassifierPrompt, PromptBuildError> {
    let transcript = render_transcript_window(&request.transcript, budget.max_transcript_bytes)?;
    let policy = render_auto_mode_policy(&request.auto_mode_policy);
    let hook_context = request
        .hook_context
        .as_ref()
        .map(pretty_json)
        .unwrap_or_else(|| "null".to_string());
    let tool_input = if request.tool_input == request.tool_classifier_input {
        pretty_json(&request.tool_input)
    } else {
        "[omitted: tool-specific classifier input is provided below]".to_string()
    };

    let user = format!(
        "purpose: {purpose}\nstage: {stage:?}\npermission_mode: {permission_mode:?}\nsandbox_mode: {sandbox_mode}\ncwd: {cwd}\ntool_name: {tool_name}\ntool_input:\n{tool_input}\ntool_classifier_input:\n{tool_classifier_input}\nauto_mode_policy:\n{policy}\nhook_context:\n{hook_context}\nrecent_transcript:\n{transcript}\n\nReturn JSON: {{\"verdict\":\"allow|deny|ask\",\"reason\":\"short reason\",\"thinking\":\"optional private summary\",\"escalate\":false}}\n",
        purpose = request.purpose.as_str(),
        permission_mode = request.permission_mode,
        sandbox_mode = request.sandbox_mode.as_deref().unwrap_or("unknown"),
        cwd = request.cwd.display(),
        tool_name = request.tool_name,
        tool_input = tool_input,
        tool_classifier_input = pretty_json(&request.tool_classifier_input),
    );

    let redacted_user = redact_classifier_text(&user);
    if redacted_user.len() > budget.max_prompt_bytes {
        return Err(PromptBuildError::TranscriptTooLong(format!(
            "classifier prompt exceeded {} bytes after redaction",
            budget.max_prompt_bytes
        )));
    }
    if estimate_tokens(&redacted_user) > budget.max_estimated_tokens {
        return Err(PromptBuildError::TranscriptTooLong(format!(
            "classifier prompt exceeded estimated {} token budget",
            budget.max_estimated_tokens
        )));
    }

    Ok(SafetyClassifierPrompt {
        system: AUTO_MODE_SYSTEM_PROMPT.to_string(),
        user: redacted_user,
    })
}

fn render_auto_mode_policy(policy: &AutoModeSettings) -> String {
    pretty_json(&json!({
        "environment": policy.environment,
        "allow": policy.allow,
        "soft_deny": policy.soft_deny,
        "extra": policy.extra,
    }))
}

fn render_transcript_window(
    messages: &[Message],
    max_bytes: usize,
) -> std::result::Result<String, PromptBuildError> {
    if messages.is_empty() {
        return Ok("[empty]".to_string());
    }

    let mut selected = VecDeque::new();
    let mut total = 0usize;
    let mut omitted = 0usize;

    for message in messages.iter().rev() {
        let rendered = redact_classifier_text(&render_message_for_classifier(message));
        let bytes = rendered.len();
        if bytes > max_bytes {
            return Err(PromptBuildError::TranscriptTooLong(format!(
                "single transcript message exceeded {max_bytes} bytes after redaction"
            )));
        }
        let separator = if selected.is_empty() { 0 } else { 2 };
        if total + separator + bytes > max_bytes {
            omitted += 1;
            continue;
        }
        total += separator + bytes;
        selected.push_front(rendered);
    }

    if selected.is_empty() {
        return Err(PromptBuildError::TranscriptTooLong(format!(
            "recent transcript exceeded {max_bytes} byte budget"
        )));
    }

    let mut rendered = selected.into_iter().collect::<Vec<_>>().join("\n\n");
    if omitted > 0 {
        rendered = format!("[{omitted} older message(s) omitted]\n\n{rendered}");
    }
    Ok(rendered)
}

fn render_message_for_classifier(message: &Message) -> String {
    match message {
        Message::User(message) => format!(
            "user{}: {}",
            if message.is_meta { " meta" } else { "" },
            message_content_text(&message.content)
        ),
        Message::Assistant(message) => format!("assistant: {}", assistant_text(message)),
        Message::System(message) => format!(
            "system {}: {}",
            system_subtype_name(&message.subtype),
            message.content
        ),
        Message::Progress(message) => {
            format!(
                "progress {}: {}",
                message.tool_use_id,
                pretty_json(&message.data)
            )
        }
        Message::Attachment(message) => {
            format!("attachment: {}", pretty_json(&json!(message.attachment)))
        }
    }
}

fn message_content_text(content: &MessageContent) -> String {
    match content {
        MessageContent::Text(text) => text.clone(),
        MessageContent::Blocks(blocks) => blocks
            .iter()
            .map(content_block_text)
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

pub fn assistant_text(message: &AssistantMessage) -> String {
    message
        .content
        .iter()
        .map(content_block_text)
        .collect::<Vec<_>>()
        .join("\n")
}

fn content_block_text(block: &ContentBlock) -> String {
    match block {
        ContentBlock::Text { text } => text.clone(),
        ContentBlock::ToolUse { id, name, input }
        | ContentBlock::ServerToolUse { id, name, input } => {
            format!("tool_use id={id} name={name} input={}", pretty_json(input))
        }
        ContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => format!(
            "tool_result id={tool_use_id} is_error={is_error} content={}",
            tool_result_text(content)
        ),
        ContentBlock::Thinking { .. } => "[thinking omitted]".to_string(),
        ContentBlock::RedactedThinking { .. } => "[redacted thinking omitted]".to_string(),
        ContentBlock::ConnectorText { connector_text, .. } => connector_text.clone(),
        ContentBlock::Image { .. } => "[image omitted]".to_string(),
    }
}

fn tool_result_text(content: &ToolResultContent) -> String {
    match content {
        ToolResultContent::Text(text) => text.clone(),
        ToolResultContent::Blocks(blocks) => blocks
            .iter()
            .map(content_block_text)
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

fn system_subtype_name(subtype: &SystemSubtype) -> &'static str {
    match subtype {
        SystemSubtype::CompactBoundary { .. } => "compact_boundary",
        SystemSubtype::MicrocompactBoundary { .. } => "microcompact_boundary",
        SystemSubtype::ApiError { .. } => "api_error",
        SystemSubtype::Informational { .. } => "informational",
        SystemSubtype::LocalCommand { .. } => "local_command",
        SystemSubtype::Warning => "warning",
    }
}

fn parse_classifier_response(
    raw: &str,
    model: String,
    stage: AutoClassifierStage,
) -> Result<StageResult> {
    let value: Value = serde_json::from_str(raw.trim()).context("response is not strict JSON")?;
    let object = value
        .as_object()
        .ok_or_else(|| anyhow!("response is not a JSON object"))?;
    let verdict = object
        .get("verdict")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("missing string verdict"))?;
    let verdict = match verdict.to_ascii_lowercase().as_str() {
        "allow" => AutoClassifierVerdict::Allow,
        "deny" => AutoClassifierVerdict::Deny,
        "ask" => AutoClassifierVerdict::Ask,
        other => return Err(anyhow!("unknown verdict `{other}`")),
    };
    let reason = object
        .get("reason")
        .and_then(Value::as_str)
        .unwrap_or("classifier did not provide a reason")
        .trim();
    if reason.is_empty() {
        return Err(anyhow!("empty reason"));
    }
    let thinking = object
        .get("thinking")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned);
    let escalate = object
        .get("escalate")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    Ok(StageResult {
        decision: AutoClassifierDecision {
            verdict,
            reason: reason.to_string(),
            model,
            stage: Some(stage),
            thinking,
            unavailable: false,
            transcript_too_long: false,
        },
        escalate,
    })
}

pub fn redact_classifier_text(input: &str) -> String {
    const SECRET_KEY_PATTERN: &str =
        r#"api[_-]?key|token|access[_-]?token|refresh[_-]?token|secret|password|passwd"#;
    let patterns = [
        (
            r#"(?i)\b(authorization)\s*[:=]\s*bearer\s+["']?[^"'\s,}]+["']?"#,
            "$1: Bearer <redacted>",
        ),
        (
            r#"(?i)\bbearer\s+[a-z0-9._~+/=-]{12,}"#,
            "Bearer <redacted>",
        ),
        (
            r#"\bsk-[A-Za-z0-9][A-Za-z0-9_-]{10,}\b"#,
            "<redacted-secret>",
        ),
        (r#"\bsk-ant-[A-Za-z0-9_-]{10,}\b"#, "<redacted-secret>"),
        (r#"\bAIza[0-9A-Za-z_-]{20,}\b"#, "<redacted-secret>"),
    ];

    let mut out = input.to_string();
    if let Ok(quoted_secret_value) = Regex::new(&format!(
        r#"(?i)(?P<prefix>["']?(?:{})["']?\s*[:=]\s*)(?P<quote>["'])[^"']*(?P<close>["'])"#,
        SECRET_KEY_PATTERN
    )) {
        out = quoted_secret_value
            .replace_all(&out, |caps: &regex::Captures| {
                format!(
                    "{}{}<redacted>{}",
                    &caps["prefix"], &caps["quote"], &caps["close"]
                )
            })
            .into_owned();
    }

    if let Ok(unquoted_secret_value) = Regex::new(&format!(
        r#"(?i)(?P<prefix>["']?(?:{})["']?\s*[:=]\s*)[^"'\s,}}\]]+"#,
        SECRET_KEY_PATTERN
    )) {
        out = unquoted_secret_value
            .replace_all(&out, "${prefix}<redacted>")
            .into_owned();
    }

    for (pattern, replacement) in patterns {
        if let Ok(regex) = Regex::new(pattern) {
            out = regex.replace_all(&out, replacement).into_owned();
        }
    }
    out
}

fn pretty_json(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

fn estimate_tokens(input: &str) -> usize {
    input.len().div_ceil(4)
}

fn shell_command_from_input(input: &Value) -> Option<String> {
    input
        .get("command")
        .or_else(|| input.get("cmd"))
        .or_else(|| input.get("script"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use allthecodes_config::settings::AutoModeSettings;
    use anyhow::bail;

    use super::*;

    struct MockClassifierModel {
        responses: Mutex<VecDeque<Result<String>>>,
        prompts: Mutex<Vec<(AutoClassifierStage, SafetyClassifierPrompt)>>,
    }

    impl MockClassifierModel {
        fn new(responses: Vec<Result<String>>) -> Self {
            Self {
                responses: Mutex::new(responses.into()),
                prompts: Mutex::new(Vec::new()),
            }
        }

        fn prompts(&self) -> Vec<(AutoClassifierStage, SafetyClassifierPrompt)> {
            self.prompts.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl SafetyClassifierModel for MockClassifierModel {
        fn model_name(&self, stage: AutoClassifierStage) -> String {
            match stage {
                AutoClassifierStage::Fast => "mock-fast".to_string(),
                AutoClassifierStage::Thinking => "mock-thinking".to_string(),
            }
        }

        async fn complete(
            &self,
            stage: AutoClassifierStage,
            prompt: SafetyClassifierPrompt,
        ) -> Result<String> {
            self.prompts.lock().unwrap().push((stage, prompt));
            let mut responses = self.responses.lock().unwrap();
            match responses.pop_front() {
                Some(response) => response,
                None => bail!("no mock classifier response left"),
            }
        }
    }

    fn request(command: &str) -> SafetyClassifierRequest {
        SafetyClassifierRequest::auto_mode_tool(
            "Bash",
            json!({ "command": command }),
            vec![Message::User(UserMessage {
                uuid: Uuid::new_v4(),
                timestamp: 1,
                role: "user".to_string(),
                content: MessageContent::Text("run the safe verification".to_string()),
                is_meta: false,
                tool_use_result: None,
                source_tool_assistant_uuid: None,
            })],
            PathBuf::from("F:/repo"),
            PermissionMode::Auto,
            Some("workspace".to_string()),
            AutoModeSettings {
                environment: vec!["Trusted repo".to_string()],
                allow: vec!["Allow cargo verification".to_string()],
                soft_deny: vec!["Avoid network writes".to_string()],
                extra: Default::default(),
            },
        )
    }

    #[tokio::test]
    async fn fast_allow_returns_allow() {
        let model = Arc::new(MockClassifierModel::new(vec![Ok(
            r#"{"verdict":"allow","reason":"cargo test is narrow verification"}"#.to_string(),
        )]));
        let classifier = SharedSafetyClassifier::new(model.clone());

        let decision = classifier.classify(&request("cargo test")).await;

        assert_eq!(decision.verdict, AutoClassifierVerdict::Allow);
        assert_eq!(decision.stage, Some(AutoClassifierStage::Fast));
        assert_eq!(model.prompts().len(), 1);
    }

    #[tokio::test]
    async fn fast_ask_escalates_to_thinking() {
        let model = Arc::new(MockClassifierModel::new(vec![
            Ok(r#"{"verdict":"ask","reason":"ambiguous","escalate":true}"#.to_string()),
            Ok(r#"{"verdict":"deny","reason":"would remove files"}"#.to_string()),
        ]));
        let classifier = SharedSafetyClassifier::new(model.clone());

        let decision = classifier.classify(&request("rm -rf target")).await;

        assert_eq!(decision.verdict, AutoClassifierVerdict::Deny);
        assert_eq!(decision.stage, Some(AutoClassifierStage::Thinking));
        assert_eq!(model.prompts().len(), 2);
    }

    #[tokio::test]
    async fn high_risk_fast_allow_still_runs_thinking() {
        let model = Arc::new(MockClassifierModel::new(vec![
            Ok(r#"{"verdict":"allow","reason":"looks okay"}"#.to_string()),
            Ok(r#"{"verdict":"ask","reason":"sudo requires confirmation"}"#.to_string()),
        ]));
        let classifier = SharedSafetyClassifier::new(model.clone());

        let decision = classifier.classify(&request("rm -rf /")).await;

        assert_eq!(decision.verdict, AutoClassifierVerdict::Ask);
        assert_eq!(decision.stage, Some(AutoClassifierStage::Thinking));
        assert_eq!(model.prompts().len(), 2);
    }

    #[tokio::test]
    async fn malformed_response_fails_closed_to_unavailable_ask() {
        let model = Arc::new(MockClassifierModel::new(vec![Ok(
            "allow because it is fine".to_string(),
        )]));
        let classifier = SharedSafetyClassifier::new(model);

        let decision = classifier.classify(&request("cargo test")).await;

        assert_eq!(decision.verdict, AutoClassifierVerdict::Ask);
        assert!(decision.unavailable);
        assert!(decision.reason.contains("malformed classifier response"));
    }

    #[tokio::test]
    async fn model_error_fails_closed_to_unavailable_ask() {
        let model = Arc::new(MockClassifierModel::new(vec![Err(anyhow!("offline"))]));
        let classifier = SharedSafetyClassifier::new(model);

        let decision = classifier.classify(&request("cargo test")).await;

        assert_eq!(decision.verdict, AutoClassifierVerdict::Ask);
        assert!(decision.unavailable);
        assert!(decision.reason.contains("offline"));
    }

    #[tokio::test]
    async fn transcript_too_long_fails_closed_before_model_call() {
        let model = Arc::new(MockClassifierModel::new(vec![]));
        let classifier = SharedSafetyClassifier::new(model.clone()).with_budget(ClassifierBudget {
            max_transcript_bytes: 8,
            max_prompt_bytes: 256,
            max_estimated_tokens: 64,
        });

        let decision = classifier.classify(&request("cargo test")).await;

        assert_eq!(decision.verdict, AutoClassifierVerdict::Ask);
        assert!(decision.transcript_too_long);
        assert!(model.prompts().is_empty());
    }

    #[test]
    fn redaction_removes_common_secret_shapes() {
        let input = "ANTHROPIC_API_KEY=sk-ant-abcdef1234567890 Authorization: Bearer token-very-secret password=hunter2";
        let redacted = redact_classifier_text(input);

        assert!(!redacted.contains("abcdef1234567890"));
        assert!(!redacted.contains("token-very-secret"));
        assert!(!redacted.contains("hunter2"));
        assert!(redacted.contains("<redacted>") || redacted.contains("<redacted-secret>"));
    }

    #[test]
    fn redaction_removes_json_secret_fields() {
        let input = r#"{"password":"hunter2","apiKey":"sk-json-secret","api_key":"sk_snake","accessToken":"abc123","refresh-token":"def456","secret":'topsecret'}"#;
        let redacted = redact_classifier_text(input);

        for secret in [
            "hunter2",
            "sk-json-secret",
            "sk_snake",
            "abc123",
            "def456",
            "topsecret",
        ] {
            assert!(!redacted.contains(secret), "secret value leaked: {secret}");
        }
        assert!(redacted.contains(r#""password":"<redacted>""#));
        assert!(redacted.contains(r#""apiKey":"<redacted>""#));
        assert!(redacted.contains(r#""api_key":"<redacted>""#));
        assert!(redacted.contains(r#""accessToken":"<redacted>""#));
        assert!(redacted.contains(r#""refresh-token":"<redacted>""#));
        assert!(redacted.contains(r#""secret":'<redacted>'"#));
    }

    #[test]
    fn prompt_shape_is_stable_and_redacted() {
        let mut req = request("echo $ANTHROPIC_API_KEY");
        req.tool_classifier_input = json!({
            "command": "echo",
            "env": "OPENAI_API_KEY=sk-testsecret1234567890",
            "metadata": {
                "password": "hunter2",
                "accessToken": "abc123"
            }
        });

        let prompt =
            build_stage_prompt(&req, AutoClassifierStage::Fast, ClassifierBudget::default())
                .expect("prompt builds");

        assert!(!prompt.user.contains("sk-testsecret1234567890"));
        assert!(!prompt.user.contains("hunter2"));
        assert!(!prompt.user.contains("abc123"));
        insta::assert_snapshot!(
            prompt
                .user
                .lines()
                .take(19)
                .collect::<Vec<_>>()
                .join("\n"),
            @r###"
purpose: auto_mode_tool_use
stage: Fast
permission_mode: Auto
sandbox_mode: workspace
cwd: F:/repo
tool_name: Bash
tool_input:
[omitted: tool-specific classifier input is provided below]
tool_classifier_input:
{
  "command": "echo",
  "env": "OPENAI_API_KEY=<redacted>",
  "metadata": {
    "accessToken": "<redacted>",
    "password": "<redacted>"
  }
}
auto_mode_policy:
{
"###
        );
    }

    #[test]
    fn invalid_verdict_is_rejected() {
        let err = parse_classifier_response(
            r#"{"verdict":"maybe","reason":"not strict"}"#,
            "mock".to_string(),
            AutoClassifierStage::Fast,
        )
        .expect_err("invalid verdict rejected");

        assert!(err.to_string().contains("unknown verdict"));
    }
}
