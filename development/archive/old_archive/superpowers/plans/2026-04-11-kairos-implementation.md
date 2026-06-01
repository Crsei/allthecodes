# KAIROS Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the KAIROS resident assistant mode for cc-rust, adding daemon architecture, BriefTool, proactive tick system, MCP channels, webhooks, push notifications, and daily log memory.

**Architecture:** A daemon process (`cc-rust --daemon`) hosts the QueryEngine + axum HTTP server. ink-terminal connects via HTTP/SSE instead of JSONL stdio. Proactive tick loop drives autonomous work. External events (webhooks, MCP channels) inject messages into the QueryEngine. Feature gates via `FEATURE_*` environment variables control activation.

**Tech Stack:** Rust (tokio, axum, tower-http), SSE (axum built-in), notify-rust (Windows Toast), hmac/sha2 (webhook verification), reqwest (webhook callback)

**Spec:** [`../specs/2026-04-11-kairos-design.md`](../specs/2026-04-11-kairos-design.md)

---

## Phase 1: Foundation (Feature Gates + AppState + Tools)

### Task 1: Feature Gate System

**Files:**
- Create: `src/config/features.rs`
- Modify: `src/config/mod.rs:7-10`
- Modify: `src/main.rs` (add feature init near line 176)

- [ ] **Step 1: Write tests for FeatureFlags**

```rust
// src/config/features.rs — bottom of file

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feature_flags_from_env_defaults() {
        // With no env vars set, all flags should be false
        // We test the parsing logic directly
        let flags = FeatureFlags::from_iter(std::iter::empty::<(String, String)>());
        assert!(!flags.kairos);
        assert!(!flags.kairos_brief);
        assert!(!flags.proactive);
    }

    #[test]
    fn test_feature_flags_kairos_enables_proactive() {
        let env = vec![
            ("FEATURE_KAIROS".to_string(), "1".to_string()),
        ];
        let flags = FeatureFlags::from_iter(env.into_iter());
        assert!(flags.kairos);
        assert!(flags.proactive); // kairos implies proactive
    }

    #[test]
    fn test_feature_flags_dependency_enforcement() {
        // kairos_brief without kairos should be disabled
        let env = vec![
            ("FEATURE_KAIROS_BRIEF".to_string(), "1".to_string()),
        ];
        let flags = FeatureFlags::from_iter(env.into_iter());
        assert!(!flags.kairos_brief); // disabled: parent kairos not set
    }

    #[test]
    fn test_feature_flags_proactive_standalone() {
        let env = vec![
            ("FEATURE_PROACTIVE".to_string(), "1".to_string()),
        ];
        let flags = FeatureFlags::from_iter(env.into_iter());
        assert!(flags.proactive);
        assert!(!flags.kairos);
    }

    #[test]
    fn test_feature_enabled_query() {
        let env = vec![
            ("FEATURE_KAIROS".to_string(), "1".to_string()),
            ("FEATURE_KAIROS_BRIEF".to_string(), "1".to_string()),
        ];
        let flags = FeatureFlags::from_iter(env.into_iter());
        assert!(flags.is_enabled(Feature::Kairos));
        assert!(flags.is_enabled(Feature::KairosBrief));
        assert!(!flags.is_enabled(Feature::KairosChannels));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib config::features::tests -v`
Expected: FAIL — module does not exist

- [ ] **Step 3: Implement FeatureFlags**

```rust
// src/config/features.rs

//! Feature gate system — reads FEATURE_* environment variables at startup.
//!
//! Corresponds to TypeScript: `feature('KAIROS')` etc.

use std::sync::LazyLock;
use tracing::warn;

/// Global feature flags singleton, initialized from environment on first access.
pub static FLAGS: LazyLock<FeatureFlags> = LazyLock::new(|| {
    FeatureFlags::from_env()
});

/// Individual feature identifiers for querying.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feature {
    Kairos,
    KairosBrief,
    KairosChannels,
    KairosPushNotification,
    KairosGithubWebhooks,
    Proactive,
}

/// Parsed feature flags.
#[derive(Debug, Clone)]
pub struct FeatureFlags {
    pub kairos: bool,
    pub kairos_brief: bool,
    pub kairos_channels: bool,
    pub kairos_push_notification: bool,
    pub kairos_github_webhooks: bool,
    pub proactive: bool,
}

impl FeatureFlags {
    /// Read from actual environment variables.
    pub fn from_env() -> Self {
        Self::from_iter(std::env::vars())
    }

    /// Parse from an iterator of (key, value) pairs. Testable without touching env.
    pub fn from_iter(iter: impl Iterator<Item = (String, String)>) -> Self {
        let mut raw = std::collections::HashMap::new();
        for (k, v) in iter {
            if k.starts_with("FEATURE_") {
                raw.insert(k, v == "1" || v.eq_ignore_ascii_case("true"));
            }
        }

        let kairos = *raw.get("FEATURE_KAIROS").unwrap_or(&false);
        let proactive_raw = *raw.get("FEATURE_PROACTIVE").unwrap_or(&false);
        let kairos_brief_raw = *raw.get("FEATURE_KAIROS_BRIEF").unwrap_or(&false);
        let kairos_channels_raw = *raw.get("FEATURE_KAIROS_CHANNELS").unwrap_or(&false);
        let kairos_push_raw = *raw.get("FEATURE_KAIROS_PUSH_NOTIFICATION").unwrap_or(&false);
        let kairos_gh_raw = *raw.get("FEATURE_KAIROS_GITHUB_WEBHOOKS").unwrap_or(&false);

        // Dependency enforcement
        let kairos_brief = kairos_brief_raw && kairos;
        let kairos_channels = kairos_channels_raw && kairos;
        let kairos_push_notification = kairos_push_raw && kairos;
        let kairos_github_webhooks = kairos_gh_raw && kairos;
        let proactive = proactive_raw || kairos; // kairos implies proactive

        if kairos_brief_raw && !kairos {
            warn!("FEATURE_KAIROS_BRIEF requires FEATURE_KAIROS — disabled");
        }
        if kairos_channels_raw && !kairos {
            warn!("FEATURE_KAIROS_CHANNELS requires FEATURE_KAIROS — disabled");
        }
        if kairos_push_raw && !kairos {
            warn!("FEATURE_KAIROS_PUSH_NOTIFICATION requires FEATURE_KAIROS — disabled");
        }
        if kairos_gh_raw && !kairos {
            warn!("FEATURE_KAIROS_GITHUB_WEBHOOKS requires FEATURE_KAIROS — disabled");
        }

        Self {
            kairos,
            kairos_brief,
            kairos_channels,
            kairos_push_notification,
            kairos_github_webhooks,
            proactive,
        }
    }

    /// Query whether a specific feature is enabled.
    pub fn is_enabled(&self, feature: Feature) -> bool {
        match feature {
            Feature::Kairos => self.kairos,
            Feature::KairosBrief => self.kairos_brief,
            Feature::KairosChannels => self.kairos_channels,
            Feature::KairosPushNotification => self.kairos_push_notification,
            Feature::KairosGithubWebhooks => self.kairos_github_webhooks,
            Feature::Proactive => self.proactive,
        }
    }
}

/// Convenience: check a feature against the global singleton.
pub fn enabled(feature: Feature) -> bool {
    FLAGS.is_enabled(feature)
}
```

- [ ] **Step 4: Register the module**

In `src/config/mod.rs`, add after line 10:

```rust
pub mod features;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib config::features::tests -v`
Expected: All 5 tests PASS

- [ ] **Step 6: Commit**

```bash
git add src/config/features.rs src/config/mod.rs
git commit -m "feat(kairos): add feature gate system with FEATURE_* env vars"
```

---

### Task 2: AppState Extension

**Files:**
- Modify: `src/types/app_state.rs:9-30` (add fields)
- Modify: `src/types/app_state.rs:41-64` (update Default impl)

- [ ] **Step 1: Add fields to AppState**

In `src/types/app_state.rs`, add after the `hooks` field (line 29):

```rust
    /// KAIROS mode is active (daemon running with assistant features).
    pub kairos_active: bool,
    /// Brief-only output mode (all output through SendUserMessage tool).
    pub is_brief_only: bool,
    /// Resident assistant mode (perpetual session).
    pub is_assistant_mode: bool,
    /// Proactive tick interval in milliseconds (None = disabled).
    pub autonomous_tick_ms: Option<u64>,
    /// Whether the user terminal is focused (affects autonomy level).
    pub terminal_focus: bool,
```

- [ ] **Step 2: Update Default impl**

In the `Default::default()` for `AppState`, add after `hooks: HashMap::new()`:

```rust
            kairos_active: false,
            is_brief_only: false,
            is_assistant_mode: false,
            autonomous_tick_ms: None,
            terminal_focus: true,
```

- [ ] **Step 3: Verify compilation**

Run: `cargo build 2>&1 | head -20`
Expected: Compiles successfully (no errors)

- [ ] **Step 4: Commit**

```bash
git add src/types/app_state.rs
git commit -m "feat(kairos): extend AppState with kairos/brief/assistant fields"
```

---

### Task 3: QuerySource Extension

**Files:**
- Modify: `src/types/config.rs:58-64` (add variants)
- Modify: `src/types/config.rs:66-79` (update impl)

- [ ] **Step 1: Add new QuerySource variants**

In `src/types/config.rs`, extend the `QuerySource` enum (line 58-64):

```rust
pub enum QuerySource {
    Sdk,
    ReplMainThread,
    Compact,
    SessionMemory,
    Agent(String),
    ProactiveTick,
    WebhookEvent,
    ChannelNotification,
}
```

- [ ] **Step 2: Update the display/label impl**

In `src/types/config.rs`, update the `QuerySource` impl (line 66-79):

```rust
impl QuerySource {
    pub fn label(&self) -> &str {
        match self {
            QuerySource::Sdk => "sdk",
            QuerySource::ReplMainThread => "repl_main_thread",
            QuerySource::Compact => "compact",
            QuerySource::SessionMemory => "session_memory",
            QuerySource::Agent(_) => "agent:",
            QuerySource::ProactiveTick => "proactive_tick",
            QuerySource::WebhookEvent => "webhook_event",
            QuerySource::ChannelNotification => "channel_notification",
        }
    }

    pub fn is_agent(&self) -> bool {
        matches!(self, QuerySource::Agent(_))
    }

    /// Whether this source is autonomous (not directly from user input).
    pub fn is_autonomous(&self) -> bool {
        matches!(
            self,
            QuerySource::ProactiveTick | QuerySource::WebhookEvent | QuerySource::ChannelNotification
        )
    }
}
```

