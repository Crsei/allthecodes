//! AI-assisted agent generation — mirrors upstream `generateAgent()`.
//!
//! The frontend sends `AgentSettingsCommand::Generate { user_prompt,
//! existing_names }`. The agent-definition handler fires a
//! `GenerateStarted` marker synchronously, then spawns a task here that
//! calls the model via [`allthecodes_api::api::client::ApiClient::messages`] and posts
//! a `Generated` or `Error` event onto the subsystem event bus.
//!
//! The system prompt is kept verbatim from the upstream TypeScript
//! implementation (`src/components/agents/generateAgent.ts`) so the JSON
//! contract (`{identifier, whenToUse, systemPrompt}`) stays compatible with
//! any future frontend that was built against the Node version.

use std::sync::LazyLock;
use std::time::Duration;

use parking_lot::Mutex;
use tokio::sync::broadcast::Sender;
use uuid::Uuid;

use allthecodes_ipc_protocol::subsystem_events::{AgentSettingsEvent, SubsystemEvent};

// ---------------------------------------------------------------------------
// Event-bus wiring
// ---------------------------------------------------------------------------

/// Global handle the headless runtime plugs in on startup so async generation
/// tasks can push events back out without owning the bus themselves. Mirrors
/// the pattern used by subsystem event sinks and friends.
static EVENT_TX: LazyLock<Mutex<Option<Sender<SubsystemEvent>>>> =
    LazyLock::new(|| Mutex::new(None));

/// Install the event sender. Called once from `HeadlessRuntime::run`.
pub fn set_event_sender(tx: Sender<SubsystemEvent>) {
    *EVENT_TX.lock() = Some(tx);
}

fn emit(event: AgentSettingsEvent) {
    if let Some(tx) = EVENT_TX.lock().as_ref() {
        let _ = tx.send(SubsystemEvent::AgentSettings(Box::new(event)));
    }
}

// ---------------------------------------------------------------------------
// Generation entrypoint
// ---------------------------------------------------------------------------

/// Kick off a generation task. Returns immediately; the result comes back as
/// an event on the subsystem bus.
pub fn spawn_generation(user_prompt: String, existing_names: Vec<String>) {
    // If we're inside a Tokio runtime (the usual path — invoked from ingress
    // dispatch), spawn a task. Outside a runtime (e.g. some tests), block on
    // a fresh current-thread runtime so at least something happens.
    if tokio::runtime::Handle::try_current().is_ok() {
        tokio::spawn(async move {
            run_generation(user_prompt, existing_names).await;
        });
    } else {
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(error) => {
                emit(AgentSettingsEvent::Error {
                    name: "__generate__".into(),
                    error: format!("Failed to create generation runtime: {error}"),
                });
                return;
            }
        };
        runtime.block_on(async { run_generation(user_prompt, existing_names).await });
    }
}

async fn run_generation(user_prompt: String, existing_names: Vec<String>) {
    let Some(client) = allthecodes_api::api::client::ApiClient::from_auth() else {
        emit(AgentSettingsEvent::Error {
            name: "__generate__".into(),
            error: "No API provider configured. Run /login or set ANTHROPIC_API_KEY.".into(),
        });
        return;
    };

    let model = client.config().default_model.clone();
    let existing_list = if existing_names.is_empty() {
        String::new()
    } else {
        format!(
            "\n\nIMPORTANT: The following identifiers already exist and must NOT be used: {}",
            existing_names.join(", ")
        )
    };
    let user_content = format!(
        "Create an agent configuration based on this request: \"{}\".{}\n  Return ONLY the JSON object, no other text.",
        user_prompt.replace('"', "\\\""),
        existing_list
    );

    let request = allthecodes_api::api::client::MessagesRequest {
        model,
        messages: vec![serde_json::json!({
            "role": "user",
            "content": user_content,
        })],
        system: Some(vec![serde_json::json!({
            "type": "text",
            "text": AGENT_CREATION_SYSTEM_PROMPT,
        })]),
        max_tokens: 4096,
        tools: None,
        stream: false,
        metadata: None,
        service_tier: None,
        stop_sequences: None,
        temperature: None,
        top_p: None,
        top_k: None,
        context_management: None,
        thinking: None,
        output_config: None,
        tool_choice: None,
        advisor_model: None,
        reasoning_effort: None,
    };

    let result = tokio::time::timeout(Duration::from_secs(120), client.messages(request)).await;

    match result {
        Err(_) => emit(AgentSettingsEvent::Error {
            name: "__generate__".into(),
            error: "Timed out after 120s waiting for the model".into(),
        }),
        Ok(Err(e)) => emit(AgentSettingsEvent::Error {
            name: "__generate__".into(),
            error: format!("model call failed: {}", e),
        }),
        Ok(Ok(assistant)) => match parse_generated(assistant) {
            Ok(parsed) => emit(AgentSettingsEvent::Generated {
                identifier: parsed.identifier,
                when_to_use: parsed.when_to_use,
                system_prompt: parsed.system_prompt,
            }),
            Err(msg) => emit(AgentSettingsEvent::Error {
                name: "__generate__".into(),
                error: msg,
            }),
        },
    }
}

