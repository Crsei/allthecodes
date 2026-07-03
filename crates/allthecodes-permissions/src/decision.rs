//! Permission decision state machine — full decision flow.
//!
//! Corresponds to: LIFECYCLE_STATE_MACHINE.md §7 (Phase F)
//!
//! Decision flow (deny > ask > allow per spec):
//!   Phase 1a: Hook deny — pre-tool hooks may force deny.
//!   Phase 1b: Unconditional rules + pattern rules at sources Managed/User/Project/Local.
//!             Deny rules win first; ask rules force a prompt; allow rules
//!             pre-approve.
//!   Phase 2:  Hook ask / hook allow — applied AFTER deny/ask rules but
//!             BEFORE the mode fallback so a hook can pre-approve a tool
//!             that would otherwise prompt.
//!   Phase 3:  Session-level grants (transient).
//!   Phase 4:  Mode fallback — Default/Plan ask, Auto/Bypass allow,
//!             AcceptEdits allows file-edit tools, DontAsk silently denies.
//!
//! Rule sources (priority descending — used by `/permissions show` only;
//! the rule engine treats them uniformly):
//!   1. Managed (enterprise / policy)
//!   2. Project (.allthecodes/settings.json)
//!   3. Local   (.allthecodes/settings.local.json)
//!   4. User    (~/.allthecodes/settings.json)
//!   5. CLI     (--permission-mode et al.)
//!   6. Session (transient grants)

use serde_json::Value;

use super::rules;
use allthecodes_types::permissions::{
    PermissionMode, ToolPermissionContext, ToolPermissionRulesBySource,
};

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

/// Full permission decision with provenance tracking.
#[derive(Debug, Clone)]
pub struct PermissionDecision {
    /// The behavior to apply.
    pub behavior: PermissionBehavior,
    /// Optionally modified input (from hooks or rules).
    pub updated_input: Option<Value>,
    /// Human-readable reason for the decision.
    pub message: Option<String>,
    /// How the decision was reached.
    pub reason: PermissionDecisionReason,
}

/// Permission behavior — the action to take.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionBehavior {
    Allow,
    Deny,
    Ask,
}

/// Stage that produced an Auto mode classifier result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoClassifierStage {
    /// Fast yes/no classifier path.
    Fast,
    /// Slower classifier path with more reasoning.
    Thinking,
}

impl AutoClassifierStage {
    fn as_str(self) -> &'static str {
        match self {
            AutoClassifierStage::Fast => "fast",
            AutoClassifierStage::Thinking => "thinking",
        }
    }
}

/// Classifier verdict for an Auto mode tool call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoClassifierVerdict {
    Allow,
    Deny,
    Ask,
}

/// Result produced by an Auto mode transcript classifier.
///
/// allthecodes does not yet run Bun's LLM classifier in this crate. This type is
/// the permission-layer adapter: callers can pass a real transcript
/// classifier result here, while the existing no-classifier path keeps the
/// historical Auto mode fallback behavior.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoClassifierDecision {
    pub verdict: AutoClassifierVerdict,
    pub reason: String,
    pub model: String,
    pub stage: Option<AutoClassifierStage>,
    pub thinking: Option<String>,
    pub unavailable: bool,
    pub transcript_too_long: bool,
}

impl AutoClassifierDecision {
    pub fn allow(model: impl Into<String>, stage: AutoClassifierStage) -> Self {
        Self {
            verdict: AutoClassifierVerdict::Allow,
            reason: "classified as safe for auto mode".to_string(),
            model: model.into(),
            stage: Some(stage),
            thinking: None,
            unavailable: false,
            transcript_too_long: false,
        }
    }

    pub fn deny(
        model: impl Into<String>,
        stage: AutoClassifierStage,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            verdict: AutoClassifierVerdict::Deny,
            reason: reason.into(),
            model: model.into(),
            stage: Some(stage),
            thinking: None,
            unavailable: false,
            transcript_too_long: false,
        }
    }

    pub fn ask(
        model: impl Into<String>,
        stage: AutoClassifierStage,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            verdict: AutoClassifierVerdict::Ask,
            reason: reason.into(),
            model: model.into(),
            stage: Some(stage),
            thinking: None,
            unavailable: false,
            transcript_too_long: false,
        }
    }

    pub fn unavailable(model: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            verdict: AutoClassifierVerdict::Ask,
            reason: reason.into(),
            model: model.into(),
            stage: None,
            thinking: None,
            unavailable: true,
            transcript_too_long: false,
        }
    }

    pub fn transcript_too_long(model: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            transcript_too_long: true,
            ..Self::unavailable(model, reason)
        }
    }
}

/// How a permission decision was reached (for audit/debugging).
#[derive(Debug, Clone)]
pub enum PermissionDecisionReason {
    /// Matched a rule from a specific source.
    Rule { source: String, pattern: String },
    /// A hook made the decision.
    Hook { detail: String },
    /// The permission mode determined the outcome.
    Mode { mode: String },
    /// Pattern matching on tool input.
    PatternMatch { tool: String, pattern: String },
}