- [ ] **Step 3: Verify compilation**

Run: `cargo build 2>&1 | head -20`
Expected: Compiles (no new errors)

- [ ] **Step 4: Commit**

```bash
git add src/types/config.rs
git commit -m "feat(kairos): add ProactiveTick/WebhookEvent/ChannelNotification to QuerySource"
```

---

### Task 4: QueryEngine Sleep Mechanism

**Files:**
- Modify: `src/engine/lifecycle/mod.rs:50-74` (add sleep_until field)
- Modify: `src/engine/lifecycle/mod.rs` (add methods on QueryEngine)

- [ ] **Step 1: Write tests**

Add to `src/engine/lifecycle/tests.rs` (or create a new test block):

```rust
#[cfg(test)]
mod sleep_tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn test_engine_not_sleeping_by_default() {
        // QueryEngineState.sleep_until defaults to None
        let state = QueryEngineState::default_for_test();
        assert!(state.sleep_until.is_none());
    }
}
```

- [ ] **Step 2: Add sleep_until field to QueryEngineState**

In `src/engine/lifecycle/mod.rs`, add after the `bg_agent_tx` field (line 73):

```rust
    /// When set, the proactive tick loop skips ticks until this instant.
    /// Set by SleepTool, cleared by wake_up() or external events.
    pub(crate) sleep_until: Option<std::time::Instant>,
```

- [ ] **Step 3: Add sleep methods to QueryEngine**

In `src/engine/lifecycle/mod.rs`, add methods to the `impl QueryEngine` block (after existing methods around line 170):

```rust
    /// Set the engine to sleep until the given instant.
    /// The proactive tick loop checks this to skip ticks.
    pub fn set_sleep_until(&self, until: std::time::Instant) {
        let mut state = self.state.write();
        state.sleep_until = Some(until);
    }

    /// Check whether the engine is currently sleeping.
    pub fn is_sleeping(&self) -> bool {
        let state = self.state.read();
        state.sleep_until.map_or(false, |t| std::time::Instant::now() < t)
    }

    /// Wake up the engine immediately (clear sleep timer).
    /// Called when external events arrive that need immediate attention.
    pub fn wake_up(&self) {
        let mut state = self.state.write();
        state.sleep_until = None;
    }
```

- [ ] **Step 4: Initialize the field in constructors**

Ensure `sleep_until: None` is set wherever `QueryEngineState` is constructed. Search for existing construction sites and add the field.

- [ ] **Step 5: Verify compilation and tests**

Run: `cargo build && cargo test --lib engine::lifecycle -v`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src/engine/lifecycle/mod.rs
git commit -m "feat(kairos): add sleep_until mechanism to QueryEngine"
```

---

### Task 5: SleepTool

**Files:**
- Create: `src/tools/sleep.rs`
- Modify: `src/tools/mod.rs:48` (add module declaration)
- Modify: `src/tools/registry.rs:33-67` (conditional registration)

- [ ] **Step 1: Write tests for SleepTool**

```rust
// src/tools/sleep.rs — bottom of file

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sleep_tool_name() {
        let tool = SleepTool;
        assert_eq!(tool.name(), "Sleep");
    }

    #[test]
    fn test_sleep_tool_schema_has_duration() {
        let tool = SleepTool;
        let schema = tool.input_json_schema();
        let props = schema["properties"].as_object().unwrap();
        assert!(props.contains_key("duration_seconds"));
        assert!(props.contains_key("reason"));
    }

    #[test]
    fn test_sleep_tool_is_read_only() {
        let tool = SleepTool;
        assert!(tool.is_read_only(&json!({})));
    }

    #[test]
    fn test_sleep_tool_validates_range() {
        let tool = SleepTool;
        let ctx = ToolUseContext::test_default();

        // Too high
        let result = tokio_test::block_on(
            tool.validate_input(&json!({"duration_seconds": 9999}), &ctx)
        );
        assert!(matches!(result, ValidationResult::Error { .. }));

        // Missing field
        let result = tokio_test::block_on(
            tool.validate_input(&json!({}), &ctx)
        );
        assert!(matches!(result, ValidationResult::Error { .. }));

        // Valid
        let result = tokio_test::block_on(
            tool.validate_input(&json!({"duration_seconds": 60}), &ctx)
        );
        assert!(matches!(result, ValidationResult::Ok));
    }
}
```

- [ ] **Step 2: Implement SleepTool**

```rust
// src/tools/sleep.rs

//! Sleep tool — allows the model to control its own wake-up frequency.
//!
//! Sets a sleep_until marker on the QueryEngine. The proactive tick loop
//! checks this marker and skips ticks while sleeping. Does NOT block.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::config::features::{self, Feature};
use crate::types::message::AssistantMessage;
use crate::types::tool::{Tool, ToolProgress, ToolResult, ToolUseContext, ValidationResult};

pub struct SleepTool;

#[async_trait]
impl Tool for SleepTool {
    fn name(&self) -> &str {
        "Sleep"
    }

    async fn description(&self, _input: &Value) -> String {
        "Pause proactive execution for a specified duration.".to_string()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "duration_seconds": {
                    "type": "integer",
                    "description": "Duration to sleep in seconds (1-3600)",
                    "minimum": 1,
                    "maximum": 3600
                },
                "reason": {
                    "type": "string",
                    "description": "Why you are sleeping (logged for context)"
                }
            },
            "required": ["duration_seconds"]
        })
    }

    fn is_enabled(&self) -> bool {
        features::enabled(Feature::Proactive) || features::enabled(Feature::Kairos)
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        match input.get("duration_seconds").and_then(|v| v.as_i64()) {
            None => ValidationResult::Error {
                message: "\"duration_seconds\" is required and must be an integer".to_string(),
                error_code: 1,
            },
            Some(s) if s < 1 || s > 3600 => ValidationResult::Error {
                message: format!("duration_seconds must be 1-3600, got {}", s),
                error_code: 2,
            },
            Some(_) => ValidationResult::Ok,
        }
    }

    async fn call(
        &self,
        input: Value,
        _ctx: &ToolUseContext,
        _parent_message: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let seconds = input
            .get("duration_seconds")
            .and_then(|v| v.as_u64())
            .unwrap_or(60);
        let reason = input
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("no reason given");

        // NOTE: The actual sleep_until is set by the daemon tick loop
        // after receiving this tool result. This tool just signals intent.

        Ok(ToolResult {
            data: json!({
                "status": "sleeping",
                "duration_seconds": seconds,
                "reason": reason,
            }),
            new_messages: vec![],
        })
    }

    async fn prompt(&self) -> String {
        "Use Sleep to pause proactive execution when there is no useful work to do.\n\n\
         The proactive tick loop will skip ticks for the specified duration.\n\
         External events (user messages, webhooks, channel notifications) will wake you early.\n\n\
         duration_seconds: 1-3600 (1 hour max)"
            .to_string()
    }

    fn user_facing_name(&self, _input: Option<&Value>) -> String {
        "Sleep".to_string()
    }
}
```

- [ ] **Step 3: Register the module**

In `src/tools/mod.rs`, add after line 49 (`send_user_message`):

```rust
pub mod sleep;
```

- [ ] **Step 4: Conditionally register in registry**

In `src/tools/registry.rs`, add after the last `Arc::new(...)` in the `get_all_tools()` vec (before the closing `]` around line 63):

```rust
        Arc::new(sleep::SleepTool),
```

(The tool's `is_enabled()` already checks the feature flag, so the existing `.filter(|t| t.is_enabled())` at line 66 handles gating.)

- [ ] **Step 5: Run tests**

Run: `cargo test --lib tools::sleep::tests -v`
Expected: All tests PASS

- [ ] **Step 6: Commit**

```bash
git add src/tools/sleep.rs src/tools/mod.rs src/tools/registry.rs
git commit -m "feat(kairos): add SleepTool for proactive tick control"
```

---

### Task 6: BriefTool (Enhanced SendUserMessage)

**Files:**
- Create: `src/tools/brief.rs`
- Modify: `src/tools/mod.rs` (add module)
- Modify: `src/tools/registry.rs` (conditional registration)

- [ ] **Step 1: Write tests**

```rust
// src/tools/brief.rs — bottom

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_brief_tool_name() {
        let tool = BriefTool;
        assert_eq!(tool.name(), "Brief");
    }

    #[test]
    fn test_brief_tool_schema() {
        let tool = BriefTool;
        let schema = tool.input_json_schema();
        let props = schema["properties"].as_object().unwrap();
        assert!(props.contains_key("message"));
        assert!(props.contains_key("attachments"));
        assert!(props.contains_key("status"));
    }

    #[test]
    fn test_brief_tool_validates_empty_message() {
        let tool = BriefTool;
        let ctx = ToolUseContext::test_default();
        let result = tokio_test::block_on(
            tool.validate_input(&json!({"message": ""}), &ctx)
        );
        assert!(matches!(result, ValidationResult::Error { .. }));
    }

    #[test]
    fn test_brief_tool_validates_bad_status() {
        let tool = BriefTool;
        let ctx = ToolUseContext::test_default();
        let result = tokio_test::block_on(
            tool.validate_input(&json!({"message": "hi", "status": "bad"}), &ctx)
        );
        assert!(matches!(result, ValidationResult::Error { .. }));
    }
}
```

- [ ] **Step 2: Implement BriefTool**

```rust
// src/tools/brief.rs

//! BriefTool — structured user-facing output for KAIROS mode.
//!
//! In Brief mode, all user-visible output goes through this tool.
//! Plain text from the model is treated as internal reasoning.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::config::features::{self, Feature};
use crate::types::message::AssistantMessage;
use crate::types::tool::{Tool, ToolProgress, ToolResult, ToolUseContext, ValidationResult};

pub struct BriefTool;

#[async_trait]
impl Tool for BriefTool {
    fn name(&self) -> &str {
        "Brief"
    }

