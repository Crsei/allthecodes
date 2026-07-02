use allthecodes_mcp::bindings::{self, BindingSelector};
use allthecodes_types::mcp::{McpBinding, McpPermission, McpToolScope};
use anyhow::Result;

use super::{CommandContext, CommandResult};

pub(super) fn handle_bindings(ctx: &CommandContext) -> Result<CommandResult> {
    let bindings = current_bindings(ctx)?;
    if bindings.is_empty() {
        return Ok(CommandResult::Output(
            "No MCP bindings configured.".to_string(),
        ));
    }

    let mut lines = Vec::new();
    lines.push(format!("MCP bindings ({}):", bindings.len()));
    lines.push(String::new());
    for binding in bindings {
        lines.push(format!("  {}", describe_binding(&binding)));
    }
    Ok(CommandResult::Output(lines.join("\n")))
}

pub(super) fn handle_bind(rest: &[&str], ctx: &CommandContext) -> Result<CommandResult> {
    let Some(server_id) = rest.first() else {
        return Ok(CommandResult::Output(bind_usage()));
    };
    let args = parse_binding_args(&rest[1..], ctx);
    if let Some(error) = args.error {
        return Ok(CommandResult::Output(error));
    }
    let Some(scope) = args.scope else {
        return Ok(CommandResult::Output(bind_usage()));
    };
    let mut binding = McpBinding {
        server_id: (*server_id).to_string(),
        scope,
        permissions: args.permissions.unwrap_or_else(|| {
            if args.read_only {
                McpBinding::read_only_permissions()
            } else {
                McpBinding::full_permissions()
            }
        }),
        read_only: args.read_only,
        ..Default::default()
    };
    match scope {
        McpToolScope::Global | McpToolScope::Project => {}
        McpToolScope::Session => {
            binding.session_id = Some(ctx.session_id.as_str().to_string());
        }
        McpToolScope::Thread => {
            binding.session_id = Some(ctx.session_id.as_str().to_string());
            binding.thread_id = args.thread_id;
        }
    }

    bindings::upsert_binding(&ctx.cwd, binding.clone())?;
    refresh_runtime_bindings(ctx)?;
    Ok(CommandResult::Output(format!(
        "Bound MCP server `{}` at {} scope with permissions: {}.",
        binding.server_id,
        binding.scope.as_str(),
        permission_list(&binding.effective_permissions())
    )))
}

pub(super) fn handle_unbind(rest: &[&str], ctx: &CommandContext) -> Result<CommandResult> {
    let Some(server_id) = rest.first() else {
        return Ok(CommandResult::Output(unbind_usage()));
    };
    let args = parse_binding_args(&rest[1..], ctx);
    if let Some(error) = args.error {
        return Ok(CommandResult::Output(error));
    }
    let Some(scope) = args.scope else {
        return Ok(CommandResult::Output(unbind_usage()));
    };
    let selector = BindingSelector::new(
        *server_id,
        scope,
        matches!(scope, McpToolScope::Session | McpToolScope::Thread)
            .then(|| ctx.session_id.as_str().to_string()),
        args.thread_id,
    );

    let removed = bindings::remove_binding(&ctx.cwd, selector)?;
    refresh_runtime_bindings(ctx)?;
    let status = if removed {
        "Unbound"
    } else {
        "No matching binding for"
    };
    Ok(CommandResult::Output(format!(
        "{} MCP server `{}` at {} scope.",
        status,
        server_id,
        scope.as_str()
    )))
}

#[derive(Default)]
struct BindingArgs {
    scope: Option<McpToolScope>,
    thread_id: Option<String>,
    read_only: bool,
    permissions: Option<Vec<McpPermission>>,
    error: Option<String>,
}

fn parse_binding_args(rest: &[&str], ctx: &CommandContext) -> BindingArgs {
    let mut out = BindingArgs::default();
    for raw in rest {
        if let Some(scope) = scope_flag(raw, ctx) {
            set_scope(&mut out, scope.0, scope.1);
        } else if *raw == "--read-only" {
            out.read_only = true;
        } else if let Some(value) = raw.strip_prefix("--permissions=") {
            match parse_permissions(value) {
                Ok(permissions) => out.permissions = Some(permissions),
                Err(error) => out.error = Some(error),
            }
        } else if raw.starts_with("--") {
            out.error = Some(format!("unknown binding flag `{}`", raw));
        } else {
            out.error = Some(format!("unexpected binding argument `{}`", raw));
        }
        if out.error.is_some() {
            break;
        }
    }
    out
}