/// Result a hook contributed to the central decision flow.
///
/// Built from [`allthecodes_types::hooks::PreToolHookResult`] in the tool pipeline
/// and threaded through [`has_permissions_to_use_tool`] so a hook can
/// allow / deny / ask / modify input *without* skipping the rule engine.
#[derive(Debug, Clone, Default)]
pub struct HookPermissionDecision {
    /// Hook said "force allow this tool".
    pub allow: bool,
    /// Hook said "deny" (string is the reason).
    pub deny: Option<String>,
    /// Hook said "ask the user" (string is the prompt message).
    pub ask: Option<String>,
    /// Hook supplied a modified input value to use.
    pub updated_input: Option<Value>,
    /// Identifier for the contributing hook (`PreToolUse:<matcher>` etc).
    pub source: Option<String>,
}

impl HookPermissionDecision {
    /// True if the hook contributed nothing actionable.
    ///
    /// Used by callers (and tests) that want to short-circuit when a
    /// hook plug returns an empty result; intentionally part of the
    /// public surface of `HookPermissionDecision`.
    pub fn is_noop(&self) -> bool {
        !self.allow && self.deny.is_none() && self.ask.is_none() && self.updated_input.is_none()
    }
}

/// Denial tracking state for Auto mode.
#[derive(Debug, Clone, Default)]
pub struct DenialTracker {
    /// Consecutive denials in auto mode.
    pub consecutive_denials: usize,
    /// Total denials in auto mode.
    pub total_denials: usize,
}

impl DenialTracker {
    /// Record a denial. Returns true if fallback to interactive should happen.
    ///
    /// Public for downstream callers (auto-mode classifier shim) and
    /// covered by tests; cargo's non-test build can't see test usage so
    /// silence the warning.
    pub fn record_denial(&mut self) -> bool {
        self.consecutive_denials += 1;
        self.total_denials += 1;
        self.should_fallback_to_interactive()
    }

    /// Reset consecutive denial counter (on a successful allow).
    pub fn record_allow(&mut self) {
        self.consecutive_denials = 0;
    }

    /// Check if we should fall back to interactive prompting.
    ///
    /// consecutiveDenials >= 3 → fallback
    /// totalDenials >= 20 → forced fallback
    pub fn should_fallback_to_interactive(&self) -> bool {
        self.consecutive_denials >= 3 || self.total_denials >= 20
    }
}

// ---------------------------------------------------------------------------
// Descriptive permission messages (Computer Use + Browser MCP)
// ---------------------------------------------------------------------------
//
// In Phase 4 (issue #73) cc-permissions moved out of the root crate; the two
// message lookups that previously called into `computer_use::detection` and
// `browser::{detection,permissions}` now go through host-registered
// callbacks to avoid a reverse dep. The host wires them from the root
// crate at startup.

use parking_lot::Mutex;
use std::sync::LazyLock;

type MessageCallback = Box<dyn Fn(&str) -> Option<String> + Send + Sync>;

static CU_MESSAGE_CB: LazyLock<Mutex<Option<MessageCallback>>> = LazyLock::new(|| Mutex::new(None));
static BROWSER_MESSAGE_CB: LazyLock<Mutex<Option<MessageCallback>>> =
    LazyLock::new(|| Mutex::new(None));

/// Register the callback that turns a Computer Use tool name into a
/// user-facing permission prompt (or `None` for non-CU tools).
pub fn set_cu_message_callback<F>(cb: F)
where
    F: Fn(&str) -> Option<String> + Send + Sync + 'static,
{
    *CU_MESSAGE_CB.lock() = Some(Box::new(cb));
}

/// Register the callback that turns a browser tool name into a
/// user-facing permission prompt (or `None` for non-browser tools).
pub fn set_browser_message_callback<F>(cb: F)
where
    F: Fn(&str) -> Option<String> + Send + Sync + 'static,
{
    *BROWSER_MESSAGE_CB.lock() = Some(Box::new(cb));
}

pub fn process_descriptive_permission_message(tool_name: &str) -> Option<String> {
    if let Some(cb) = CU_MESSAGE_CB.lock().as_ref() {
        if let Some(m) = cb(tool_name) {
            return Some(m);
        }
    }

    if let Some(cb) = BROWSER_MESSAGE_CB.lock().as_ref() {
        if let Some(m) = cb(tool_name) {
            return Some(m);
        }
    }

    None
}

type PermissionMessageResolverRef<'a> = &'a (dyn Fn(&str) -> Option<String> + Send + Sync);

fn descriptive_permission_message(
    tool_name: &str,
    resolver: Option<PermissionMessageResolverRef<'_>>,
) -> Option<String> {
    match resolver {
        Some(resolver) => resolver(tool_name),
        None => process_descriptive_permission_message(tool_name),
    }
}

// ---------------------------------------------------------------------------
// Permission matcher (Phase 1b: pattern matching)
// ---------------------------------------------------------------------------

/// Prepare a permission matcher for a tool invocation. Used only by the
/// session-grant pass below — full pattern matching for Bash / Read / etc.
/// is delegated to [`super::rules::rule_matches`].
fn prepare_matcher(tool_name: &str, input: &Value) -> Option<String> {
    match tool_name {
        "Bash" => input
            .get("command")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        "Read" | "Write" | "Edit" => input
            .get("file_path")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        "Glob" => input
            .get("pattern")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        _ => None,
    }
}