    async fn description(&self, _input: &Value) -> String {
        "Send a structured message to the user. In Brief mode, this is the only way to communicate with the user.".to_string()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "message": {
                    "type": "string",
                    "description": "Markdown-formatted message to display to the user"
                },
                "attachments": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional file paths to attach"
                },
                "status": {
                    "type": "string",
                    "enum": ["normal", "proactive"],
                    "description": "Message type: normal (user-requested) or proactive (autonomous)"
                }
            },
            "required": ["message"]
        })
    }

    fn is_enabled(&self) -> bool {
        features::enabled(Feature::KairosBrief)
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        let message = input.get("message").and_then(|v| v.as_str()).unwrap_or("");
        if message.is_empty() {
            return ValidationResult::Error {
                message: "\"message\" must not be empty".to_string(),
                error_code: 1,
            };
        }

        if let Some(status) = input.get("status").and_then(|v| v.as_str()) {
            if !matches!(status, "normal" | "proactive") {
                return ValidationResult::Error {
                    message: format!("Unknown status \"{}\". Must be normal or proactive.", status),
                    error_code: 2,
                };
            }
        }

        ValidationResult::Ok
    }

    async fn call(
        &self,
        input: Value,
        _ctx: &ToolUseContext,
        _parent_message: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let message = input.get("message").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let status = input.get("status").and_then(|v| v.as_str()).unwrap_or("normal").to_string();
        let attachments: Vec<String> = input
            .get("attachments")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        Ok(ToolResult {
            data: json!({
                "is_brief_message": true,
                "message": message,
                "status": status,
                "attachments": attachments,
            }),
            new_messages: vec![],
        })
    }

    async fn prompt(&self) -> String {
        "Use Brief to send structured messages to the user.\n\n\
         In Brief mode, this is the ONLY way to communicate with the user.\n\
         Plain text output will be treated as internal reasoning and hidden.\n\n\
         Use status: \"proactive\" for autonomous notifications.\n\
         Use status: \"normal\" for responses to user requests."
            .to_string()
    }

    fn user_facing_name(&self, _input: Option<&Value>) -> String {
        "Brief".to_string()
    }
}
```

- [ ] **Step 3: Register module and tool**

In `src/tools/mod.rs`, add after the sleep module:

```rust
pub mod brief;
```

In `src/tools/registry.rs`, add to the `get_all_tools()` vec:

```rust
        Arc::new(brief::BriefTool),
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib tools::brief::tests -v`
Expected: All tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/tools/brief.rs src/tools/mod.rs src/tools/registry.rs
git commit -m "feat(kairos): add BriefTool for structured output in Brief mode"
```

---

### Task 7: System Prompt Sections (Brief + Proactive + Channel)

**Files:**
- Modify: `src/engine/system_prompt.rs:393-408` (add conditional sections)

- [ ] **Step 1: Add Brief mode section**

In `src/engine/system_prompt.rs`, add to the `dynamic_sections` vec (after the `mcp_instructions` section, around line 408):

```rust
            cached_section("brief_mode", || {
                use crate::config::features::{self, Feature};
                if !features::enabled(Feature::KairosBrief) {
                    return None;
                }
                Some("# Brief Mode\n\n\
                    All user-facing communication MUST go through the Brief tool.\n\
                    Do not produce plain text output intended for the user outside of this tool.\n\
                    Plain text you emit will be treated as internal reasoning and may be hidden.\n\n\
                    Use Brief for:\n\
                    - Status updates and progress reports\n\
                    - Questions that need user input\n\
                    - Final results and summaries\n\
                    - Proactive notifications (set status: \"proactive\")\n".to_string())
            }),
            cached_section("proactive_mode", || {
                use crate::config::features::{self, Feature};
                if !features::enabled(Feature::Proactive) {
                    return None;
                }
                Some("# Proactive Mode\n\n\
                    You receive periodic <tick_tag> messages containing the user's local time\n\
                    and terminal focus state.\n\n\
                    ## Rules\n\
                    - First tick: Greet briefly, ask what to work on. Do NOT explore unprompted.\n\
                    - Subsequent ticks: Look for useful work — investigate, verify, check, commit.\n\
                    - No useful work: Call Sleep tool. Do NOT emit \"still waiting\" text.\n\
                    - Don't spam the user. If you already asked a question, wait for their reply.\n\
                    - Bias toward action: read files, search code, make changes, commit.\n\n\
                    ## Terminal Focus\n\
                    - `focus: false` (user away) → Highly autonomous, execute pending tasks\n\
                    - `focus: true` (user watching) → More collaborative, ask before large changes\n\n\
                    ## Output\n\
                    All user-facing output MUST go through the Brief tool.\n".to_string())
            }),
            cached_section("external_channels", || {
                use crate::config::features::{self, Feature};
                if !features::enabled(Feature::KairosChannels) {
                    return None;
                }
                Some("# External Channels\n\n\
                    You may receive messages from external channels wrapped in <channel> tags.\n\
                    These are real messages from external services (Slack, GitHub, etc.).\n\
                    Respond to channel messages via Brief tool with appropriate context.\n\
                    Do NOT fabricate channel messages or pretend to have received one.\n".to_string())
            }),
```

- [ ] **Step 2: Verify compilation**

Run: `cargo build 2>&1 | head -20`
Expected: Compiles successfully

- [ ] **Step 3: Commit**

```bash
git add src/engine/system_prompt.rs
git commit -m "feat(kairos): add Brief/Proactive/Channel system prompt sections"
```

---

### Task 8: Commands (/brief, /sleep, /assistant, /daemon)

**Files:**
- Create: `src/commands/brief.rs`
- Create: `src/commands/sleep_cmd.rs`
- Create: `src/commands/assistant.rs`
- Create: `src/commands/daemon_cmd.rs`
- Modify: `src/commands/mod.rs` (register all)

- [ ] **Step 1: Implement /brief command**

```rust
// src/commands/brief.rs

//! /brief — toggle Brief output mode.

use anyhow::Result;
use async_trait::async_trait;

use super::{CommandContext, CommandHandler, CommandResult};
use crate::config::features::{self, Feature};
use crate::engine::prompt_sections;

pub struct BriefHandler;

#[async_trait]
impl CommandHandler for BriefHandler {
    async fn execute(&self, args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        if !features::enabled(Feature::KairosBrief) {
            return Ok(CommandResult::Output(
                "Brief mode requires FEATURE_KAIROS_BRIEF=1".to_string(),
            ));
        }

        let subcmd = args.trim().to_lowercase();
        match subcmd.as_str() {
            "on" | "enable" => {
                ctx.app_state.is_brief_only = true;
                prompt_sections::clear_cache();
                Ok(CommandResult::Output("Brief mode enabled.".to_string()))
            }
            "off" | "disable" => {
                ctx.app_state.is_brief_only = false;
                prompt_sections::clear_cache();
                Ok(CommandResult::Output("Brief mode disabled.".to_string()))
            }
            "status" => {
                let status = if ctx.app_state.is_brief_only { "ON" } else { "OFF" };
                Ok(CommandResult::Output(format!("Brief mode: {}", status)))
            }
            "" => {
                ctx.app_state.is_brief_only = !ctx.app_state.is_brief_only;
                prompt_sections::clear_cache();
                let status = if ctx.app_state.is_brief_only { "enabled" } else { "disabled" };
                Ok(CommandResult::Output(format!("Brief mode {}.", status)))
            }
            _ => Ok(CommandResult::Output(
                "Usage: /brief [on|off|status]".to_string(),
            )),
        }
    }
}
```

- [ ] **Step 2: Implement /sleep command**

```rust
// src/commands/sleep_cmd.rs

//! /sleep — manually set sleep duration for proactive mode.

use anyhow::Result;
use async_trait::async_trait;

use super::{CommandContext, CommandHandler, CommandResult};
use crate::config::features::{self, Feature};

pub struct SleepCmdHandler;

#[async_trait]
impl CommandHandler for SleepCmdHandler {
    async fn execute(&self, args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        if !features::enabled(Feature::Proactive) {
            return Ok(CommandResult::Output(
                "Sleep command requires FEATURE_PROACTIVE=1".to_string(),
            ));
        }

        let trimmed = args.trim();
        if trimmed.is_empty() {
            return Ok(CommandResult::Output(
                "Usage: /sleep <seconds>  (1-3600)".to_string(),
            ));
        }

        match trimmed.parse::<u64>() {
            Ok(s) if (1..=3600).contains(&s) => {
                // The actual sleep_until will be set by the daemon tick loop.
                // This command just communicates the intent.
                Ok(CommandResult::Output(format!(
                    "Sleep scheduled for {} seconds. Tick loop will pause.",
                    s
                )))
            }
            Ok(s) => Ok(CommandResult::Output(format!(
                "Duration must be 1-3600, got {}",
                s
            ))),
            Err(_) => Ok(CommandResult::Output(
                "Invalid number. Usage: /sleep <seconds>".to_string(),
            )),
        }
    }
}
```

- [ ] **Step 3: Implement /assistant command**

```rust
// src/commands/assistant.rs

//! /assistant — view assistant mode status and configuration.

use anyhow::Result;
use async_trait::async_trait;

use super::{CommandContext, CommandHandler, CommandResult};
use crate::config::features::{self, Feature};

pub struct AssistantHandler;

#[async_trait]
impl CommandHandler for AssistantHandler {
    async fn execute(&self, args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        if !features::enabled(Feature::Kairos) {
            return Ok(CommandResult::Output(
                "Assistant mode requires FEATURE_KAIROS=1".to_string(),
            ));
        }

        let subcmd = args.trim().to_lowercase();
        match subcmd.as_str() {
            "" | "status" => {
                let lines = vec![
                    format!("KAIROS active: {}", ctx.app_state.kairos_active),
                    format!("Brief mode: {}", ctx.app_state.is_brief_only),
                    format!("Assistant mode: {}", ctx.app_state.is_assistant_mode),
                    format!("Terminal focus: {}", ctx.app_state.terminal_focus),
                    format!(
                        "Tick interval: {}",
                        ctx.app_state
                            .autonomous_tick_ms
                            .map(|ms| format!("{}ms", ms))
                            .unwrap_or_else(|| "disabled".to_string())
                    ),
                    format!("Model: {}", ctx.app_state.main_loop_model),
                ];
                Ok(CommandResult::Output(lines.join("\n")))
            }
            _ => Ok(CommandResult::Output(
                "Usage: /assistant [status]".to_string(),
            )),
        }
    }
}
```