fn scope_flag(raw: &str, ctx: &CommandContext) -> Option<(McpToolScope, Option<String>)> {
    match raw {
        "--global" => Some((McpToolScope::Global, None)),
        "--project" => Some((McpToolScope::Project, None)),
        "--session" => Some((McpToolScope::Session, None)),
        value if value.starts_with("--thread=") => {
            let thread_id = value.trim_start_matches("--thread=").trim();
            Some((McpToolScope::Thread, Some(thread_id.to_string())))
        }
        "--thread" => Some((
            McpToolScope::Thread,
            Some(ctx.session_id.as_str().to_string()),
        )),
        _ => None,
    }
}

fn set_scope(out: &mut BindingArgs, scope: McpToolScope, thread_id: Option<String>) {
    if out.scope.is_some() {
        out.error = Some("choose exactly one binding scope".to_string());
        return;
    }
    if scope == McpToolScope::Thread
        && thread_id
            .as_deref()
            .map(str::trim)
            .unwrap_or_default()
            .is_empty()
    {
        out.error = Some("--thread requires a non-empty thread id".to_string());
        return;
    }
    out.scope = Some(scope);
    out.thread_id = thread_id;
}

fn parse_permissions(raw: &str) -> Result<Vec<McpPermission>, String> {
    let mut permissions = Vec::new();
    for part in raw.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let permission = match part {
            "connect" => McpPermission::Connect,
            "list_tools" | "list-tools" => McpPermission::ListTools,
            "call_tools" | "call-tools" => McpPermission::CallTools,
            "read_resources" | "read-resources" => McpPermission::ReadResources,
            other => {
                return Err(format!(
                    "unknown MCP permission `{}` (expected connect,list_tools,call_tools,read_resources)",
                    other
                ));
            }
        };
        if !permissions.contains(&permission) {
            permissions.push(permission);
        }
    }
    if permissions.is_empty() {
        return Err("--permissions must list at least one permission".to_string());
    }
    Ok(permissions)
}

fn current_bindings(ctx: &CommandContext) -> Result<Vec<McpBinding>> {
    if let Some(manager) = allthecodes_mcp::runtime::current_manager() {
        if let Ok(manager) = manager.try_lock() {
            return Ok(manager.bindings());
        }
    }
    Ok(allthecodes_mcp::discovery::discover_bound_mcp_servers(
        &ctx.cwd,
        Some(ctx.session_id.as_str()),
    )?
    .bindings)
}

fn refresh_runtime_bindings(ctx: &CommandContext) -> Result<()> {
    let Some(manager) = allthecodes_mcp::runtime::current_manager() else {
        return Ok(());
    };
    let Ok(mut manager) = manager.try_lock() else {
        return Ok(());
    };
    let discovered = allthecodes_mcp::discovery::discover_bound_mcp_servers(
        &ctx.cwd,
        Some(ctx.session_id.as_str()),
    )?;
    manager.set_bindings(discovered.bindings);
    Ok(())
}

fn describe_binding(binding: &McpBinding) -> String {
    let mut parts = vec![
        binding.server_id.clone(),
        format!("scope={}", binding.scope.as_str()),
    ];
    if let Some(source) = &binding.source_scope {
        parts.push(format!("source={source}"));
    }
    if let Some(project) = &binding.project_path {
        parts.push(format!("project={}", project.display()));
    }
    if let Some(session) = &binding.session_id {
        parts.push(format!("session={session}"));
    }
    if let Some(thread) = &binding.thread_id {
        parts.push(format!("thread={thread}"));
    }
    if binding.read_only {
        parts.push("readOnly=true".to_string());
    }
    parts.push(format!(
        "permissions={}",
        permission_list(&binding.effective_permissions())
    ));
    parts.join(" -- ")
}

fn permission_list(permissions: &[McpPermission]) -> String {
    permissions
        .iter()
        .map(|permission| permission.as_str())
        .collect::<Vec<_>>()
        .join(",")
}

fn bind_usage() -> String {
    "Usage: /mcp bind <server> --global|--project|--session|--thread=<id> [--read-only] [--permissions=connect,list_tools,call_tools,read_resources]".to_string()
}

fn unbind_usage() -> String {
    "Usage: /mcp unbind <server> --global|--project|--session|--thread=<id>".to_string()
}