/// Check pattern rules against all sources.
fn check_pattern_rules(
    tool_name: &str,
    input: &Value,
    rules_by_source: &ToolPermissionRulesBySource,
) -> Option<(String, String)> {
    for (source, patterns) in rules_by_source.iter() {
        for pattern in patterns {
            if rules::rule_matches(tool_name, input, pattern) {
                return Some((source.clone(), pattern.clone()));
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Full decision flow
// ---------------------------------------------------------------------------

/// Execute the full permission decision flow.
///
/// Corresponds to TypeScript: `hasPermissionsToUseTool()` in permissions.ts
pub fn has_permissions_to_use_tool(
    tool_name: &str,
    input: &Value,
    ctx: &ToolPermissionContext,
    denial_tracker: Option<&mut DenialTracker>,
) -> PermissionDecision {
    has_permissions_to_use_tool_with_hook(tool_name, input, ctx, None, denial_tracker)
}

/// Like [`has_permissions_to_use_tool`] but folds in a hook-supplied
/// decision per the precedence documented at the top of this module.
pub fn has_permissions_to_use_tool_with_hook(
    tool_name: &str,
    input: &Value,
    ctx: &ToolPermissionContext,
    hook: Option<&HookPermissionDecision>,
    denial_tracker: Option<&mut DenialTracker>,
) -> PermissionDecision {
    has_permissions_to_use_tool_with_hook_and_auto_classifier(
        tool_name,
        input,
        ctx,
        hook,
        None,
        denial_tracker,
    )
}

/// Like [`has_permissions_to_use_tool_with_hook`] but folds in an optional
/// Auto mode classifier result during mode fallback.
pub fn has_permissions_to_use_tool_with_hook_and_auto_classifier(
    tool_name: &str,
    input: &Value,
    ctx: &ToolPermissionContext,
    hook: Option<&HookPermissionDecision>,
    auto_classifier: Option<&AutoClassifierDecision>,
    denial_tracker: Option<&mut DenialTracker>,
) -> PermissionDecision {
    has_permissions_to_use_tool_with_hook_auto_classifier_and_message_resolver(
        tool_name,
        input,
        ctx,
        hook,
        auto_classifier,
        denial_tracker,
        None,
    )
}

/// Like [`has_permissions_to_use_tool_with_hook_and_auto_classifier`] but uses
/// an explicit descriptive permission message resolver, or process-wide
/// compatibility callbacks when no explicit resolver is provided.
pub fn has_permissions_to_use_tool_with_hook_auto_classifier_and_message_resolver(
    tool_name: &str,
    input: &Value,
    ctx: &ToolPermissionContext,
    hook: Option<&HookPermissionDecision>,
    auto_classifier: Option<&AutoClassifierDecision>,
    mut denial_tracker: Option<&mut DenialTracker>,
    permission_message_resolver: Option<PermissionMessageResolverRef<'_>>,
) -> PermissionDecision {
    let updated_input = hook.and_then(|h| h.updated_input.clone());

    // ── Phase 1a: Hook deny — short-circuit even before deny rules.
    // (Hook deny is treated as policy: it cannot be overridden.)
    if let Some(h) = hook {
        if let Some(reason) = &h.deny {
            return PermissionDecision {
                behavior: PermissionBehavior::Deny,
                updated_input,
                message: Some(reason.clone()),
                reason: PermissionDecisionReason::Hook {
                    detail: h.source.clone().unwrap_or_else(|| "PreToolUse".into()),
                },
            };
        }
    }

    // Pick the input the rest of the flow sees.
    let effective_input: &Value = updated_input.as_ref().unwrap_or(input);

    if ctx.mode == PermissionMode::Bypass {
        if let Some(tracker) = denial_tracker {
            tracker.record_allow();
        }
        return PermissionDecision {
            behavior: PermissionBehavior::Allow,
            updated_input,
            message: None,
            reason: PermissionDecisionReason::Mode {
                mode: "bypass".into(),
            },
        };
    }

    // ── Phase 1b: Deny rules.
    if let Some((source, pattern)) =
        check_pattern_rules(tool_name, effective_input, &ctx.always_deny_rules)
    {
        return PermissionDecision {
            behavior: PermissionBehavior::Deny,
            updated_input: updated_input.clone(),
            message: Some(format!("Denied by rule: {} (source={})", pattern, source)),
            reason: PermissionDecisionReason::PatternMatch {
                tool: tool_name.into(),
                pattern,
            },
        };
    }

    // ── Phase 1c: Ask rules — force a prompt regardless of allow rules.
    if let Some((source, pattern)) =
        check_pattern_rules(tool_name, effective_input, &ctx.always_ask_rules)
    {
        let message = descriptive_permission_message(tool_name, permission_message_resolver)
            .unwrap_or_else(|| {
                format!(
                    "Ask rule '{}' (source={}) requires confirmation.",
                    pattern, source
                )
            });
        return PermissionDecision {
            behavior: PermissionBehavior::Ask,
            updated_input: updated_input.clone(),
            message: Some(message),
            reason: PermissionDecisionReason::PatternMatch {
                tool: tool_name.into(),
                pattern,
            },
        };
    }

    // ── Phase 1d: Allow rules.
    if let Some((source, pattern)) =
        check_pattern_rules(tool_name, effective_input, &ctx.always_allow_rules)
    {
        if let Some(tracker) = denial_tracker {
            tracker.record_allow();
        }
        return PermissionDecision {
            behavior: PermissionBehavior::Allow,
            updated_input: updated_input.clone(),
            message: None,
            reason: PermissionDecisionReason::Rule { source, pattern },
        };
    }

    // ── Phase 2: Hook ask / hook allow.
    if let Some(h) = hook {
        if let Some(prompt) = &h.ask {
            return PermissionDecision {
                behavior: PermissionBehavior::Ask,
                updated_input: updated_input.clone(),
                message: Some(prompt.clone()),
                reason: PermissionDecisionReason::Hook {
                    detail: h.source.clone().unwrap_or_else(|| "PreToolUse".into()),
                },
            };
        }
        if h.allow {
            if let Some(tracker) = denial_tracker {
                tracker.record_allow();
            }
            return PermissionDecision {
                behavior: PermissionBehavior::Allow,
                updated_input: updated_input.clone(),
                message: None,
                reason: PermissionDecisionReason::Hook {
                    detail: h.source.clone().unwrap_or_else(|| "PreToolUse".into()),
                },
            };
        }
    }

    // ── Phase 3: Session-level grants ───────────────────────────────────
    let matcher_for_session = prepare_matcher(tool_name, effective_input);
    if let Some((_source, pattern)) = check_pattern_rules_via_matcher(
        tool_name,
        matcher_for_session.as_deref(),
        &ctx.session_allow_rules,
    ) {
        if let Some(tracker) = denial_tracker {
            tracker.record_allow();
        }
        return PermissionDecision {
            behavior: PermissionBehavior::Allow,
            updated_input: updated_input.clone(),
            message: None,
            reason: PermissionDecisionReason::Rule {
                source: "session".into(),
                pattern,
            },
        };
    }

    // ── Phase 4: Mode fallback ──────────────────────────────────────────
    match ctx.mode {
        PermissionMode::Bypass => unreachable!("bypass mode returns before rule evaluation"),
        PermissionMode::Auto => {
            if let Some(tracker) = denial_tracker.as_mut() {
                if tracker.should_fallback_to_interactive() {
                    return PermissionDecision {
                        behavior: PermissionBehavior::Ask,
                        updated_input,
                        message: Some(format!(
                            "Auto mode fallback: {} consecutive denials",
                            tracker.consecutive_denials
                        )),
                        reason: PermissionDecisionReason::Mode {
                            mode: "auto_fallback".into(),
                        },
                    };
                }
            }

            if let Some(classifier) = auto_classifier {
                return auto_classifier_decision(
                    tool_name,
                    updated_input,
                    classifier,
                    denial_tracker,
                );
            }

            if let Some(tracker) = denial_tracker {
                tracker.record_allow();
            }
            PermissionDecision {
                behavior: PermissionBehavior::Allow,
                updated_input,
                message: None,
                reason: PermissionDecisionReason::Mode {
                    mode: "auto".into(),
                },
            }
        }
        PermissionMode::Plan => {
            let message = descriptive_permission_message(tool_name, permission_message_resolver)
                .unwrap_or_else(|| {
                    format!("Tool '{}' requires confirmation in plan mode.", tool_name)
                });
            PermissionDecision {
                behavior: PermissionBehavior::Ask,
                updated_input,
                message: Some(message),
                reason: PermissionDecisionReason::Mode {
                    mode: "plan".into(),
                },
            }
        }
        PermissionMode::Default => {
            let message = descriptive_permission_message(tool_name, permission_message_resolver)
                .unwrap_or_else(|| format!("Allow tool '{}'?", tool_name));
            PermissionDecision {
                behavior: PermissionBehavior::Ask,
                updated_input,
                message: Some(message),
                reason: PermissionDecisionReason::Mode {
                    mode: "default".into(),
                },
            }
        }
        PermissionMode::AcceptEdits => {
            if rules::is_accept_edits_tool_call(tool_name, effective_input, ctx) {
                if let Some(tracker) = denial_tracker {
                    tracker.record_allow();
                }
                PermissionDecision {
                    behavior: PermissionBehavior::Allow,
                    updated_input,
                    message: None,
                    reason: PermissionDecisionReason::Mode {
                        mode: "acceptEdits".into(),
                    },
                }
            } else {
                let message =
                    descriptive_permission_message(tool_name, permission_message_resolver)
                        .unwrap_or_else(|| format!("Allow tool '{}'?", tool_name));
                PermissionDecision {
                    behavior: PermissionBehavior::Ask,
                    updated_input,
                    message: Some(message),
                    reason: PermissionDecisionReason::Mode {
                        mode: "acceptEdits".into(),
                    },
                }
            }
        }
        PermissionMode::DontAsk => PermissionDecision {
            behavior: PermissionBehavior::Deny,
            updated_input,
            message: Some(format!(
                "Permission mode 'dontAsk': '{}' silently denied (add an allow rule to permit).",
                tool_name
            )),
            reason: PermissionDecisionReason::Mode {
                mode: "dontAsk".into(),
            },
        },
    }
}

fn auto_classifier_decision(
    tool_name: &str,
    updated_input: Option<Value>,
    classifier: &AutoClassifierDecision,
    denial_tracker: Option<&mut DenialTracker>,
) -> PermissionDecision {
    let mode = classifier
        .stage
        .map(|stage| format!("auto_classifier_{}", stage.as_str()))
        .unwrap_or_else(|| "auto_classifier".to_string());

    if classifier.unavailable || classifier.transcript_too_long {
        let unavailable_reason = if classifier.transcript_too_long {
            format!(
                "{} could not classify `{}` because the transcript is too long: {}",
                classifier.model, tool_name, classifier.reason
            )
        } else {
            format!(
                "{} is temporarily unavailable, so auto mode cannot determine the safety of `{}` right now: {}",
                classifier.model, tool_name, classifier.reason
            )
        };
        return PermissionDecision {
            behavior: PermissionBehavior::Ask,
            updated_input,
            message: Some(unavailable_reason),
            reason: PermissionDecisionReason::Mode {
                mode: "auto_classifier_unavailable".into(),
            },
        };
    }

    match classifier.verdict {
        AutoClassifierVerdict::Allow => {
            if let Some(tracker) = denial_tracker {
                tracker.record_allow();
            }
            PermissionDecision {
                behavior: PermissionBehavior::Allow,
                updated_input,
                message: None,
                reason: PermissionDecisionReason::Mode { mode },
            }
        }
        AutoClassifierVerdict::Deny => {
            let fallback = denial_tracker
                .map(|tracker| tracker.record_denial())
                .unwrap_or(false);
            if fallback {
                PermissionDecision {
                    behavior: PermissionBehavior::Ask,
                    updated_input,
                    message: Some(format!(
                        "Auto mode fallback after repeated classifier denials for `{}`: {}",
                        tool_name, classifier.reason
                    )),
                    reason: PermissionDecisionReason::Mode {
                        mode: "auto_fallback".into(),
                    },
                }
            } else {
                PermissionDecision {
                    behavior: PermissionBehavior::Deny,
                    updated_input,
                    message: Some(classifier.reason.clone()),
                    reason: PermissionDecisionReason::Mode { mode },
                }
            }
        }
        AutoClassifierVerdict::Ask => PermissionDecision {
            behavior: PermissionBehavior::Ask,
            updated_input,
            message: Some(classifier.reason.clone()),
            reason: PermissionDecisionReason::Mode { mode },
        },
    }
}

/// Match session-grant patterns using a lightweight matcher (no
/// per-tool argument parsing — sessions store tool names verbatim).
fn check_pattern_rules_via_matcher(
    tool_name: &str,
    matcher: Option<&str>,
    rules_by_source: &ToolPermissionRulesBySource,
) -> Option<(String, String)> {
    for (source, patterns) in rules_by_source.iter() {
        for pattern in patterns {
            if pattern_rule_matches_simple(pattern, tool_name, matcher) {
                return Some((source.clone(), pattern.clone()));
            }
        }
    }
    None
}

fn pattern_rule_matches_simple(rule: &str, tool_name: &str, matcher: Option<&str>) -> bool {
    if let Some(inner_start) = rule.find('(') {
        if let Some(inner_end) = rule.rfind(')') {
            let rule_tool = &rule[..inner_start];
            if rule_tool != tool_name {
                return false;
            }
            let pattern = &rule[inner_start + 1..inner_end];
            if let Some(prefix_val) = pattern.strip_prefix("prefix:") {
                return matcher.is_some_and(|m| m.starts_with(prefix_val));
            }
            return matcher.is_some_and(|m| rules::glob_match_public(m, pattern));
        }
    }
    rule == tool_name
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Register the CU message callback used by tests.
    ///
    /// The host normally wires this from the root crate at startup, but
    /// cc-permissions' standalone tests need their own copy of the
    /// Computer Use lookup logic to exercise the `medium risk` / `HIGH
    /// RISK` / `keyboard` code paths. Idempotent — later callers overwrite
    /// an earlier registration, which is fine in this test suite.
    fn install_test_cu_callback() {
        set_cu_message_callback(|tool_name: &str| {
            let rest = tool_name.strip_prefix("mcp__computer-use__")?;
            let (risk, verb) = match rest {
                "screenshot" => ("medium risk", "take a screenshot"),
                "left_click" | "right_click" | "middle_click" | "double_click" => {
                    ("HIGH RISK", "click the mouse on your screen")
                }
                "type_text" | "type" => ("HIGH RISK", "type text using the keyboard"),
                _ => ("medium risk", rest),
            };
            Some(format!("Allow {} [{}]?", verb, risk))
        });
    }

    fn default_ctx() -> ToolPermissionContext {
        ToolPermissionContext {
            mode: PermissionMode::Default,
            additional_working_directories: HashMap::new(),
            always_allow_rules: HashMap::new(),
            always_deny_rules: HashMap::new(),
            always_ask_rules: HashMap::new(),
            session_allow_rules: HashMap::new(),
            auto_mode_stripped_always_allow_rules: Vec::new(),
            auto_mode_stripped_session_allow_rules: Vec::new(),
            is_bypass_permissions_mode_available: false,
            is_auto_mode_available: None,
            pre_plan_mode: None,
        }
    }

    #[test]
    fn test_bypass_mode_allows_everything() {
        let mut ctx = default_ctx();
        ctx.mode = PermissionMode::Bypass;
        let decision = has_permissions_to_use_tool("Bash", &Value::Null, &ctx, None);
        assert_eq!(decision.behavior, PermissionBehavior::Allow);
    }

    #[test]
    fn test_bypass_mode_ignores_ask_and_deny_rules() {
        let mut ctx = default_ctx();
        ctx.mode = PermissionMode::Bypass;
        ctx.always_ask_rules
            .insert("user".into(), vec!["Bash".into()]);
        ctx.always_deny_rules
            .insert("user".into(), vec!["Write".into()]);

        let bash = has_permissions_to_use_tool("Bash", &Value::Null, &ctx, None);
        let write = has_permissions_to_use_tool("Write", &Value::Null, &ctx, None);

        assert_eq!(bash.behavior, PermissionBehavior::Allow);
        assert_eq!(write.behavior, PermissionBehavior::Allow);
    }

    #[test]
    fn test_deny_rule_blocks() {
        let mut ctx = default_ctx();
        ctx.always_deny_rules
            .insert("policy".into(), vec!["Bash".into()]);
        let decision = has_permissions_to_use_tool("Bash", &Value::Null, &ctx, None);
        assert_eq!(decision.behavior, PermissionBehavior::Deny);
    }

    #[test]
    fn test_allow_rule_allows() {
        let mut ctx = default_ctx();
        ctx.always_allow_rules
            .insert("user".into(), vec!["Read".into()]);
        let decision = has_permissions_to_use_tool("Read", &Value::Null, &ctx, None);
        assert_eq!(decision.behavior, PermissionBehavior::Allow);
    }

    #[test]
    fn test_default_mode_asks() {
        let ctx = default_ctx();
        let decision = has_permissions_to_use_tool("SomeTool", &Value::Null, &ctx, None);
        assert_eq!(decision.behavior, PermissionBehavior::Ask);
    }

    #[test]
    fn test_pattern_match_bash_prefix() {
        let mut ctx = default_ctx();
        ctx.always_allow_rules
            .insert("user".into(), vec!["Bash(prefix:git)".into()]);
        let input = serde_json::json!({"command": "git status"});
        let decision = has_permissions_to_use_tool("Bash", &input, &ctx, None);
        assert_eq!(decision.behavior, PermissionBehavior::Allow);
    }

    #[test]
    fn test_pattern_no_match_different_prefix() {
        let mut ctx = default_ctx();
        ctx.always_allow_rules
            .insert("user".into(), vec!["Bash(prefix:git)".into()]);
        let input = serde_json::json!({"command": "rm -rf /"});
        let decision = has_permissions_to_use_tool("Bash", &input, &ctx, None);
        assert_eq!(decision.behavior, PermissionBehavior::Ask);
    }

    #[test]
    fn test_compound_command_caught_by_deny() {
        let mut ctx = default_ctx();
        ctx.always_deny_rules
            .insert("policy".into(), vec!["Bash(rm)".into()]);
        ctx.always_allow_rules
            .insert("user".into(), vec!["Bash(prefix:git)".into()]);
        let input = serde_json::json!({"command": "git status && rm -rf /tmp/x"});
        let decision = has_permissions_to_use_tool("Bash", &input, &ctx, None);
        assert_eq!(decision.behavior, PermissionBehavior::Deny);
    }

    #[test]
    fn test_ask_overrides_allow_in_decision() {
        let mut ctx = default_ctx();
        ctx.always_allow_rules
            .insert("user".into(), vec!["Read".into()]);
        ctx.always_ask_rules
            .insert("project".into(), vec!["Read".into()]);
        let decision = has_permissions_to_use_tool("Read", &Value::Null, &ctx, None);
        assert_eq!(decision.behavior, PermissionBehavior::Ask);
    }

    #[test]
    fn test_accept_edits_mode_allows_edit_tools() {
        let mut ctx = default_ctx();
        ctx.mode = PermissionMode::AcceptEdits;
        let decision = has_permissions_to_use_tool("Edit", &Value::Null, &ctx, None);
        assert_eq!(decision.behavior, PermissionBehavior::Allow);
        let decision = has_permissions_to_use_tool("Bash", &Value::Null, &ctx, None);
        assert_eq!(decision.behavior, PermissionBehavior::Ask);
    }

    #[test]
    fn test_accept_edits_mode_allows_workspace_bash_commands() {
        let dir = tempfile::tempdir().unwrap();
        {
            let mut ps = allthecodes_bootstrap::PROCESS_STATE.write();
            ps.original_cwd = dir.path().to_path_buf();
        }

        let mut ctx = default_ctx();
        ctx.mode = PermissionMode::AcceptEdits;
        let input = serde_json::json!({"command": "mkdir src && touch src/lib.rs"});
        let decision = has_permissions_to_use_tool("Bash", &input, &ctx, None);
        assert_eq!(decision.behavior, PermissionBehavior::Allow);
    }

    #[test]
    fn test_dont_ask_mode_silent_deny() {
        let mut ctx = default_ctx();
        ctx.mode = PermissionMode::DontAsk;
        let decision = has_permissions_to_use_tool("Bash", &Value::Null, &ctx, None);
        assert_eq!(decision.behavior, PermissionBehavior::Deny);
    }

    // -- Hook decision tests --

    #[test]
    fn test_hook_deny_overrides_allow_rule() {
        let mut ctx = default_ctx();
        ctx.always_allow_rules
            .insert("user".into(), vec!["Bash".into()]);
        let hook = HookPermissionDecision {
            deny: Some("PolicyViolation".into()),
            source: Some("PreToolUse:audit".into()),
            ..Default::default()
        };
        let decision =
            has_permissions_to_use_tool_with_hook("Bash", &Value::Null, &ctx, Some(&hook), None);
        assert_eq!(decision.behavior, PermissionBehavior::Deny);
        assert!(decision.message.unwrap().contains("PolicyViolation"));
    }

    #[test]
    fn test_deny_rule_still_blocks_hook_allow() {
        let mut ctx = default_ctx();
        ctx.always_deny_rules
            .insert("policy".into(), vec!["Bash".into()]);
        let hook = HookPermissionDecision {
            allow: true,
            ..Default::default()
        };
        let decision =
            has_permissions_to_use_tool_with_hook("Bash", &Value::Null, &ctx, Some(&hook), None);
        // Per spec: deny rule wins over hook allow.
        assert_eq!(decision.behavior, PermissionBehavior::Deny);
    }

    #[test]
    fn test_ask_rule_still_overrides_hook_allow() {
        let mut ctx = default_ctx();
        ctx.always_ask_rules
            .insert("project".into(), vec!["Bash".into()]);
        let hook = HookPermissionDecision {
            allow: true,
            ..Default::default()
        };
        let decision =
            has_permissions_to_use_tool_with_hook("Bash", &Value::Null, &ctx, Some(&hook), None);
        // Per spec: ask rule wins over hook allow.
        assert_eq!(decision.behavior, PermissionBehavior::Ask);
    }

    #[test]
    fn test_hook_allow_lifts_default_ask() {
        let ctx = default_ctx();
        let hook = HookPermissionDecision {
            allow: true,
            source: Some("PreToolUse:trustedAgent".into()),
            ..Default::default()
        };
        let decision =
            has_permissions_to_use_tool_with_hook("Bash", &Value::Null, &ctx, Some(&hook), None);
        assert_eq!(decision.behavior, PermissionBehavior::Allow);
    }

    #[test]
    fn test_hook_modifies_input() {
        let ctx = default_ctx();
        let modified = serde_json::json!({"command": "ls -la"});
        let hook = HookPermissionDecision {
            allow: true,
            updated_input: Some(modified.clone()),
            ..Default::default()
        };
        let original = serde_json::json!({"command": "rm -rf /"});
        let decision =
            has_permissions_to_use_tool_with_hook("Bash", &original, &ctx, Some(&hook), None);
        assert_eq!(decision.behavior, PermissionBehavior::Allow);
        assert_eq!(decision.updated_input.as_ref(), Some(&modified));
    }

    #[test]
    fn test_hook_decision_is_noop_when_empty() {
        let h = HookPermissionDecision::default();
        assert!(h.is_noop());
    }

    // -- Auto mode + tracker --

    #[test]
    fn test_auto_mode_denial_tracker_fallback() {
        let mut tracker = DenialTracker::default();
        tracker.record_denial();
        tracker.record_denial();
        tracker.record_denial();
        assert!(tracker.should_fallback_to_interactive());
    }

    #[test]
    fn test_auto_mode_allows_by_default() {
        let mut ctx = default_ctx();
        ctx.mode = PermissionMode::Auto;
        let decision = has_permissions_to_use_tool("SomeTool", &Value::Null, &ctx, None);
        assert_eq!(decision.behavior, PermissionBehavior::Allow);
    }

    #[test]
    fn test_auto_mode_classifier_allow_resets_denials() {
        let mut ctx = default_ctx();
        ctx.mode = PermissionMode::Auto;
        let classifier = AutoClassifierDecision::allow("sonnet-test", AutoClassifierStage::Fast);
        let mut tracker = DenialTracker::default();
        tracker.record_denial();

        let decision = has_permissions_to_use_tool_with_hook_and_auto_classifier(
            "Bash",
            &serde_json::json!({"command": "cargo test"}),
            &ctx,
            None,
            Some(&classifier),
            Some(&mut tracker),
        );

        assert_eq!(decision.behavior, PermissionBehavior::Allow);
        assert_eq!(tracker.consecutive_denials, 0);
        if let PermissionDecisionReason::Mode { mode } = decision.reason {
            assert_eq!(mode, "auto_classifier_fast");
        } else {
            panic!("expected classifier mode reason");
        }
    }

    #[test]
    fn test_auto_mode_classifier_deny_blocks_and_records_denial() {
        let mut ctx = default_ctx();
        ctx.mode = PermissionMode::Auto;
        let classifier = AutoClassifierDecision::deny(
            "sonnet-test",
            AutoClassifierStage::Thinking,
            "command modifies production resources",
        );
        let mut tracker = DenialTracker::default();

        let decision = has_permissions_to_use_tool_with_hook_and_auto_classifier(
            "Bash",
            &serde_json::json!({"command": "kubectl delete pod prod"}),
            &ctx,
            None,
            Some(&classifier),
            Some(&mut tracker),
        );

        assert_eq!(decision.behavior, PermissionBehavior::Deny);
        assert_eq!(tracker.consecutive_denials, 1);
        assert_eq!(
            decision.message.as_deref(),
            Some("command modifies production resources")
        );
    }

    #[test]
    fn test_auto_mode_classifier_repeated_denials_fall_back_to_ask() {
        let mut ctx = default_ctx();
        ctx.mode = PermissionMode::Auto;
        let classifier = AutoClassifierDecision::deny(
            "sonnet-test",
            AutoClassifierStage::Thinking,
            "still unsafe",
        );
        let mut tracker = DenialTracker::default();
        tracker.record_denial();
        tracker.record_denial();

        let decision = has_permissions_to_use_tool_with_hook_and_auto_classifier(
            "Bash",
            &serde_json::json!({"command": "rm -rf data"}),
            &ctx,
            None,
            Some(&classifier),
            Some(&mut tracker),
        );

        assert_eq!(decision.behavior, PermissionBehavior::Ask);
        assert_eq!(tracker.consecutive_denials, 3);
        assert!(decision.message.unwrap().contains("fallback"));
    }

    #[test]
    fn test_auto_mode_classifier_unavailable_falls_back_to_ask() {
        let mut ctx = default_ctx();
        ctx.mode = PermissionMode::Auto;
        let classifier = AutoClassifierDecision::unavailable("sonnet-test", "rate limited");

        let decision = has_permissions_to_use_tool_with_hook_and_auto_classifier(
            "Write",
            &serde_json::json!({"file_path": "src/lib.rs"}),
            &ctx,
            None,
            Some(&classifier),
            None,
        );

        assert_eq!(decision.behavior, PermissionBehavior::Ask);
        assert!(decision
            .message
            .unwrap()
            .contains("temporarily unavailable"));
    }

    #[test]
    fn test_denial_tracker_reset_on_allow() {
        let mut tracker = DenialTracker::default();
        tracker.record_denial();
        tracker.record_denial();
        assert_eq!(tracker.consecutive_denials, 2);
        tracker.record_allow();
        assert_eq!(tracker.consecutive_denials, 0);
        assert_eq!(tracker.total_denials, 2);
    }

    // -- Session grants --

    #[test]
    fn test_session_grant_allows_tool() {
        let mut ctx = default_ctx();
        ctx.session_allow_rules.insert(
            "session".into(),
            vec!["mcp__computer-use__screenshot".into()],
        );
        let decision =
            has_permissions_to_use_tool("mcp__computer-use__screenshot", &Value::Null, &ctx, None);
        assert_eq!(decision.behavior, PermissionBehavior::Allow);
        if let PermissionDecisionReason::Rule { source, .. } = &decision.reason {
            assert_eq!(source, "session");
        } else {
            panic!("expected Rule reason with session source");
        }
    }

    #[test]
    fn test_session_grant_does_not_override_deny_rules() {
        let mut ctx = default_ctx();
        ctx.always_deny_rules.insert(
            "policy".into(),
            vec!["mcp__computer-use__left_click".into()],
        );
        ctx.session_allow_rules.insert(
            "session".into(),
            vec!["mcp__computer-use__left_click".into()],
        );
        let decision =
            has_permissions_to_use_tool("mcp__computer-use__left_click", &Value::Null, &ctx, None);
        assert_eq!(decision.behavior, PermissionBehavior::Deny);
    }

    #[test]
    fn test_session_grant_via_context_method() {
        let mut ctx = default_ctx();
        ctx.grant_session_allow("mcp__computer-use__screenshot");
        assert!(ctx.has_session_grant("mcp__computer-use__screenshot"));
        assert!(!ctx.has_session_grant("mcp__computer-use__left_click"));
    }

    #[test]
    fn test_session_grant_clear() {
        let mut ctx = default_ctx();
        ctx.grant_session_allow("mcp__computer-use__screenshot");
        assert!(ctx.has_session_grant("mcp__computer-use__screenshot"));
        ctx.clear_session_grants();
        assert!(!ctx.has_session_grant("mcp__computer-use__screenshot"));
    }

    // -- CU permission message --

    #[test]
    fn test_cu_screenshot_permission_message() {
        install_test_cu_callback();
        let ctx = default_ctx();
        let decision =
            has_permissions_to_use_tool("mcp__computer-use__screenshot", &Value::Null, &ctx, None);
        assert_eq!(decision.behavior, PermissionBehavior::Ask);
        let msg = decision.message.unwrap();
        assert!(msg.contains("screenshot"));
        assert!(msg.contains("medium risk"));
    }

    #[test]
    fn test_cu_click_permission_message() {
        install_test_cu_callback();
        let ctx = default_ctx();
        let decision = has_permissions_to_use_tool(
            "mcp__computer-use__left_click",
            &serde_json::json!({"x": 100, "y": 200}),
            &ctx,
            None,
        );
        assert_eq!(decision.behavior, PermissionBehavior::Ask);
        let msg = decision.message.unwrap();
        assert!(msg.contains("click"));
        assert!(msg.contains("HIGH RISK"));
    }

    #[test]
    fn test_cu_type_text_permission_message() {
        install_test_cu_callback();
        let ctx = default_ctx();
        let decision = has_permissions_to_use_tool(
            "mcp__computer-use__type_text",
            &serde_json::json!({"text": "hello"}),
            &ctx,
            None,
        );
        let msg = decision.message.unwrap();
        assert!(msg.contains("keyboard"));
    }

    #[test]
    fn test_non_cu_tool_gets_generic_message() {
        let ctx = default_ctx();
        let decision = has_permissions_to_use_tool("Bash", &Value::Null, &ctx, None);
        let msg = decision.message.unwrap();
        assert_eq!(msg, "Allow tool 'Bash'?");
    }
}
