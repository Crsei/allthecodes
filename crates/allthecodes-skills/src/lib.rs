//! Skill system: discovery, package validation, dependency resolution, and
//! registry management for bundled, user, project, plugin, and MCP skills.
//!
//! Corresponds to TypeScript: `src/skills/`.

pub mod bundled;
pub mod invocation;
pub mod loader;
pub mod usage;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering as CmpOrdering;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::LazyLock;

/// Version used for skills that do not declare an explicit package version.
///
/// Missing versions are allowed for compatibility with existing skill files,
/// but dependency and conflict resolution still need a stable value.
pub const DEFAULT_SKILL_VERSION: &str = "0.0.0";

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

/// Source of a skill definition.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SkillSource {
    Bundled,
    User,
    Project,
    Plugin(String),
    Mcp(String),
}

/// Execution context for a skill.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SkillContext {
    /// Expand the skill prompt inline in the current conversation.
    #[default]
    Inline,
    /// Run the skill in a forked sub-agent with isolated context.
    Fork,
}

/// A package dependency declared by a skill.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillDependency {
    pub name: String,
    /// Simple version requirement, for example `1.2.0`, `>=1.0.0`, or `<2.0`.
    pub version: Option<String>,
}

impl SkillDependency {
    pub fn new(name: impl Into<String>, version: Option<String>) -> Self {
        Self {
            name: name.into(),
            version,
        }
    }

    pub fn label(&self) -> String {
        match &self.version {
            Some(req) if !req.trim().is_empty() => format!("{} {}", self.name, req),
            _ => self.name.clone(),
        }
    }
}

/// Parsed frontmatter fields from a `SKILL.md` file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkillFrontmatter {
    /// Display name or canonical override, depending on loader source.
    pub name: Option<String>,
    /// Human-readable description.
    pub description: String,
    /// When the model should use this skill.
    pub when_to_use: Option<String>,
    /// Allowed tool names (empty = all tools allowed).
    pub allowed_tools: Vec<String>,
    /// Hint text for argument input.
    pub argument_hint: Option<String>,
    /// Named argument placeholders.
    pub argument_names: Vec<String>,
    /// Model override.
    pub model: Option<String>,
    /// Whether the user can invoke this skill via `/skill-name`.
    pub user_invocable: bool,
    /// Whether the model can invoke this skill autonomously.
    pub disable_model_invocation: bool,
    /// Execution context.
    pub context: SkillContext,
    /// Agent type for forked execution.
    pub agent: Option<String>,
    /// Effort level override.
    pub effort: Option<String>,
    /// Package version string.
    pub version: Option<String>,
    /// Compatible allthecodes app version requirement.
    pub compatible_app_version: Option<String>,
    /// Package dependencies.
    pub dependencies: Vec<SkillDependency>,
    /// Path glob patterns: skill is only visible when matching files are touched.
    pub paths: Vec<String>,
    /// Relative files or directories the package expects to ship with.
    pub assets: Vec<String>,
    /// Relative markdown/reference docs that are part of the package entry set.
    pub entry_docs: Vec<String>,
}