// ---------------------------------------------------------------------------
// JSON extraction
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct GeneratedAgent {
    identifier: String,
    when_to_use: String,
    system_prompt: String,
}

fn parse_generated(
    msg: allthecodes_types::message::AssistantMessage,
) -> Result<GeneratedAgent, String> {
    use allthecodes_types::message::ContentBlock;

    let mut text_parts: Vec<String> = Vec::new();
    for block in &msg.content {
        if let ContentBlock::Text { text } = block {
            text_parts.push(text.clone());
        }
    }
    if text_parts.is_empty() {
        return Err("model returned no text content".into());
    }
    let body = text_parts.join("\n");

    // Try the whole thing first; if that fails, fall back to the first JSON
    // object in the body. Matches upstream `generateAgent.ts`.
    let value: serde_json::Value = match serde_json::from_str(body.trim()) {
        Ok(v) => v,
        Err(_) => {
            let Some(start) = body.find('{') else {
                return Err("no JSON object found in model response".into());
            };
            let Some(end) = body.rfind('}') else {
                return Err("truncated JSON in model response".into());
            };
            serde_json::from_str(&body[start..=end])
                .map_err(|e| format!("invalid JSON from model: {}", e))?
        }
    };

    let identifier = value
        .get("identifier")
        .and_then(|v| v.as_str())
        .ok_or("missing `identifier` in model response")?
        .trim()
        .to_string();
    let when_to_use = value
        .get("whenToUse")
        .and_then(|v| v.as_str())
        .ok_or("missing `whenToUse` in model response")?
        .trim()
        .to_string();
    let system_prompt = value
        .get("systemPrompt")
        .and_then(|v| v.as_str())
        .ok_or("missing `systemPrompt` in model response")?
        .trim()
        .to_string();

    if identifier.is_empty() || when_to_use.is_empty() || system_prompt.is_empty() {
        return Err("model returned blank fields".into());
    }
    // Swallow the uuid so the caller can't exfiltrate the raw response in
    // a debug log without going through this validation.
    let _ = Uuid::new_v4();
    Ok(GeneratedAgent {
        identifier,
        when_to_use,
        system_prompt,
    })
}

// ---------------------------------------------------------------------------
// The agent-creation system prompt (verbatim from upstream generateAgent.ts)
// ---------------------------------------------------------------------------

const AGENT_CREATION_SYSTEM_PROMPT: &str = r#"You are an elite AI agent architect specializing in crafting high-performance agent configurations. Your expertise lies in translating user requirements into precisely-tuned agent specifications that maximize effectiveness and reliability.

**Important Context**: You may have access to project-specific instructions from AGENTS.md files (or CLAUDE.md as fallback) and other context that may include coding standards, project structure, and custom requirements. Consider this context when creating agents to ensure they align with the project's established patterns and practices.

When a user describes what they want an agent to do, you will:

1. **Extract Core Intent**: Identify the fundamental purpose, key responsibilities, and success criteria for the agent. Look for both explicit requirements and implicit needs. Consider any project-specific context from AGENTS.md (or CLAUDE.md) files. For agents that are meant to review code, you should assume that the user is asking to review recently written code and not the whole codebase, unless the user has explicitly instructed you otherwise.

2. **Design Expert Persona**: Create a compelling expert identity that embodies deep domain knowledge relevant to the task. The persona should inspire confidence and guide the agent's decision-making approach.

3. **Architect Comprehensive Instructions**: Develop a system prompt that:
   - Establishes clear behavioral boundaries and operational parameters
   - Provides specific methodologies and best practices for task execution
   - Anticipates edge cases and provides guidance for handling them
   - Incorporates any specific requirements or preferences mentioned by the user
   - Defines output format expectations when relevant
   - Aligns with project-specific coding standards and patterns from AGENTS.md (or CLAUDE.md)

4. **Optimize for Performance**: Include:
   - Decision-making frameworks appropriate to the domain
   - Quality control mechanisms and self-verification steps
   - Efficient workflow patterns
   - Clear escalation or fallback strategies