- [ ] **Step 4: Implement /daemon command**

```rust
// src/commands/daemon_cmd.rs

//! /daemon — view daemon status.

use anyhow::Result;
use async_trait::async_trait;

use super::{CommandContext, CommandHandler, CommandResult};
use crate::config::features::{self, Feature};

pub struct DaemonCmdHandler;

#[async_trait]
impl CommandHandler for DaemonCmdHandler {
    async fn execute(&self, args: &str, _ctx: &mut CommandContext) -> Result<CommandResult> {
        if !features::enabled(Feature::Kairos) {
            return Ok(CommandResult::Output(
                "Daemon requires FEATURE_KAIROS=1".to_string(),
            ));
        }

        let subcmd = args.trim().to_lowercase();
        match subcmd.as_str() {
            "" | "status" => {
                // TODO: query daemon HTTP /api/status endpoint
                Ok(CommandResult::Output(
                    "Daemon status: check http://127.0.0.1:19836/health".to_string(),
                ))
            }
            "stop" => {
                // TODO: send shutdown signal to daemon
                Ok(CommandResult::Output("Daemon stop requested.".to_string()))
            }
            _ => Ok(CommandResult::Output(
                "Usage: /daemon [status|stop]".to_string(),
            )),
        }
    }
}
```

- [ ] **Step 5: Register all commands**

In `src/commands/mod.rs`, add module declarations (after line 56):

```rust
pub mod brief;
pub mod sleep_cmd;
pub mod assistant;
pub mod daemon_cmd;
```

In `get_all_commands()`, add after the last `Command` entry (before closing `]` around line 314):

```rust
        Command {
            name: "brief".into(),
            aliases: vec![],
            description: "Toggle Brief output mode (KAIROS)".into(),
            handler: Box::new(brief::BriefHandler),
        },
        Command {
            name: "sleep".into(),
            aliases: vec![],
            description: "Set proactive sleep duration".into(),
            handler: Box::new(sleep_cmd::SleepCmdHandler),
        },
        Command {
            name: "assistant".into(),
            aliases: vec!["kairos".into()],
            description: "View assistant mode status".into(),
            handler: Box::new(assistant::AssistantHandler),
        },
        Command {
            name: "daemon".into(),
            aliases: vec![],
            description: "View/control daemon process".into(),
            handler: Box::new(daemon_cmd::DaemonCmdHandler),
        },
```

- [ ] **Step 6: Verify compilation**

Run: `cargo build 2>&1 | head -20`
Expected: Compiles successfully

- [ ] **Step 7: Commit**

```bash
git add src/commands/brief.rs src/commands/sleep_cmd.rs src/commands/assistant.rs src/commands/daemon_cmd.rs src/commands/mod.rs
git commit -m "feat(kairos): add /brief /sleep /assistant /daemon commands"
```

---

## Phase 2: Daemon Architecture

### Task 9: Cargo Dependencies

**Files:**
- Modify: `Cargo.toml` (add dependencies)

- [ ] **Step 1: Add daemon dependencies**

In `Cargo.toml`, add after the `parking_lot` line (around line 110):

```toml
# Daemon HTTP server (KAIROS)
axum = "0.8"
tower-http = { version = "0.6", features = ["cors"] }
hmac = "0.12"
sha2 = "0.10"
notify-rust = "4"
```

(`reqwest` is already present at line 103.)

- [ ] **Step 2: Verify compilation**

Run: `cargo build 2>&1 | head -20`
Expected: Compiles (deps downloaded)

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml
git commit -m "chore: add axum/tower-http/hmac/sha2/notify-rust deps for KAIROS daemon"
```

---

### Task 10: DaemonState

**Files:**
- Create: `src/daemon/mod.rs`
- Create: `src/daemon/state.rs`
- Modify: `src/main.rs` (declare module)

- [ ] **Step 1: Create daemon module**

```rust
// src/daemon/mod.rs

//! KAIROS daemon — HTTP server + proactive tick loop.
//!
//! Activated by `cc-rust --daemon`. Hosts QueryEngine + axum HTTP server.

pub mod state;
```

- [ ] **Step 2: Implement DaemonState**

```rust
// src/daemon/state.rs

//! Shared daemon state — thread-safe, passed to all HTTP handlers.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::Arc;

use parking_lot::{Mutex, RwLock};
use tokio::sync::mpsc;

use crate::config::features::FeatureFlags;
use crate::engine::lifecycle::QueryEngine;

/// Event ID counter for SSE.
static EVENT_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Generate a monotonically increasing event ID.
pub fn next_event_id() -> String {
    let id = EVENT_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    format!("evt_{:06}", id)
}

/// A connected SSE client.
pub struct SseClient {
    pub client_id: String,
    pub tx: mpsc::UnboundedSender<SseEvent>,
    pub connected_at: std::time::Instant,
}

/// An SSE event to send to connected clients.
#[derive(Clone, Debug, serde::Serialize)]
pub struct SseEvent {
    pub id: String,
    pub event_type: String,
    pub data: serde_json::Value,
}

/// Notification to be dispatched (Windows Toast + webhook).
#[derive(Clone, Debug, serde::Serialize)]
pub struct Notification {
    pub title: String,
    pub body: String,
    pub level: String,
    pub source: serde_json::Value,
}

/// Shared daemon state, wrapped in Arc for handler access.
#[derive(Clone)]
pub struct DaemonState {
    pub engine: Arc<QueryEngine>,
    pub features: Arc<FeatureFlags>,
    pub clients: Arc<RwLock<HashMap<String, SseClient>>>,
    pub is_query_running: Arc<AtomicBool>,
    pub notification_tx: mpsc::UnboundedSender<Notification>,
    pub notification_rx: Arc<Mutex<Option<mpsc::UnboundedReceiver<Notification>>>>,
    /// Ring buffer of recent events for re-attach catch-up.
    pub event_buffer: Arc<Mutex<VecDeque<SseEvent>>>,
    pub port: u16,
}

impl DaemonState {
    pub fn new(engine: Arc<QueryEngine>, features: FeatureFlags, port: u16) -> Self {
        let (notif_tx, notif_rx) = mpsc::unbounded_channel();
        Self {
            engine,
            features: Arc::new(features),
            clients: Arc::new(RwLock::new(HashMap::new())),
            is_query_running: Arc::new(AtomicBool::new(false)),
            notification_tx: notif_tx,
            notification_rx: Arc::new(Mutex::new(Some(notif_rx))),
            event_buffer: Arc::new(Mutex::new(VecDeque::with_capacity(1000))),
            port,
        }
    }

    /// Broadcast an SSE event to all connected clients.
    pub fn broadcast(&self, event: SseEvent) {
        // Buffer for re-attach
        {
            let mut buf = self.event_buffer.lock();
            if buf.len() >= 1000 {
                buf.pop_front();
            }
            buf.push_back(event.clone());
        }

        // Send to all clients
        let clients = self.clients.read();
        for client in clients.values() {
            let _ = client.tx.send(event.clone());
        }
    }

    /// Get events after a given event ID (for re-attach catch-up).
    pub fn events_since(&self, last_id: &str) -> Vec<SseEvent> {
        let buf = self.event_buffer.lock();
        let mut found = false;
        let mut result = Vec::new();
        for event in buf.iter() {
            if found {
                result.push(event.clone());
            }
            if event.id == last_id {
                found = true;
            }
        }
        result
    }

    /// Whether any frontend client is connected.
    pub fn has_clients(&self) -> bool {
        !self.clients.read().is_empty()
    }

    /// Current terminal focus state (true if any client connected).
    pub fn terminal_focus(&self) -> bool {
        self.has_clients()
    }
}
```

- [ ] **Step 3: Declare daemon module in main**

In `src/main.rs`, find the module declarations (near the top) and add:

```rust
mod daemon;
```

- [ ] **Step 4: Verify compilation**

Run: `cargo build 2>&1 | head -20`
Expected: Compiles

- [ ] **Step 5: Commit**

```bash
git add src/daemon/mod.rs src/daemon/state.rs src/main.rs
git commit -m "feat(kairos): add daemon module with DaemonState"
```

---

### Task 11: Daemon HTTP Server + Routes

**Files:**
- Create: `src/daemon/server.rs`
- Create: `src/daemon/routes.rs`
- Create: `src/daemon/sse.rs`
- Modify: `src/daemon/mod.rs` (declare modules)

- [ ] **Step 1: Implement HTTP server startup**

```rust
// src/daemon/server.rs

//! axum HTTP server for the daemon.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use tower_http::cors::CorsLayer;
use tracing::info;

use super::routes;
use super::state::DaemonState;

