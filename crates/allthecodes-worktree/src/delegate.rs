use std::path::PathBuf;

use anyhow::{bail, Result};

use allthecodes_engine::worktree_hooks::{
    default_agent_worktree_path, validate_allowed_worktree_path,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DelegateWorktreePlan {
    pub slug: String,
    pub worktree_path: PathBuf,
    pub branch_name: String,
}

pub fn delegate_worktree_plan(
    child_session_id: &str,
    requested_worktree: Option<&str>,
) -> Result<DelegateWorktreePlan> {
    let slug = delegate_worktree_slug(child_session_id, requested_worktree)?;
    let worktree_path = default_agent_worktree_path(&slug);
    validate_allowed_worktree_path(&worktree_path)?;
    Ok(DelegateWorktreePlan {
        branch_name: format!("agent-worktree-{slug}"),
        worktree_path,
        slug,
    })
}

fn delegate_worktree_slug(
    child_session_id: &str,
    requested_worktree: Option<&str>,
) -> Result<String> {
    let requested = requested_worktree
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let slug = match requested {
        Some(value)
            if !matches!(
                value.to_ascii_lowercase().as_str(),
                "true" | "worktree" | "isolated"
            ) =>
        {
            value.to_string()
        }
        _ => child_session_id.chars().take(12).collect(),
    };
    validate_delegate_worktree_slug(&slug)?;
    Ok(slug)
}

fn validate_delegate_worktree_slug(slug: &str) -> Result<()> {
    if slug.trim().is_empty() {
        bail!("delegate worktree slug cannot be empty");
    }
    if slug.contains("..") || slug.contains('/') || slug.contains('\\') {
        bail!("delegate worktree slug cannot contain path separators or '..'");
    }
    if slug.len() > 64 {
        bail!("delegate worktree slug too long (max 64 chars)");
    }
    if !slug
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        bail!("delegate worktree slug may only contain ASCII letters, numbers, '-' and '_'");
    }
    Ok(())
}
