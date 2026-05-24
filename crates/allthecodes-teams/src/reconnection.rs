//! Team/Swarm reconnection and context initialization.
//!
//! Corresponds to TypeScript: `utils/swarm/reconnection.ts`
//!
//! Handles initialization of team context for teammates:
//! - **Fresh spawns**: Compute TeamContext from the persisted team config,
//!   determining the caller's role (leader vs teammate).
//! - **Resumed sessions**: Restore TeamContext from the team config when a
//!   previously-persisted session is reloaded, so mailbox polling and other
//!   swarm features can resume correctly.

use anyhow::Result;
use tracing::{debug, warn};

use super::helpers;
use super::types::{TeamContext, TeamFile, TeamMember};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Compute the initial TeamContext for the current agent.
///
/// Reads the team config file and determines whether the caller is the
/// team leader or a teammate based on the provided `agent_id`.
///
/// When `agent_id` is `None`, the caller is assumed to be the team leader
/// (this is the common case — the leader process reads the team file at
/// startup to populate its AppState).
///
/// Corresponds to TS: `computeInitialTeamContext()`
pub fn compute_team_context(
    team_name: &str,
    agent_id: Option<&str>,
) -> Result<Option<TeamContext>> {
    let Some(team_file) = read_optional_team_file(team_name)? else {
        return Ok(None);
    };

    let lead_agent_id = team_file.lead_agent_id.clone();

    // No agent_id → caller is the leader
    let Some(agent_id) = agent_id else {
        return build_leader_context(&team_file, &lead_agent_id);
    };

    // agent_id matches the leader
    if agent_id == lead_agent_id {
        return build_leader_context(&team_file, &lead_agent_id);
    }

    // agent_id matches a teammate
    let member = team_file.members.iter().find(|m| m.agent_id == agent_id);
    let Some(member) = member else {
        return Ok(None);
    };

    Ok(Some(build_teammate_context(
        &team_file,
        &member.name,
        Some(member),
    )))
}

/// Restore TeamContext from a team config when resuming a session.
///
/// Looks up the member by `agent_name` in the team file and reconstructs
/// the TeamContext so swarm features (heartbeat, mailbox polling, etc.)
/// have valid routing state.
///
/// Returns `None` when the team file or the named member cannot be found.
///
/// Corresponds to TS: `initializeTeammateContextFromSession()`
pub fn restore_team_context(team_name: &str, agent_name: &str) -> Result<Option<TeamContext>> {
    let Some(team_file) = read_optional_team_file(team_name)? else {
        return Ok(None);
    };

    // Look up by agent_name (short name, without @team suffix)
    let member = team_file.members.iter().find(|m| m.name == agent_name);
    Ok(Some(build_teammate_context(&team_file, agent_name, member)))
}

