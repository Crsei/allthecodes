//! Teammate identity system.
//!
//! Corresponds to TypeScript: `utils/teammate.ts`
//!
//! Provides agent ID formatting/parsing and identity resolution functions.
//! In TS these use AsyncLocalStorage for context propagation; in Rust we
//! use `tokio::task_local!` (set up in `context.rs`).

use std::env;

use super::constants::*;
use super::context;
use super::types::TeamContext;

// ---------------------------------------------------------------------------
// Agent ID formatting / parsing
// ---------------------------------------------------------------------------

/// Format an agent ID from a name and team name.
///
/// Format: `"{agent_name}@{team_name}"`
///
/// Corresponds to TS: `formatAgentId(name, team)`
pub fn format_agent_id(agent_name: &str, team_name: &str) -> String {
    format!("{}@{}", agent_name, team_name)
}

/// Parse an agent ID into `(agent_name, team_name)`.
///
/// Returns `None` if the ID doesn't contain `@`.
///
/// Corresponds to TS: `parseAgentId(id)`
pub fn parse_agent_id(agent_id: &str) -> Option<(String, String)> {
    let at_pos = agent_id.find('@')?;
    let agent_name = agent_id[..at_pos].to_string();
    let team_name = agent_id[at_pos + 1..].to_string();
    if agent_name.is_empty() || team_name.is_empty() {
        return None;
    }
    Some((agent_name, team_name))
}

// ---------------------------------------------------------------------------
// Identity resolution (3-layer priority chain)
// ---------------------------------------------------------------------------

/// Get the current agent ID.
///
/// Priority: task_local > dynamic context > env var
///
/// Corresponds to TS: `getAgentId()`
pub fn get_agent_id() -> Option<String> {
    // 1. Check task_local (in-process teammate)
    if let Some(id) = context::try_get_agent_id() {
        return Some(id);
    }
    // 2. Check environment variable (tmux/iTerm2 teammate)
    env::var("ALLTHECODES_AGENT_ID").ok()
}

/// Get the current agent name (part before `@`).
///
/// Corresponds to TS: `getAgentName()`
pub fn get_agent_name() -> Option<String> {
    get_agent_id().and_then(|id| parse_agent_id(&id).map(|(name, _)| name))
}

/// Get the current team name.
///
/// Priority: task_local > dynamic context > env var
///
/// Corresponds to TS: `getTeamName(ctx?)`
pub fn get_team_name() -> Option<String> {
    // 1. task_local
    if let Some(name) = context::try_get_team_name() {
        return Some(name);
    }
    // 2. env var / dynamic context
    get_agent_id().and_then(|id| parse_agent_id(&id).map(|(_, team)| team))
}

/// Check if the current execution context is a teammate.
///
/// True if we have both an agent_id and a team_name.
///
/// Corresponds to TS: `isTeammate()`
pub fn is_teammate() -> bool {
    get_agent_id().is_some() && get_team_name().is_some()
}

/// Get the teammate's assigned UI color.
///
/// Corresponds to TS: `getTeammateColor()`
pub fn get_teammate_color() -> Option<String> {
    context::try_get_color().or_else(|| env::var(TEAMMATE_COLOR_ENV_VAR).ok())
}

/// Check if plan mode is required for this teammate.
///
/// Corresponds to TS: `isPlanModeRequired()`
pub fn is_plan_mode_required() -> bool {
    if let Some(required) = context::try_get_plan_mode_required() {
        return required;
    }
    env::var(PLAN_MODE_REQUIRED_ENV_VAR)
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// Check if the current agent is the team leader.
///
/// Corresponds to TS: `isTeamLead(ctx?)`
pub fn is_team_lead(team_ctx: Option<&TeamContext>) -> bool {
    if let Some(ctx) = team_ctx {
        if let Some(ref self_id) = ctx.self_agent_id {
            return *self_id == ctx.lead_agent_id;
        }
    }
    // Fallback: check if name is "team-lead"
    get_agent_name()
        .map(|n| n == TEAM_LEAD_NAME)
        .unwrap_or(false)
}

/// Build the lead agent ID for a given team.
pub fn lead_agent_id(team_name: &str) -> String {
    format_agent_id(TEAM_LEAD_NAME, team_name)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_agent_id() {
        assert_eq!(
            format_agent_id("researcher", "my-team"),
            "researcher@my-team"
        );
        assert_eq!(
            format_agent_id("team-lead", "project-x"),
            "team-lead@project-x"
        );
    }

    #[test]
    fn test_parse_agent_id_valid() {
        let (name, team) = parse_agent_id("researcher@my-team").unwrap();
        assert_eq!(name, "researcher");
        assert_eq!(team, "my-team");
    }

    #[test]
    fn test_parse_agent_id_invalid() {
        assert!(parse_agent_id("no-at-sign").is_none());
        assert!(parse_agent_id("@no-name").is_none());
        assert!(parse_agent_id("no-team@").is_none());
    }

    #[test]
    fn test_parse_agent_id_multiple_at() {
        let (name, team) = parse_agent_id("user@team@extra").unwrap();
        assert_eq!(name, "user");
        assert_eq!(team, "team@extra");
    }

    #[test]
    fn test_lead_agent_id() {
        assert_eq!(lead_agent_id("my-team"), "team-lead@my-team");
    }

    #[test]
    fn test_is_team_lead_with_context() {
        let ctx = TeamContext {
            lead_agent_id: "team-lead@t".into(),
            self_agent_id: Some("team-lead@t".into()),
            ..TeamContext::default()
        };
        assert!(is_team_lead(Some(&ctx)));

        let ctx2 = TeamContext {
            lead_agent_id: "team-lead@t".into(),
            self_agent_id: Some("researcher@t".into()),
            ..TeamContext::default()
        };
        assert!(!is_team_lead(Some(&ctx2)));
    }

    #[test]
    fn test_not_teammate_without_context() {
        // Without task_local or env vars set, should not be a teammate
        assert!(!is_teammate());
    }
}
