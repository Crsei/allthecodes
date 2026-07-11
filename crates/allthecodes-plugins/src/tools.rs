//! Executable plugin tools.
//!
//! Plugin manifests may contribute tools that are backed by an external
//! subprocess runtime. This module wraps those contributions into the shared
//! `Tool` trait so they flow through the normal tool pipeline.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tracing::{info, warn};

use allthecodes_tools::exec::truncate_output;
use allthecodes_tools::tool::{
    InterruptBehavior, PermissionMode, PermissionResult, Tool, ToolAppState, ToolPermissionContext,
    ToolProgress, ToolResult, ToolUseContext,
};
use allthecodes_types::message::AssistantMessage;

use crate::manifest::{StdioToolRuntime, ToolContribution, ToolRuntime};

/// Executable wrapper around a plugin-contributed tool manifest entry.
pub struct PluginToolWrapper {
    plugin_id: String,
    plugin_root: PathBuf,
    contribution: ToolContribution,
}

impl PluginToolWrapper {
    pub fn new(plugin_id: String, plugin_root: PathBuf, contribution: ToolContribution) -> Self {
        Self {
            plugin_id,
            plugin_root,
            contribution,
        }
    }

    fn runtime(&self) -> Option<&ToolRuntime> {
        self.contribution.runtime.as_ref()
    }

    fn resolved_command(&self, command: &str) -> PathBuf {
        let path = Path::new(command);
        let has_path_components = command.contains('/') || command.contains('\\');

        if path.is_absolute() {
            path.to_path_buf()
        } else if has_path_components {
            self.plugin_root.join(path)
        } else {
            PathBuf::from(command)
        }
    }

    fn resolved_cwd(&self, runtime: &StdioToolRuntime) -> PathBuf {
        match runtime.cwd.as_deref() {
            Some(cwd) if !cwd.trim().is_empty() => {
                let path = Path::new(cwd);
                if path.is_absolute() {
                    path.to_path_buf()
                } else {
                    self.plugin_root.join(path)
                }
            }
            _ => self.plugin_root.clone(),
        }
    }

    fn permission_context_for_call(&self, app_state: ToolAppState) -> ToolPermissionContext {
        app_state.tool_permission_context
    }

    fn has_rule_for_tool(
        rules: &allthecodes_tools::tool::ToolPermissionRulesBySource,
        tool: &str,
    ) -> bool {
        rules
            .values()
            .any(|entries| entries.iter().any(|entry| entry == tool))
    }

    fn timeout_duration(timeout_ms: Option<u64>) -> std::time::Duration {
        const DEFAULT_TIMEOUT_MS: u64 = 120_000;
        const MAX_TIMEOUT_MS: u64 = 600_000;

        std::time::Duration::from_millis(
            timeout_ms
                .unwrap_or(DEFAULT_TIMEOUT_MS)
                .clamp(1, MAX_TIMEOUT_MS),
        )
    }

    async fn run_stdio_runtime(
        &self,
        runtime: &StdioToolRuntime,
        input: Value,
    ) -> Result<ToolResult> {
        info!(
            plugin_id = %self.plugin_id,
            tool_name = %self.contribution.name,
            command = %runtime.command,
            "Plugin tool execution started"
        );

        let command = self.resolved_command(&runtime.command);
        let command_display = command.display().to_string();
        let input_json =
            serde_json::to_string(&input).context("failed to serialize plugin tool input")?;

        let mut cmd = Command::new(&command);
        cmd.args(&runtime.args);
        cmd.current_dir(self.resolved_cwd(runtime));
        cmd.stdin(std::process::Stdio::piped());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());
        cmd.env("ALLTHECODES_PLUGIN_ID", &self.plugin_id);
        cmd.env("ALLTHECODES_PLUGIN_TOOL_NAME", &self.contribution.name);
        cmd.env("ALLTHECODES_PLUGIN_ROOT", &self.plugin_root);
        cmd.env("ALLTHECODES_TOOL_INPUT", &input_json);

        for (key, value) in &runtime.env {
            cmd.env(key, value);
        }

