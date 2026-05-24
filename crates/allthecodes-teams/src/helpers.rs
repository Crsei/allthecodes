//! TeamFile CRUD and directory management.
//!
//! Corresponds to TypeScript: `utils/swarm/teamHelpers.ts`
//!
//! Handles reading/writing TeamFile config, directory creation/cleanup,
//! worktree destruction, and color assignment.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tracing::{debug, info, warn};

use super::constants::*;
use super::identity;
use super::layout_manager;
use super::mailbox;
use super::types::*;

// ---------------------------------------------------------------------------
// TeamFile I/O
// ---------------------------------------------------------------------------

/// Read a TeamFile from disk.
///
/// Path: `{data_root}/teams/{team_name}/config.json`
pub fn read_team_file(team_name: &str) -> Result<TeamFile> {
    let path = team_config_path(team_name);
    let content = fs::read_to_string(&path)
        .with_context(|| format!("failed to read team config: {}", path.display()))?;
    let tf: TeamFile = serde_json::from_str(&content)
        .with_context(|| format!("failed to parse team config: {}", path.display()))?;
    Ok(tf)
}

/// Write a TeamFile to disk.
pub fn write_team_file(team_name: &str, team_file: &TeamFile) -> Result<()> {
    let path = team_config_path(team_name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(team_file)?;
    fs::write(&path, json)?;
    Ok(())
}

/// Get the config.json path for a team.
pub fn team_config_path(team_name: &str) -> PathBuf {
    mailbox::team_dir(team_name).join(TEAM_CONFIG_FILENAME)
}

/// Get the tasks directory for a team.
pub fn team_tasks_dir(team_name: &str) -> PathBuf {
    crate::storage_paths::tasks_dir().join(sanitize_team_name(team_name))
}

// ---------------------------------------------------------------------------
// Team creation
// ---------------------------------------------------------------------------

/// Generate a unique team name (append slug if name already exists).
pub fn generate_unique_team_name(base_name: &str) -> String {
    let sanitized = sanitize_team_name(base_name);
    let dir = mailbox::team_dir(&sanitized);
    if !dir.exists() {
        return sanitized;
    }
    // Append a short random suffix
    let suffix = &uuid::Uuid::new_v4().to_string()[..8];
    format!("{}-{}", sanitized, suffix)
}

/// Create a new team and return the TeamFile.
///
/// Steps: validate → generate name → build TeamFile → write to disk.
pub fn create_team(
    name: &str,
    description: Option<String>,
    session_id: Option<String>,
    cwd: &str,
) -> Result<TeamFile> {
    let team_name = generate_unique_team_name(name);
    let lead_id = identity::lead_agent_id(&team_name);
    let now = chrono::Utc::now().timestamp();

    let team_file = TeamFile {
        name: team_name.clone(),
        description,
        created_at: now,
        lead_agent_id: lead_id.clone(),
        lead_session_id: session_id,
        hidden_pane_ids: vec![],
        team_allowed_paths: vec![],
        members: vec![TeamMember {
            agent_id: lead_id,
            name: TEAM_LEAD_NAME.to_string(),
            agent_type: None,
            model: None,
            prompt: None,
            color: None,
            plan_mode_required: None,
            joined_at: now,
            tmux_pane_id: String::new(),
            cwd: cwd.to_string(),
            worktree_path: None,
            session_id: None,
            subscriptions: vec![],
            backend_type: Some(BackendType::InProcess),
            is_active: Some(true),
            mode: None,
        }],
    };

    write_team_file(&team_name, &team_file)?;

    // Ensure tasks directory exists
    let tasks_dir = team_tasks_dir(&team_name);
    fs::create_dir_all(&tasks_dir)?;

    info!(team = %team_name, "team created");
    Ok(team_file)
}

// ---------------------------------------------------------------------------
// Team member management
// ---------------------------------------------------------------------------

/// Add a member to an existing team.
pub fn add_member(team_name: &str, member: TeamMember) -> Result<()> {
    let mut tf = read_team_file(team_name)?;
    tf.members.push(member);
    write_team_file(team_name, &tf)
}

/// Update a member's active status.
pub fn set_member_active(team_name: &str, agent_id: &str, active: bool) -> Result<()> {
    let mut tf = read_team_file(team_name)?;
    if let Some(member) = tf.members.iter_mut().find(|m| m.agent_id == agent_id) {
        member.is_active = Some(active);
    }
    write_team_file(team_name, &tf)
}

/// Get active (non-lead) members of a team.
pub fn get_active_members(team_file: &TeamFile) -> Vec<&TeamMember> {
    team_file
        .members
        .iter()
        .filter(|m| m.name != TEAM_LEAD_NAME && m.is_active != Some(false))
        .collect()
}

/// Get all non-lead members.
pub fn get_non_lead_members(team_file: &TeamFile) -> Vec<&TeamMember> {
    team_file
        .members
        .iter()
        .filter(|m| m.name != TEAM_LEAD_NAME)
        .collect()
}

// ---------------------------------------------------------------------------
// Color assignment
// ---------------------------------------------------------------------------

/// Assign a color to a new teammate (round-robin from available colors).
pub fn assign_color(team_file: &TeamFile) -> String {
    let used_colors: HashSet<&str> = team_file
        .members
        .iter()
        .filter_map(|m| m.color.as_deref())
        .collect();

    AGENT_COLORS
        .iter()
        .find(|c| !used_colors.contains(*c))
        .unwrap_or(&AGENT_COLORS[0])
        .to_string()
}

/// Assign a stable color for a specific teammate ID.
pub fn assign_color_for_teammate(team_file: &TeamFile, teammate_id: &str) -> String {
    if let Some(existing) = team_file
        .members
        .iter()
        .find(|m| m.agent_id == teammate_id)
        .and_then(|m| m.color.clone())
    {
        layout_manager::remember_teammate_color(teammate_id, &existing);
        return existing;
    }

    let assigned = layout_manager::assign_teammate_color(teammate_id).to_string();
    let used_by_other = team_file
        .members
        .iter()
        .filter(|m| m.agent_id != teammate_id)
        .filter_map(|m| m.color.as_deref())
        .any(|color| color == assigned);

    if used_by_other && team_file.members.len() < AGENT_COLORS.len() {
        let fallback = assign_color(team_file);
        layout_manager::remember_teammate_color(teammate_id, &fallback);
        fallback
    } else {
        assigned
    }
}

/// Map a logical color name to a tmux color.
pub fn tmux_color(color: &str) -> &str {
    match color {
        "red" => "red",
        "blue" => "blue",
        "green" => "green",
        "yellow" => "yellow",
        "purple" => "magenta",
        "orange" => "colour208",
        "pink" => "colour205",
        "cyan" => "cyan",
        _ => "default",
    }
}

// ---------------------------------------------------------------------------
// Cleanup
// ---------------------------------------------------------------------------

/// Cleanup all team directories (config, inboxes, worktrees, tasks).
///
/// Corresponds to TS: `cleanupTeamDirectories(teamName)`
pub fn cleanup_team_directories(team_name: &str) -> Result<()> {
    // Read team file for worktree paths before deletion
    let worktree_paths: Vec<String> = read_team_file(team_name)
        .map(|tf| {
            tf.members
                .iter()
                .filter_map(|m| m.worktree_path.clone())
                .collect()
        })
        .unwrap_or_default();

    // Destroy worktrees
    for wt_path in &worktree_paths {
        destroy_worktree(wt_path);
    }

    // Remove team directory
    let team_path = mailbox::team_dir(team_name);
    if team_path.exists() {
        fs::remove_dir_all(&team_path)
            .with_context(|| format!("failed to remove team dir: {}", team_path.display()))?;
        debug!(team = %team_name, "team directory removed");
    }

    // Remove tasks directory
    let tasks_path = team_tasks_dir(team_name);
    if tasks_path.exists() {
        let _ = fs::remove_dir_all(&tasks_path);
        debug!(team = %team_name, "tasks directory removed");
    }

    layout_manager::clear_teammate_colors();

    info!(team = %team_name, "team directories cleaned up");
    Ok(())
}

/// Destroy a git worktree (with force fallback to rm -rf).
fn destroy_worktree(path: &str) {
    let wt = Path::new(path);
    if !wt.exists() {
        return;
    }

    // Try git worktree remove --force first
    let result = std::process::Command::new("git")
        .args(["worktree", "remove", "--force", path])
        .output();

    match result {
        Ok(output) if output.status.success() => {
            debug!(path, "worktree removed via git");
        }
        _ => {
            // Fallback: direct removal
            if let Err(e) = fs::remove_dir_all(wt) {
                warn!(path, error = %e, "failed to remove worktree directory");
            } else {
                debug!(path, "worktree removed via rm");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Sanitize a team name for use in directory names.
fn sanitize_team_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Check if a team exists on disk.
pub fn team_exists(team_name: &str) -> bool {
    team_config_path(team_name).exists()
}

/// List team names that have a readable config path on disk.
pub fn list_team_names() -> Result<Vec<String>> {
    let teams_root = crate::storage_paths::teams_dir();
    if !teams_root.exists() {
        return Ok(Vec::new());
    }

    let mut names = Vec::new();
    for entry in fs::read_dir(&teams_root)
        .with_context(|| format!("failed to read teams dir: {}", teams_root.display()))?
    {
        let entry = entry?;
        if !entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
            continue;
        }
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if team_exists(&name) {
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
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

    fn cleanup(team_name: &str) {
        let dir = mailbox::team_dir(team_name);
        let _ = fs::remove_dir_all(&dir);
        let tasks = team_tasks_dir(team_name);
        let _ = fs::remove_dir_all(&tasks);
    }

    #[test]
    fn test_sanitize_team_name() {
        assert_eq!(sanitize_team_name("my-team"), "my-team");
        assert_eq!(sanitize_team_name("my team!"), "my_team_");
        assert_eq!(sanitize_team_name("test@123"), "test_123");
    }

    #[test]
    #[serial]
    fn test_create_and_read_team() {
        let tmp = TempDir::new().expect("tempdir");
        let _home = EnvGuard::set(
            "ALLTHECODES_HOME",
            tmp.path().to_str().expect("utf8 tempdir"),
        );

        let name = format!("test-{}", &uuid::Uuid::new_v4().to_string()[..8]);
        let tf = create_team(&name, Some("Test team".into()), None, "/tmp").unwrap();
        assert_eq!(tf.members.len(), 1);
        assert_eq!(tf.members[0].name, TEAM_LEAD_NAME);

        let read_back = read_team_file(&tf.name).unwrap();
        assert_eq!(read_back.name, tf.name);
        assert_eq!(read_back.description, Some("Test team".into()));

        cleanup(&tf.name);
    }

    #[test]
    #[serial]
    fn test_add_member() {
        let tmp = TempDir::new().expect("tempdir");
        let _home = EnvGuard::set(
            "ALLTHECODES_HOME",
            tmp.path().to_str().expect("utf8 tempdir"),
        );

        let name = format!("test-{}", &uuid::Uuid::new_v4().to_string()[..8]);
        let tf = create_team(&name, None, None, "/tmp").unwrap();

        let member = TeamMember {
            agent_id: identity::format_agent_id("researcher", &tf.name),
            name: "researcher".into(),
            agent_type: Some("researcher".into()),
            model: None,
            prompt: None,
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
        add_member(&tf.name, member).unwrap();

        let updated = read_team_file(&tf.name).unwrap();
        assert_eq!(updated.members.len(), 2);

        cleanup(&tf.name);
    }

    #[test]
    #[serial]
    fn create_team_honors_allthecodes_home_for_config_and_tasks() {
        let tmp = TempDir::new().expect("tempdir");
        let _home = EnvGuard::set(
            "ALLTHECODES_HOME",
            tmp.path().to_str().expect("utf8 tempdir"),
        );

        let name = format!("test-{}", &uuid::Uuid::new_v4().to_string()[..8]);
        let tf = create_team(&name, Some("Test team".into()), None, "/tmp").unwrap();

        let config_path = team_config_path(&tf.name);
        let tasks_path = team_tasks_dir(&tf.name);
        let data_root = tmp.path().join(".allthecodes");

        assert!(
            config_path.starts_with(&data_root),
            "config path escaped ALLTHECODES_HOME: {}",
            config_path.display()
        );
        assert!(
            tasks_path.starts_with(&data_root),
            "tasks path escaped ALLTHECODES_HOME: {}",
            tasks_path.display()
        );
        assert!(
            config_path.exists(),
            "team config missing at {}",
            config_path.display()
        );
        assert!(
            tasks_path.is_dir(),
            "tasks dir missing at {}",
            tasks_path.display()
        );

        cleanup(&tf.name);
    }

    #[test]
    fn test_get_active_members() {
        let tf = TeamFile {
            name: "t".into(),
            description: None,
            created_at: 0,
            lead_agent_id: "team-lead@t".into(),
            lead_session_id: None,
            hidden_pane_ids: vec![],
            team_allowed_paths: vec![],
            members: vec![
                TeamMember {
                    agent_id: "team-lead@t".into(),
                    name: TEAM_LEAD_NAME.into(),
                    agent_type: None,
                    model: None,
                    prompt: None,
                    color: None,
                    plan_mode_required: None,
                    joined_at: 0,
                    tmux_pane_id: String::new(),
                    cwd: ".".into(),
                    worktree_path: None,
                    session_id: None,
                    subscriptions: vec![],
                    backend_type: None,
                    is_active: Some(true),
                    mode: None,
                },
                TeamMember {
                    agent_id: "r@t".into(),
                    name: "researcher".into(),
                    agent_type: None,
                    model: None,
                    prompt: None,
                    color: None,
                    plan_mode_required: None,
                    joined_at: 0,
                    tmux_pane_id: String::new(),
                    cwd: ".".into(),
                    worktree_path: None,
                    session_id: None,
                    subscriptions: vec![],
                    backend_type: None,
                    is_active: Some(true),
                    mode: None,
                },
                TeamMember {
                    agent_id: "w@t".into(),
                    name: "writer".into(),
                    agent_type: None,
                    model: None,
                    prompt: None,
                    color: None,
                    plan_mode_required: None,
                    joined_at: 0,
                    tmux_pane_id: String::new(),
                    cwd: ".".into(),
                    worktree_path: None,
                    session_id: None,
                    subscriptions: vec![],
                    backend_type: None,
                    is_active: Some(false),
                    mode: None,
                },
            ],
        };

        let active = get_active_members(&tf);
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].name, "researcher");
    }

    #[test]
    fn test_assign_color() {
        let tf = TeamFile {
            name: "t".into(),
            description: None,
            created_at: 0,
            lead_agent_id: "lead@t".into(),
            lead_session_id: None,
            hidden_pane_ids: vec![],
            team_allowed_paths: vec![],
            members: vec![TeamMember {
                agent_id: "a@t".into(),
                name: "a".into(),
                agent_type: None,
                model: None,
                prompt: None,
                color: Some("red".into()),
                plan_mode_required: None,
                joined_at: 0,
                tmux_pane_id: String::new(),
                cwd: ".".into(),
                worktree_path: None,
                session_id: None,
                subscriptions: vec![],
                backend_type: None,
                is_active: None,
                mode: None,
            }],
        };
        let color = assign_color(&tf);
        assert_ne!(color, "red");
        assert!(AGENT_COLORS.contains(&color.as_str()));
    }

    #[test]
    #[serial_test::serial]
    fn test_assign_color_for_teammate_uses_layout_cache() {
        layout_manager::clear_teammate_colors();
        let tf = TeamFile {
            name: "t".into(),
            description: None,
            created_at: 0,
            lead_agent_id: "lead@t".into(),
            lead_session_id: None,
            hidden_pane_ids: vec![],
            team_allowed_paths: vec![],
            members: vec![],
        };

        let color = assign_color_for_teammate(&tf, "worker@t");
        assert_eq!(
            layout_manager::get_teammate_color("worker@t"),
            Some(color.as_str())
        );
    }

    #[test]
    fn test_tmux_color_mapping() {
        assert_eq!(tmux_color("red"), "red");
        assert_eq!(tmux_color("purple"), "magenta");
        assert_eq!(tmux_color("orange"), "colour208");
        assert_eq!(tmux_color("unknown"), "default");
    }

    #[test]
    fn test_team_exists() {
        assert!(!team_exists("definitely-not-existing-team-xyz"));
    }

    #[test]
    fn test_cleanup_nonexistent() {
        // Should not error on nonexistent team
        let result = cleanup_team_directories("nonexistent-team-for-test");
        assert!(result.is_ok());
    }
}
