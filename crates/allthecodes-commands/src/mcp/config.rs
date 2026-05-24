use anyhow::Result;

use super::flags::{oauth_from_flags, parse_flags, parse_mcpjson_decision_args};
use super::help::help_text;
use super::settings::{
    describe_entry, discover_config_entries, entry_to_settings_value, persist_mcpjson_decision,
    read_settings_value, write_settings_value,
};
use super::{CommandContext, CommandResult};
use allthecodes_ipc_protocol::subsystem_types::{ConfigScope, McpServerConfigEntry};

// ---------------------------------------------------------------------------
// add / edit
// ---------------------------------------------------------------------------

pub(super) fn handle_add(rest: &[&str], ctx: &mut CommandContext) -> Result<CommandResult> {
    let Some(name) = rest.first() else {
        return Ok(CommandResult::Output(
            "Usage: /mcp add <name> [--command=<cmd>] [--arg=<arg>] [--env=K=V] [--scope=user|project]"
                .to_string(),
        ));
    };
    let flags = parse_flags(&rest[1..]);
    if let Some(msg) = &flags.error {
        return Ok(CommandResult::Output(format!("{}\n\n{}", msg, help_text())));
    }

    let scope = flags.scope.clone().unwrap_or(ConfigScope::User);
    let entry = McpServerConfigEntry {
        name: (*name).to_string(),
        scope: scope.clone(),
        transport: flags
            .transport
            .clone()
            .unwrap_or_else(|| "stdio".to_string()),
        command: flags.command.clone(),
        args: (!flags.args.is_empty()).then(|| flags.args.clone()),
        url: flags.url.clone(),
        headers: None,
        oauth: oauth_from_flags(&flags, None),
        env: (!flags.env.is_empty()).then(|| flags.env.clone()),
        browser_mcp: flags.browser,
        disabled: None,
    };
    if entry.transport == "stdio" && entry.command.is_none() {
        return Ok(CommandResult::Output(
            "`stdio` transport requires --command=<cmd>. Use --transport=sse or --transport=streamable-http with --url=<url> for remote servers."
                .to_string(),
        ));
    }
    if matches!(entry.transport.as_str(), "sse" | "streamable-http") && entry.url.is_none() {
        return Ok(CommandResult::Output(format!(
            "`{}` transport requires --url=<url>.",
            entry.transport
        )));
    }

    persist_upsert(&ctx.cwd, entry).map(CommandResult::Output)
}

pub(super) fn handle_edit(rest: &[&str], ctx: &mut CommandContext) -> Result<CommandResult> {
    let Some(name) = rest.first() else {
        return Ok(CommandResult::Output(
            "Usage: /mcp edit <name> [--command=<cmd>] [--arg=<arg>] [--env=K=V] [--scope=user|project]"
                .to_string(),
        ));
    };
    let flags = parse_flags(&rest[1..]);
    if let Some(msg) = &flags.error {
        return Ok(CommandResult::Output(format!("{}\n\n{}", msg, help_text())));
    }

    // Locate the current entry to edit (respect --scope override if supplied).
    let existing = discover_config_entries(&ctx.cwd);
    let current = match flags.scope.as_ref() {
        Some(wanted) => existing
            .iter()
            .find(|e| e.name == *name && e.scope == *wanted),
        None => existing.iter().find(|e| e.name == *name),
    };
    let Some(current) = current.cloned() else {
        return Ok(CommandResult::Output(format!(
            "No MCP server named `{}` found{}. Use /mcp add to create one.",
            name,
            flags
                .scope
                .as_ref()
                .map(|s| format!(" in scope {}", s.label()))
                .unwrap_or_default()
        )));
    };
    if !current.scope.is_editable() {
        return Ok(CommandResult::Output(format!(
            "`{}` is contributed by scope `{}`, which is read-only. Edit the owning config instead.",
            name,
            current.scope.label()
        )));
    }

    // Overlay the flags onto the existing config.
    let args = if !flags.args.is_empty() {
        Some(flags.args.clone())
    } else {
        current.args.clone()
    };
    let env = if !flags.env.is_empty() {
        Some(flags.env.clone())
    } else {
        current.env.clone()
    };
    let transport = flags.transport.clone().unwrap_or(current.transport.clone());
    let command = flags.command.clone().or(current.command.clone());
    let url = flags.url.clone().or(current.url.clone());
    let oauth = oauth_from_flags(&flags, current.oauth.as_ref()).or(current.oauth.clone());
    let browser_mcp = flags.browser.or(current.browser_mcp);

    let entry = McpServerConfigEntry {
        name: (*name).to_string(),
        scope: flags.scope.unwrap_or(current.scope.clone()),
        transport,
        command,
        args,
        url,
        headers: current.headers.clone(),
        oauth,
        env,
        browser_mcp,
        disabled: current.disabled,
    };
    if matches!(entry.transport.as_str(), "sse" | "streamable-http") && entry.url.is_none() {
        return Ok(CommandResult::Output(format!(
            "`{}` transport requires --url=<url>.",
            entry.transport
        )));
    }
    persist_upsert(&ctx.cwd, entry).map(CommandResult::Output)
}