        let mut child = cmd.spawn().with_context(|| {
            format!("failed to spawn plugin tool runtime '{}'", command_display)
        })?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(input_json.as_bytes())
                .await
                .with_context(|| {
                    format!("failed to write stdin for plugin tool '{}'", self.name())
                })?;
        }

        let stdout_task = child.stdout.take().map(|mut stdout| {
            tokio::spawn(async move {
                let mut buf = Vec::new();
                stdout.read_to_end(&mut buf).await?;
                Ok::<Vec<u8>, std::io::Error>(buf)
            })
        });
        let stderr_task = child.stderr.take().map(|mut stderr| {
            tokio::spawn(async move {
                let mut buf = Vec::new();
                stderr.read_to_end(&mut buf).await?;
                Ok::<Vec<u8>, std::io::Error>(buf)
            })
        });

        let timeout_duration = Self::timeout_duration(runtime.timeout_ms);
        let status = match tokio::time::timeout(timeout_duration, child.wait()).await {
            Ok(wait_result) => wait_result.with_context(|| {
                format!(
                    "failed while waiting for plugin tool '{}' to exit",
                    self.name()
                )
            })?,
            Err(_) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                return Ok(ToolResult {
                    data: json!({
                        "error": format!(
                            "Plugin tool '{}' timed out after {}ms",
                            self.name(),
                            timeout_duration.as_millis()
                        )
                    }),
                    new_messages: vec![],
                    ..Default::default()
                });
            }
        };

        let stdout = match stdout_task {
            Some(task) => task.await.context("plugin tool stdout reader panicked")??,
            None => Vec::new(),
        };
        let stderr = match stderr_task {
            Some(task) => task.await.context("plugin tool stderr reader panicked")??,
            None => Vec::new(),
        };

        let stdout_text = String::from_utf8_lossy(&stdout).to_string();
        let stderr_text = String::from_utf8_lossy(&stderr).to_string();
        let exit_code = status.code().unwrap_or(-1);

        if !status.success() {
            warn!(
                plugin_id = %self.plugin_id,
                tool_name = %self.contribution.name,
                exit_code = exit_code,
                stderr = %truncate_output(&stderr_text, 500),
                "Plugin tool exited with non-zero status"
            );

            return Ok(ToolResult {
                data: json!({
                    "error": format!("Plugin tool '{}' exited with status {}", self.name(), exit_code),
                    "stdout": truncate_output(&stdout_text, self.max_result_size_chars()),
                    "stderr": truncate_output(&stderr_text, self.max_result_size_chars()),
                    "exit_code": exit_code,
                }),
                new_messages: vec![],
                ..Default::default()
            });
        }

        let trimmed_stdout = stdout_text.trim();
        if !trimmed_stdout.is_empty() {
            if let Ok(parsed) = serde_json::from_str::<Value>(trimmed_stdout) {
                return Ok(ToolResult {
                    data: parsed,
                    new_messages: vec![],
                    ..Default::default()
                });
            }
        }

        let mut combined = String::new();
        if !stdout_text.is_empty() {
            combined.push_str(&stdout_text);
        }
        if !stderr_text.is_empty() {
            if !combined.is_empty() {
                combined.push('\n');
            }
            combined.push_str(&stderr_text);
        }
        if combined.is_empty() {
            combined = "(plugin tool returned no output)".to_string();
        }

        let combined = truncate_output(&combined, self.max_result_size_chars());
        Ok(ToolResult {
            data: Value::String(combined),
            new_messages: vec![],
            ..Default::default()
        })
    }
}

#[async_trait]
impl Tool for PluginToolWrapper {
    fn name(&self) -> &str {
        &self.contribution.name
    }

    async fn description(&self, _input: &Value) -> String {
        if self.contribution.description.trim().is_empty() {
            format!(
                "Plugin tool '{}' contributed by '{}'.",
                self.name(),
                self.plugin_id
            )
        } else {
            self.contribution.description.clone()
        }
    }

    fn input_json_schema(&self) -> Value {
        self.contribution
            .input_schema
            .clone()
            .unwrap_or_else(|| json!({"type": "object", "properties": {}}))
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        self.contribution.concurrency_safe
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        self.contribution.read_only
    }

    fn is_destructive(&self, _input: &Value) -> bool {
        !self.contribution.read_only
    }

    fn interrupt_behavior(&self) -> InterruptBehavior {
        InterruptBehavior::Cancel
    }

