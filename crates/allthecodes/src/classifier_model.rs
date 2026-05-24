/// Auto-mode classifier model that uses ApiClient directly.
///
/// This avoids depending on QueryDeps (which only exists inside the
/// query loop) — the ApiClient is available at engine-build time so the
/// classifier callback can be installed before submit_message runs.
use std::sync::Arc;

use allthecodes_api::api::client::{ApiClient, MessagesRequest};
use allthecodes_permissions::decision::AutoClassifierStage;
use allthecodes_safety::classifier::{
    assistant_text, SafetyClassifierModel, SafetyClassifierPrompt,
};
use async_trait::async_trait;

const FAST_MAX_TOKENS: usize = 512;
const THINKING_MAX_TOKENS: usize = 4096;
const THINKING_BUDGET_TOKENS: usize = 2048;

pub(crate) struct ApiClientClassifierModel {
    pub(crate) client: Arc<ApiClient>,
    pub(crate) model: String,
}

fn request_limits_for_stage(stage: AutoClassifierStage) -> (usize, Option<serde_json::Value>) {
    match stage {
        AutoClassifierStage::Fast => (FAST_MAX_TOKENS, None),
        AutoClassifierStage::Thinking => (
            THINKING_MAX_TOKENS,
            Some(serde_json::json!({
                "type": "enabled",
                "budget_tokens": THINKING_BUDGET_TOKENS
            })),
        ),
    }
}

#[async_trait]
impl SafetyClassifierModel for ApiClientClassifierModel {
    fn model_name(&self, _stage: AutoClassifierStage) -> String {
        self.model.clone()
    }

    async fn complete(
        &self,
        stage: AutoClassifierStage,
        prompt: SafetyClassifierPrompt,
    ) -> anyhow::Result<String> {
        let (max_tokens, thinking) = request_limits_for_stage(stage);
        let request = MessagesRequest {
            model: self.model.clone(),
            messages: vec![serde_json::json!({"role": "user", "content": prompt.user})],
            system: Some(vec![
                serde_json::json!({"type": "text", "text": prompt.system}),
            ]),
            max_tokens,
            tools: None,
            stream: false,
            metadata: None,
            service_tier: None,
            stop_sequences: None,
            temperature: None,
            top_p: None,
            top_k: None,
            context_management: None,
            thinking,
            output_config: None,
            tool_choice: None,
            reasoning_effort: None,
            advisor_model: None,
        };
        let response = self.client.messages(request).await?;
        Ok(assistant_text(&response))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thinking_stage_budget_fits_under_max_tokens() {
        let (max_tokens, thinking) = request_limits_for_stage(AutoClassifierStage::Thinking);
        let budget = thinking
            .as_ref()
            .and_then(|value| value.get("budget_tokens"))
            .and_then(serde_json::Value::as_u64)
            .expect("thinking budget present");

        assert!(budget < max_tokens as u64);
    }

    #[test]
    fn fast_stage_omits_thinking() {
        let (max_tokens, thinking) = request_limits_for_stage(AutoClassifierStage::Fast);
        assert_eq!(max_tokens, FAST_MAX_TOKENS);
        assert!(thinking.is_none());
    }
}