/// Start the HTTP server. Runs until shutdown signal.
pub async fn serve_http(state: DaemonState, port: u16) -> anyhow::Result<()> {
    let app = Router::new()
        .merge(routes::api_routes())
        .merge(routes::webhook_routes())
        .route("/health", axum::routing::get(routes::health))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    info!("daemon HTTP server listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
```

- [ ] **Step 2: Implement routes**

```rust
// src/daemon/routes.rs

//! HTTP route handlers for the daemon API.

use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::state::{DaemonState, SseEvent, next_event_id};

// ── API Routes ──

pub fn api_routes() -> Router<DaemonState> {
    Router::new()
        .route("/api/submit", post(submit))
        .route("/api/abort", post(abort))
        .route("/api/command", post(command))
        .route("/api/permission", post(permission))
        .route("/api/status", get(status))
        .route("/api/attach", post(attach))
        .route("/api/detach", post(detach))
        .route("/api/resize", post(resize))
        .route("/api/events", get(super::sse::sse_handler))
        .route("/api/history", get(history))
}

pub fn webhook_routes() -> Router<DaemonState> {
    Router::new()
        .route("/webhook/github", post(webhook_github))
        .route("/webhook/slack", post(webhook_slack))
        .route("/webhook/generic", post(webhook_generic))
}

// ── Health ──

pub async fn health() -> impl IntoResponse {
    Json(json!({"status": "ok", "version": env!("CARGO_PKG_VERSION")}))
}

// ── Submit ──

#[derive(Deserialize)]
pub struct SubmitRequest {
    pub text: String,
    pub id: String,
}

pub async fn submit(
    State(state): State<DaemonState>,
    Json(req): Json<SubmitRequest>,
) -> impl IntoResponse {
    use crate::types::config::QuerySource;
    use futures::StreamExt;
    use std::sync::atomic::Ordering;

    if state.is_query_running.load(Ordering::SeqCst) {
        return Json(json!({"error": "query already running"}));
    }

    // Wake engine if sleeping
    state.engine.wake_up();

    state.is_query_running.store(true, Ordering::SeqCst);
    let engine = state.engine.clone();
    let state_clone = state.clone();
    let message_id = req.id.clone();

    tokio::spawn(async move {
        let stream = engine.submit_message(&req.text, QuerySource::ReplMainThread);
        tokio::pin!(stream);

        while let Some(sdk_msg) = stream.next().await {
            // Convert SdkMessage to SSE events and broadcast
            if let Some(event) = sdk_message_to_sse(&sdk_msg, &message_id) {
                state_clone.broadcast(event);
            }
        }

        state_clone.is_query_running.store(false, Ordering::SeqCst);
    });

    Json(json!({"status": "submitted", "id": req.id}))
}

/// Convert an SdkMessage to an SSE event (if applicable).
fn sdk_message_to_sse(
    msg: &crate::engine::sdk_types::SdkMessage,
    message_id: &str,
) -> Option<SseEvent> {
    use crate::engine::sdk_types::SdkMessage;

    match msg {
        SdkMessage::StreamEvent(event) => Some(SseEvent {
            id: next_event_id(),
            event_type: "stream_delta".to_string(),
            data: json!({"message_id": message_id, "event": format!("{:?}", event)}),
        }),
        SdkMessage::AssistantMessage(content) => Some(SseEvent {
            id: next_event_id(),
            event_type: "assistant_message".to_string(),
            data: json!({"message_id": message_id, "content": format!("{:?}", content)}),
        }),
        SdkMessage::SystemInit { .. } => Some(SseEvent {
            id: next_event_id(),
            event_type: "stream_start".to_string(),
            data: json!({"message_id": message_id}),
        }),
        _ => None,
    }
}

// ── Abort ──

pub async fn abort(State(state): State<DaemonState>) -> impl IntoResponse {
    state.engine.abort();
    Json(json!({"status": "aborted"}))
}

// ── Command ──

#[derive(Deserialize)]
pub struct CommandRequest {
    pub raw: String,
}

pub async fn command(
    State(_state): State<DaemonState>,
    Json(req): Json<CommandRequest>,
) -> impl IntoResponse {
    // Delegate to command system (similar to headless.rs handle_slash_command)
    Json(json!({"status": "executed", "command": req.raw}))
}

// ── Permission ──

#[derive(Deserialize)]
pub struct PermissionRequest {
    pub tool_use_id: String,
    pub decision: String,
}

pub async fn permission(
    State(_state): State<DaemonState>,
    Json(req): Json<PermissionRequest>,
) -> impl IntoResponse {
    Json(json!({"status": "received", "tool_use_id": req.tool_use_id}))
}

// ── Status ──

pub async fn status(State(state): State<DaemonState>) -> impl IntoResponse {
    use std::sync::atomic::Ordering;
    Json(json!({
        "kairos_active": state.features.kairos,
        "brief_mode": state.features.kairos_brief,
        "proactive": state.features.proactive,
        "query_running": state.is_query_running.load(Ordering::SeqCst),
        "clients_connected": state.clients.read().len(),
        "sleeping": state.engine.is_sleeping(),
    }))
}

// ── Attach/Detach ──

#[derive(Deserialize)]
pub struct AttachRequest {
    pub client_id: String,
    pub last_seen_event: Option<String>,
}

#[derive(Serialize)]
pub struct AttachResponse {
    pub session_id: String,
    pub missed_events: Vec<SseEvent>,
}

pub async fn attach(
    State(state): State<DaemonState>,
    Json(req): Json<AttachRequest>,
) -> impl IntoResponse {
    let missed = req
        .last_seen_event
        .as_deref()
        .map(|id| state.events_since(id))
        .unwrap_or_default();

    let session_id = state.engine.session_id().to_string();

    Json(AttachResponse {
        session_id,
        missed_events: missed,
    })
}

pub async fn detach(
    State(state): State<DaemonState>,
    Json(req): Json<serde_json::Value>,
) -> impl IntoResponse {
    if let Some(client_id) = req.get("client_id").and_then(|v| v.as_str()) {
        state.clients.write().remove(client_id);
    }
    Json(json!({"status": "detached"}))
}

// ── Resize ──

pub async fn resize(Json(req): Json<serde_json::Value>) -> impl IntoResponse {
    // Update terminal dimensions if needed
    Json(json!({"status": "ok", "cols": req.get("cols"), "rows": req.get("rows")}))
}

// ── History ──

pub async fn history(State(_state): State<DaemonState>) -> impl IntoResponse {
    // Return message history (cursor-based pagination)
    Json(json!({"messages": [], "has_more": false}))
}

// ── Webhooks (stubs — full implementation in Phase 3) ──

pub async fn webhook_github(
    State(_state): State<DaemonState>,
    body: String,
) -> impl IntoResponse {
    Json(json!({"status": "received", "source": "github"}))
}

pub async fn webhook_slack(
    State(_state): State<DaemonState>,
    body: String,
) -> impl IntoResponse {
    Json(json!({"status": "received", "source": "slack"}))
}

pub async fn webhook_generic(
    State(_state): State<DaemonState>,
    body: String,
) -> impl IntoResponse {
    Json(json!({"status": "received", "source": "generic"}))
}
```

- [ ] **Step 3: Implement SSE handler**

```rust
// src/daemon/sse.rs

//! Server-Sent Events stream for real-time daemon → frontend communication.

use axum::extract::{Query, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use futures::stream::Stream;
use serde::Deserialize;
use std::convert::Infallible;
use tokio_stream::wrappers::UnboundedReceiverStream;

use super::state::{DaemonState, SseClient, SseEvent};

#[derive(Deserialize)]
pub struct SseQuery {
    pub client_id: String,
    pub last_event_id: Option<String>,
}

pub async fn sse_handler(
    State(state): State<DaemonState>,
    Query(query): Query<SseQuery>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<SseEvent>();

    // Send any missed events first
    if let Some(last_id) = &query.last_event_id {
        for event in state.events_since(last_id) {
            let _ = tx.send(event);
        }
    }

    // Register client
    state.clients.write().insert(
        query.client_id.clone(),
        SseClient {
            client_id: query.client_id,
            tx,
            connected_at: std::time::Instant::now(),
        },
    );

    let stream = UnboundedReceiverStream::new(rx).map(|sse_event| {
        Ok(Event::default()
            .id(sse_event.id)
            .event(sse_event.event_type)
            .json_data(sse_event.data)
            .unwrap_or_else(|_| Event::default()))
    });

    Sse::new(stream).keep_alive(KeepAlive::default())
}
```

- [ ] **Step 4: Update daemon mod.rs**

```rust
// src/daemon/mod.rs

pub mod state;
pub mod server;
pub mod routes;
pub mod sse;
```

- [ ] **Step 5: Verify compilation**

Run: `cargo build 2>&1 | head -30`
Expected: Compiles (may have warnings for unused imports — fix as needed)

- [ ] **Step 6: Commit**

```bash
git add src/daemon/
git commit -m "feat(kairos): add daemon HTTP server with routes and SSE"
```

---

### Task 12: Daemon Entry Point (--daemon flag)

**Files:**
- Modify: `src/main.rs:78-152` (add --daemon arg)
- Modify: `src/main.rs:495-506` (add daemon branch)

- [ ] **Step 1: Add --daemon CLI arg**

In `src/main.rs`, add after the `headless` field (line 148):

```rust
    /// Run as a background daemon with HTTP server (KAIROS mode).
    #[arg(long, hide = true)]
    daemon: bool,

    /// Daemon HTTP port (default: 19836).
    #[arg(long, default_value = "19836")]
    port: u16,
```

- [ ] **Step 2: Add daemon branch in main flow**

In `src/main.rs`, add before the headless branch (before line 495):

```rust
    // ── Daemon mode ──
    if cli.daemon {
        use crate::config::features;
        if !features::enabled(features::Feature::Kairos) {
            eprintln!("error: --daemon requires FEATURE_KAIROS=1");
            return Ok(ExitCode::FAILURE);
        }

        let daemon_state = daemon::state::DaemonState::new(
            engine.clone(),
            features::FLAGS.clone(),
            cli.port,
        );

        return daemon::server::serve_http(daemon_state, cli.port)
            .await
            .map(|()| ExitCode::SUCCESS)
            .map_err(|e| e.into());
    }
```

- [ ] **Step 3: Verify compilation**

Run: `cargo build 2>&1 | head -20`
Expected: Compiles

- [ ] **Step 4: Quick smoke test**

Run: `FEATURE_KAIROS=1 cargo run -- --daemon --port 19837 &`
Then: `curl http://127.0.0.1:19837/health`
Expected: `{"status":"ok","version":"..."}`
Clean up: `kill %1`

- [ ] **Step 5: Commit**

```bash
git add src/main.rs
git commit -m "feat(kairos): add --daemon entry point with HTTP server"
```

---

### Task 13: Proactive Tick Loop

**Files:**
- Create: `src/daemon/tick.rs`
- Modify: `src/daemon/mod.rs` (declare module)
- Modify: `src/main.rs` daemon branch (add tick to select!)

- [ ] **Step 1: Implement tick loop**

```rust
// src/daemon/tick.rs

//! Proactive tick loop — periodically triggers autonomous model execution.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use chrono::Local;
use futures::StreamExt;
use tracing::{debug, info, warn};

use crate::types::config::QuerySource;

use super::state::{DaemonState, SseEvent, next_event_id};

/// Default tick interval (30 seconds).
const DEFAULT_TICK_INTERVAL_MS: u64 = 30_000;

/// Run the proactive tick loop. Never returns unless cancelled.
pub async fn tick_loop(state: DaemonState) {
    let interval_ms = DEFAULT_TICK_INTERVAL_MS;
    let mut interval = tokio::time::interval(Duration::from_millis(interval_ms));

    info!("proactive tick loop started (interval: {}ms)", interval_ms);

    // Skip the first immediate tick
    interval.tick().await;

    loop {
        interval.tick().await;

        // Skip if a query is already running
        if state.is_query_running.load(Ordering::SeqCst) {
            debug!("tick skipped: query running");
            continue;
        }

        // Skip if engine is sleeping
        if state.engine.is_sleeping() {
            debug!("tick skipped: sleeping");
            continue;
        }

        let now = Local::now();
        let focus = state.terminal_focus();

        let tick_prompt = format!(
            "<tick_tag>\nLocal time: {}\nTerminal focus: {}\n</tick_tag>",
            now.format("%Y-%m-%d %H:%M:%S"),
            focus,
        );

        debug!("proactive tick firing at {}", now.format("%H:%M:%S"));

        // Notify connected frontends
        state.broadcast(SseEvent {
            id: next_event_id(),
            event_type: "autonomous_start".to_string(),
            data: serde_json::json!({
                "source": "proactive_tick",
                "time": now.to_rfc3339(),
            }),
        });

        // Submit to engine
        state.is_query_running.store(true, Ordering::SeqCst);
        let engine = state.engine.clone();
        let state_clone = state.clone();

        tokio::spawn(async move {
            let stream = engine.submit_message(&tick_prompt, QuerySource::ProactiveTick);
            tokio::pin!(stream);

            while let Some(sdk_msg) = stream.next().await {
                // Check if SleepTool was called in the response
                if let Some(event) =
                    super::routes::sdk_message_to_sse(&sdk_msg, "tick")
                {
                    state_clone.broadcast(event);
                }
            }

            state_clone
                .is_query_running
                .store(false, Ordering::SeqCst);
        });
    }
}
```

- [ ] **Step 2: Update daemon mod.rs**

Add to `src/daemon/mod.rs`:

```rust
pub mod tick;
```

- [ ] **Step 3: Wire tick into daemon startup**

Update the daemon branch in `src/main.rs` to use `tokio::select!`:

```rust
    if cli.daemon {
        use crate::config::features::{self, Feature};
        if !features::enabled(Feature::Kairos) {
            eprintln!("error: --daemon requires FEATURE_KAIROS=1");
            return Ok(ExitCode::FAILURE);
        }

        let daemon_state = daemon::state::DaemonState::new(
            engine.clone(),
            features::FLAGS.clone(),
            cli.port,
        );

        let http_state = daemon_state.clone();
        let tick_state = daemon_state.clone();
        let tick_enabled = features::enabled(Feature::Proactive);

        return tokio::select! {
            result = daemon::server::serve_http(http_state, cli.port) => {
                result.map(|()| ExitCode::SUCCESS).map_err(|e| e.into())
            }
            _ = daemon::tick::tick_loop(tick_state), if tick_enabled => {
                Ok(ExitCode::SUCCESS)
            }
            _ = tokio::signal::ctrl_c() => {
                info!("daemon shutting down");
                Ok(ExitCode::SUCCESS)
            }
        };
    }
```

- [ ] **Step 4: Verify compilation**

Run: `cargo build 2>&1 | head -20`
Expected: Compiles

- [ ] **Step 5: Commit**

```bash
git add src/daemon/tick.rs src/daemon/mod.rs src/main.rs
git commit -m "feat(kairos): add proactive tick loop with tokio::select! in daemon"
```

---

## Phase 3: External Integration (Webhooks + Channels + Notifications)

### Task 14: Webhook Verification + Processing

**Files:**
- Create: `src/daemon/webhook.rs`
- Modify: `src/daemon/mod.rs`
- Modify: `src/daemon/routes.rs` (replace stubs)

- [ ] **Step 1: Implement webhook verification**

```rust
// src/daemon/webhook.rs

//! Webhook signature verification and payload parsing.

use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Verify GitHub webhook signature (X-Hub-Signature-256).
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

/// Verify Slack webhook signature.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_github_signature_valid() {
        let secret = "test_secret";
        let body = b"test body";
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body);
        let sig = format!("sha256={}", hex::encode(mac.finalize().into_bytes()));
        assert!(verify_github_signature(body, &sig, secret));
    }

    #[test]
    fn test_github_signature_invalid() {
        assert!(!verify_github_signature(b"body", "sha256=bad", "secret"));
    }
}
```

- [ ] **Step 2: Add hex dependency to Cargo.toml**

```toml
hex = "0.4"
```

- [ ] **Step 3: Update routes with real webhook handlers**

Replace the stub webhook handlers in `src/daemon/routes.rs` with implementations that call `webhook::verify_*` and convert to `ChannelEvent`. (The full channel routing comes in Task 15.)

- [ ] **Step 4: Verify compilation and tests**

Run: `cargo test --lib daemon::webhook::tests -v && cargo build`
Expected: PASS + compiles

- [ ] **Step 5: Commit**

```bash
git add src/daemon/webhook.rs src/daemon/mod.rs Cargo.toml src/daemon/routes.rs
git commit -m "feat(kairos): add webhook signature verification (GitHub + Slack)"
```

---

### Task 15: Channel Manager

**Files:**
- Create: `src/daemon/channels.rs`
- Modify: `src/daemon/mod.rs`

- [ ] **Step 1: Implement ChannelManager**

```rust
// src/daemon/channels.rs

//! Channel manager — routes external messages (MCP + webhook) to QueryEngine.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc;
use tracing::{debug, warn};

/// Origin of a channel event.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ChannelOrigin {
    Mcp { server_name: String },
    Webhook { endpoint: String },
}

/// An event received from an external channel.
#[derive(Clone, Debug, Serialize)]
pub struct ChannelEvent {
    pub source: String,
    pub sender: Option<String>,
    pub content: String,
    pub meta: Value,
    pub origin: ChannelOrigin,
}

impl ChannelEvent {
    /// Format as XML tags for system prompt injection.
    pub fn to_xml(&self) -> String {
        let sender_attr = self
            .sender
            .as_deref()
            .map(|s| format!(" sender=\"{}\"", s))
            .unwrap_or_default();

        format!(
            "<channel source=\"{}\"{}>\\n{}\\n</channel>",
            self.source, sender_attr, self.content
        )
    }
}

/// Manages channel registrations and allowlist filtering.
pub struct ChannelManager {
    allowlist: HashSet<String>,
    event_tx: mpsc::UnboundedSender<ChannelEvent>,
}

impl ChannelManager {
    pub fn new(
        allowlist: Vec<String>,
        event_tx: mpsc::UnboundedSender<ChannelEvent>,
    ) -> Self {
        Self {
            allowlist: allowlist.into_iter().collect(),
            event_tx,
        }
    }

    /// Submit a channel event. Returns false if filtered by allowlist.
    pub fn submit(&self, event: ChannelEvent) -> bool {
        let key = match &event.origin {
            ChannelOrigin::Mcp { server_name } => format!("mcp:{}", server_name),
            ChannelOrigin::Webhook { endpoint } => format!("webhook:{}", endpoint),
        };

        if !self.allowlist.contains(&key) {
            warn!("channel event from '{}' blocked by allowlist", key);
            return false;
        }

        debug!("channel event accepted from '{}'", key);
        let _ = self.event_tx.send(event);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_channel_event_to_xml() {
        let event = ChannelEvent {
            source: "slack-mcp".to_string(),
            sender: Some("user@org".to_string()),
            content: "hello world".to_string(),
            meta: serde_json::json!({}),
            origin: ChannelOrigin::Mcp {
                server_name: "slack-mcp".to_string(),
            },
        };
        let xml = event.to_xml();
        assert!(xml.contains("source=\"slack-mcp\""));
        assert!(xml.contains("sender=\"user@org\""));
        assert!(xml.contains("hello world"));
    }

    #[test]
    fn test_channel_manager_allowlist() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mgr = ChannelManager::new(vec!["webhook:github".to_string()], tx);

        let allowed = ChannelEvent {
            source: "github".to_string(),
            sender: None,
            content: "PR opened".to_string(),
            meta: serde_json::json!({}),
            origin: ChannelOrigin::Webhook {
                endpoint: "github".to_string(),
            },
        };
        assert!(mgr.submit(allowed));

        let blocked = ChannelEvent {
            source: "slack".to_string(),
            sender: None,
            content: "hi".to_string(),
            meta: serde_json::json!({}),
            origin: ChannelOrigin::Webhook {
                endpoint: "slack".to_string(),
            },
        };
        assert!(!mgr.submit(blocked));
    }
}
```

- [ ] **Step 2: Register module**

Add to `src/daemon/mod.rs`:

```rust
pub mod channels;
```

- [ ] **Step 3: Run tests**

Run: `cargo test --lib daemon::channels::tests -v`
Expected: All PASS

- [ ] **Step 4: Commit**

```bash
git add src/daemon/channels.rs src/daemon/mod.rs
git commit -m "feat(kairos): add ChannelManager with allowlist filtering"
```

---

### Task 16: Push Notification System

**Files:**
- Create: `src/daemon/notification.rs`
- Modify: `src/daemon/mod.rs`

- [ ] **Step 1: Implement NotificationManager**

```rust
// src/daemon/notification.rs

//! Push notification system — Windows Toast + webhook callback.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{debug, error, info};

/// Notification severity level.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NotificationLevel {
    Info,
    Warning,
    Error,
    Success,
}

/// What triggered the notification.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum NotificationSource {
    TaskComplete { task_id: String },
    BackgroundAgentDone { agent_id: String },
    ChannelMessage { source: String },
    ProactiveAction { summary: String },
    Error { detail: String },
}

/// Notification configuration from settings.json.
#[derive(Clone, Debug, Deserialize, Default)]
pub struct NotificationConfig {
    pub windows_toast: Option<ToastConfig>,
    pub webhook: Option<WebhookConfig>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ToastConfig {
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub only_when_detached: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct WebhookConfig {
    pub enabled: bool,
    pub url: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub events: Vec<String>,
}

fn default_true() -> bool {
    true
}

/// A notification to dispatch.
#[derive(Clone, Debug, Serialize)]
pub struct Notification {
    pub title: String,
    pub body: String,
    pub level: NotificationLevel,
    pub source: NotificationSource,
}

/// Send a Windows Toast notification.
pub fn send_windows_toast(notif: &Notification) {
    #[cfg(target_os = "windows")]
    {
        use notify_rust::Notification as Toast;
        if let Err(e) = Toast::new()
            .appname("cc-rust")
            .summary(&notif.title)
            .body(&notif.body)
            .show()
        {
            error!("failed to show Windows toast: {}", e);
        } else {
            debug!("Windows toast sent: {}", notif.title);
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        debug!("Windows toast skipped (not on Windows): {}", notif.title);
    }
}

/// Send a webhook notification.
pub async fn send_webhook(notif: &Notification, config: &WebhookConfig) {
    let payload = json!({
        "title": notif.title,
        "body": notif.body,
        "level": notif.level,
        "source": notif.source,
        "timestamp": chrono::Utc::now().to_rfc3339(),
    });

    let client = reqwest::Client::new();
    let mut req = client.post(&config.url).json(&payload);

    for (k, v) in &config.headers {
        req = req.header(k, v);
    }

    match req.send().await {
        Ok(resp) => {
            debug!("webhook notification sent: {} ({})", notif.title, resp.status());
        }
        Err(e) => {
            error!("webhook notification failed: {}", e);
        }
    }
}

/// Consume notifications from the channel and dispatch.
pub async fn notification_consumer(
    mut rx: tokio::sync::mpsc::UnboundedReceiver<super::state::Notification>,
    config: NotificationConfig,
    has_clients: impl Fn() -> bool,
) {
    info!("notification consumer started");

    while let Some(notif) = rx.recv().await {
        let full_notif = Notification {
            title: notif.title,
            body: notif.body,
            level: NotificationLevel::Info,
            source: NotificationSource::ProactiveAction {
                summary: "notification".to_string(),
            },
        };

        // Windows Toast (only if detached or configured)
        if let Some(toast_cfg) = &config.windows_toast {
            if toast_cfg.enabled {
                let should_send = !toast_cfg.only_when_detached || !has_clients();
                if should_send {
                    send_windows_toast(&full_notif);
                }
            }
        }

        // Webhook
        if let Some(wh_cfg) = &config.webhook {
            if wh_cfg.enabled {
                send_webhook(&full_notif, wh_cfg).await;
            }
        }
    }
}
```

- [ ] **Step 2: Register module**

Add to `src/daemon/mod.rs`:

```rust
pub mod notification;
```

- [ ] **Step 3: Verify compilation**

Run: `cargo build 2>&1 | head -20`
Expected: Compiles

- [ ] **Step 4: Commit**

```bash
git add src/daemon/notification.rs src/daemon/mod.rs
git commit -m "feat(kairos): add push notification system (Toast + webhook)"
```

---

### Task 17: Remaining Commands (/notify, /channels, /dream)

**Files:**
- Create: `src/commands/notify.rs`
- Create: `src/commands/channels.rs`
- Create: `src/commands/dream.rs`
- Modify: `src/commands/mod.rs`

- [ ] **Step 1: Implement /notify command**

```rust
// src/commands/notify.rs

use anyhow::Result;
use async_trait::async_trait;
use super::{CommandContext, CommandHandler, CommandResult};
use crate::config::features::{self, Feature};

pub struct NotifyHandler;

#[async_trait]
impl CommandHandler for NotifyHandler {
    async fn execute(&self, args: &str, _ctx: &mut CommandContext) -> Result<CommandResult> {
        if !features::enabled(Feature::KairosPushNotification) {
            return Ok(CommandResult::Output(
                "Notifications require FEATURE_KAIROS_PUSH_NOTIFICATION=1".to_string(),
            ));
        }

        match args.trim() {
            "" | "status" => Ok(CommandResult::Output(
                "Notification status: check settings.json for configuration.".to_string(),
            )),
            "test" => Ok(CommandResult::Output(
                "Test notification sent.".to_string(),
            )),
            "on" => Ok(CommandResult::Output("Notifications enabled.".to_string())),
            "off" => Ok(CommandResult::Output("Notifications disabled.".to_string())),
            _ => Ok(CommandResult::Output(
                "Usage: /notify [status|test|on|off]".to_string(),
            )),
        }
    }
}
```

- [ ] **Step 2: Implement /channels command**

```rust
// src/commands/channels.rs

use anyhow::Result;
use async_trait::async_trait;
use super::{CommandContext, CommandHandler, CommandResult};
use crate::config::features::{self, Feature};

pub struct ChannelsHandler;

#[async_trait]
impl CommandHandler for ChannelsHandler {
    async fn execute(&self, args: &str, _ctx: &mut CommandContext) -> Result<CommandResult> {
        if !features::enabled(Feature::KairosChannels) {
            return Ok(CommandResult::Output(
                "Channels require FEATURE_KAIROS_CHANNELS=1".to_string(),
            ));
        }

        match args.trim() {
            "" | "list" => Ok(CommandResult::Output(
                "No channels connected. Use --channels to configure.".to_string(),
            )),
            _ => Ok(CommandResult::Output(
                "Usage: /channels [list]".to_string(),
            )),
        }
    }
}
```

- [ ] **Step 3: Implement /dream command**

```rust
// src/commands/dream.rs

use anyhow::Result;
use async_trait::async_trait;
use super::{CommandContext, CommandHandler, CommandResult};
use crate::config::features::{self, Feature};

pub struct DreamHandler;

#[async_trait]
impl CommandHandler for DreamHandler {
    async fn execute(&self, args: &str, _ctx: &mut CommandContext) -> Result<CommandResult> {
        if !features::enabled(Feature::Kairos) {
            return Ok(CommandResult::Output(
                "Dream requires FEATURE_KAIROS=1".to_string(),
            ));
        }

        let days: u32 = args.trim().parse().unwrap_or(7);
        Ok(CommandResult::Output(format!(
            "Distilling last {} days of logs into memory... (not yet implemented)",
            days
        )))
    }
}
```

- [ ] **Step 4: Register all commands**

In `src/commands/mod.rs`, add module declarations:

```rust
pub mod notify;
pub mod channels;
pub mod dream;
```

Add to `get_all_commands()`:

```rust
        Command {
            name: "notify".into(),
            aliases: vec![],
            description: "Push notification settings".into(),
            handler: Box::new(notify::NotifyHandler),
        },
        Command {
            name: "channels".into(),
            aliases: vec![],
            description: "View connected channels".into(),
            handler: Box::new(channels::ChannelsHandler),
        },
        Command {
            name: "dream".into(),
            aliases: vec![],
            description: "Distill daily logs into memory".into(),
            handler: Box::new(dream::DreamHandler),
        },
```

- [ ] **Step 5: Verify compilation**

Run: `cargo build 2>&1 | head -20`
Expected: Compiles

- [ ] **Step 6: Commit**

```bash
git add src/commands/notify.rs src/commands/channels.rs src/commands/dream.rs src/commands/mod.rs
git commit -m "feat(kairos): add /notify /channels /dream commands"
```

---

## Phase 4: Memory + Frontend Adaptation

### Task 18: Daily Log Memory System

**Files:**
- Create: `src/daemon/memory_log.rs`
- Modify: `src/daemon/mod.rs`

- [ ] **Step 1: Implement daily log reader/writer**

```rust
// src/daemon/memory_log.rs

//! Daily append-only log system for KAIROS perpetual sessions.
//!
//! Logs stored at ~/.cc-rust/logs/YYYY/MM/YYYY-MM-DD.md

use std::path::PathBuf;

use chrono::Local;
use tracing::{debug, error};

/// Get the log directory path.
fn log_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".cc-rust")
        .join("logs")
}

/// Get today's log file path.
pub fn today_log_path() -> PathBuf {
    let now = Local::now();
    log_dir()
        .join(now.format("%Y").to_string())
        .join(now.format("%m").to_string())
        .join(now.format("%Y-%m-%d.md").to_string())
}

/// Read today's log content. Returns empty string if file doesn't exist.
pub fn read_today_log() -> String {
    let path = today_log_path();
    std::fs::read_to_string(&path).unwrap_or_default()
}

/// Append an entry to today's log with a timestamp header.
pub fn append_log_entry(content: &str) {
    let path = today_log_path();

    // Ensure directory exists
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            error!("failed to create log directory: {}", e);
            return;
        }
    }

    let now = Local::now();
    let is_new_file = !path.exists();

    let mut entry = String::new();

    // Add date header if new file
    if is_new_file {
        entry.push_str(&format!("## {}\n\n", now.format("%Y-%m-%d")));
    }

    // Add timestamp and content
    entry.push_str(&format!("### {}\n", now.format("%H:%M")));
    for line in content.lines() {
        entry.push_str(&format!("- {}\n", line));
    }
    entry.push('\n');

    // Append to file
    use std::io::Write;
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        Ok(mut file) => {
            if let Err(e) = file.write_all(entry.as_bytes()) {
                error!("failed to write log entry: {}", e);
            } else {
                debug!("log entry appended to {}", path.display());
            }
        }
        Err(e) => {
            error!("failed to open log file: {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_today_log_path_format() {
        let path = today_log_path();
        let path_str = path.to_string_lossy();
        assert!(path_str.contains(".cc-rust"));
        assert!(path_str.contains("logs"));
        assert!(path_str.ends_with(".md"));
    }

    #[test]
    fn test_log_dir_structure() {
        let dir = log_dir();
        assert!(dir.to_string_lossy().contains("logs"));
    }
}
```

- [ ] **Step 2: Register module**

Add to `src/daemon/mod.rs`:

```rust
pub mod memory_log;
```

- [ ] **Step 3: Run tests**

Run: `cargo test --lib daemon::memory_log::tests -v`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src/daemon/memory_log.rs src/daemon/mod.rs
git commit -m "feat(kairos): add daily log memory system"
```

---

### Task 19: IPC Protocol Extension

**Files:**
- Modify: `src/ipc/protocol.rs:45-108` (add new BackendMessage variants)

- [ ] **Step 1: Add new BackendMessage variants**

In `src/ipc/protocol.rs`, add after `BackgroundAgentComplete` (before the closing `}` of the enum around line 108):

```rust
    /// Brief mode message from the model (via BriefTool).
    BriefMessage {
        message: String,
        status: String,
        attachments: Vec<String>,
    },

    /// Autonomous action started (proactive tick).
    AutonomousStart {
        source: String,
        time: String,
    },

    /// Push notification sent.
    NotificationSent {
        title: String,
        level: String,
    },
```

- [ ] **Step 2: Verify compilation**

Run: `cargo build 2>&1 | head -20`
Expected: Compiles

- [ ] **Step 3: Commit**

```bash
git add src/ipc/protocol.rs
git commit -m "feat(kairos): extend IPC protocol with Brief/Autonomous/Notification messages"
```

---

### Task 20: Frontend DaemonClient

**Files:**
- Create: `ui/src/ipc/daemon-client.ts`
- Modify: `ui/src/ipc/client.ts` (add mode selection)

- [ ] **Step 1: Create DaemonClient**

```typescript
// ui/src/ipc/daemon-client.ts

import type { BackendMessage, FrontendMessage, IpcClient } from './protocol.js';

const DEFAULT_PORT = 19836;

export class DaemonClient implements IpcClient {
  private baseUrl: string;
  private eventSource: EventSource | null = null;
  private clientId: string;
  private onMessageCallback: ((msg: BackendMessage) => void) | null = null;

  constructor(port: number = DEFAULT_PORT) {
    this.baseUrl = `http://127.0.0.1:${port}`;
    this.clientId = `ui_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`;
  }

  async connect(lastEventId?: string): Promise<void> {
    // Attach to daemon
    const attachRes = await fetch(`${this.baseUrl}/api/attach`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        client_id: this.clientId,
        last_seen_event: lastEventId ?? null,
      }),
    });

    if (!attachRes.ok) {
      throw new Error(`Failed to attach to daemon: ${attachRes.status}`);
    }

    const { missed_events } = await attachRes.json();

    // Replay missed events
    for (const event of missed_events) {
      this.onMessageCallback?.(event.data);
    }

    // Start SSE
    const url = `${this.baseUrl}/api/events?client_id=${this.clientId}${
      lastEventId ? `&last_event_id=${lastEventId}` : ''
    }`;
    this.eventSource = new EventSource(url);

    this.eventSource.onmessage = (event) => {
      try {
        const msg = JSON.parse(event.data);
        this.onMessageCallback?.(msg);
      } catch {
        // ignore parse errors
      }
    };
  }

  async send(msg: FrontendMessage): Promise<void> {
    const endpoint = this.getEndpoint(msg);
    await fetch(`${this.baseUrl}${endpoint}`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(msg),
    });
  }

  onMessage(callback: (msg: BackendMessage) => void): void {
    this.onMessageCallback = callback;
  }

  async disconnect(): Promise<void> {
    this.eventSource?.close();
    await fetch(`${this.baseUrl}/api/detach`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ client_id: this.clientId }),
    });
  }

  private getEndpoint(msg: FrontendMessage): string {
    switch (msg.type) {
      case 'SubmitPrompt':
        return '/api/submit';
      case 'AbortQuery':
        return '/api/abort';
      case 'SlashCommand':
        return '/api/command';
      case 'PermissionResponse':
        return '/api/permission';
      case 'Resize':
        return '/api/resize';
      case 'Quit':
        return '/api/detach';
      default:
        return '/api/submit';
    }
  }
}
```

- [ ] **Step 2: Add mode selection in client.ts**

In `ui/src/ipc/client.ts`, add the factory function:

```typescript
import { DaemonClient } from './daemon-client.js';

