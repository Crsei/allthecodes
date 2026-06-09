//! Shared test infrastructure for handler tests.
//!
//! Re-exports test utilities used by multiple handler submodule test suites.
//! All items use `pub(super)` visibility so handler files can access them via
//! `use crate::handlers::test_support::*;`.

#![cfg(test)]

use crate::state::WebState;
use allthecodes_engine::lifecycle::QueryEngine;
use allthecodes_engine::types::config::QueryEngineConfig;
use allthecodes_ipc_protocol::subsystem_types::{
    AgentDefinitionEntry, AgentDefinitionSource, ConfigScope, McpServerConfigEntry,
};
use allthecodes_skills::{SkillDefinition, SkillFrontmatter, SkillSource};
use axum::body::to_bytes;
use serde_json::Value;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tempfile::TempDir;

pub(super) struct EnvGuard {
    key: &'static str,
    previous: Option<String>,
}

impl EnvGuard {
    pub(super) fn set_path(key: &'static str, value: &Path) -> Self {
        let previous = std::env::var(key).ok();
        std::env::set_var(key, value);
        Self { key, previous }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        if let Some(previous) = &self.previous {
            std::env::set_var(self.key, previous);
        } else {
            std::env::remove_var(self.key);
        }
    }
}

pub(super) fn make_web_state() -> WebState {
    make_web_state_with_cwd(Path::new("."))
}

pub(super) fn make_web_state_with_cwd(cwd: &Path) -> WebState {
    let engine = Arc::new(QueryEngine::new(QueryEngineConfig {
        cwd: cwd.to_string_lossy().to_string(),
        tools: vec![],
        custom_system_prompt: None,
        append_system_prompt: None,
        user_specified_model: None,
        fallback_model: None,
        max_turns: None,
        max_budget_usd: None,
        task_budget: None,
        verbose: false,
        initial_messages: None,
        commands: vec![],
        thinking_config: None,
        json_schema: None,
        replay_user_messages: false,
        persist_session: false,
        resolved_model: None,
        auto_save_session: false,
        agent_context: None,
    }));
    WebState::new(engine, Arc::new(AtomicBool::new(false)))
}

pub(super) fn make_agent_entry(name: &str, source: AgentDefinitionSource) -> AgentDefinitionEntry {
    AgentDefinitionEntry {
        name: name.to_string(),
        description: format!("Agent {name}"),
        system_prompt: "You are a test agent.".to_string(),
        tools: vec!["Read".to_string()],
        disallowed_tools: vec![],
        model: None,
        color: None,
        permission_mode: None,
        memory: None,
        max_turns: None,
        effort: None,
        background: false,
        isolation: None,
        skills: vec![],
        hooks: Value::Null,
        mcp_servers: vec![],
        initial_prompt: None,
        filename: None,
        source,
        file_path: None,
    }
}

pub(super) fn make_mcp_entry(name: &str, scope: ConfigScope) -> McpServerConfigEntry {
    McpServerConfigEntry {
        name: name.to_string(),
        transport: "stdio".to_string(),
        command: Some("echo".to_string()),
        args: Some(vec!["ok".to_string()]),
        url: None,
        headers: None,
        oauth: None,
        env: None,
        browser_mcp: None,
        disabled: None,
        scope,
    }
}

pub(super) async fn response_json(response: axum::response::Response) -> Value {
    let body = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("response body");
    serde_json::from_slice(&body).expect("json body")
}

pub(super) fn temp_home() -> (TempDir, EnvGuard) {
    let temp = tempfile::tempdir().expect("tempdir");
    let guard = EnvGuard::set_path("ALLTHECODES_HOME", temp.path());
    (temp, guard)
}

pub(super) fn read_user_settings(home: &TempDir) -> allthecodes_config::settings::RawSettings {
    let path = home.path().join("settings.json");
    serde_json::from_str(&std::fs::read_to_string(path).expect("settings file"))
        .expect("settings json")
}

pub(super) fn make_test_skill(name: &str) -> SkillDefinition {
    SkillDefinition {
        name: name.to_string(),
        source: SkillSource::Bundled,
        base_dir: None,
        frontmatter: SkillFrontmatter {
            description: format!("Skill {} description", name),
            version: Some("1.0.0".to_string()),
            user_invocable: true,
            ..Default::default()
        },
        prompt_body: format!("Do the {} thing.", name),
    }
}