/// A fully loaded skill definition.
#[derive(Debug, Clone)]
pub struct SkillDefinition {
    /// Canonical skill name, for example `commit` or `mcp__server__review`.
    pub name: String,
    /// Where this skill was loaded from.
    pub source: SkillSource,
    /// Directory containing `SKILL.md`, used for variable substitution and
    /// package-layout validation.
    pub base_dir: Option<PathBuf>,
    /// Parsed frontmatter.
    pub frontmatter: SkillFrontmatter,
    /// Raw markdown prompt body after frontmatter.
    pub prompt_body: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillProposalAction {
    Create,
    Patch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillProposalScope {
    User,
    Project,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillProposalDraft {
    pub action: SkillProposalAction,
    pub scope: SkillProposalScope,
    pub skill_name: String,
    pub source_session_id: Option<String>,
    pub markdown: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillProposal {
    pub id: String,
    pub action: SkillProposalAction,
    pub scope: SkillProposalScope,
    pub skill_name: String,
    pub source_session_id: Option<String>,
    pub proposed_path: PathBuf,
    pub markdown: String,
    pub created_at: String,
}

impl SkillDefinition {
    /// Whether this skill is user-invocable (can be called via `/name`).
    pub fn is_user_invocable(&self) -> bool {
        self.frontmatter.user_invocable
    }

    /// Whether this skill is model-invocable (the model can call it).
    pub fn is_model_invocable(&self) -> bool {
        !self.frontmatter.disable_model_invocation
            && (!self.frontmatter.description.is_empty() || self.frontmatter.when_to_use.is_some())
    }

    /// Get the display name (frontmatter name or canonical name).
    pub fn display_name(&self) -> &str {
        self.frontmatter.name.as_deref().unwrap_or(&self.name)
    }

    /// Version used by package resolution.
    pub fn effective_version(&self) -> &str {
        self.frontmatter
            .version
            .as_deref()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or(DEFAULT_SKILL_VERSION)
    }

    /// Expand the prompt body with argument substitution.
    pub fn expand_prompt(&self, args: &str, session_id: Option<&str>) -> String {
        let mut body = self.prompt_body.clone();

        if let Some(dir) = &self.base_dir {
            let dir_str = dir.to_string_lossy().replace('\\', "/");
            body = body.replace("${CLAUDE_SKILL_DIR}", &dir_str);
        }

        if let Some(sid) = session_id {
            body = body.replace("${CLAUDE_SESSION_ID}", sid);
        }

        if !args.is_empty() {
            body = body.replace("$ARGUMENTS", args);
            let arg_parts: Vec<&str> = args
                .splitn(self.frontmatter.argument_names.len().max(1), ' ')
                .collect();
            for (i, name) in self.frontmatter.argument_names.iter().enumerate() {
                let val = arg_parts.get(i).copied().unwrap_or("");
                body = body.replace(&format!("${{{}}}", name), val);
            }
        }

        body
    }
}

// ---------------------------------------------------------------------------
// Diagnostics and load reports
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SkillDiagnosticSeverity {
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillDiagnostic {
    pub severity: SkillDiagnosticSeverity,
    pub code: String,
    pub message: String,
    pub skill: Option<String>,
    pub source: Option<SkillSource>,
    pub path: Option<PathBuf>,
}

impl SkillDiagnostic {
    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity: SkillDiagnosticSeverity::Error,
            code: code.into(),
            message: message.into(),
            skill: None,
            source: None,
            path: None,
        }
    }

    pub fn warning(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity: SkillDiagnosticSeverity::Warning,
            code: code.into(),
            message: message.into(),
            skill: None,
            source: None,
            path: None,
        }
    }

    pub fn with_skill(mut self, skill: impl Into<String>) -> Self {
        self.skill = Some(skill.into());
        self
    }

    pub fn with_source(mut self, source: SkillSource) -> Self {
        self.source = Some(source);
        self
    }

    pub fn with_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.path = Some(path.into());
        self
    }

    pub fn is_error(&self) -> bool {
        self.severity == SkillDiagnosticSeverity::Error
    }
}

#[derive(Debug, Clone)]
pub struct SkillLoadOptions {
    pub app_version: String,
}

impl Default for SkillLoadOptions {
    fn default() -> Self {
        Self {
            app_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

impl SkillLoadOptions {
    pub fn for_app_version(version: impl Into<String>) -> Self {
        Self {
            app_version: version.into(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SkillLoadReport {
    pub loaded: usize,
    pub skipped: usize,
    pub diagnostics: Vec<SkillDiagnostic>,
    pub revision: u64,
}

impl SkillLoadReport {
    pub fn error_count(&self) -> usize {
        self.diagnostics.iter().filter(|d| d.is_error()).count()
    }

    pub fn warning_count(&self) -> usize {
        self.diagnostics.len().saturating_sub(self.error_count())
    }
}

// ---------------------------------------------------------------------------
// Subsystem event emission
// ---------------------------------------------------------------------------

/// Minimal event set emitted by the skill subsystem. The host adapts these
/// into its own subsystem-event wrapper.
#[derive(Debug, Clone)]
pub enum SkillSubsystemEvent {
    /// Skills were loaded / reloaded.
    SkillsLoaded { count: usize },
}

type EventCallback = Box<dyn Fn(SkillSubsystemEvent) + Send + Sync>;

static EVENT_CALLBACK: LazyLock<Mutex<Option<EventCallback>>> = LazyLock::new(|| Mutex::new(None));

/// Register the host's event adapter. Replaces any previous callback.
pub fn set_event_callback<F>(cb: F)
where
    F: Fn(SkillSubsystemEvent) + Send + Sync + 'static,
{
    *EVENT_CALLBACK.lock() = Some(Box::new(cb));
}

fn emit_event(event: SkillSubsystemEvent) {
    if let Some(cb) = EVENT_CALLBACK.lock().as_ref() {
        cb(event);
    }
}

// ---------------------------------------------------------------------------
// Global skill registry
// ---------------------------------------------------------------------------

static REGISTRY: LazyLock<Mutex<Vec<SkillDefinition>>> = LazyLock::new(|| Mutex::new(Vec::new()));
static REGISTRY_DIAGNOSTICS: LazyLock<Mutex<Vec<SkillDiagnostic>>> =
    LazyLock::new(|| Mutex::new(Vec::new()));
static REGISTRY_REVISION: AtomicU64 = AtomicU64::new(0);

/// Register a skill in the global registry.
///
/// This compatibility helper keeps first-registration-wins behavior for tests
/// and legacy call sites. Startup and reload paths use the validating package
/// resolver instead.
pub fn register_skill(skill: SkillDefinition) {
    let mut reg = REGISTRY.lock();
    if !reg.iter().any(|s| s.name == skill.name) {
        reg.push(skill);
        let revision = REGISTRY_REVISION.fetch_add(1, Ordering::SeqCst) + 1;
        emit_event(SkillSubsystemEvent::SkillsLoaded { count: reg.len() });
        let mut diagnostics = REGISTRY_DIAGNOSTICS.lock();
        diagnostics.retain(|d| d.code != "registry-revision");
        diagnostics.push(
            SkillDiagnostic::warning(
                "registry-revision",
                format!("Registry changed outside package reload at revision {revision}."),
            )
            .with_skill("registry"),
        );
    }
}

/// Merge additional skills into the current registry through dependency and
/// version resolution.
pub fn register_skills_resolved(
    skills: Vec<SkillDefinition>,
    options: SkillLoadOptions,
) -> SkillLoadReport {
    register_skills_resolved_with_diagnostics(skills, Vec::new(), options)
}

/// Merge additional skills and pre-existing diagnostics into the current
/// registry through dependency and version resolution.
pub fn register_skills_resolved_with_diagnostics(
    skills: Vec<SkillDefinition>,
    diagnostics: Vec<SkillDiagnostic>,
    options: SkillLoadOptions,
) -> SkillLoadReport {
    let mut candidates = get_all_skills();
    let candidate_total = candidates.len() + skills.len();
    candidates.extend(skills);
    replace_with_resolved(candidates, diagnostics, candidate_total, options)
}

/// Remove MCP-provided skills owned by one server from the global registry.
pub fn clear_mcp_skills_for_server(server: &str) -> usize {
    let removed = {
        let mut reg = REGISTRY.lock();
        let before = reg.len();
        reg.retain(|skill| !skill_from_mcp_server(skill, server));
        before.saturating_sub(reg.len())
    };
    if removed > 0 {
        REGISTRY_DIAGNOSTICS
            .lock()
            .retain(|diagnostic| !diagnostic_from_mcp_server(diagnostic, server));
        let revision = REGISTRY_REVISION.fetch_add(1, Ordering::SeqCst) + 1;
        let count = REGISTRY.lock().len();
        emit_event(SkillSubsystemEvent::SkillsLoaded { count });
        let mut diagnostics = REGISTRY_DIAGNOSTICS.lock();
        diagnostics.retain(|d| d.code != "registry-revision");
        diagnostics.push(
            SkillDiagnostic::warning(
                "registry-revision",
                format!("Registry changed outside package reload at revision {revision}."),
            )
            .with_skill("registry"),
        );
    }
    removed
}

/// Replace MCP-provided skills for one server, dropping stale entries first.
pub fn replace_mcp_skills_for_server(
    server: &str,
    skills: Vec<SkillDefinition>,
    diagnostics: Vec<SkillDiagnostic>,
    options: SkillLoadOptions,
) -> SkillLoadReport {
    let mut candidates = get_all_skills()
        .into_iter()
        .filter(|skill| !skill_from_mcp_server(skill, server))
        .collect::<Vec<_>>();
    let candidate_total = candidates.len() + skills.len();
    candidates.extend(skills);

    let mut merged_diagnostics = get_skill_diagnostics()
        .into_iter()
        .filter(|diagnostic| !diagnostic_from_mcp_server(diagnostic, server))
        .collect::<Vec<_>>();
    merged_diagnostics.extend(diagnostics);

    replace_with_resolved(candidates, merged_diagnostics, candidate_total, options)
}

fn skill_from_mcp_server(skill: &SkillDefinition, server: &str) -> bool {
    matches!(&skill.source, SkillSource::Mcp(source_server) if source_server == server)
}

fn diagnostic_from_mcp_server(diagnostic: &SkillDiagnostic, server: &str) -> bool {
    matches!(&diagnostic.source, Some(SkillSource::Mcp(source_server)) if source_server == server)
}

/// Get all registered skills.
pub fn get_all_skills() -> Vec<SkillDefinition> {
    REGISTRY.lock().clone()
}

/// Find a skill by name.
pub fn find_skill(name: &str) -> Option<SkillDefinition> {
    REGISTRY.lock().iter().find(|s| s.name == name).cloned()
}

/// Get user-invocable skills (for slash command listing).
pub fn get_user_invocable_skills() -> Vec<SkillDefinition> {
    get_all_skills()
        .into_iter()
        .filter(|s| s.is_user_invocable())
        .collect()
}

/// Get model-invocable skills (for SkillTool prompt).
pub fn get_model_invocable_skills() -> Vec<SkillDefinition> {
    get_all_skills()
        .into_iter()
        .filter(|s| s.is_model_invocable())
        .collect()
}

/// Last successful registry revision.
pub fn registry_revision() -> u64 {
    REGISTRY_REVISION.load(Ordering::SeqCst)
}

/// Diagnostics from the latest validating load or reload.
pub fn get_skill_diagnostics() -> Vec<SkillDiagnostic> {
    REGISTRY_DIAGNOSTICS.lock().clone()
}

/// Clear all skills and diagnostics.
pub fn clear_skills() {
    REGISTRY.lock().clear();
    REGISTRY_DIAGNOSTICS.lock().clear();
    REGISTRY_REVISION.fetch_add(1, Ordering::SeqCst);
}

type SkillProposalResult<T> =
    std::result::Result<T, Box<dyn std::error::Error + Send + Sync + 'static>>;

pub fn skill_proposals_dir(scope: SkillProposalScope, cwd: &Path) -> PathBuf {
    match scope {
        SkillProposalScope::User => allthecodes_config::paths::data_root().join("skill_proposals"),
        SkillProposalScope::Project => {
            allthecodes_config::paths::project_allthecodes_dir(cwd).join("skill_proposals")
        }
    }
}

pub fn proposal_storage_path(id: &str, scope: SkillProposalScope, cwd: &Path) -> PathBuf {
    skill_proposals_dir(scope, cwd).join(format!("{}.json", sanitize_proposal_id(id)))
}

pub fn stage_skill_proposal(
    draft: SkillProposalDraft,
    cwd: &Path,
) -> SkillProposalResult<SkillProposal> {
    let skill_name = draft.skill_name.trim();
    if !is_valid_skill_name(skill_name) {
        return Err(format!("invalid skill name '{}'", draft.skill_name).into());
    }
    if draft.markdown.trim().is_empty() {
        return Err("proposal markdown cannot be empty".into());
    }

    let id = format!("skill-proposal-{}", uuid::Uuid::new_v4());
    let proposed_path = proposed_skill_path(draft.scope, cwd, skill_name);
    let proposal = SkillProposal {
        id: id.clone(),
        action: draft.action,
        scope: draft.scope,
        skill_name: skill_name.to_string(),
        source_session_id: draft.source_session_id,
        proposed_path,
        markdown: draft.markdown,
        created_at: chrono::Utc::now().to_rfc3339(),
    };

    let path = proposal_storage_path(&id, draft.scope, cwd);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_vec_pretty(&proposal)?)?;
    Ok(proposal)
}

pub fn list_skill_proposals(cwd: &Path) -> SkillProposalResult<Vec<SkillProposal>> {
    let mut proposals = Vec::new();
    for scope in [SkillProposalScope::Project, SkillProposalScope::User] {
        let dir = skill_proposals_dir(scope, cwd);
        if !dir.exists() {
            continue;
        }
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            if let Ok(raw) = std::fs::read_to_string(&path) {
                if let Ok(proposal) = serde_json::from_str::<SkillProposal>(&raw) {
                    proposals.push(proposal);
                }
            }
        }
    }
    proposals.sort_by(|a, b| {
        b.created_at
            .cmp(&a.created_at)
            .then_with(|| a.skill_name.cmp(&b.skill_name))
            .then_with(|| a.id.cmp(&b.id))
    });
    Ok(proposals)
}

pub fn load_skill_proposal(id: &str, cwd: &Path) -> SkillProposalResult<SkillProposal> {
    for scope in [SkillProposalScope::Project, SkillProposalScope::User] {
        let path = proposal_storage_path(id, scope, cwd);
        if path.exists() {
            let raw = std::fs::read_to_string(path)?;
            return Ok(serde_json::from_str(&raw)?);
        }
    }
    Err(format!("skill proposal '{}' not found", id).into())
}

pub fn approve_skill_proposal(id: &str, cwd: &Path) -> SkillProposalResult<PathBuf> {
    let proposal = load_skill_proposal(id, cwd)?;
    let allowed_root = skills_root_for_scope(proposal.scope, cwd);
    if !proposal.proposed_path.starts_with(&allowed_root) {
        return Err(format!(
            "proposal '{}' target escapes skills directory {}",
            proposal.id,
            allowed_root.display()
        )
        .into());
    }

    if let Some(parent) = proposal.proposed_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&proposal.proposed_path, proposal.markdown.as_bytes())?;
    let storage = proposal_storage_path(&proposal.id, proposal.scope, cwd);
    if storage.exists() {
        std::fs::remove_file(storage)?;
    }
    Ok(proposal.proposed_path)
}

pub fn reject_skill_proposal(id: &str, cwd: &Path) -> SkillProposalResult<SkillProposal> {
    let proposal = load_skill_proposal(id, cwd)?;
    let storage = proposal_storage_path(&proposal.id, proposal.scope, cwd);
    if storage.exists() {
        std::fs::remove_file(storage)?;
    }
    Ok(proposal)
}

fn proposed_skill_path(scope: SkillProposalScope, cwd: &Path, skill_name: &str) -> PathBuf {
    skills_root_for_scope(scope, cwd)
        .join(skill_name)
        .join("SKILL.md")
}

fn skills_root_for_scope(scope: SkillProposalScope, cwd: &Path) -> PathBuf {
    match scope {
        SkillProposalScope::User => allthecodes_config::paths::skills_dir_global(),
        SkillProposalScope::Project => allthecodes_config::paths::project_skills_dir(cwd),
    }
}

fn sanitize_proposal_id(id: &str) -> String {
    id.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Global skill usage tracker
// ---------------------------------------------------------------------------

/// Global runtime usage tracker for skills.
///
/// Tracks invocation counts and rolling usage scores with decay across
/// process lifetime. Persistence is the caller's responsibility.
pub static SKILL_USAGE: LazyLock<Mutex<usage::SkillUsageTracker>> =
    LazyLock::new(|| Mutex::new(usage::SkillUsageTracker::new()));

/// Record a skill invocation in the global usage tracker.
///
/// Debounced: repeated calls within 60 seconds for the same skill are ignored.
pub fn record_skill_usage(name: &str) {
    let mut guard = SKILL_USAGE.lock();
    guard.record_invocation(name);
}

/// Get the current usage score for a skill from the global tracker.
pub fn skill_usage_score(name: &str) -> f64 {
    let guard = SKILL_USAGE.lock();
    guard.usage_score(name)
}

/// Get all ranked skill usage data from the global tracker.
pub fn ranked_skill_usage() -> Vec<usage::SkillUsageData> {
    let guard = SKILL_USAGE.lock();
    guard.ranked_skills()
}

/// Persist the global skill usage tracker to a JSON file.
pub fn save_skill_usage(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let guard = SKILL_USAGE.lock();
    guard.save(path)
}

/// Load skill usage data from a JSON file into the global tracker.
pub fn load_skill_usage(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let loaded = usage::SkillUsageTracker::load(path)?;
    let mut guard = SKILL_USAGE.lock();
    *guard = loaded;
    Ok(())
}

/// Initialize the skill system: bundled + user + project skills.
pub fn init_skills(user_skills_dir: &Path, project_dir: Option<&Path>) {
    let _ = reload_skills_with_extra(
        user_skills_dir,
        project_dir,
        Vec::new(),
        SkillLoadOptions::default(),
    );
}

/// Initialize the skill system with caller-provided app-version metadata.
pub fn init_skills_with_options(
    user_skills_dir: &Path,
    project_dir: Option<&Path>,
    options: SkillLoadOptions,
) -> SkillLoadReport {
    reload_skills_with_extra(user_skills_dir, project_dir, Vec::new(), options)
}

/// Reload bundled, user, project, and caller-provided skills.
pub fn reload_skills_with_extra(
    user_skills_dir: &Path,
    project_dir: Option<&Path>,
    extra_skills: Vec<SkillDefinition>,
    options: SkillLoadOptions,
) -> SkillLoadReport {
    let mut candidates = bundled::bundled_skills();
    let mut diagnostics = Vec::new();
    let mut candidate_total = candidates.len();

    if user_skills_dir.is_dir() {
        let batch =
            loader::load_skills_from_dir_with_diagnostics(user_skills_dir, SkillSource::User);
        candidate_total += batch.skills.len() + batch.skipped;
        diagnostics.extend(batch.diagnostics);
        candidates.extend(batch.skills);
    }

    if let Some(proj) = project_dir {
        let project_skills_dir = allthecodes_config::paths::project_skills_dir(proj);
        if project_skills_dir.is_dir() {
            let batch = loader::load_skills_from_dir_with_diagnostics(
                &project_skills_dir,
                SkillSource::Project,
            );
            candidate_total += batch.skills.len() + batch.skipped;
            diagnostics.extend(batch.diagnostics);
            candidates.extend(batch.skills);
        }
    }

    candidate_total += extra_skills.len();
    candidates.extend(extra_skills);

    replace_with_resolved(candidates, diagnostics, candidate_total, options)
}

fn replace_with_resolved(
    candidates: Vec<SkillDefinition>,
    mut diagnostics: Vec<SkillDiagnostic>,
    candidate_total: usize,
    options: SkillLoadOptions,
) -> SkillLoadReport {
    let resolved = resolve_skill_packages(candidates, &mut diagnostics, &options);
    let loaded = resolved.len();
    let skipped = candidate_total.saturating_sub(loaded);

    {
        let mut reg = REGISTRY.lock();
        *reg = resolved;
    }
    {
        let mut stored = REGISTRY_DIAGNOSTICS.lock();
        *stored = diagnostics.clone();
    }

    let revision = REGISTRY_REVISION.fetch_add(1, Ordering::SeqCst) + 1;
    emit_event(SkillSubsystemEvent::SkillsLoaded { count: loaded });

    SkillLoadReport {
        loaded,
        skipped,
        diagnostics,
        revision,
    }
}

// ---------------------------------------------------------------------------
// Package validation and dependency resolution
// ---------------------------------------------------------------------------

fn resolve_skill_packages(
    candidates: Vec<SkillDefinition>,
    diagnostics: &mut Vec<SkillDiagnostic>,
    options: &SkillLoadOptions,
) -> Vec<SkillDefinition> {
    let mut accepted: Vec<SkillDefinition> = Vec::new();
    let mut by_name: HashMap<String, usize> = HashMap::new();

    for skill in candidates {
        if !validate_skill_package(&skill, diagnostics, options) {
            continue;
        }

        if let Some(existing_idx) = by_name.get(&skill.name).copied() {
            let existing = &accepted[existing_idx];
            if existing.effective_version() != skill.effective_version() {
                diagnostics.push(
                    SkillDiagnostic::error(
                        "version-conflict",
                        format!(
                            "Skill '{}' is already loaded at version {}, cannot also load version {}.",
                            skill.name,
                            existing.effective_version(),
                            skill.effective_version()
                        ),
                    )
                    .with_skill(skill.name.clone())
                    .with_source(skill.source.clone()),
                );
            } else {
                diagnostics.push(
                    SkillDiagnostic::warning(
                        "duplicate-skill",
                        format!(
                            "Duplicate skill '{}' version {} skipped; first source wins.",
                            skill.name,
                            skill.effective_version()
                        ),
                    )
                    .with_skill(skill.name.clone())
                    .with_source(skill.source.clone()),
                );
            }
            continue;
        }

        by_name.insert(skill.name.clone(), accepted.len());
        accepted.push(skill);
    }

    let invalid = dependency_invalid_names(&accepted, diagnostics);
    let mut filtered = Vec::new();
    for skill in accepted {
        if !invalid.contains(&skill.name) {
            filtered.push(skill);
        }
    }

    topo_sort_skills(filtered)
}

fn validate_skill_package(
    skill: &SkillDefinition,
    diagnostics: &mut Vec<SkillDiagnostic>,
    options: &SkillLoadOptions,
) -> bool {
    let mut valid = true;
    let source = skill.source.clone();

    if !is_valid_skill_name(&skill.name) {
        diagnostics.push(
            SkillDiagnostic::error(
                "invalid-name",
                format!(
                    "Skill name '{}' is invalid. Use letters, numbers, '.', ':', '_' or '-'.",
                    skill.name
                ),
            )
            .with_skill(skill.name.clone())
            .with_source(source.clone()),
        );
        valid = false;
    }

    if skill.frontmatter.description.trim().is_empty() {
        diagnostics.push(
            SkillDiagnostic::error("missing-description", "Skill description cannot be empty.")
                .with_skill(skill.name.clone())
                .with_source(source.clone()),
        );
        valid = false;
    }

    if let Some(version) = &skill.frontmatter.version {
        if parse_version(version).is_none() {
            diagnostics.push(
                SkillDiagnostic::error(
                    "invalid-version",
                    format!(
                        "Skill '{}' declares invalid version '{}'. Use numeric semantic form like 1.2.3.",
                        skill.name, version
                    ),
                )
                .with_skill(skill.name.clone())
                .with_source(source.clone()),
            );
            valid = false;
        }
    }

    if let Some(req) = &skill.frontmatter.compatible_app_version {
        match version_req_satisfied(&options.app_version, req) {
            Ok(true) => {}
            Ok(false) => {
                diagnostics.push(
                    SkillDiagnostic::error(
                        "incompatible-app-version",
                        format!(
                            "Skill '{}' requires app version '{}', current app version is {}.",
                            skill.name, req, options.app_version
                        ),
                    )
                    .with_skill(skill.name.clone())
                    .with_source(source.clone()),
                );
                valid = false;
            }
            Err(message) => {
                diagnostics.push(
                    SkillDiagnostic::error("invalid-app-version-requirement", message)
                        .with_skill(skill.name.clone())
                        .with_source(source.clone()),
                );
                valid = false;
            }
        }
    }

    for dep in &skill.frontmatter.dependencies {
        if !is_valid_skill_name(&dep.name) {
            diagnostics.push(
                SkillDiagnostic::error(
                    "invalid-dependency",
                    format!(
                        "Skill '{}' declares invalid dependency '{}'.",
                        skill.name, dep.name
                    ),
                )
                .with_skill(skill.name.clone())
                .with_source(source.clone()),
            );
            valid = false;
        }
        if let Some(req) = &dep.version {
            if version_req_satisfied(DEFAULT_SKILL_VERSION, req).is_err() {
                diagnostics.push(
                    SkillDiagnostic::error(
                        "invalid-dependency-requirement",
                        format!(
                            "Skill '{}' dependency '{}' has invalid version requirement '{}'.",
                            skill.name, dep.name, req
                        ),
                    )
                    .with_skill(skill.name.clone())
                    .with_source(source.clone()),
                );
                valid = false;
            }
        }
    }

    for rel in skill
        .frontmatter
        .assets
        .iter()
        .chain(skill.frontmatter.entry_docs.iter())
    {
        if !is_safe_relative_path(rel) {
            diagnostics.push(
                SkillDiagnostic::error(
                    "invalid-package-path",
                    format!(
                        "Skill '{}' package path '{}' escapes the skill directory.",
                        skill.name, rel
                    ),
                )
                .with_skill(skill.name.clone())
                .with_source(source.clone()),
            );
            valid = false;
            continue;
        }

        if let Some(base_dir) = &skill.base_dir {
            let full = base_dir.join(rel);
            if !full.exists() {
                diagnostics.push(
                    SkillDiagnostic::error(
                        "missing-package-path",
                        format!(
                            "Skill '{}' declares package path '{}' but it does not exist.",
                            skill.name, rel
                        ),
                    )
                    .with_skill(skill.name.clone())
                    .with_source(source.clone())
                    .with_path(full),
                );
                valid = false;
            }
        }
    }

    valid
}

fn dependency_invalid_names(
    skills: &[SkillDefinition],
    diagnostics: &mut Vec<SkillDiagnostic>,
) -> HashSet<String> {
    let by_name: HashMap<&str, &SkillDefinition> =
        skills.iter().map(|s| (s.name.as_str(), s)).collect();
    let mut invalid = HashSet::new();

    for skill in skills {
        for dep in &skill.frontmatter.dependencies {
            let Some(provider) = by_name.get(dep.name.as_str()) else {
                diagnostics.push(
                    SkillDiagnostic::error(
                        "missing-dependency",
                        format!(
                            "Skill '{}' depends on missing skill '{}'.",
                            skill.name, dep.name
                        ),
                    )
                    .with_skill(skill.name.clone())
                    .with_source(skill.source.clone()),
                );
                invalid.insert(skill.name.clone());
                continue;
            };

            if let Some(req) = &dep.version {
                match version_req_satisfied(provider.effective_version(), req) {
                    Ok(true) => {}
                    Ok(false) => {
                        diagnostics.push(
                            SkillDiagnostic::error(
                                "dependency-version-mismatch",
                                format!(
                                    "Skill '{}' requires dependency '{}' {}, but loaded version is {}.",
                                    skill.name,
                                    dep.name,
                                    req,
                                    provider.effective_version()
                                ),
                            )
                            .with_skill(skill.name.clone())
                            .with_source(skill.source.clone()),
                        );
                        invalid.insert(skill.name.clone());
                    }
                    Err(message) => {
                        diagnostics.push(
                            SkillDiagnostic::error("invalid-dependency-requirement", message)
                                .with_skill(skill.name.clone())
                                .with_source(skill.source.clone()),
                        );
                        invalid.insert(skill.name.clone());
                    }
                }
            }
        }
    }

    detect_dependency_cycles(skills, diagnostics, &mut invalid);
    invalid
}

fn detect_dependency_cycles(
    skills: &[SkillDefinition],
    diagnostics: &mut Vec<SkillDiagnostic>,
    invalid: &mut HashSet<String>,
) {
    let deps_by_name: HashMap<&str, Vec<&str>> = skills
        .iter()
        .map(|s| {
            (
                s.name.as_str(),
                s.frontmatter
                    .dependencies
                    .iter()
                    .map(|d| d.name.as_str())
                    .collect(),
            )
        })
        .collect();
    let mut state: HashMap<&str, u8> = HashMap::new();
    let mut stack = Vec::new();
    let mut reported = HashSet::new();

    for skill in skills {
        visit_cycle(
            skill.name.as_str(),
            &deps_by_name,
            &mut state,
            &mut stack,
            diagnostics,
            invalid,
            &mut reported,
        );
    }
}

fn visit_cycle<'a>(
    name: &'a str,
    deps_by_name: &HashMap<&'a str, Vec<&'a str>>,
    state: &mut HashMap<&'a str, u8>,
    stack: &mut Vec<&'a str>,
    diagnostics: &mut Vec<SkillDiagnostic>,
    invalid: &mut HashSet<String>,
    reported: &mut HashSet<String>,
) {
    match state.get(name).copied() {
        Some(2) => return,
        Some(1) => {
            if let Some(pos) = stack.iter().position(|n| *n == name) {
                let cycle = stack[pos..].to_vec();
                let key = cycle.join(" -> ");
                if reported.insert(key) {
                    for item in &cycle {
                        invalid.insert((*item).to_string());
                    }
                    diagnostics.push(
                        SkillDiagnostic::error(
                            "dependency-cycle",
                            format!(
                                "Skill dependency cycle detected: {} -> {}",
                                cycle.join(" -> "),
                                name
                            ),
                        )
                        .with_skill(name.to_string()),
                    );
                }
            }
            return;
        }
        _ => {}
    }

    state.insert(name, 1);
    stack.push(name);
    if let Some(deps) = deps_by_name.get(name) {
        for dep in deps {
            if deps_by_name.contains_key(dep) {
                visit_cycle(
                    dep,
                    deps_by_name,
                    state,
                    stack,
                    diagnostics,
                    invalid,
                    reported,
                );
            }
        }
    }
    stack.pop();
    state.insert(name, 2);
}

fn topo_sort_skills(skills: Vec<SkillDefinition>) -> Vec<SkillDefinition> {
    let by_name: HashMap<String, SkillDefinition> =
        skills.into_iter().map(|s| (s.name.clone(), s)).collect();
    let mut output = Vec::new();
    let mut visited = HashSet::new();
    let names: Vec<String> = by_name.keys().cloned().collect();

    for name in names {
        topo_visit(&name, &by_name, &mut visited, &mut output);
    }

    output
}

fn topo_visit(
    name: &str,
    by_name: &HashMap<String, SkillDefinition>,
    visited: &mut HashSet<String>,
    output: &mut Vec<SkillDefinition>,
) {
    if !visited.insert(name.to_string()) {
        return;
    }
    let Some(skill) = by_name.get(name) else {
        return;
    };
    for dep in &skill.frontmatter.dependencies {
        topo_visit(&dep.name, by_name, visited, output);
    }
    output.push(skill.clone());
}

pub fn is_valid_skill_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_alphanumeric() {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':' | '.'))
}

pub fn is_safe_relative_path(path: &str) -> bool {
    let path = path.trim();
    if path.is_empty() {
        return false;
    }
    let candidate = Path::new(path);
    if candidate.is_absolute() {
        return false;
    }
    !candidate
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct VersionParts(u64, u64, u64);

fn parse_version(raw: &str) -> Option<VersionParts> {
    let raw = raw.trim().trim_start_matches('v');
    let core = raw.split(['-', '+']).next().unwrap_or(raw);
    if core.is_empty() {
        return None;
    }
    let parts: Vec<&str> = core.split('.').collect();
    if parts.len() > 3 || parts.iter().any(|p| p.is_empty()) {
        return None;
    }
    let major = parts.first()?.parse().ok()?;
    let minor = parts.get(1).unwrap_or(&"0").parse().ok()?;
    let patch = parts.get(2).unwrap_or(&"0").parse().ok()?;
    Some(VersionParts(major, minor, patch))
}

fn cmp_versions(a: VersionParts, b: VersionParts) -> CmpOrdering {
    (a.0, a.1, a.2).cmp(&(b.0, b.1, b.2))
}

fn version_req_satisfied(version: &str, requirement: &str) -> Result<bool, String> {
    let requirement = requirement.trim();
    if requirement.is_empty() || requirement == "*" {
        return Ok(true);
    }

    let (op, rhs) = for_req_op(requirement);
    let current = parse_version(version)
        .ok_or_else(|| format!("Invalid version '{}'. Use numeric semantic form.", version))?;
    let wanted = parse_version(rhs).ok_or_else(|| {
        format!(
            "Invalid version requirement '{}'. Use forms like 1.2.3, >=1.0.0, or <2.0.0.",
            requirement
        )
    })?;
    let cmp = cmp_versions(current, wanted);

    match op {
        "=" => Ok(cmp == CmpOrdering::Equal),
        ">" => Ok(cmp == CmpOrdering::Greater),
        ">=" => Ok(matches!(cmp, CmpOrdering::Greater | CmpOrdering::Equal)),
        "<" => Ok(cmp == CmpOrdering::Less),
        "<=" => Ok(matches!(cmp, CmpOrdering::Less | CmpOrdering::Equal)),
        "^" => {
            if current.0 == 0 {
                Ok(current.0 == wanted.0
                    && current.1 == wanted.1
                    && matches!(cmp, CmpOrdering::Greater | CmpOrdering::Equal))
            } else {
                Ok(current.0 == wanted.0
                    && matches!(cmp, CmpOrdering::Greater | CmpOrdering::Equal))
            }
        }
        "~" => Ok(current.0 == wanted.0
            && current.1 == wanted.1
            && matches!(cmp, CmpOrdering::Greater | CmpOrdering::Equal)),
        _ => Err(format!(
            "Unsupported version requirement '{}'.",
            requirement
        )),
    }
}

fn for_req_op(requirement: &str) -> (&'static str, &str) {
    for op in [">=", "<=", ">", "<", "=", "^", "~"] {
        if let Some(rest) = requirement.strip_prefix(op) {
            return (op, rest.trim());
        }
    }
    ("=", requirement)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    static SKILL_REGISTRY_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn make_skill(name: &str) -> SkillDefinition {
        SkillDefinition {
            name: name.to_string(),
            source: SkillSource::Bundled,
            base_dir: None,
            frontmatter: SkillFrontmatter {
                description: "Test skill".to_string(),
                version: Some("1.0.0".to_string()),
                user_invocable: true,
                ..Default::default()
            },
            prompt_body: "Do the thing.".to_string(),
        }
    }

    #[test]
    fn test_skill_display_name() {
        let mut skill = make_skill("test");
        assert_eq!(skill.display_name(), "test");

        skill.frontmatter.name = Some("Custom Name".to_string());
        assert_eq!(skill.display_name(), "Custom Name");
    }

    #[test]
    fn test_skill_is_model_invocable() {
        let skill = make_skill("test");
        assert!(skill.is_model_invocable());

        let mut skill2 = make_skill("test2");
        skill2.frontmatter.disable_model_invocation = true;
        assert!(!skill2.is_model_invocable());

        let mut skill3 = make_skill("test3");
        skill3.frontmatter.description = String::new();
        skill3.frontmatter.when_to_use = None;
        assert!(!skill3.is_model_invocable());
    }

    #[test]
    fn test_expand_prompt_arguments() {
        let mut skill = make_skill("greet");
        skill.prompt_body = "Hello $ARGUMENTS, welcome!".to_string();
        let result = skill.expand_prompt("world", None);
        assert_eq!(result, "Hello world, welcome!");
    }

    #[test]
    fn test_expand_prompt_named_args() {
        let mut skill = make_skill("greet");
        skill.frontmatter.argument_names = vec!["NAME".to_string(), "LANG".to_string()];
        skill.prompt_body = "Hi ${NAME}, you speak ${LANG}.".to_string();
        let result = skill.expand_prompt("Alice Rust", None);
        assert_eq!(result, "Hi Alice, you speak Rust.");
    }

    #[test]
    fn test_expand_prompt_session_id() {
        let mut skill = make_skill("test");
        skill.prompt_body = "Session: ${CLAUDE_SESSION_ID}".to_string();
        let result = skill.expand_prompt("", Some("abc-123"));
        assert_eq!(result, "Session: abc-123");
    }

    #[test]
    fn test_expand_prompt_skill_dir() {
        let mut skill = make_skill("test");
        skill.base_dir = Some(PathBuf::from("/home/user/.allthecodes/skills/test"));
        skill.prompt_body = "Dir: ${CLAUDE_SKILL_DIR}".to_string();
        let result = skill.expand_prompt("", None);
        assert_eq!(result, "Dir: /home/user/.allthecodes/skills/test");
    }

    #[test]
    fn test_skill_source_variants() {
        let sources = [
            SkillSource::Bundled,
            SkillSource::User,
            SkillSource::Project,
            SkillSource::Plugin("my-plugin".to_string()),
            SkillSource::Mcp("my-server".to_string()),
        ];
        assert_eq!(sources.len(), 5);
    }

    #[test]
    fn reload_loads_project_skills_from_allthecodes_project_dir() {
        clear_skills();
        let tmp = tempfile::tempdir().unwrap();
        let user_dir = tmp.path().join("user-skills");
        let project_skill_dir =
            allthecodes_config::paths::project_skills_dir(tmp.path()).join("review");
        std::fs::create_dir_all(&project_skill_dir).unwrap();
        std::fs::write(
            project_skill_dir.join("SKILL.md"),
            "---\ndescription: Review local changes.\n---\nCheck the diff.\n",
        )
        .unwrap();

        let report = reload_skills_with_extra(
            &user_dir,
            Some(tmp.path()),
            Vec::new(),
            SkillLoadOptions::default(),
        );

        assert_eq!(report.error_count(), 0, "{:?}", report.diagnostics);
        let project = get_all_skills()
            .into_iter()
            .find(|skill| skill.name == "review")
            .expect("project skill should load");
        assert_eq!(project.source, SkillSource::Project);
        assert_eq!(
            project.base_dir.as_deref(),
            Some(project_skill_dir.as_path())
        );
        clear_skills();
    }

    #[test]
    fn test_skill_context_default() {
        assert_eq!(SkillContext::default(), SkillContext::Inline);
    }

    #[test]
    fn dependency_resolution_orders_dependencies_first() {
        let mut child = make_skill("child");
        child.frontmatter.dependencies = vec![SkillDependency::new("base", Some(">=1.0.0".into()))];
        let base = make_skill("base");

        let mut diagnostics = Vec::new();
        let resolved = resolve_skill_packages(
            vec![child, base],
            &mut diagnostics,
            &SkillLoadOptions::for_app_version("1.0.0"),
        );

        let names: Vec<&str> = resolved.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["base", "child"]);
        assert!(diagnostics.iter().all(|d| !d.is_error()));
    }

    #[test]
    fn dependency_resolution_rejects_missing_dependency() {
        let mut skill = make_skill("child");
        skill.frontmatter.dependencies = vec![SkillDependency::new("missing", None)];

        let mut diagnostics = Vec::new();
        let resolved = resolve_skill_packages(
            vec![skill],
            &mut diagnostics,
            &SkillLoadOptions::for_app_version("1.0.0"),
        );

        assert!(resolved.is_empty());
        assert!(diagnostics.iter().any(|d| d.code == "missing-dependency"));
    }

    #[test]
    fn dependency_resolution_rejects_cycles() {
        let mut a = make_skill("a");
        a.frontmatter.dependencies = vec![SkillDependency::new("b", None)];
        let mut b = make_skill("b");
        b.frontmatter.dependencies = vec![SkillDependency::new("a", None)];

        let mut diagnostics = Vec::new();
        let resolved = resolve_skill_packages(
            vec![a, b],
            &mut diagnostics,
            &SkillLoadOptions::for_app_version("1.0.0"),
        );

        assert!(resolved.is_empty());
        assert!(diagnostics.iter().any(|d| d.code == "dependency-cycle"));
    }

    #[test]
    fn version_requirement_comparison() {
        assert!(version_req_satisfied("1.2.3", ">=1.0.0").unwrap());
        assert!(version_req_satisfied("1.2.3", "1.2.3").unwrap());
        assert!(!version_req_satisfied("1.2.3", "<1.0.0").unwrap());
        assert!(version_req_satisfied("1.2.3", "^1.0.0").unwrap());
        assert!(!version_req_satisfied("2.0.0", "^1.0.0").unwrap());
    }

    #[test]
    fn duplicate_versions_conflict() {
        let first = make_skill("same");
        let mut second = make_skill("same");
        second.frontmatter.version = Some("2.0.0".to_string());

        let mut diagnostics = Vec::new();
        let resolved = resolve_skill_packages(
            vec![first, second],
            &mut diagnostics,
            &SkillLoadOptions::for_app_version("1.0.0"),
        );

        assert_eq!(resolved.len(), 1);
        assert!(diagnostics.iter().any(|d| d.code == "version-conflict"));
    }

    #[test]
    fn clear_mcp_skills_for_server_only_removes_that_servers_skills() {
        let _lock = SKILL_REGISTRY_TEST_LOCK.lock().unwrap();
        clear_skills();
        let mut alpha = make_skill("alpha-review");
        alpha.source = SkillSource::Mcp("alpha".to_string());
        let mut beta = make_skill("beta-review");
        beta.source = SkillSource::Mcp("beta".to_string());
        let user = make_skill("user-review");
        register_skill(alpha);
        register_skill(beta);
        register_skill(user);

        let removed = clear_mcp_skills_for_server("alpha");

        assert_eq!(removed, 1);
        assert!(find_skill("alpha-review").is_none());
        assert!(find_skill("beta-review").is_some());
        assert!(find_skill("user-review").is_some());
        clear_skills();
    }

    #[test]
    fn replace_mcp_skills_for_server_drops_stale_entries_before_resolve() {
        let _lock = SKILL_REGISTRY_TEST_LOCK.lock().unwrap();
        clear_skills();
        let mut stale = make_skill("mcp__alpha__old");
        stale.source = SkillSource::Mcp("alpha".to_string());
        let mut fresh = make_skill("mcp__alpha__new");
        fresh.source = SkillSource::Mcp("alpha".to_string());
        let mut other = make_skill("mcp__beta__keep");
        other.source = SkillSource::Mcp("beta".to_string());
        register_skill(stale);
        register_skill(other);

        let report = replace_mcp_skills_for_server(
            "alpha",
            vec![fresh],
            Vec::new(),
            SkillLoadOptions::for_app_version("1.0.0"),
        );

        assert_eq!(report.error_count(), 0, "{:?}", report.diagnostics);
        assert!(find_skill("mcp__alpha__old").is_none());
        assert!(find_skill("mcp__alpha__new").is_some());
        assert!(find_skill("mcp__beta__keep").is_some());
        clear_skills();
    }

    #[test]
    fn incompatible_app_version_is_rejected() {
        let mut skill = make_skill("future");
        skill.frontmatter.compatible_app_version = Some(">=9.0.0".to_string());

        let mut diagnostics = Vec::new();
        let resolved = resolve_skill_packages(
            vec![skill],
            &mut diagnostics,
            &SkillLoadOptions::for_app_version("1.0.0"),
        );

        assert!(resolved.is_empty());
        assert!(diagnostics
            .iter()
            .any(|d| d.code == "incompatible-app-version"));
    }

    #[test]
    fn skill_proposal_approval_writes_project_skill_package() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path().join("project");
        std::fs::create_dir_all(&cwd).unwrap();

        let proposal = stage_skill_proposal(
            SkillProposalDraft {
                action: SkillProposalAction::Create,
                scope: SkillProposalScope::Project,
                skill_name: "hermes-review".to_string(),
                source_session_id: Some("session-one".to_string()),
                markdown: "---\ndescription: Review Hermes runtime changes.\n---\nCheck the diff."
                    .to_string(),
            },
            &cwd,
        )
        .unwrap();

        assert!(proposal_storage_path(&proposal.id, SkillProposalScope::Project, &cwd).exists());
        assert!(!proposal.proposed_path.exists());
        assert!(find_skill("hermes-review").is_none());

        let approved_path = approve_skill_proposal(&proposal.id, &cwd).unwrap();

        assert_eq!(
            approved_path,
            allthecodes_config::paths::project_skills_dir(&cwd)
                .join("hermes-review")
                .join("SKILL.md")
        );
        assert!(approved_path.exists());
        assert!(!proposal_storage_path(&proposal.id, SkillProposalScope::Project, &cwd).exists());
        let loaded = loader::load_skill_from_file_path(&approved_path, SkillSource::Project)
            .expect("approved skill should load");
        assert_eq!(loaded.name, "hermes-review");
    }

    #[test]
    fn skill_proposal_reject_removes_without_active_skill() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path().join("project");
        std::fs::create_dir_all(&cwd).unwrap();

        let proposal = stage_skill_proposal(
            SkillProposalDraft {
                action: SkillProposalAction::Create,
                scope: SkillProposalScope::Project,
                skill_name: "discarded-skill".to_string(),
                source_session_id: None,
                markdown: "---\ndescription: Discarded skill.\n---\nDo not install.".to_string(),
            },
            &cwd,
        )
        .unwrap();

        reject_skill_proposal(&proposal.id, &cwd).unwrap();

        assert!(!proposal_storage_path(&proposal.id, SkillProposalScope::Project, &cwd).exists());
        assert!(!proposal.proposed_path.exists());
        assert!(find_skill("discarded-skill").is_none());
    }
}
