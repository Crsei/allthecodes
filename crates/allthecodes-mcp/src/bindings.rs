//! MCP binding persistence and scope helpers.

use std::path::{Path, PathBuf};

use allthecodes_config::settings::{
    project_settings_path, user_settings_path, write_settings_file,
};
use allthecodes_config::{paths, settings::RawSettings};
use allthecodes_types::mcp::{McpBinding, McpPermission, McpToolScope};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct SessionBindingFile {
    bindings: Vec<McpBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindingSelector {
    pub server_id: String,
    pub scope: McpToolScope,
    pub session_id: Option<String>,
    pub thread_id: Option<String>,
}

impl BindingSelector {
    pub fn new(
        server_id: impl Into<String>,
        scope: McpToolScope,
        session_id: Option<String>,
        thread_id: Option<String>,
    ) -> Self {
        Self {
            server_id: server_id.into(),
            scope,
            session_id,
            thread_id,
        }
    }
}

pub fn canonical_project_path(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

pub fn canonical_workspace_root(cwd: &Path) -> PathBuf {
    let start = canonical_project_path(cwd);
    for candidate in start.ancestors() {
        if candidate.join(".allthecodes").is_dir()
            || candidate.join(".git").is_dir()
            || candidate.join("AGENTS.md").is_file()
            || candidate.join("CLAUDE.md").is_file()
        {
            return candidate.to_path_buf();
        }
    }
    start
}

pub fn session_binding_path(session_id: &str) -> Result<PathBuf> {
    validate_non_empty("sessionId", session_id)?;
    Ok(paths::runs_dir(session_id).join("mcp-bindings.json"))
}

pub fn load_session_bindings(session_id: &str) -> Result<Vec<McpBinding>> {
    let path = session_binding_path(session_id)?;
    if !path
        .try_exists()
        .with_context(|| format!("failed to inspect {}", path.display()))?
    {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let value: serde_json::Value = serde_json::from_str(&content)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    let bindings = if value.is_array() {
        serde_json::from_value::<Vec<McpBinding>>(value)?
    } else {
        serde_json::from_value::<SessionBindingFile>(value)?.bindings
    };
    normalize_loaded_bindings(bindings, None)
}

pub fn save_session_bindings(session_id: &str, bindings: &[McpBinding]) -> Result<()> {
    let path = session_binding_path(session_id)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    for binding in bindings {
        binding.validate().map_err(|error| anyhow::anyhow!(error))?;
    }
    let payload = SessionBindingFile {
        bindings: bindings.to_vec(),
    };
    let json = serde_json::to_string_pretty(&payload)?;
    std::fs::write(&path, json).with_context(|| format!("failed to write {}", path.display()))
}

pub fn load_settings_bindings(
    path: &Path,
    workspace_root: Option<&Path>,
) -> Result<Vec<McpBinding>> {
    if !path
        .try_exists()
        .with_context(|| format!("failed to inspect {}", path.display()))?
    {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read settings {}", path.display()))?;
    let raw: RawSettings = serde_json::from_str(&content)
        .with_context(|| format!("failed to parse settings {}", path.display()))?;
    normalize_loaded_bindings(raw.mcp_bindings.unwrap_or_default(), workspace_root)
}

pub fn list_explicit_bindings(cwd: &Path, session_id: Option<&str>) -> Result<Vec<McpBinding>> {
    let workspace_root = canonical_workspace_root(cwd);
    let mut bindings = Vec::new();

    bindings.extend(load_settings_bindings(
        &user_settings_path(),
        Some(&workspace_root),
    )?);
    bindings.extend(load_settings_bindings(
        &project_settings_path(&workspace_root),
        Some(&workspace_root),
    )?);
    if let Some(session_id) = session_id {
        if !session_id.trim().is_empty() {
            bindings.extend(load_session_bindings(session_id)?);
        }
    }

    Ok(bindings
        .into_iter()
        .filter(|binding| binding_relevant_to_workspace(binding, &workspace_root))
        .collect())
}

pub fn upsert_binding(cwd: &Path, binding: McpBinding) -> Result<()> {
    let workspace_root = canonical_workspace_root(cwd);
    let binding = normalize_binding_for_write(binding, &workspace_root)?;
    match binding.scope {
        McpToolScope::Global | McpToolScope::Project => {
            let path = settings_path_for_scope(cwd, binding.scope);
            update_settings_bindings(&path, |bindings| upsert_in_vec(bindings, binding))
        }
        McpToolScope::Session | McpToolScope::Thread => {
            let session_id = binding
                .session_id
                .clone()
                .ok_or_else(|| anyhow::anyhow!("session-scoped binding requires sessionId"))?;
            let mut bindings = load_session_bindings(&session_id)?;
            upsert_in_vec(&mut bindings, binding);
            save_session_bindings(&session_id, &bindings)
        }
    }
}

pub fn remove_binding(cwd: &Path, selector: BindingSelector) -> Result<bool> {
    validate_non_empty("serverId", &selector.server_id)?;
    let workspace_root = canonical_workspace_root(cwd);
    match selector.scope {
        McpToolScope::Global | McpToolScope::Project => {
            let path = settings_path_for_scope(cwd, selector.scope);
            let mut removed = false;
            update_settings_bindings(&path, |bindings| {
                let before = bindings.len();
                bindings.retain(|binding| {
                    !selector_matches_binding(&selector, binding, &workspace_root)
                });
                removed = before != bindings.len();
            })?;
            Ok(removed)
        }
        McpToolScope::Session | McpToolScope::Thread => {
            let session_id = selector
                .session_id
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("sessionId is required"))?;
            validate_non_empty("sessionId", session_id)?;
            let mut bindings = load_session_bindings(session_id)?;
            let before = bindings.len();
            bindings
                .retain(|binding| !selector_matches_binding(&selector, binding, &workspace_root));
            let removed = before != bindings.len();
            save_session_bindings(session_id, &bindings)?;
            Ok(removed)
        }
    }
}

pub fn set_binding_permissions(
    cwd: &Path,
    selector: BindingSelector,
    permissions: Vec<McpPermission>,
) -> Result<bool> {
    validate_non_empty("serverId", &selector.server_id)?;
    let workspace_root = canonical_workspace_root(cwd);
    match selector.scope {
        McpToolScope::Global | McpToolScope::Project => {
            let path = settings_path_for_scope(cwd, selector.scope);
            let mut updated = false;
            update_settings_bindings(&path, |bindings| {
                for binding in &mut *bindings {
                    if selector_matches_binding(&selector, binding, &workspace_root) {
                        binding.permissions = permissions.clone();
                        updated = true;
                    }
                }
                if !updated {
                    bindings.push(binding_from_selector(
                        &selector,
                        &workspace_root,
                        permissions.clone(),
                    ));
                    updated = true;
                }
            })?;
            Ok(updated)
        }
        McpToolScope::Session | McpToolScope::Thread => {
            let session_id = selector
                .session_id
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("sessionId is required"))?;
            validate_non_empty("sessionId", session_id)?;
            let mut bindings = load_session_bindings(session_id)?;
            let mut updated = false;
            for binding in &mut bindings {
                if selector_matches_binding(&selector, binding, &workspace_root) {
                    binding.permissions = permissions.clone();
                    updated = true;
                }
            }
            if !updated {
                bindings.push(binding_from_selector(
                    &selector,
                    &workspace_root,
                    permissions.clone(),
                ));
                updated = true;
            }
            save_session_bindings(session_id, &bindings)?;
            Ok(updated)
        }
    }
}

pub fn merge_bindings(mut base: Vec<McpBinding>, over: Vec<McpBinding>) -> Vec<McpBinding> {
    for binding in over {
        upsert_in_vec(&mut base, binding);
    }
    base
}

pub fn normalize_binding_for_write(
    mut binding: McpBinding,
    workspace_root: &Path,
) -> Result<McpBinding> {
    binding.server_id = binding.server_id.trim().to_string();
    match binding.scope {
        McpToolScope::Global => {
            binding.project_path = None;
            binding.session_id = None;
            binding.thread_id = None;
        }
        McpToolScope::Project => {
            let path = binding
                .project_path
                .as_deref()
                .unwrap_or(workspace_root)
                .to_path_buf();
            binding.project_path = Some(canonical_project_path(&path));
            binding.session_id = None;
            binding.thread_id = None;
        }
        McpToolScope::Session => {
            let session_id = binding.session_id.clone().unwrap_or_default();
            validate_non_empty("sessionId", &session_id)?;
            binding.session_id = Some(session_id);
            binding.thread_id = None;
        }
        McpToolScope::Thread => {
            let session_id = binding.session_id.clone().unwrap_or_default();
            let thread_id = binding.thread_id.clone().unwrap_or_default();
            validate_non_empty("sessionId", &session_id)?;
            validate_non_empty("threadId", &thread_id)?;
            binding.session_id = Some(session_id);
            binding.thread_id = Some(thread_id);
        }
    }
    binding.validate().map_err(|error| anyhow::anyhow!(error))?;
    Ok(binding)
}

fn normalize_loaded_bindings(
    bindings: Vec<McpBinding>,
    workspace_root: Option<&Path>,
) -> Result<Vec<McpBinding>> {
    let mut out = Vec::new();
    for mut binding in bindings {
        if binding.scope == McpToolScope::Project {
            if binding.project_path.is_none() {
                if let Some(workspace_root) = workspace_root {
                    binding.project_path = Some(canonical_project_path(workspace_root));
                }
            } else if let Some(path) = binding.project_path.take() {
                binding.project_path = Some(canonical_project_path(&path));
            }
        }
        binding.validate().map_err(|error| anyhow::anyhow!(error))?;
        out.push(binding);
    }
    Ok(out)
}

fn update_settings_bindings<F>(path: &Path, update: F) -> Result<()>
where
    F: FnOnce(&mut Vec<McpBinding>),
{
    let mut raw = if path
        .try_exists()
        .with_context(|| format!("failed to inspect {}", path.display()))?
    {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read settings {}", path.display()))?;
        serde_json::from_str::<RawSettings>(&content)
            .with_context(|| format!("failed to parse settings {}", path.display()))?
    } else {
        RawSettings::default()
    };
    let mut bindings = raw.mcp_bindings.take().unwrap_or_default();
    update(&mut bindings);
    raw.mcp_bindings = if bindings.is_empty() {
        None
    } else {
        Some(bindings)
    };
    write_settings_file(path, &raw)
}

fn upsert_in_vec(bindings: &mut Vec<McpBinding>, binding: McpBinding) {
    let key = binding.identity_key();
    bindings.retain(|existing| existing.identity_key() != key);
    bindings.push(binding);
}

fn settings_path_for_scope(cwd: &Path, scope: McpToolScope) -> PathBuf {
    match scope {
        McpToolScope::Global => user_settings_path(),
        McpToolScope::Project => project_settings_path(&canonical_workspace_root(cwd)),
        McpToolScope::Session | McpToolScope::Thread => user_settings_path(),
    }
}

fn selector_matches_binding(
    selector: &BindingSelector,
    binding: &McpBinding,
    workspace_root: &Path,
) -> bool {
    if binding.server_id != selector.server_id || binding.scope != selector.scope {
        return false;
    }
    match binding.scope {
        McpToolScope::Global => true,
        McpToolScope::Project => binding
            .project_path
            .as_ref()
            .map(|path| canonical_project_path(path) == canonical_project_path(workspace_root))
            .unwrap_or(false),
        McpToolScope::Session => binding.session_id == selector.session_id,
        McpToolScope::Thread => {
            binding.session_id == selector.session_id && binding.thread_id == selector.thread_id
        }
    }
}

fn binding_from_selector(
    selector: &BindingSelector,
    workspace_root: &Path,
    permissions: Vec<McpPermission>,
) -> McpBinding {
    McpBinding {
        server_id: selector.server_id.clone(),
        scope: selector.scope,
        project_path: (selector.scope == McpToolScope::Project)
            .then(|| canonical_project_path(workspace_root)),
        session_id: selector.session_id.clone(),
        thread_id: selector.thread_id.clone(),
        permissions,
        ..Default::default()
    }
}

fn binding_relevant_to_workspace(binding: &McpBinding, workspace_root: &Path) -> bool {
    match binding.scope {
        McpToolScope::Project => binding
            .project_path
            .as_ref()
            .map(|path| canonical_project_path(path) == canonical_project_path(workspace_root))
            .unwrap_or(false),
        _ => true,
    }
}

fn validate_non_empty(field: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("{field} cannot be empty");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
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

    #[test]
    #[serial]
    fn session_bindings_round_trip_under_allthecodes_home() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path().to_str().unwrap());
        let binding = McpBinding {
            server_id: "github".to_string(),
            scope: McpToolScope::Session,
            session_id: Some("s1".to_string()),
            permissions: vec![McpPermission::Connect],
            ..Default::default()
        };

        save_session_bindings("s1", std::slice::from_ref(&binding)).unwrap();
        let path = session_binding_path("s1").unwrap();
        assert_eq!(
            path,
            home.path()
                .join("runs")
                .join("s1")
                .join("mcp-bindings.json")
        );
        assert!(!path.to_string_lossy().contains(".Codex"));
        assert_eq!(load_session_bindings("s1").unwrap(), vec![binding]);
    }

    #[test]
    fn empty_session_and_thread_ids_are_rejected() {
        assert!(session_binding_path("").is_err());
        let err = normalize_binding_for_write(
            McpBinding {
                server_id: "github".to_string(),
                scope: McpToolScope::Thread,
                session_id: Some("s1".to_string()),
                thread_id: Some(String::new()),
                ..Default::default()
            },
            Path::new("/tmp/project"),
        )
        .unwrap_err();
        assert!(err.to_string().contains("threadId cannot be empty"));
    }

    #[test]
    #[serial]
    fn project_bindings_match_canonical_workspace_root() {
        let home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let nested = project.path().join("src").join("module");
        std::fs::create_dir_all(project.path().join(".allthecodes")).unwrap();
        std::fs::create_dir_all(&nested).unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path().to_str().unwrap());

        upsert_binding(
            &nested,
            McpBinding {
                server_id: "github".to_string(),
                scope: McpToolScope::Project,
                permissions: vec![McpPermission::Connect],
                ..Default::default()
            },
        )
        .unwrap();

        let bindings = list_explicit_bindings(&nested, None).unwrap();
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].project_path.as_deref(), Some(project.path()));

        let other = tempfile::tempdir().unwrap();
        assert!(list_explicit_bindings(other.path(), None)
            .unwrap()
            .is_empty());
    }
}