5. **Create Identifier**: Design a concise, descriptive identifier that:
   - Uses lowercase letters, numbers, and hyphens only
   - Is typically 2-4 words joined by hyphens
   - Clearly indicates the agent's primary function
   - Is memorable and easy to type
   - Avoids generic terms like "helper" or "assistant"

6. **Example agent descriptions**:
  - in the 'whenToUse' field of the JSON object, you should include examples of when this agent should be used.
  - examples should be of the form:
    - <example>
      Context: The user is creating a test-runner agent that should be called after a logical chunk of code is written.
      user: "Please write a function that checks if a number is prime"
      assistant: "Here is the relevant function: "
      <function call omitted for brevity only for this example>
      <commentary>
      Since a significant piece of code was written, use the Agent tool to launch the test-runner agent to run the tests.
      </commentary>
      assistant: "Now let me use the test-runner agent to run the tests"
    </example>
    - <example>
      Context: User is creating an agent to respond to the word "hello" with a friendly joke.
      user: "Hello"
      assistant: "I'm going to use the Agent tool to launch the greeting-responder agent to respond with a friendly joke"
      <commentary>
      Since the user is greeting, use the greeting-responder agent to respond with a friendly joke.
      </commentary>
    </example>
  - If the user mentioned or implied that the agent should be used proactively, you should include examples of this.
- NOTE: Ensure that in the examples, you are making the assistant use the Agent tool and not simply respond directly to the task.

Your output must be a valid JSON object with exactly these fields:
{
  "identifier": "A unique, descriptive identifier using lowercase letters, numbers, and hyphens (e.g., 'test-runner', 'api-docs-writer', 'code-formatter')",
  "whenToUse": "A precise, actionable description starting with 'Use this agent when...' that clearly defines the triggering conditions and use cases. Ensure you include examples as described above.",
  "systemPrompt": "The complete system prompt that will govern the agent's behavior, written in second person ('You are...', 'You will...') and structured for maximum clarity and effectiveness"
}

Key principles for your system prompts:
- Be specific rather than generic - avoid vague instructions
- Include concrete examples when they would clarify behavior
- Balance comprehensiveness with clarity - every instruction should add value
- Ensure the agent has enough context to handle variations of the core task
- Make the agent proactive in seeking clarification when needed
- Build in quality assurance and self-correction mechanisms

Remember: The agents you create should be autonomous experts capable of handling their designated tasks with minimal additional guidance. Your system prompts are their complete operational manual.
"#;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_types::message::AssistantMessage;
    use allthecodes_types::message::ContentBlock;
    use uuid::Uuid;

    fn assistant_with_text(text: &str) -> AssistantMessage {
        AssistantMessage {
            uuid: Uuid::new_v4(),
            timestamp: 0,
            role: "assistant".into(),
            content: vec![ContentBlock::Text {
                text: text.to_string(),
            }],
            usage: None,
            stop_reason: None,
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        }
    }

    #[test]
    fn parse_generated_accepts_raw_json() {
        let raw = r#"{"identifier":"test-runner","whenToUse":"Use when writing tests","systemPrompt":"You run tests."}"#;
        let parsed = parse_generated(assistant_with_text(raw)).unwrap();
        assert_eq!(parsed.identifier, "test-runner");
        assert_eq!(parsed.when_to_use, "Use when writing tests");
        assert_eq!(parsed.system_prompt, "You run tests.");
    }

    #[test]
    fn parse_generated_extracts_json_from_wrapping_text() {
        let raw = "Here is the agent: \n{\"identifier\":\"foo\",\"whenToUse\":\"bar\",\"systemPrompt\":\"baz\"}\nhope that helps";
        let parsed = parse_generated(assistant_with_text(raw)).unwrap();
        assert_eq!(parsed.identifier, "foo");
    }

    #[test]
    fn parse_generated_errors_on_missing_field() {
        let raw = r#"{"identifier":"x","whenToUse":"y"}"#;
        let err = parse_generated(assistant_with_text(raw)).unwrap_err();
        assert!(err.contains("systemPrompt"));
    }

    #[test]
    fn parse_generated_errors_on_empty_content() {
        let msg = AssistantMessage {
            uuid: Uuid::new_v4(),
            timestamp: 0,
            role: "assistant".into(),
            content: vec![],
            usage: None,
            stop_reason: None,
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        };
        assert!(parse_generated(msg).is_err());
    }

    #[test]
    fn parse_generated_errors_on_blank_fields() {
        let raw = r#"{"identifier":"  ","whenToUse":"  ","systemPrompt":"  "}"#;
        let err = parse_generated(assistant_with_text(raw)).unwrap_err();
        assert!(err.contains("blank"));
    }
}