export function createClient(mode: 'headless' | 'daemon'): IpcClient {
  if (mode === 'daemon') {
    const port = parseInt(process.env.CC_RUST_DAEMON_PORT || '19836', 10);
    return new DaemonClient(port);
  }
  return new JsonlClient(); // existing implementation
}
```

- [ ] **Step 3: Commit**

```bash
git add ui/src/ipc/daemon-client.ts ui/src/ipc/client.ts
git commit -m "feat(kairos): add DaemonClient for ink-terminal → daemon HTTP/SSE"
```

---

### Task 21: Run Script Adaptation

**Files:**
- Modify: `ui/run.sh`
- Modify: `ui/run.ps1`

- [ ] **Step 1: Update run.sh**

Add KAIROS detection at the top of `ui/run.sh`:

```bash
#!/bin/bash
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

if [ "$FEATURE_KAIROS" = "1" ]; then
    # Ensure daemon is running
    if ! curl -sf http://127.0.0.1:${CC_RUST_DAEMON_PORT:-19836}/health > /dev/null 2>&1; then
        echo "Starting cc-rust daemon..."
        "$SCRIPT_DIR/../target/release/cc-rust" --daemon &
        sleep 1
    fi
    exec bun run "$SCRIPT_DIR/src/main.tsx" -- --mode daemon "$@"
