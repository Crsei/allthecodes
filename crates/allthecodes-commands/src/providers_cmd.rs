//! `/providers` non-interactive provider-profile management.

use std::path::Path;

use allthecodes_config::settings::{
    load_effective, ProviderProfileReplacement, ProviderProfileSettings, ProviderProfileStore,
};
use anyhow::{bail, Context, Result};
use async_trait::async_trait;

use crate::{CommandContext, CommandHandler, CommandResult};

pub struct ProvidersHandler;

#[async_trait]
impl CommandHandler for ProvidersHandler {
    async fn execute(&self, args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        let parts = shell_words::split(args).context("invalid /providers quoting")?;
        let store = ProviderProfileStore::global();
        let output = match parts.first().map(String::as_str) {
            None | Some("list") => render_list(&store)?,
            Some("show") => {
                let id = required_id(&parts, "show")?;
                serde_json::to_string_pretty(&store.get(id)?)?
            }
            Some("activate") => {
                let id = required_id(&parts, "activate")?;
                store.activate(id)?;
                sync_context(ctx)?;
                format!("Activated provider profile `{id}`.")
            }
            Some("delete") => {
                let id = required_id(&parts, "delete")?;
                store.delete(id)?;
                format!("Deleted provider profile `{id}`.")
            }
            Some("create") => {
                let id = required_id(&parts, "create")?;
                let profile = read_document::<ProviderProfileSettings>(&parts[2..], &ctx.cwd)?;
                store.create(id, profile)?;
                format!("Created provider profile `{id}`.")
            }
            Some("replace") => {
                let id = required_id(&parts, "replace")?;
                let replacement =
                    read_document::<ProviderProfileReplacement>(&parts[2..], &ctx.cwd)?;
                store.replace(id, replacement)?;
                if store.get(id)?.active {
                    sync_context(ctx)?;
                }
                format!("Replaced provider profile `{id}`.")
            }
            Some(other) => bail!(
                "unknown /providers action `{other}`; use list, show, activate, create, replace, or delete"
            ),
        };
        Ok(CommandResult::Output(output))
    }
}

fn required_id<'a>(parts: &'a [String], action: &str) -> Result<&'a str> {
    parts
        .get(1)
        .map(String::as_str)
        .filter(|id| !id.trim().is_empty())
        .with_context(|| format!("usage: /providers {action} \"<profile id>\""))
}

fn read_document<T: serde::de::DeserializeOwned>(parts: &[String], cwd: &Path) -> Result<T> {
    match parts {
        [flag, value] if flag == "--json" => {
            serde_json::from_str(value).context("invalid provider JSON document")
        }
        [flag, value] if flag == "--file" => {
            let path = Path::new(value);
            let path = if path.is_absolute() {
                path.to_path_buf()
            } else {
                cwd.join(path)
            };
            let body = std::fs::read_to_string(&path)
                .with_context(|| format!("failed to read {}", path.display()))?;
            serde_json::from_str(&body)
                .with_context(|| format!("invalid provider JSON in {}", path.display()))
        }
        _ => bail!("expected exactly one of --json '<document>' or --file <path>"),
    }
}

fn render_list(store: &ProviderProfileStore) -> Result<String> {
    let profiles = store.list()?;
    if profiles.is_empty() {
        return Ok(
            "No configured provider profiles. Open `/providers` in the Rust TUI to create one."
                .to_string(),
        );
    }
    let mut lines = vec!["Provider profiles:".to_string()];
    for profile in profiles {
        lines.push(format!(
            "{} {}  provider={}  runtime={}  credential={}",
            if profile.active { "*" } else { " " },
            profile.id,
            profile.api_provider.as_deref().unwrap_or("unset"),
            profile.runtime_support,
            if profile.api_key_configured || !profile.env_keys.is_empty() {
                "configured"
            } else {
                "missing"
            }
        ));
    }
    Ok(lines.join("\n"))
}

fn sync_context(ctx: &mut CommandContext) -> Result<()> {
    let loaded = load_effective(&ctx.cwd)?;
    let effective = loaded.effective;
    let provider_default = effective
        .api_provider
        .as_deref()
        .and_then(allthecodes_api::api::providers::get_provider)
        .map(|provider| provider.default_model.to_string());
    ctx.app_state.main_loop_model = effective
        .model
        .clone()
        .or(provider_default)
        .unwrap_or_else(allthecodes_types::models::default_model_id);
    ctx.app_state.main_loop_backend = effective
        .backend
        .clone()
        .unwrap_or_else(|| "native".to_string());
    ctx.app_state.settings = allthecodes_config::runtime_settings::SettingsJson::from_effective(
        &effective,
        loaded.sources,
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoted_profile_ids_are_parsed_as_one_argument() {
        let parts = shell_words::split("show \"Local OpenAI\"").unwrap();
        assert_eq!(required_id(&parts, "show").unwrap(), "Local OpenAI");
    }
}