fn persist_upsert(cwd: &std::path::Path, entry: McpServerConfigEntry) -> Result<String> {
    let scope_label = entry.scope.label();
    let name = entry.name.clone();
    let path = match &entry.scope {
        ConfigScope::User => allthecodes_config::settings::user_settings_path(),
        // Keep the write path aligned with the scoped discovery layer -
        // see the aside in `ipc::subsystem_handlers::settings_path_for_scope`.
        ConfigScope::Project => cwd.join(".allthecodes").join("settings.json"),
        _ => {
            return Ok(format!(
                "Cannot upsert `{}` into scope `{}` (read-only).",
                name, scope_label
            ));
        }
    };
    let mut value = read_settings_value(&path)?;
    let obj = match value.as_object_mut() {
        Some(obj) => obj,
        None => {
            return Ok(format!("{} is not a JSON object", path.display()));
        }
    };
    let servers = obj
        .entry("mcpServers")
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
    let servers_obj = match servers.as_object_mut() {
        Some(m) => m,
        None => {
            return Ok(format!(
                "{} has a non-object `mcpServers` field",
                path.display()
            ));
        }
    };
    servers_obj.insert(name.clone(), entry_to_settings_value(&entry));
    write_settings_value(&path, &value)?;

    let flags_summary = describe_entry(&entry);
    Ok(format!(
        "Upserted MCP server `{}` in scope `{}` (at {}).\n  {}",
        name,
        scope_label,
        path.display(),
        flags_summary
    ))
}

pub(super) fn handle_remove(rest: &[&str], ctx: &mut CommandContext) -> Result<CommandResult> {
    let Some(name) = rest.first() else {
        return Ok(CommandResult::Output(
            "Usage: /mcp remove <name> [--scope=user|project]".to_string(),
        ));
    };
    let flags = parse_flags(&rest[1..]);
    if let Some(msg) = &flags.error {
        return Ok(CommandResult::Output(format!("{}\n\n{}", msg, help_text())));
    }

    let existing = discover_config_entries(&ctx.cwd);
    let matches: Vec<&McpServerConfigEntry> = existing
        .iter()
        .filter(|e| {
            e.name == *name
                && match flags.scope.as_ref() {
                    Some(wanted) => e.scope == *wanted,
                    None => true,
                }
        })
        .collect();

    if matches.is_empty() {
        return Ok(CommandResult::Output(format!(
            "No MCP server named `{}` found{}.",
            name,
            flags
                .scope
                .as_ref()
                .map(|s| format!(" in scope {}", s.label()))
                .unwrap_or_default()
        )));
    }

    if matches.len() > 1 && flags.scope.is_none() {
        let labels: Vec<String> = matches.iter().map(|e| e.scope.label()).collect();
        return Ok(CommandResult::Output(format!(
            "`{}` exists in multiple scopes ({}). Re-run with --scope=<scope> to pick one.",
            name,
            labels.join(", ")
        )));
    }

    // Pick the most specific editable match: prefer Project, then User.
    let target = matches
        .iter()
        .find(|e| e.scope == ConfigScope::Project)
        .or_else(|| matches.iter().find(|e| e.scope == ConfigScope::User))
        .or_else(|| matches.first())
        .cloned();
    let Some(target) = target else {
        return Ok(CommandResult::Output(format!(
            "No removable match for `{}`.",
            name
        )));
    };
    if !target.scope.is_editable() {
        return Ok(CommandResult::Output(format!(
            "`{}` in scope `{}` is read-only. Edit the owning config source to remove it.",
            name,
            target.scope.label()
        )));
    }

    let path = match &target.scope {
        ConfigScope::User => allthecodes_config::settings::user_settings_path(),
        ConfigScope::Project => ctx.cwd.join(".allthecodes").join("settings.json"),
        _ => unreachable!("editable check above covers plugin/ide"),
    };
    let mut value = read_settings_value(&path)?;
    let removed = value
        .get_mut("mcpServers")
        .and_then(|v| v.as_object_mut())
        .and_then(|obj| obj.remove(name.to_string().as_str()))
        .is_some();
    if !removed {
        return Ok(CommandResult::Output(format!(
            "Entry `{}` not found in {}; nothing to remove.",
            name,
            path.display()
        )));
    }
    write_settings_value(&path, &value)?;
    Ok(CommandResult::Output(format!(
        "Removed MCP server `{}` from scope `{}` (at {}).",
        name,
        target.scope.label(),
        path.display()
    )))
}

pub(super) fn handle_mcpjson_decision(
    rest: &[&str],
    ctx: &mut CommandContext,
    approve: bool,
) -> Result<CommandResult> {
    let parsed = parse_mcpjson_decision_args(rest);
    if let Some(error) = parsed.error {
        return Ok(CommandResult::Output(format!(
            "{}\n\n{}",
            error,
            help_text()
        )));
    }
    if parsed.names.is_empty() {
        let verb = if approve { "approve" } else { "reject" };
        return Ok(CommandResult::Output(format!(
            "Usage: /mcp {verb} <name...>{}",
            if approve { " [--all-project]" } else { "" }
        )));
    }

    persist_mcpjson_decision(&ctx.cwd, &parsed.names, approve, parsed.all_project)
        .map(CommandResult::Output)
}