    async fn check_permissions(&self, input: &Value, ctx: &ToolUseContext) -> PermissionResult {
        let app_state = (ctx.get_app_state)();
        let permission_ctx = self.permission_context_for_call(app_state);
        if Self::has_rule_for_tool(&permission_ctx.always_deny_rules, self.name()) {
            return PermissionResult::Deny {
                message: format!("Permission denied for plugin tool '{}'", self.name()),
            };
        }
        if Self::has_rule_for_tool(&permission_ctx.always_allow_rules, self.name())
            || Self::has_rule_for_tool(&permission_ctx.session_allow_rules, self.name())
        {
            return PermissionResult::Allow {
                updated_input: input.clone(),
            };
        }
        if Self::has_rule_for_tool(&permission_ctx.always_ask_rules, self.name()) {
            return PermissionResult::Ask {
                message: format!(
                    "Allow plugin tool '{}' from '{}'?",
                    self.name(),
                    self.plugin_id
                ),
            };
        }

        match permission_ctx.mode {
            PermissionMode::Bypass => PermissionResult::Allow {
                updated_input: input.clone(),
            },
            PermissionMode::DontAsk => PermissionResult::Deny {
                message: format!("Permission denied for plugin tool '{}'", self.name()),
            },
            PermissionMode::Plan if !self.contribution.read_only => PermissionResult::Deny {
                message: format!(
                    "Plugin tool '{}' is not available in plan mode because it can write.",
                    self.name()
                ),
            },
            _ if self.contribution.read_only => PermissionResult::Allow {
                updated_input: input.clone(),
            },
            _ => PermissionResult::Ask {
                message: format!(
                    "Allow plugin tool '{}' from '{}'?",
                    self.name(),
                    self.plugin_id
                ),
            },
        }
    }

    async fn call(
        &self,
        input: Value,
        _ctx: &ToolUseContext,
        _parent_message: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        match self.runtime() {
            Some(ToolRuntime::Stdio(runtime)) => self.run_stdio_runtime(runtime, input).await,
            None => {
                warn!(
                    plugin_id = %self.plugin_id,
                    tool_name = %self.contribution.name,
                    "Plugin tool is metadata-only: no runtime configured"
                );
                Ok(ToolResult {
                    data: json!({
                        "error": format!(
                            "Plugin tool '{}' is metadata-only and has no runtime configured",
                            self.name()
                        )
                    }),
                    new_messages: vec![],
                    ..Default::default()
                })
            }
        }
    }

    async fn prompt(&self) -> String {
        let base = if self.contribution.description.trim().is_empty() {
            format!(
                "Plugin tool '{}' contributed by '{}'.",
                self.name(),
                self.plugin_id
            )
        } else {
            self.contribution.description.clone()
        };

        format!("{}\n\nContributed by plugin '{}'.", base, self.plugin_id)
    }