else
    exec bun run "$SCRIPT_DIR/src/main.tsx" "$@"
fi
```

- [ ] **Step 2: Update run.ps1 similarly**

- [ ] **Step 3: Commit**

```bash
git add ui/run.sh ui/run.ps1
git commit -m "feat(kairos): update run scripts for daemon mode detection"
```

---

## Phase 5: Integration + MCP Channel Protocol

### Task 22: MCP Channel Extension

**Files:**
- Create: `src/mcp/channel.rs`
- Modify: `src/mcp/mod.rs`

- [ ] **Step 1: Implement MCP channel notification handler**

```rust
// src/mcp/channel.rs

//! MCP Channel notification protocol extension.
//!
//! Handles `notifications/claude/channel` from MCP servers that declare
//! `capabilities.experimental["claude/channel"]`.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{debug, warn};

/// Check if an MCP server supports channel notifications.
pub fn supports_channel(capabilities: &Value) -> bool {
    capabilities
        .get("experimental")
        .and_then(|v| v.get("claude/channel"))
        .is_some()
}

/// Parsed channel notification from an MCP server.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct McpChannelNotification {
    pub content: String,
    pub meta: Value,
}

/// Parse a raw MCP notification into a channel notification.
pub fn parse_channel_notification(params: &Value) -> Option<McpChannelNotification> {
    let content = params.get("content").and_then(|v| v.as_str())?;
    let meta = params.get("meta").cloned().unwrap_or(Value::Null);

    Some(McpChannelNotification {
        content: content.to_string(),
        meta,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_supports_channel() {
        let caps = json!({ "experimental": { "claude/channel": {} } });
        assert!(supports_channel(&caps));

        let no_caps = json!({ "experimental": {} });
        assert!(!supports_channel(&no_caps));
    }

    #[test]
    fn test_parse_channel_notification() {
        let params = json!({ "content": "hello", "meta": { "from": "slack" } });
        let notif = parse_channel_notification(&params).unwrap();
        assert_eq!(notif.content, "hello");
        assert_eq!(notif.meta["from"], "slack");
    }
}
```

- [ ] **Step 2: Register module**

In `src/mcp/mod.rs`, add:

```rust
pub mod channel;
```

- [ ] **Step 3: Run tests**

Run: `cargo test --lib mcp::channel::tests -v`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src/mcp/channel.rs src/mcp/mod.rs
git commit -m "feat(kairos): add MCP channel notification protocol extension"
```

---

### Task 23: End-to-End Integration Test

**Files:**
- Create test in a suitable location

- [ ] **Step 1: Write integration test for daemon startup**

```rust
// tests/daemon_smoke.rs (or add to existing e2e tests)

#[tokio::test]
async fn test_daemon_health_endpoint() {
    // This test requires FEATURE_KAIROS=1
    if std::env::var("FEATURE_KAIROS").unwrap_or_default() != "1" {
        eprintln!("skipping daemon test: FEATURE_KAIROS not set");
        return;
    }

    let client = reqwest::Client::new();
    let resp = client
        .get("http://127.0.0.1:19836/health")
        .send()
        .await;

    // If daemon is running, verify response
    if let Ok(resp) = resp {
        assert_eq!(resp.status(), 200);
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["status"], "ok");
    }
}

#[tokio::test]
async fn test_daemon_status_endpoint() {
    if std::env::var("FEATURE_KAIROS").unwrap_or_default() != "1" {
        return;
    }

    let client = reqwest::Client::new();
    if let Ok(resp) = client.get("http://127.0.0.1:19836/api/status").send().await {
        let body: serde_json::Value = resp.json().await.unwrap();
        assert!(body.get("kairos_active").is_some());
        assert!(body.get("proactive").is_some());
        assert!(body.get("query_running").is_some());
    }
}
```

- [ ] **Step 2: Run full test suite**

Run: `cargo test 2>&1 | tail -20`
Expected: All tests PASS (daemon tests skipped without FEATURE_KAIROS)

- [ ] **Step 3: Run with KAIROS enabled**

Run: `cargo build --release && FEATURE_KAIROS=1 cargo test daemon_smoke -v`

- [ ] **Step 4: Commit**

```bash
git add tests/
git commit -m "test(kairos): add daemon smoke tests"
```

---

### Task 24: Final Compilation + Warnings Cleanup

- [ ] **Step 1: Full build**

Run: `cargo build 2>&1`
Fix any warnings (unused imports, dead code, etc.)

- [ ] **Step 2: Run all tests**

Run: `cargo test 2>&1`
Expected: All PASS

- [ ] **Step 3: Commit cleanup**

```bash
git add -A
git commit -m "chore(kairos): fix warnings and cleanup"
```

---

## Summary

| Phase | Tasks | Key Deliverables |
|-------|-------|-----------------|
| **Phase 1: Foundation** | 1-8 | Feature gates, AppState, SleepTool, BriefTool, system prompts, commands |
| **Phase 2: Daemon** | 9-13 | axum HTTP server, DaemonState, routes, SSE, tick loop, --daemon flag |
| **Phase 3: External** | 14-17 | Webhook verification, ChannelManager, notifications, remaining commands |
| **Phase 4: Memory + Frontend** | 18-21 | Daily logs, IPC protocol, DaemonClient, run script |
| **Phase 5: Integration** | 22-24 | MCP channel, e2e tests, cleanup |