/// Restore TeamContext by matching a persisted session ID to a team file.
pub fn restore_team_context_for_session(session_id: &str) -> Result<Option<TeamContext>> {
    if session_id.trim().is_empty() {
        return Ok(None);
    }

    for team_name in helpers::list_team_names()? {
        let team_file = match helpers::read_team_file(&team_name) {
            Ok(team_file) => team_file,
            Err(err) => {
                warn!(team = %team_name, error = %err, "failed to read team config while restoring session context");
                continue;
            }
        };

        if team_file.lead_session_id.as_deref() == Some(session_id) {
            return build_leader_context(&team_file, &team_file.lead_agent_id);
        }

        if let Some(member) = team_file
            .members
            .iter()
            .find(|member| member.session_id.as_deref() == Some(session_id))
        {
            return Ok(Some(build_teammate_context(
                &team_file,
                &member.name,
                Some(member),
            )));
        }
    }

    Ok(None)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Build a leader-scoped TeamContext from the team file.
fn build_leader_context(team_file: &TeamFile, lead_agent_id: &str) -> Result<Option<TeamContext>> {
    let team_name = &team_file.name;
    let team_file_path = helpers::team_config_path(team_name)
        .to_string_lossy()
        .into_owned();

    // Find the leader record in the member list; fall back to first member.
    let leader = team_file
        .members
        .iter()
        .find(|m| m.agent_id == *lead_agent_id)
        .or_else(|| team_file.members.first());

    let Some(leader) = leader else {
        return Ok(None);
    };

    Ok(Some(TeamContext {
        team_name: team_name.clone(),
        team_file_path,
        lead_agent_id: lead_agent_id.to_string(),
        self_agent_id: Some(leader.agent_id.clone()),
        self_agent_name: Some(leader.name.clone()),
        is_leader: Some(true),
        self_agent_color: leader.color.clone(),
        ..Default::default()
    }))
}

fn build_teammate_context(
    team_file: &TeamFile,
    agent_name: &str,
    member: Option<&TeamMember>,
) -> TeamContext {
    let team_name = &team_file.name;
    let team_file_path = helpers::team_config_path(team_name)
        .to_string_lossy()
        .into_owned();

    TeamContext {
        team_name: team_name.clone(),
        team_file_path,
        lead_agent_id: team_file.lead_agent_id.clone(),
        self_agent_id: member.map(|m| m.agent_id.clone()),
        self_agent_name: Some(agent_name.to_string()),
        is_leader: Some(false),
        self_agent_color: member.and_then(|m| m.color.clone()),
        ..Default::default()
    }
}

fn read_optional_team_file(team_name: &str) -> Result<Option<TeamFile>> {
    if !helpers::team_exists(team_name) {
        debug!(team = %team_name, "team config not found while restoring team context");
        return Ok(None);
    }
    helpers::read_team_file(team_name).map(Some)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::helpers;
    use crate::identity;
    use crate::types::*;
    use serial_test::serial;
    use tempfile::TempDir;

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
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }

    fn setup_team(tmp: &TempDir, team_name: &str) -> String {
        let _home = EnvGuard::set("ALLTHECODES_HOME", tmp.path().to_str().unwrap());
        let tf = helpers::create_team(team_name, None, None, "/tmp").unwrap();
        let lead_id = identity::lead_agent_id(&tf.name);

        let researcher = TeamMember {
            agent_id: identity::format_agent_id("researcher", &tf.name),
            name: "researcher".into(),
            agent_type: Some("researcher".into()),
            model: None,
            prompt: Some("Research things".into()),
            color: Some("blue".into()),
            plan_mode_required: None,
            joined_at: chrono::Utc::now().timestamp(),
            tmux_pane_id: String::new(),
            cwd: "/tmp".into(),
            worktree_path: None,
            session_id: None,
            subscriptions: vec![],
            backend_type: Some(BackendType::InProcess),
            is_active: Some(true),
            mode: None,
        };
        helpers::add_member(&tf.name, researcher).unwrap();

        lead_id
    }

    #[test]
    #[serial]
    fn compute_context_resolves_leader_when_no_agent_id() {
        let tmp = TempDir::new().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", tmp.path().to_str().unwrap());
        let team_name = "compute-leader-test";
        let lead_id = setup_team(&tmp, team_name);

        // Read back actual name from create_team (may have suffix)
        let team_file = helpers::read_team_file(team_name).unwrap();
        let actual_name = &team_file.name;

        let ctx = compute_team_context(actual_name, None)
            .unwrap()
            .expect("context should be Some");

        assert_eq!(ctx.lead_agent_id, lead_id);
        assert_eq!(ctx.self_agent_id, Some(lead_id.clone()));
        assert_eq!(ctx.is_leader, Some(true));
        helpers::cleanup_team_directories(actual_name).ok();
    }

    #[test]
    #[serial]
    fn compute_context_resolves_leader_by_agent_id() {
        let tmp = TempDir::new().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", tmp.path().to_str().unwrap());
        let team_name = "compute-leader-id";
        let lead_id = setup_team(&tmp, team_name);

        let team_file = helpers::read_team_file(team_name).unwrap();
        let actual_name = &team_file.name;

        let ctx = compute_team_context(actual_name, Some(&lead_id))
            .unwrap()
            .expect("context should be Some");

        assert_eq!(ctx.is_leader, Some(true));
        assert_eq!(ctx.self_agent_id, Some(lead_id));
        helpers::cleanup_team_directories(actual_name).ok();
    }

    #[test]
    #[serial]
    fn compute_context_resolves_teammate_by_agent_id() {
        let tmp = TempDir::new().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", tmp.path().to_str().unwrap());
        let team_name = "compute-teammate";
        let _lead_id = setup_team(&tmp, team_name);

        let team_file = helpers::read_team_file(team_name).unwrap();
        let actual_name = &team_file.name;
        let researcher_id = identity::format_agent_id("researcher", actual_name);

        let ctx = compute_team_context(actual_name, Some(&researcher_id))
            .unwrap()
            .expect("context should be Some");

        assert_eq!(ctx.is_leader, Some(false));
        assert_eq!(ctx.self_agent_id, Some(researcher_id));
        assert_eq!(ctx.self_agent_name.as_deref(), Some("researcher"));
        assert_eq!(ctx.self_agent_color.as_deref(), Some("blue"));
        helpers::cleanup_team_directories(actual_name).ok();
    }

    #[test]
    #[serial]
    fn compute_context_returns_none_for_nonexistent_agent() {
        let tmp = TempDir::new().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", tmp.path().to_str().unwrap());
        let team_name = "compute-missing";
        let _lead_id = setup_team(&tmp, team_name);

        let team_file = helpers::read_team_file(team_name).unwrap();
        let actual_name = &team_file.name;

        let ctx = compute_team_context(actual_name, Some("ghost@nonexistent")).unwrap();
        assert!(ctx.is_none());
        helpers::cleanup_team_directories(actual_name).ok();
    }

    #[test]
    #[serial]
    fn restore_context_finds_member_by_name() {
        let tmp = TempDir::new().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", tmp.path().to_str().unwrap());
        let team_name = "restore-test";
        let _lead_id = setup_team(&tmp, team_name);

        let team_file = helpers::read_team_file(team_name).unwrap();
        let actual_name = &team_file.name;

        let ctx = restore_team_context(actual_name, "researcher")
            .unwrap()
            .expect("context should be Some");

        assert_eq!(ctx.is_leader, Some(false));
        assert_eq!(ctx.self_agent_name.as_deref(), Some("researcher"));
        assert_eq!(ctx.self_agent_color.as_deref(), Some("blue"));

        // Lead field should be populated from the file
        let expected_lead = identity::lead_agent_id(actual_name);
        assert_eq!(ctx.lead_agent_id, expected_lead);
        helpers::cleanup_team_directories(actual_name).ok();
    }

    #[test]
    #[serial]
    fn restore_context_preserves_unknown_member_name() {
        let tmp = TempDir::new().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", tmp.path().to_str().unwrap());
        let team_name = "restore-missing";
        let _lead_id = setup_team(&tmp, team_name);

        let team_file = helpers::read_team_file(team_name).unwrap();
        let actual_name = &team_file.name;

        let ctx = restore_team_context(actual_name, "nonexistent")
            .unwrap()
            .expect("context should preserve resumed agent name");
        assert_eq!(ctx.self_agent_id, None);
        assert_eq!(ctx.self_agent_name.as_deref(), Some("nonexistent"));
        assert_eq!(ctx.is_leader, Some(false));
        helpers::cleanup_team_directories(actual_name).ok();
    }

    #[test]
    fn compute_context_returns_none_for_nonexistent_team() {
        let result = compute_team_context("definitely-not-a-real-team-xyz", None);
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn restore_context_returns_none_for_nonexistent_team() {
        let result = restore_team_context("definitely-not-a-real-team-xyz", "researcher");
        assert!(result.unwrap().is_none());
    }

    #[test]
    #[serial]
    fn restore_context_for_session_resolves_leader() {
        let tmp = TempDir::new().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", tmp.path().to_str().unwrap());
        let tf =
            helpers::create_team("session-restore", None, Some("sess-123".into()), "/tmp").unwrap();

        let ctx = restore_team_context_for_session("sess-123")
            .unwrap()
            .expect("leader context should be found");

        assert_eq!(ctx.team_name, tf.name);
        assert_eq!(
            ctx.self_agent_id.as_deref(),
            Some(tf.lead_agent_id.as_str())
        );
        assert_eq!(ctx.is_leader, Some(true));
        helpers::cleanup_team_directories(&ctx.team_name).ok();
    }
}
