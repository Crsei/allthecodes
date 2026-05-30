use tracing::{debug, info, warn};

pub(crate) fn log_skill_report(scope: &str, report: &allthecodes_skills::SkillLoadReport) {
    info!(
        scope,
        loaded = report.loaded,
        skipped = report.skipped,
        revision = report.revision,
        warnings = report.warning_count(),
        errors = report.error_count(),
        "skills loaded"
    );

    for diagnostic in &report.diagnostics {
        match diagnostic.severity {
            allthecodes_skills::SkillDiagnosticSeverity::Error => warn!(
                scope,
                code = %diagnostic.code,
                skill = ?diagnostic.skill,
                source = ?diagnostic.source,
                path = ?diagnostic.path,
                message = %diagnostic.message,
                "skill load error"
            ),
            allthecodes_skills::SkillDiagnosticSeverity::Warning => debug!(
                scope,
                code = %diagnostic.code,
                skill = ?diagnostic.skill,
                source = ?diagnostic.source,
                path = ?diagnostic.path,
                message = %diagnostic.message,
                "skill load warning"
            ),
        }
    }
}

pub(crate) fn discover_plugin_skills_for_root() -> Vec<allthecodes_skills::SkillDefinition> {
    let mut out = Vec::new();

    for contributed in allthecodes_plugins::discover_plugin_skill_definitions() {
        let source = allthecodes_skills::SkillSource::Plugin(contributed.plugin_id.clone());
        let mut skill = match allthecodes_skills::loader::load_skill_from_file_path(
            &contributed.path,
            source,
        ) {
            Some(skill) => skill,
            None => {
                warn!(
                    plugin = %contributed.plugin_id,
                    path = %contributed.path.display(),
                    "Plugin: failed to load contributed skill file"
                );
                continue;
            }
        };

        skill.name = contributed.name;
        if let Some(desc) = contributed.description {
            if !desc.trim().is_empty() {
                skill.frontmatter.description = desc;
            }
        }
        out.push(skill);
    }

    out
}

pub(crate) fn register_user_invocable_skill_commands() {
    use allthecodes_commands::dynamic_registry::{
        CommandSource, DynamicCommandEntry, ExecutionStrategy,
    };

    let skills = allthecodes_skills::get_user_invocable_skills();
    let mut registry = allthecodes_commands::DYNAMIC_REGISTRY.lock();
    let stale_skill_names: Vec<String> = registry
        .list_all()
        .into_iter()
        .filter(|entry| entry.source == CommandSource::Skill)
        .map(|entry| entry.name.clone())
        .collect();
    for name in stale_skill_names {
        registry.unregister(&name, CommandSource::Skill);
    }

    for skill in skills {
        registry.register(DynamicCommandEntry {
            name: skill.name.clone(),
            aliases: Vec::new(),
            description: skill.frontmatter.description.clone(),
            source: CommandSource::Skill,
            plugin_id: None,
            hidden: false,
            usage_score: allthecodes_skills::skill_usage_score(&skill.name),
            execution_strategy: ExecutionStrategy::Skill,
        });
    }
}

pub(crate) fn persist_skill_usage() {
    let path = allthecodes_config::paths::skill_usage_path();
    if let Err(error) = allthecodes_skills::save_skill_usage(&path) {
        warn!(
            error = %error,
            path = %path.display(),
            "failed to persist skill usage"
        );
    }
}
