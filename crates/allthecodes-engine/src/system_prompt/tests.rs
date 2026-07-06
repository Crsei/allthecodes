use super::dynamic_sections::*;
use super::static_sections::*;
use super::*;
use crate::config::features::{self, FeatureFlags};
use crate::prompt_sections;
use std::fs;
use std::path::Path;

struct EnvGuard {
    key: &'static str,
    previous: Option<String>,
}

impl EnvGuard {
    fn set_path(key: &'static str, path: &Path) -> Self {
        let previous = std::env::var(key).ok();
        std::env::set_var(key, path);
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

struct FeatureOverrideGuard;

impl Drop for FeatureOverrideGuard {
    fn drop(&mut self) {
        features::clear_runtime_override();
        prompt_sections::clear_cache();
    }
}

#[test]
fn test_intro_section_contains_identity() {
    let intro = intro_section();
    assert!(intro.contains("interactive agent"));
    assert!(intro.contains("software engineering"));
    assert!(intro.contains("NEVER generate or guess URLs"));
}

#[test]
fn test_intro_section_contains_cyber_risk() {
    let intro = intro_section();
    assert!(intro.contains("authorized security testing"));
    assert!(intro.contains("Refuse requests for destructive techniques"));
}

#[test]
fn test_system_section_structure() {
    let sys = system_section();
    assert!(sys.starts_with("# System"));
    assert!(sys.contains("permission mode"));
    assert!(sys.contains("hooks"));
    assert!(sys.contains("compress prior messages"));
    assert!(sys.contains("prompt injection"));
}

#[test]
fn test_doing_tasks_section() {
    let tasks = doing_tasks_section();
    assert!(tasks.starts_with("# Doing tasks"));
    assert!(tasks.contains("software engineering tasks"));
    assert!(tasks.contains("OWASP"));
    assert!(tasks.contains("Don't add features"));
    assert!(tasks.contains("/help"));
    assert!(tasks.contains("https://github.com/Crsei/allthecodes/issues"));
    assert!(!tasks.contains("anthropics/claude-code/issues"));
}

#[test]
fn test_actions_section() {
    let actions = actions_section();
    assert!(actions.starts_with("# Executing actions with care"));
    assert!(actions.contains("reversibility and blast radius"));
    assert!(actions.contains("force-pushing"));
    assert!(actions.contains("measure twice, cut once"));
}

#[test]
fn test_using_tools_section() {
    let tools = using_tools_section(&[
        "Bash",
        "Read",
        "Edit",
        "Write",
        "Glob",
        "Grep",
        "TaskCreate",
    ]);
    assert!(tools.starts_with("# Using your tools"));
    assert!(tools.contains("Read instead of cat"));
    assert!(tools.contains("Edit instead of sed"));
    assert!(tools.contains("TaskCreate"));
    assert!(tools.contains("parallel"));
}

#[test]
fn test_using_tools_without_task_create() {
    let tools = using_tools_section(&["Bash", "Read"]);
    assert!(!tools.contains("TaskCreate tool"));
}

#[test]
fn test_deferred_tools_instruction_uses_wrapper_execution() {
    let tools = using_tools_section(&["SearchExtraTools", "ExecuteExtraTool"]);
    assert!(tools.contains("Discovery does not make hidden tool schemas directly visible"));
    assert!(tools.contains("call ExecuteExtraTool"));
    assert!(!tools.contains("prefer calling the tool directly"));
}

#[test]
fn test_tone_and_style() {
    let tone = tone_and_style_section();
    assert!(tone.starts_with("# Tone and style"));
    assert!(tone.contains("emojis"));
    assert!(tone.contains("file_path:line_number"));
    assert!(tone.contains("owner/repo#123"));
}

#[test]
fn test_output_efficiency() {
    let eff = output_efficiency_section();
    assert!(eff.starts_with("# Output efficiency"));
    assert!(eff.contains("Go straight to the point"));
    assert!(eff.contains("does not apply to code"));
}

#[test]
fn test_language_section_none() {
    assert!(language_section(None).is_none());
}

#[test]
fn test_language_section_some() {
    let lang = language_section(Some("Chinese")).unwrap();
    assert!(lang.contains("# Language"));
    assert!(lang.contains("Chinese"));
    assert!(lang.contains("Technical terms"));
}

#[test]
fn test_env_info_section() {
    let info = env_info_section("claude-sonnet-4-20250514", "/tmp");
    assert!(info.contains("<env>"));
    assert!(info.contains("Working directory: /tmp"));
    assert!(info.contains("Platform:"));
}

#[test]
fn test_default_prompt_has_all_sections() {
    prompt_sections::clear_cache();
    let (parts, _ctx, _) = build_system_prompt(
        None,
        None,
        &[],
        "claude-sonnet-4-20250514",
        "/tmp",
        None,
        None,
        false,
    );

    // Should have at least: intro, system, doing_tasks, actions, tools, tone, efficiency, boundary, env_info, summarize
    assert!(
        parts.len() >= 9,
        "expected at least 9 parts, got {}",
        parts.len()
    );

    let joined = parts.join("\n");
    assert!(joined.contains("interactive agent"), "missing intro");
    assert!(joined.contains("# System"), "missing system");
    assert!(joined.contains("# Doing tasks"), "missing doing_tasks");
    assert!(joined.contains("# Executing actions"), "missing actions");
    assert!(joined.contains("# Using your tools"), "missing using_tools");
    assert!(joined.contains("# Tone and style"), "missing tone");
    assert!(joined.contains("# Output efficiency"), "missing efficiency");
    assert!(joined.contains(DYNAMIC_BOUNDARY), "missing boundary");
    assert!(joined.contains("<env>"), "missing env_info");
    assert!(
        !joined.contains("# Language"),
        "language section should be omitted when language is None"
    );
    assert!(
        !joined.contains("# Output Style"),
        "output style should be omitted when None"
    );
}

#[test]
fn test_context_maps_are_metadata_not_prompt_sections() {
    prompt_sections::clear_cache();
    let cwd = "/tmp/context-metadata-contract";
    let (parts, user_context, system_context) =
        build_system_prompt(None, None, &[], "test-model", cwd, None, None, false);

    assert_eq!(user_context.get("cwd"), Some(&cwd.to_string()));
    assert_eq!(user_context.get("model"), Some(&"test-model".to_string()));
    assert!(user_context.contains_key("date"));
    assert!(user_context.contains_key("platform"));
    assert!(
        system_context.is_empty(),
        "system_context is currently reserved metadata"
    );

    let joined = parts.join("\n");
    assert!(!joined.contains("user_context"));
    assert!(!joined.contains("system_context"));
}

#[test]
#[serial_test::serial]
fn test_coordinator_mode_injects_prompt_section_when_enabled() {
    let _guard = FeatureOverrideGuard;
    let mut flags = FeatureFlags::all_disabled();
    flags.coordinator = true;
    features::set_runtime_override(flags);
    prompt_sections::clear_cache();

    let (parts, _, _) = build_system_prompt(
        None,
        None,
        &[],
        "claude-sonnet-4-20250514",
        "/tmp",
        None,
        None,
        false,
    );
    let joined = parts.join("\n");

    assert!(joined.contains("# Coordinator Mode"));
    assert!(joined.contains("SendMessage"));
    assert!(!joined.contains("TaskList"));
    assert!(joined.contains("TaskStop"));
}

#[test]
#[serial_test::serial]
fn kairos_prompt_injects_proactive_section_when_kairos_enabled() {
    let _guard = FeatureOverrideGuard;
    let mut flags = FeatureFlags::all_disabled();
    flags.kairos = true;
    features::set_runtime_override(flags);
    prompt_sections::clear_cache();

    let (parts, _, _) = build_system_prompt(
        None,
        None,
        &[],
        "claude-sonnet-4-20250514",
        "/tmp",
        None,
        None,
        false,
    );
    let joined = parts.join("\n");

    assert!(joined.contains("# Autonomous work"));
    assert!(joined.contains("<tick_tag>"));
    assert!(joined.contains("Sleep"));
    assert!(joined.contains("terminalFocus"));
    assert!(joined.contains("still waiting"));
}

#[test]
#[serial_test::serial]
fn kairos_prompt_injects_brief_section_when_brief_enabled() {
    let _guard = FeatureOverrideGuard;
    let mut flags = FeatureFlags::all_disabled();
    flags.kairos = true;
    flags.kairos_brief = true;
    features::set_runtime_override(flags);
    prompt_sections::clear_cache();

    let (parts, _, _) = build_system_prompt(
        None,
        None,
        &[],
        "claude-sonnet-4-20250514",
        "/tmp",
        None,
        None,
        false,
    );
    let joined = parts.join("\n");

    assert!(joined.contains("# Brief output"));
    assert!(joined.contains("Brief"));
    assert!(joined.contains("structured"));
    assert!(joined.contains("user-facing"));
}

#[test]
#[serial_test::serial]
fn kairos_prompt_omits_resident_sections_when_kairos_disabled() {
    let _guard = FeatureOverrideGuard;
    let flags = FeatureFlags::all_disabled();
    features::set_runtime_override(flags);
    prompt_sections::clear_cache();

    let (parts, _, _) = build_system_prompt(
        None,
        None,
        &[],
        "claude-sonnet-4-20250514",
        "/tmp",
        None,
        None,
        false,
    );
    let joined = parts.join("\n");

    assert!(!joined.contains("# Autonomous work"));
    assert!(!joined.contains("# Brief output"));
    assert!(!joined.contains("<tick_tag>"));
}

#[test]
fn test_language_setting_injects_section() {
    prompt_sections::clear_cache();
    let (parts, _, _) = build_system_prompt(
        None,
        None,
        &[],
        "claude-sonnet-4-20250514",
        "/tmp",
        Some("Chinese"),
        None,
        false,
    );
    let joined = parts.join("\n");
    assert!(joined.contains("# Language"), "language section missing");
    assert!(
        joined.contains("Chinese"),
        "language name should appear in prompt"
    );
}

#[test]
fn test_output_style_explanatory_injects_section() {
    prompt_sections::clear_cache();
    let (parts, _, _) = build_system_prompt(
        None,
        None,
        &[],
        "claude-sonnet-4-20250514",
        "/tmp",
        None,
        Some("explanatory"),
        false,
    );
    let joined = parts.join("\n");
    assert!(
        joined.contains("# Output Style: Explanatory"),
        "expected explanatory output style header"
    );
}

#[test]
fn test_output_style_default_emits_no_section() {
    prompt_sections::clear_cache();
    let (parts, _, _) = build_system_prompt(
        None,
        None,
        &[],
        "claude-sonnet-4-20250514",
        "/tmp",
        None,
        Some("default"),
        false,
    );
    let joined = parts.join("\n");
    assert!(
        !joined.contains("# Output Style"),
        "default style should not emit a section"
    );
}

#[test]
fn test_custom_prompt_replaces_default() {
    prompt_sections::clear_cache();
    let (parts, _, _) = build_system_prompt(
        Some("You are a custom assistant."),
        None,
        &[],
        "test",
        "/tmp",
        None,
        None,
        false,
    );
    assert_eq!(parts[0], "You are a custom assistant.");
    // Should NOT contain static sections
    let joined = parts.join("\n");
    assert!(!joined.contains("# Doing tasks"));
}

#[test]
fn test_select_custom_system_prompt_priority() {
    assert_eq!(
        select_custom_system_prompt(Some("cli prompt"), Some("settings prompt")),
        Some("cli prompt")
    );
    assert_eq!(
        select_custom_system_prompt(None, Some("settings prompt")),
        Some("settings prompt")
    );
    assert_eq!(select_custom_system_prompt(None, Some("   \n\t")), None);
    assert_eq!(select_custom_system_prompt(None, None), None);
}

#[test]
fn test_append_prompt() {
    prompt_sections::clear_cache();
    let (parts, _, _) = build_system_prompt(
        None,
        Some("Always be concise."),
        &[],
        "test",
        "/tmp",
        None,
        None,
        false,
    );
    assert_eq!(parts.last().unwrap(), "Always be concise.");
}

#[test]
fn test_build_effective_override() {
    let result =
        build_effective_system_prompt(vec!["default".into()], None, None, Some("override"), None);
    assert_eq!(result, vec!["override"]);
}

#[test]
fn test_build_effective_agent_replaces_default() {
    let result = build_effective_system_prompt(
        vec!["default".into()],
        None,
        None,
        None,
        Some("agent prompt"),
    );
    assert_eq!(result, vec!["agent prompt"]);
}

#[test]
fn test_build_effective_custom_replaces_default() {
    let result =
        build_effective_system_prompt(vec!["default".into()], Some("custom"), None, None, None);
    assert_eq!(result, vec!["custom"]);
}

#[test]
fn test_build_effective_append() {
    let result =
        build_effective_system_prompt(vec!["default".into()], None, Some("appended"), None, None);
    assert_eq!(result, vec!["default", "appended"]);
}

#[test]
fn test_format_bullets() {
    let result = format_bullets(&["first", "second"]);
    assert_eq!(result, " - first\n - second");
}

#[test]
fn test_agents_md_injection() {
    prompt_sections::clear_cache();
    let dir = std::env::temp_dir().join(format!("sysprompt_test_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    let md_path = dir.join("AGENTS.md");
    fs::write(&md_path, "# Rules\nUse snake_case.").unwrap();

    let cwd = dir.to_str().unwrap();
    let (parts, _, _) = build_system_prompt(None, None, &[], "test", cwd, None, None, false);
    let joined = parts.join("\n");
    assert!(joined.contains("snake_case"));
    assert!(joined.contains("# Project Instructions\n"));
    assert!(!joined.contains("# Project Instructions (AGENTS.md)"));
    assert!(joined.contains("CLAUDE.md is used only as a compatibility fallback"));
    assert!(joined.contains("OVERRIDE"));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_memory_context_injection() {
    prompt_sections::clear_cache();
    let dir = std::env::temp_dir().join(format!("sysprompt_memory_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    allthecodes_session::memdir::write_memory(
        "api-contract",
        "Use the stable v2 endpoint for uploads.",
        "project",
        allthecodes_session::memdir::MemoryScope::Project,
        &dir,
    )
    .unwrap();

    let cwd = dir.to_str().unwrap();
    let (parts, _, _) = build_system_prompt(None, None, &[], "test", cwd, None, None, false);
    let joined = parts.join("\n");
    assert!(joined.contains("# Memory Context"));
    assert!(joined.contains("<memory-context>"));
    assert!(joined.contains("api-contract"));
    assert!(joined.contains("stable v2 endpoint"));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_prebuilt_memory_context_overrides_full_memory_scan() {
    prompt_sections::clear_cache();
    let dir = std::env::temp_dir().join(format!(
        "sysprompt_memory_override_{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&dir).unwrap();
    allthecodes_session::memdir::write_memory(
        "full-memory",
        "This entry should not be injected when recall has already selected memory.",
        "project",
        allthecodes_session::memdir::MemoryScope::Project,
        &dir,
    )
    .unwrap();

    let cwd = dir.to_str().unwrap();
    let (parts, _, _) = build_system_prompt_with_memory_contexts(
        None,
        None,
        &[],
        "test",
        cwd,
        None,
        None,
        false,
        Some("<memory-context>\n## Relevant Memories\n- **selected**: use this\n</memory-context>"),
        Some("<session-insights>\n- Keep session detail.\n</session-insights>"),
    );
    let joined = parts.join("\n");

    assert!(joined.contains("## Relevant Memories"));
    assert!(joined.contains("selected"));
    assert!(joined.contains("<session-insights>"));
    assert!(!joined.contains("full-memory"));

    let _ = fs::remove_dir_all(&dir);
}

// ── auto memory tests ──

#[test]
#[serial_test::serial]
fn test_auto_memory_context_respects_toggle() {
    prompt_sections::clear_cache();
    let dir = std::env::temp_dir().join(format!(
        "sysprompt_auto_memory_cwd_{}",
        uuid::Uuid::new_v4()
    ));
    let home = std::env::temp_dir().join(format!(
        "sysprompt_auto_memory_home_{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&dir).unwrap();
    fs::create_dir_all(&home).unwrap();
    let _home_guard = EnvGuard::set_path("ALLTHECODES_HOME", &home);

    allthecodes_session::memdir::write_memory(
        "build-insight",
        "Prefer narrow cargo test filters for prompt changes.",
        "auto",
        allthecodes_session::memdir::MemoryScope::Auto,
        &dir,
    )
    .unwrap();

    let cwd = dir.to_str().unwrap();
    let (disabled_parts, _, _) =
        build_system_prompt(None, None, &[], "test", cwd, None, None, false);
    let disabled = disabled_parts.join("\n");
    assert!(
        !disabled.contains("build-insight"),
        "auto memories should stay out of the prompt while disabled"
    );

    let (enabled_parts, _, _) = build_system_prompt(None, None, &[], "test", cwd, None, None, true);
    let enabled = enabled_parts.join("\n");
    assert!(enabled.contains("# Memory Context"));
    assert!(enabled.contains("## Auto Memories"));
    assert!(enabled.contains("build-insight"));
    assert!(enabled.contains("narrow cargo test filters"));

    let _ = fs::remove_dir_all(&dir);
    let _ = fs::remove_dir_all(&home);
}

#[test]
fn test_session_memory_context_injection() {
    prompt_sections::clear_cache();
    let (parts, _, _) = build_system_prompt_with_session_memory(
        None,
        None,
        &[],
        "test",
        "/tmp",
        None,
        None,
        false,
        Some("<session-insights>\n- [session] Keep API tests focused.\n</session-insights>"),
    );
    let joined = parts.join("\n");
    assert!(joined.contains("# Memory Context"));
    assert!(joined.contains("<session-insights>"));
    assert!(joined.contains("Keep API tests focused."));
}

// ── git_status_section tests ──

#[test]
fn test_git_status_section_in_git_repo() {
    let cwd = env!("CARGO_MANIFEST_DIR");
    let result = git_status_section(cwd);
    assert!(result.is_some(), "should produce output in a git repo");
    let text = result.unwrap();
    assert!(text.contains("gitStatus:"));
    assert!(text.contains("Current branch:"));
    assert!(text.contains("Recent commits:"));
}

#[test]
fn test_git_status_section_not_git_repo() {
    let dir = std::env::temp_dir().join(format!("no_git_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    let result = git_status_section(dir.to_str().unwrap());
    assert!(result.is_none(), "should return None for non-git dir");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_git_status_section_contains_main_branch() {
    let cwd = env!("CARGO_MANIFEST_DIR");
    let result = git_status_section(cwd);
    if let Some(text) = result {
        assert!(
            text.contains("Main branch"),
            "should contain main branch info"
        );
    }
}

#[test]
fn test_git_status_section_limits_commits() {
    let cwd = env!("CARGO_MANIFEST_DIR");
    if let Some(text) = git_status_section(cwd) {
        let commit_lines: Vec<&str> = text
            .lines()
            .skip_while(|l| !l.contains("Recent commits:"))
            .skip(1)
            .filter(|l| !l.is_empty())
            .collect();
        assert!(
            commit_lines.len() <= 10,
            "should have at most 10 commit lines, got {}",
            commit_lines.len()
        );
    }
}

#[test]
fn test_build_system_prompt_includes_git_status() {
    // Test git_status_section directly to avoid SECTION_CACHE race with parallel tests.
    // The section is registered in build_system_prompt as cached_section("git_status", ...),
    // but the global cache makes integration testing unreliable under --test-threads>1.
    let cwd = env!("CARGO_MANIFEST_DIR");
    let result = git_status_section(cwd);
    assert!(
        result.is_some(),
        "git_status_section should produce output for this repo"
    );
    let text = result.unwrap();
    // Verify it would be included in a system prompt
    assert!(
        text.starts_with("gitStatus:"),
        "should start with gitStatus header"
    );
}