    fn user_facing_name(&self, _input: Option<&Value>) -> String {
        self.name().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use allthecodes_tools::tool::{FileStateCache, ToolUseOptions};
    use uuid::Uuid;

    fn dummy_ctx() -> ToolUseContext {
        let (_tx, rx) = tokio::sync::watch::channel(false);

        ToolUseContext {
            cwd: ".".to_string(),
            options: ToolUseOptions {
                debug: false,
                main_loop_model: "test-model".to_string(),
                verbose: false,
                is_non_interactive_session: false,
                custom_system_prompt: None,
                append_system_prompt: None,
                max_budget_usd: None,
            },
            abort_signal: rx,
            read_file_state: FileStateCache::default(),
            get_app_state: Arc::new(ToolAppState::default),
            set_app_state: Arc::new(|_| {}),
            session_id: "test-session".to_string(),
            langfuse_session_id: "test-session".to_string(),
            messages: vec![],
            agent_id: None,
            agent_type: None,
            query_tracking: None,
            permission_callback: None,
            permission_event_callback: None,
            ask_user_callback: None,
            bg_agent_tx: None,
            hook_runner: Arc::new(allthecodes_types::hooks::NoopHookRunner::new()),
            command_dispatcher: Arc::new(allthecodes_types::commands::NoopCommandDispatcher::new()),
            available_tools: vec![],
            execute_deferred_tool: None,
        }
    }

    fn dummy_parent() -> AssistantMessage {
        AssistantMessage {
            uuid: Uuid::new_v4(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            role: "assistant".to_string(),
            content: vec![],
            usage: None,
            stop_reason: None,
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        }
    }

    fn echo_runtime() -> StdioToolRuntime {
        if cfg!(windows) {
            StdioToolRuntime {
                command: "cmd".to_string(),
                args: vec!["/d".into(), "/s".into(), "/c".into(), "more".into()],
                env: Default::default(),
                cwd: None,
                timeout_ms: Some(5_000),
            }
        } else {
            StdioToolRuntime {
                command: "sh".to_string(),
                args: vec!["-c".into(), "cat".into()],
                env: Default::default(),
                cwd: None,
                timeout_ms: Some(5_000),
            }
        }
    }

    #[tokio::test]
    async fn plugin_tool_executes_stdio_runtime_and_parses_json() {
        let tool = PluginToolWrapper::new(
            "demo-plugin@local".to_string(),
            std::env::temp_dir(),
            ToolContribution {
                name: "plugin_echo".to_string(),
                description: "Echo JSON input".to_string(),
                input_schema: Some(json!({"type": "object"})),
                read_only: true,
                concurrency_safe: true,
                runtime: Some(ToolRuntime::Stdio(echo_runtime())),
            },
        );

        let input = json!({"hello": "world", "answer": 42});
        let result = tool
            .call(input.clone(), &dummy_ctx(), &dummy_parent(), None)
            .await
            .expect("plugin tool should execute");

        assert_eq!(result.data, input);
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn plugin_tool_executes_through_deferred_discovery_wrapper() {
        allthecodes_tools::deferred_tools::clear_discovered_tools_for_tests();
        let plugin_tool = Arc::new(PluginToolWrapper::new(
            "demo-plugin@local".to_string(),
            std::env::temp_dir(),
            ToolContribution {
                name: "PluginEcho".to_string(),
                description: "Echo JSON input from a plugin runtime".to_string(),
                input_schema: Some(json!({"type": "object"})),
                read_only: true,
                concurrency_safe: true,
                runtime: Some(ToolRuntime::Stdio(echo_runtime())),
            },
        )) as Arc<dyn Tool>;
        let mut ctx = dummy_ctx();
        ctx.session_id = "plugin-deferred".to_string();
        ctx.available_tools = vec![plugin_tool.clone()];

        let search = allthecodes_tools::deferred_tools::SearchExtraToolsTool
            .call(
                json!({"query": "select:PluginEcho", "max_results": 10}),
                &ctx,
                &dummy_parent(),
                None,
            )
            .await
            .expect("plugin tool should be discoverable");
        assert_eq!(search.data["deferred_tools_delta"], json!(["PluginEcho"]));

        let plugin_tool_for_executor = plugin_tool.clone();
        ctx.execute_deferred_tool = Some(Arc::new(move |request| {
            let plugin_tool = plugin_tool_for_executor.clone();
            Box::pin(async move {
                let call_ctx = dummy_ctx();
                let parent = dummy_parent();
                let result = plugin_tool
                    .call(request.input, &call_ctx, &parent, None)
                    .await?;
                Ok(allthecodes_tools::tool::DeferredToolExecutionResult {
                    tool_use_id: request.tool_use_id,
                    tool_name: request.tool_name,
                    result,
                    is_error: false,
                })
            })
        }));

        let input = json!({"hello": "plugin", "answer": 42});
        let result = allthecodes_tools::deferred_tools::ExecuteExtraToolTool
            .call(
                json!({"tool_name": "PluginEcho", "params": input.clone()}),
                &ctx,
                &dummy_parent(),
                None,
            )
            .await
            .expect("plugin tool should execute through deferred wrapper");

        assert_eq!(result.data["tool_name"], "PluginEcho");
        assert_eq!(result.data["result"], input);
        assert_eq!(
            result.display_preview.as_deref(),
            Some("Executed deferred tool PluginEcho")
        );
    }

    #[tokio::test]
    async fn read_only_plugin_tool_is_allowed_without_prompt() {
        let tool = PluginToolWrapper::new(
            "demo-plugin@local".to_string(),
            std::env::temp_dir(),
            ToolContribution {
                name: "plugin_read".to_string(),
                description: "".to_string(),
                input_schema: None,
                read_only: true,
                concurrency_safe: false,
                runtime: Some(ToolRuntime::Stdio(echo_runtime())),
            },
        );

        let decision = tool.check_permissions(&json!({}), &dummy_ctx()).await;
        assert!(matches!(decision, PermissionResult::Allow { .. }));
    }

    #[tokio::test]
    async fn writable_plugin_tool_asks_for_permission_in_default_mode() {
        let tool = PluginToolWrapper::new(
            "demo-plugin@local".to_string(),
            std::env::temp_dir(),
            ToolContribution {
                name: "plugin_write".to_string(),
                description: "".to_string(),
                input_schema: None,
                read_only: false,
                concurrency_safe: false,
                runtime: Some(ToolRuntime::Stdio(echo_runtime())),
            },
        );

        let decision = tool.check_permissions(&json!({}), &dummy_ctx()).await;
        assert!(matches!(decision, PermissionResult::Ask { .. }));
    }
}
